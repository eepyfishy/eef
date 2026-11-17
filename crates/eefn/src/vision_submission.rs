//! JSON-free camera/model intent. Selection is explicit and immutable; authoring
//! never probes a provider, changes permissions, loads a model or starts work.
use crate::{
    camera_submission::{
        CameraCaptureOptions, CameraCommand, CameraSourceOptions, CameraSubmission,
    },
    microphone_submission::{MicrophoneEventOptions, MicrophoneLimitOptions},
    workload_leases::definition::{
        MotionParameters,
        vision::{AnalysisLimits, Images, ModelSelection, Parameters, Trigger},
    },
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Analysis {
    pub images: Images,
    pub model: ModelSelection,
    pub prompt: String,
    pub inference: AnalysisLimits,
}
impl Analysis {
    pub fn parameters(&self, capture: MotionParameters) -> Parameters {
        Parameters {
            capture,
            images: self.images.clone(),
            model: self.model.clone(),
            prompt: self.prompt.clone(),
            inference: self.inference.clone(),
        }
    }
}

// Boxed flatten groups keep the native command parser's stack bounded. Existing
// camera/microphone defaults are unchanged; vision reserves text as well as events.
#[derive(Clone, Debug, clap::Args)]
pub struct VisionSubmission {
    #[command(flatten)]
    pub source: Box<CameraSourceOptions>,
    #[command(flatten)]
    pub capture: Box<CameraCaptureOptions>,
    #[command(flatten)]
    pub limits: Box<MicrophoneLimitOptions>,
    #[command(flatten)]
    pub events: Box<EventOptions>,
    #[command(flatten)]
    pub model: Box<ModelOptions>,
    #[command(flatten)]
    pub images: Box<ImageOptions>,
    #[command(flatten)]
    pub analysis: Box<AnalysisOptions>,
    /// Keep this exact receipt after uncertainty; do not generate a retry ID.
    #[arg(long)]
    pub operation_id: Option<String>,
}
#[derive(Clone, Debug, clap::Args)]
pub struct ModelOptions {
    /// Exact applied local model selection on the camera node.
    #[arg(long)]
    pub model: String,
    #[arg(long, value_parser=["ollama","llamacpp"])]
    pub backend: String,
    /// Ollama's exact installed digest from `eefn models installed`; not a path.
    #[arg(long, required_if_eq("backend", "ollama"))]
    pub model_digest: Option<String>,
}
#[derive(Clone, Debug, clap::Args)]
pub struct ImageOptions {
    #[arg(long, default_value_t = 320)]
    pub image_width: u32,
    #[arg(long, default_value_t = 240)]
    pub image_height: u32,
    #[arg(long, default_value_t = 65536)]
    pub max_image_bytes: usize,
    #[arg(long, default_value_t = 70)]
    pub jpeg_quality: u32,
    #[arg(long, default_value_t = 1)]
    pub max_images: u64,
    #[arg(long, default_value_t = 1000)]
    pub image_interval_ms: u64,
    #[arg(long, default_value="rising", value_parser=["rising","active-interval"])]
    pub trigger: String,
}
#[derive(Clone, Debug, clap::Args)]
pub struct AnalysisOptions {
    #[arg(long)]
    pub prompt: String,
    #[arg(long, default_value_t = 128)]
    pub max_tokens: u64,
    #[arg(long, default_value_t = 8192)]
    pub max_response_bytes: usize,
    #[arg(long, default_value_t = 1024)]
    pub max_result_bytes: usize,
    #[arg(long, default_value_t = 2000)]
    pub analysis_timeout_ms: u64,
}
#[derive(Clone, Debug, clap::Args)]
pub struct EventOptions {
    #[arg(long, default_value_t = 100)]
    pub min_interval_ms: u64,
    #[arg(long, default_value_t = 100)]
    pub max_events: u64,
    #[arg(long, default_value_t = 1024)]
    pub max_event_bytes: usize,
    /// Metadata budget, separate from the bounded analysis text reservation.
    #[arg(long, default_value_t = 32768)]
    pub max_total_bytes: usize,
    #[arg(long, default_value_t = 4)]
    pub max_pending: usize,
}
impl VisionSubmission {
    pub fn command(&self) -> Result<CameraCommand> {
        let mut command = CameraSubmission {
            source: self.source.clone(),
            capture: self.capture.clone(),
            limits: self.limits.clone(),
            events: Box::new(MicrophoneEventOptions {
                min_interval_ms: self.events.min_interval_ms,
                max_events: self.events.max_events,
                max_event_bytes: self.events.max_event_bytes,
                max_total_bytes: self.events.max_total_bytes,
                max_pending: self.events.max_pending,
            }),
            operation_id: self.operation_id.clone(),
        }
        .command()?;
        command.input.schema_version = 2;
        if self.source.text.is_none() {
            command.input.original_text = format!(
                "Review bounded camera activity and up to {} local analyses using {} on {}.",
                self.images.max_images,
                self.model.model,
                self.source.source_node.as_deref().unwrap_or("this node")
            );
        }
        command.input.vision = Some(Analysis {
            images: Images {
                width: self.images.image_width,
                height: self.images.image_height,
                max_image_bytes: self.images.max_image_bytes,
                jpeg_quality: self.images.jpeg_quality,
                max_images: self.images.max_images,
                min_interval_ms: self.images.image_interval_ms,
                trigger: match self.images.trigger.as_str() {
                    "rising" => Trigger::Rising,
                    "active-interval" => Trigger::ActiveInterval,
                    _ => bail!("Unsupported vision image trigger"),
                },
            },
            model: ModelSelection {
                backend: match self.model.backend.as_str() {
                    "ollama" => crate::model_selection::Backend::Ollama,
                    "llamacpp" => crate::model_selection::Backend::Llamacpp,
                    _ => bail!("Unsupported local vision backend"),
                },
                model_id: self.model.model.clone(),
                installed_digest: self.model.model_digest.clone(),
            },
            prompt: self.analysis.prompt.clone(),
            inference: AnalysisLimits {
                max_tokens: self.analysis.max_tokens,
                max_response_bytes: self.analysis.max_response_bytes,
                max_result_bytes: self.analysis.max_result_bytes,
                timeout_ms: self.analysis.analysis_timeout_ms,
            },
        });
        command.validate()?;
        Ok(command)
    }
}

#[cfg(test)]
#[path = "vision_submission_tests.rs"]
mod tests;
