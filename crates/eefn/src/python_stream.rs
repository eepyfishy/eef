//! Owned streaming extension transport. This is not workload/listening authority.
//! No node advertisement, RPC, grant renewal or ambient default enables it.
use super::{BridgeFailure, BridgeLimits, PythonPlugin, PythonRuntime};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{future::Future, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader},
    time::{Instant, timeout, timeout_at},
};

const HOST: &str = include_str!("../../../python/eef_stream_host.py");

/// Encoded transport bounds, not physical memory/CPU or external-effect proof.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamBounds {
    pub max_frame_bytes: usize,
    pub max_stdout_bytes: usize,
    pub max_observations: u64,
    pub max_source_frames: u64,
}
impl Default for StreamBounds {
    fn default() -> Self {
        Self {
            max_frame_bytes: 1024,
            max_stdout_bytes: 64 * 1024,
            max_observations: 1024,
            max_source_frames: 1_000_000_000,
        }
    }
}
impl StreamBounds {
    pub fn validate(&self, local: &BridgeLimits) -> Result<()> {
        local.validate()?;
        if !(128..=16384).contains(&self.max_frame_bytes)
            || !(1024..=4 * 1024 * 1024).contains(&self.max_stdout_bytes)
            || self.max_frame_bytes > self.max_stdout_bytes
            || self.max_stdout_bytes > local.max_stdout_bytes
            || !(1..=100_000).contains(&self.max_observations)
            || !(1..=1_000_000_000_000).contains(&self.max_source_frames)
        {
            bail!("Invalid or widened streaming bridge bounds")
        }
        Ok(())
    }
}

/// None means uncertainty, never quiet/false. No PCM, image, text or path field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamObservation {
    pub sequence: u64,
    pub source_frames: u64,
    pub active: Option<bool>,
    pub level: Option<u16>,
    pub discontinuity: bool,
}

/// Completed transport/source iterator, not physical capture/stop attestation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamSummary {
    pub stream_id: String,
    pub observations: u64,
    pub source_frames: u64,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Frame {
    Ready {
        schema_version: u32,
        stream_id: String,
    },
    Observation {
        stream_id: String,
        sequence: u64,
        source_frames: u64,
        active: Option<bool>,
        level: Option<u16>,
        discontinuity: bool,
    },
    End {
        stream_id: String,
        observations: u64,
        source_frames: u64,
    },
}

struct State<'a> {
    id: &'a str,
    bounds: &'a StreamBounds,
    ready: bool,
    ended: bool,
    observations: u64,
    frames: u64,
}
impl State<'_> {
    fn accept(&mut self, frame: Frame) -> Result<Option<StreamObservation>> {
        if self.ended {
            return Err(BridgeFailure::Reply.into());
        }
        match frame {
            Frame::Ready {
                schema_version,
                stream_id,
            } => {
                if self.ready || schema_version != 1 || stream_id != self.id {
                    return Err(BridgeFailure::Reply.into());
                }
                // Host protocol readiness is not proof that a device opened.
                self.ready = true;
                Ok(None)
            }
            Frame::Observation {
                stream_id,
                sequence,
                source_frames,
                active,
                level,
                discontinuity,
            } => {
                if !self.ready
                    || stream_id != self.id
                    || sequence != self.observations + 1
                    || sequence > self.bounds.max_observations
                    || source_frames <= self.frames
                    || source_frames > self.bounds.max_source_frames
                    || level.is_some_and(|v| v > 32768)
                    || (active.is_some() && level.is_none())
                    || (discontinuity && active.is_some())
                {
                    return Err(BridgeFailure::Reply.into());
                }
                self.observations = sequence;
                self.frames = source_frames;
                Ok(Some(StreamObservation {
                    sequence,
                    source_frames,
                    active,
                    level,
                    discontinuity,
                }))
            }
            Frame::End {
                stream_id,
                observations,
                source_frames,
            } => {
                if !self.ready
                    || stream_id != self.id
                    || observations == 0
                    || observations != self.observations
                    || source_frames != self.frames
                {
                    return Err(BridgeFailure::Reply.into());
                }
                self.ended = true;
                Ok(None)
            }
        }
    }
}

async fn read_frames<R, F, C>(
    stdout: R,
    id: &str,
    bounds: &StreamBounds,
    mut observe: F,
) -> Result<StreamSummary>
where
    R: AsyncRead + Unpin,
    F: FnMut(StreamObservation) -> C,
    C: Future<Output = Result<()>>,
{
    let mut reader = BufReader::new(stdout);
    let mut state = State {
        id,
        bounds,
        ready: false,
        ended: false,
        observations: 0,
        frames: 0,
    };
    let mut total = 0usize;
    loop {
        let mut line = Vec::new();
        let n = (&mut reader)
            .take(bounds.max_frame_bytes as u64 + 1)
            .read_until(b'\n', &mut line)
            .await
            .context("Stream reply pipe failed")?;
        if n == 0 {
            if !state.ended {
                return Err(BridgeFailure::Reply.into());
            }
            return Ok(StreamSummary {
                stream_id: id.into(),
                observations: state.observations,
                source_frames: state.frames,
            });
        }
        total = total
            .checked_add(n)
            .filter(|v| *v <= bounds.max_stdout_bytes)
            .ok_or(BridgeFailure::StdoutLimit)?;
        if n > bounds.max_frame_bytes {
            return Err(BridgeFailure::StdoutLimit.into());
        }
        if line.last() != Some(&b'\n') {
            return Err(BridgeFailure::Reply.into());
        }
        let frame = serde_json::from_slice(&line).map_err(|_| BridgeFailure::Reply)?;
        if let Some(observation) = state.accept(frame)? {
            // One owned callback at a time. No detached task or unbounded queue.
            // Backpressure and cancellation consume the same absolute deadline.
            observe(observation)
                .await
                .map_err(|_| BridgeFailure::Reply)?;
        }
    }
}

impl PythonRuntime {
    /// Low-level trusted extension API, like invoke(), not a network execution
    /// entrypoint. Product callers must supply validated workload/resource and
    /// local listening authority before use. Startup never calls this method.
    /// Metadata is delivered before EOF; confirmation waits for EOF/root cleanup.
    pub async fn observe_stream<F, C>(
        &self,
        plugin: &PythonPlugin,
        action: &str,
        params: Value,
        stream_id: &str,
        bounds: StreamBounds,
        execution_deadline: Instant,
        observe: F,
    ) -> Result<StreamSummary>
    where
        F: FnMut(StreamObservation) -> C + Send,
        C: Future<Output = Result<()>> + Send,
    {
        bounds.validate(&self.limits)?;
        crate::model_memory::supported(self.limits.max_job_memory_mib)?;
        crate::job_commands::validate_operation_id(stream_id)?;
        if uuid::Uuid::parse_str(stream_id)?.is_nil()
            || !params.is_object()
            || !plugin.actions.iter().any(|v| v == action)
        {
            return Err(BridgeFailure::RequestLimit.into());
        }
        let deadline = execution_deadline
            .min(Instant::now() + Duration::from_secs(self.limits.timeout_seconds));
        let input = super::encode_request(
            &json!({"schema_version":1,"stream_id":stream_id,"action":action,"params":params,"bounds":bounds}),
            self.limits.max_request_bytes.min(64 * 1024),
        )?;
        if Instant::now() >= deadline {
            return Err(BridgeFailure::Deadline.into());
        }
        let metadata = timeout_at(deadline, tokio::fs::metadata(&plugin.path))
            .await
            .map_err(|_| BridgeFailure::Deadline)??;
        if !metadata.is_file() {
            return Err(BridgeFailure::Reply.into());
        }
        let mut command = self.command();
        command
            .arg("-c")
            .arg(HOST)
            .arg(&plugin.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let (mut child, tree) = crate::process_lifetime::spawn_with_job_memory_limit_before(
            &mut command,
            self.limits.max_job_memory_mib,
            deadline,
        )
        .map_err(super::bridge_spawn_failure)?;
        let stdin = child.stdin.take().context("Stream stdin unavailable")?;
        let stdout = child.stdout.take().context("Stream stdout unavailable")?;
        let stderr = child.stderr.take().context("Stream stderr unavailable")?;
        let outcome = timeout_at(deadline, async {
            let (_, summary, _, status) = tokio::try_join!(
                super::send_input(stdin, &input),
                read_frames(stdout, stream_id, &bounds, observe),
                super::read_pipe(
                    stderr,
                    self.limits.max_stderr_bytes,
                    BridgeFailure::StderrLimit
                ),
                async {
                    let status = child.wait().await.context("Stream child wait failed")?;
                    #[cfg(windows)]
                    tree.terminate_and_observe_until(deadline).await?;
                    Ok::<_, anyhow::Error>(status)
                }
            )?;
            if !status.success() {
                return Err(BridgeFailure::Process.into());
            }
            Ok(summary)
        })
        .await
        .map_err(|_| anyhow::Error::from(BridgeFailure::Deadline))
        .and_then(|v| v);
        // Retire the exact owned process before returning even on malformed data,
        // failed observers, deadline or cancellation. Drop is not external proof.
        drop(tree);
        if outcome.is_err() {
            let _ = child.start_kill();
            let _ = timeout(Duration::from_secs(1), child.wait()).await;
        }
        outcome
    }
}

#[cfg(test)]
#[path = "python_stream_tests.rs"]
mod tests;
