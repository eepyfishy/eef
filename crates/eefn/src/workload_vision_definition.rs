//! Exact reviewed camera/model scope, not source or model execution authority.
use super::MotionParameters;
pub use crate::engine::vision::{AnalysisLimits, ModelSelection};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Rising,
    ActiveInterval,
}

/// One image is eligible for at most one model call. These settings enter the
/// capture process; prompts, model choice and assistant context never do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Images {
    pub width: u32,
    pub height: u32,
    pub max_image_bytes: usize,
    pub jpeg_quality: u32,
    pub max_images: u64,
    pub min_interval_ms: u64,
    pub trigger: Trigger,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameters {
    pub capture: MotionParameters,
    pub images: Images,
    pub model: ModelSelection,
    pub prompt: String,
    pub inference: AnalysisLimits,
}
impl Parameters {
    pub fn validate(&self) -> Result<()> {
        self.capture.validate()?;
        self.model.validate()?;
        self.inference.validate()?;
        ensure!(
            (16..=640).contains(&self.images.width)
                && (16..=480).contains(&self.images.height)
                && self.images.width <= self.capture.width
                && self.images.height <= self.capture.height
                && (128..=2 * 1024 * 1024).contains(&self.images.max_image_bytes)
                && (1..=95).contains(&self.images.jpeg_quality)
                && (1..=100).contains(&self.images.max_images)
                && self.images.max_images <= u64::from(self.capture.max_frames)
                && self.images.min_interval_ms <= 3_600_000
                && !self.prompt.trim().is_empty()
                && self.prompt.len() <= 4096
                && !self.prompt.contains('\0')
                && self.inference.timeout_ms <= self.capture.max_duration_ms,
            "Invalid reviewed camera vision parameters"
        );
        Ok(())
    }
    pub fn max_calls(&self) -> u64 {
        self.images.max_images
    }
    /// Reservation of requested output tokens, not measured backend token use.
    pub fn max_reserved_output_tokens(&self) -> Result<u64> {
        self.validate()?;
        self.max_calls()
            .checked_mul(self.inference.max_tokens)
            .ok_or_else(|| anyhow::anyhow!("Vision token reservation overflow"))
    }
    pub fn max_total_result_bytes(&self) -> Result<usize> {
        self.validate()?;
        usize::try_from(self.max_calls())?
            .checked_mul(self.inference.max_result_bytes)
            .ok_or_else(|| anyhow::anyhow!("Vision result reservation overflow"))
    }
    pub(super) fn python_parameters(&self) -> Result<Value> {
        self.validate()?;
        let mut value = serde_json::to_value(&self.capture)?;
        value["analysis"] = serde_json::to_value(&self.images)?;
        Ok(value)
    }
}

#[cfg(test)]
#[path = "workload_vision_definition_tests.rs"]
pub(crate) mod tests;
