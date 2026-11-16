use super::*;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

fn required_option<'de, D, T>(de: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(de)
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Frame {
    Ready {
        schema_version: u32,
        protocol: String,
        stream_id: String,
    },
    Observation {
        stream_id: String,
        sequence: u64,
        source_frames: u64,
        #[serde(deserialize_with = "required_option")]
        active: Option<bool>,
        #[serde(deserialize_with = "required_option")]
        level: Option<u16>,
        discontinuity: bool,
        analysis_state: AnalysisState,
        #[serde(deserialize_with = "required_option")]
        image_base64: Option<String>,
    },
    End {
        stream_id: String,
        observations: u64,
        source_frames: u64,
        images: u64,
    },
}

pub(super) struct State<'a> {
    id: &'a str,
    bounds: &'a VisionBounds,
    expected: u64,
    ready: bool,
    ended: bool,
    observations: u64,
    images: u64,
}
impl<'a> State<'a> {
    pub(super) fn new(id: &'a str, bounds: &'a VisionBounds, expected: u64) -> Self {
        Self {
            id,
            bounds,
            expected,
            ready: false,
            ended: false,
            observations: 0,
            images: 0,
        }
    }
    pub(super) fn accept_json(&mut self, raw: &[u8]) -> Result<Option<VisionObservation>> {
        let frame: Frame = parse(raw)?;
        if self.ended {
            return Err(BridgeFailure::Reply.into());
        }
        match frame {
            Frame::Ready {
                schema_version,
                protocol,
                stream_id,
            } => {
                if self.ready
                    || schema_version != 1
                    || protocol != "camera_vision_v1"
                    || stream_id != self.id
                {
                    return Err(BridgeFailure::Reply.into());
                }
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
                analysis_state,
                image_base64,
            } => {
                if !self.ready
                    || stream_id != self.id
                    || sequence != self.observations + 1
                    || sequence > self.bounds.max_observations
                    || sequence > self.expected
                    || source_frames != sequence
                    || source_frames > self.bounds.max_source_frames
                    || level.is_some_and(|n| n > 255)
                    || active.is_some() && level.is_none()
                    || discontinuity && (active.is_some() || level.is_some())
                    || sequence == 1 && (active.is_some() || level.is_some() || discontinuity)
                    || analysis_state != AnalysisState::NotRequested
                        && (active != Some(true) || discontinuity)
                {
                    return Err(BridgeFailure::Reply.into());
                }
                let image = match (analysis_state, image_base64) {
                    (AnalysisState::Frame, Some(data)) => {
                        if self.images >= self.bounds.max_images {
                            return Err(BridgeFailure::Reply.into());
                        }
                        let image = PrivateJpeg::checked(data, self.bounds.max_image_bytes)?;
                        self.images += 1;
                        Some(image)
                    }
                    (AnalysisState::Frame, None) | (_, Some(_)) => {
                        return Err(BridgeFailure::Reply.into());
                    }
                    (_, None) => None,
                };
                self.observations = sequence;
                Ok(Some(VisionObservation {
                    motion: StreamObservation {
                        sequence,
                        source_frames,
                        active,
                        level,
                        discontinuity,
                    },
                    analysis: analysis_state,
                    image,
                }))
            }
            Frame::End {
                stream_id,
                observations,
                source_frames,
                images,
            } => {
                if !self.ready
                    || stream_id != self.id
                    || observations == 0
                    || observations != self.expected
                    || observations != self.observations
                    || source_frames != observations
                    || images != self.images
                {
                    return Err(BridgeFailure::Reply.into());
                }
                self.ended = true;
                Ok(None)
            }
        }
    }
    pub(super) fn summary(self) -> Result<VisionSummary> {
        if !self.ended {
            return Err(BridgeFailure::Reply.into());
        }
        Ok(VisionSummary {
            stream: StreamSummary {
                stream_id: self.id.into(),
                observations: self.observations,
                source_frames: self.observations,
            },
            images: self.images,
        })
    }
}
fn parse<T: DeserializeOwned>(raw: &[u8]) -> Result<T> {
    serde_json::from_slice(raw).map_err(|_| BridgeFailure::Reply.into())
}
async fn control<W: AsyncWrite + Unpin>(writer: &mut W, value: Value) -> Result<()> {
    let mut bytes = super::super::encode_request(&value, 511)?;
    bytes.push(b'\n');
    writer
        .write_all(&bytes)
        .await
        .context("Private vision control pipe failed")?;
    writer
        .flush()
        .await
        .context("Private vision control pipe failed")?;
    Ok(())
}

pub(super) async fn exchange<R, W, F, C>(
    mut stdin: W,
    stdout: R,
    input: Vec<u8>,
    id: &str,
    bounds: &VisionBounds,
    expected: u64,
    mut observe: F,
) -> Result<VisionSummary>
where
    R: AsyncRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send,
    F: FnMut(VisionObservation) -> C + Send,
    C: Future<Output = Result<()>> + Send,
{
    stdin
        .write_all(&input)
        .await
        .context("Private vision request pipe failed")?;
    stdin
        .flush()
        .await
        .context("Private vision request pipe failed")?;
    let mut reader = BufReader::new(stdout);
    let mut state = State::new(id, bounds, expected);
    let mut total = 0usize;
    let mut started = false;
    loop {
        let mut line = Vec::new();
        let n = (&mut reader)
            .take(bounds.max_frame_bytes as u64 + 1)
            .read_until(b'\n', &mut line)
            .await
            .context("Private vision reply pipe failed")?;
        if n == 0 {
            return state.summary();
        }
        total = total
            .checked_add(n)
            .filter(|n| *n <= bounds.max_stdout_bytes)
            .ok_or(BridgeFailure::StdoutLimit)?;
        if n > bounds.max_frame_bytes {
            return Err(BridgeFailure::StdoutLimit.into());
        }
        if line.last() != Some(&b'\n') {
            return Err(BridgeFailure::Reply.into());
        }
        if let Some(observation) = state.accept_json(&line)? {
            let sequence = observation.motion.sequence;
            observe(observation)
                .await
                .map_err(|_| BridgeFailure::Reply)?;
            control(
                &mut stdin,
                json!({"type":"ack","schema_version":1,"stream_id":id,"sequence":sequence}),
            )
            .await?;
        } else if state.ready && !started {
            control(
                &mut stdin,
                json!({"type":"start","schema_version":1,"stream_id":id}),
            )
            .await?;
            started = true;
        }
    }
}
