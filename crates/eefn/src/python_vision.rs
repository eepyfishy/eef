//! Private image pipe for an explicitly admitted camera-vision source.
//! No network entrypoint, workload permission, model call or startup producer.
use super::{
    BridgeFailure, BridgeLimits, PythonRuntime,
    stream::{PinnedStreamSource, StreamObservation, StreamSummary},
};
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fmt, future::Future, time::Duration};
use tokio::time::Instant;

#[path = "python_vision_wire.rs"]
mod wire;
const HOST: &str = include_str!("../../../python/eef_vision_stream_host.py");

/// Encoded local-pipe limits, not physical RAM/VRAM or execution authority.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VisionBounds {
    pub max_frame_bytes: usize,
    pub max_stdout_bytes: usize,
    pub max_observations: u64,
    pub max_source_frames: u64,
    pub max_image_bytes: usize,
    pub max_images: u64,
}
impl VisionBounds {
    pub(crate) fn validate(&self, local: &BridgeLimits) -> Result<()> {
        local.validate()?;
        if !(1024..=3 * 1024 * 1024).contains(&self.max_frame_bytes)
            || !(1024..=4 * 1024 * 1024).contains(&self.max_stdout_bytes)
            || self.max_frame_bytes > self.max_stdout_bytes
            || self.max_stdout_bytes > local.max_stdout_bytes
            || !(1..=100000).contains(&self.max_observations)
            || !(1..=100000).contains(&self.max_source_frames)
            || !(128..=2 * 1024 * 1024).contains(&self.max_image_bytes)
            || !(1..=100).contains(&self.max_images)
            || self.max_images > self.max_source_frames
            || 4 * self.max_image_bytes.div_ceil(3) + 1024 > self.max_frame_bytes
        {
            bail!("Invalid or widened private vision bounds")
        }
        Ok(())
    }
    fn expected_frames(&self, params: &Value) -> Result<u64> {
        let frames = params
            .get("max_frames")
            .and_then(Value::as_u64)
            .context("Vision source frame bound unavailable")?;
        if frames == 0
            || frames > self.max_source_frames
            || frames > self.max_observations
            || params
                .pointer("/analysis/max_image_bytes")
                .and_then(Value::as_u64)
                != Some(self.max_image_bytes as u64)
            || params
                .pointer("/analysis/max_images")
                .and_then(Value::as_u64)
                != Some(self.max_images)
        {
            bail!("Private vision source differs from its transport bounds")
        }
        Ok(frames)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AnalysisState {
    NotRequested,
    Frame,
    BudgetExhausted,
    RateLimited,
}

/// Intentionally not Clone or Serialize. Debug exposes size, never image bytes.
pub(crate) struct PrivateJpeg {
    encoded: String,
    bytes: usize,
    sha256: String,
}
impl fmt::Debug for PrivateJpeg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrivateJpeg")
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}
impl PrivateJpeg {
    fn checked(encoded: String, maximum: usize) -> Result<Self> {
        if encoded.len() > 4 * maximum.div_ceil(3) {
            return Err(BridgeFailure::Reply.into());
        }
        let raw = STANDARD
            .decode(&encoded)
            .map_err(|_| BridgeFailure::Reply)?;
        if !(4..=maximum).contains(&raw.len())
            || !raw.starts_with(&[0xff, 0xd8])
            || !raw.ends_with(&[0xff, 0xd9])
            || STANDARD.encode(&raw) != encoded
        {
            return Err(BridgeFailure::Reply.into());
        }
        Ok(Self {
            encoded,
            bytes: raw.len(),
            sha256: hex::encode(Sha256::digest(&raw)),
        })
    }
    pub(crate) fn as_base64(&self) -> &str {
        &self.encoded
    }
    pub(crate) fn digest(&self) -> &str {
        &self.sha256
    }
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }
}

/// Local consumer data; no Serialize path into coordinator events or reports.
#[derive(Debug)]
pub(crate) struct VisionObservation {
    pub motion: StreamObservation,
    pub analysis: AnalysisState,
    pub image: Option<PrivateJpeg>,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct VisionSummary {
    pub stream: StreamSummary,
    pub images: u64,
}
/// Created only after exact successful I/O and an observed empty owned job.
#[derive(Debug)]
pub(crate) struct ObservedVisionStream {
    summary: VisionSummary,
    source_sha256: String,
    _exit: crate::process_lifetime::ObservedExit,
}
impl ObservedVisionStream {
    pub(crate) fn summary(&self) -> &VisionSummary {
        &self.summary
    }
    pub(crate) fn source_digest(&self) -> &str {
        &self.source_sha256
    }
}

impl PythonRuntime {
    /// The caller must own consumed workload, current permissions/leases and
    /// model scope. This method supplies transport/exit evidence only.
    pub(crate) async fn observe_reserved_vision<F, C>(
        &self,
        source: &PinnedStreamSource,
        params: Value,
        stream_id: &str,
        bounds: VisionBounds,
        execution_deadline: Instant,
        reservation: crate::capture_ownership::Reservation,
        observe: F,
    ) -> Result<ObservedVisionStream>
    where
        F: FnMut(VisionObservation) -> C + Send,
        C: Future<Output = Result<()>> + Send,
    {
        if !cfg!(windows) {
            bail!("Owned vision exit observation is unsupported on this platform")
        }
        bounds.validate(&self.limits)?;
        crate::model_memory::supported(self.limits.max_job_memory_mib)?;
        crate::job_commands::validate_operation_id(stream_id)?;
        if uuid::Uuid::parse_str(stream_id)?.is_nil() {
            return Err(BridgeFailure::RequestLimit.into());
        }
        let expected = bounds.expected_frames(&params)?;
        let deadline = execution_deadline
            .min(Instant::now() + Duration::from_secs(self.limits.timeout_seconds));
        if Instant::now() >= deadline {
            return Err(BridgeFailure::Deadline.into());
        }
        let request = json!({"schema_version":1,"stream_id":stream_id,
            "source_utf8":source.source_utf8(),"source_sha256":source.digest(),"params":params,"bounds":bounds});
        let mut input = super::encode_request(
            &request,
            self.limits
                .max_request_bytes
                .min(64 * 1024)
                .saturating_sub(1),
        )?;
        input.push(b'\n');
        let mut command = self.command();
        #[cfg(all(test, windows, feature = "native-test-fixtures"))]
        let host = self
            .stream_test_prelude
            .as_ref()
            .map(|prelude| format!("{prelude}\n{HOST}"));
        #[cfg(all(test, windows, feature = "native-test-fixtures"))]
        let host = host.as_deref().unwrap_or(HOST);
        #[cfg(not(all(test, windows, feature = "native-test-fixtures")))]
        let host = HOST;
        command.arg("-c").arg(host).arg("--pinned-camera-vision-v1");
        let outcome = self
            .run_owned_stream(
                &mut command,
                deadline,
                Some(reservation),
                |stdin, stdout| {
                    wire::exchange(stdin, stdout, input, stream_id, &bounds, expected, observe)
                },
            )
            .await?;
        Ok(ObservedVisionStream {
            summary: outcome.value,
            source_sha256: source.digest().into(),
            _exit: outcome
                .owned_exit
                .context("Private vision owned exit was not observed")?,
        })
    }
}

#[cfg(test)]
#[path = "python_vision_tests.rs"]
mod tests;
