//! Finite PCM/WAV capture -> transfer -> playback contract, not ambient capture,
//! continuous streaming, a virtual audio device or physical-effect confirmation.
use super::scope::Role;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[cfg(test)]
#[path = "audio_tests.rs"]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Audio {
    pub duration_ms: u32,
    pub sample_rate: u32,
    pub channels: u8,
    pub source_resource_id: String,
    pub destination_resource_id: String,
}
impl Audio {
    pub fn validate(&self) -> Result<()> {
        if !(100..=10_000).contains(&self.duration_ms)
            || ![8000, 16000, 24000, 32000, 44100, 48000].contains(&self.sample_rate)
            || !(1..=2).contains(&self.channels)
        {
            bail!("Invalid finite audio format/duration")
        }
        Ok(())
    }
    pub fn maximum_bytes(&self) -> u64 {
        // One frame of rounding slack for the existing seconds-based capture
        // adapter, plus at most 1 KiB of bounded RIFF headers/metadata.
        (u64::from(self.duration_ms) * u64::from(self.sample_rate) / 1000 + 1)
            * u64::from(self.channels)
            * 2
            + 1024
    }
    pub fn minimum_execution_ms(&self) -> u64 {
        u64::from(self.duration_ms) * 2 + 1000
    }
    pub fn minimum_bytes(&self) -> u64 {
        (u64::from(self.duration_ms) * u64::from(self.sample_rate) / 1000).saturating_sub(1)
            * u64::from(self.channels)
            * 2
            + 44
    }
    pub fn resource(&self, role: Role) -> &str {
        if role == Role::Source {
            &self.source_resource_id
        } else {
            &self.destination_resource_id
        }
    }
    pub(crate) fn operation(&self, role: Role) -> (&'static str, &'static str, Value) {
        if role == Role::Source {
            (
                "audio.capture",
                "record",
                json!({"resource_id":self.source_resource_id,
                "duration_seconds":f64::from(self.duration_ms)/1000.0,
                "sample_rate":self.sample_rate,"channels":self.channels}),
            )
        } else {
            (
                "audio.play",
                "play",
                json!({"resource_id":self.destination_resource_id}),
            )
        }
    }
    pub(crate) fn validate_nodes(&self, source: &str, destination: &str) -> Result<()> {
        self.validate()?;
        for (node, role) in [(source, Role::Source), (destination, Role::Destination)] {
            let (cap, action, params) = self.operation(role);
            crate::grants::OperationShape {
                capability: cap,
                action,
                params: &params,
                resource_id: Some(self.resource(role)),
                model_id: None,
                backend: None,
                model_role: None,
            }
            .validate(Some(node))?;
        }
        Ok(())
    }
    /// Validate an actual bounded PCM16 WAV, not source-supplied metadata alone.
    /// No decoder, device effect, recording, log payload or retained file.
    pub(crate) fn validate_wav(&self, bytes: &[u8]) -> Result<()> {
        self.validate()?;
        if bytes.len() < 44
            || bytes.len() as u64 > self.maximum_bytes()
            || &bytes[..4] != b"RIFF"
            || &bytes[8..12] != b"WAVE"
            || u64::from(u32::from_le_bytes(bytes[4..8].try_into()?)) + 8 != bytes.len() as u64
        {
            bail!("Audio does not match the bounded WAV contract")
        }
        let (mut offset, mut format, mut data) = (12usize, false, None);
        while offset < bytes.len() {
            let header = bytes
                .get(offset..offset.checked_add(8).context("WAV chunk overflow")?)
                .context("Truncated WAV header")?;
            let n = u32::from_le_bytes(header[4..8].try_into()?) as usize;
            let start = offset + 8;
            let end = start
                .checked_add(n)
                .filter(|end| *end <= bytes.len())
                .context("Truncated WAV chunk")?;
            match &header[..4] {
                b"fmt " => {
                    let chunk = bytes
                        .get(start..start.checked_add(16).context("WAV format overflow")?)
                        .context("Truncated PCM format")?;
                    if format
                        || n < 16
                        || u16::from_le_bytes(chunk[0..2].try_into()?) != 1
                        || u16::from_le_bytes(chunk[2..4].try_into()?) != u16::from(self.channels)
                        || u32::from_le_bytes(chunk[4..8].try_into()?) != self.sample_rate
                        || u32::from_le_bytes(chunk[8..12].try_into()?)
                            != self.sample_rate * u32::from(self.channels) * 2
                        || u16::from_le_bytes(chunk[12..14].try_into()?)
                            != u16::from(self.channels) * 2
                        || u16::from_le_bytes(chunk[14..16].try_into()?) != 16
                    {
                        bail!("Audio PCM format differs from approved capture")
                    }
                    format = true;
                }
                b"data" => {
                    if data.replace(n).is_some() {
                        bail!("Ambiguous WAV data")
                    }
                }
                _ => {}
            }
            offset = end
                .checked_add(n % 2)
                .filter(|end| *end <= bytes.len())
                .context("Truncated WAV padding")?;
            if offset.saturating_sub(data.unwrap_or(0)) > 1024 {
                bail!("WAV metadata exceeds its bound")
            }
        }
        let data = data.context("WAV data unavailable")? as u64;
        let frame = u64::from(self.channels) * 2;
        let expected = u64::from(self.duration_ms) * u64::from(self.sample_rate) / 1000;
        if !format
            || data % frame != 0
            || (data / frame).abs_diff(expected) > 1
            || bytes.len() as u64 - data > 1024
        {
            bail!("Audio frames differ from approved finite duration")
        }
        Ok(())
    }
    /// Do not relay incidental WAV metadata or hand arbitrary RIFF chunk kinds
    /// to the sink decoder. Only canonical PCM16 headers and approved frames.
    pub(crate) fn canonicalize(&self, bytes: Vec<u8>) -> Result<Vec<u8>> {
        self.validate_wav(&bytes)?;
        if self.validate_transport_wav(&bytes).is_ok() {
            return Ok(bytes);
        }
        let mut offset = 12;
        while offset < bytes.len() {
            let n = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into()?) as usize;
            if &bytes[offset..offset + 4] == b"data" {
                let pcm = &bytes[offset + 8..offset + 8 + n];
                let mut out = Vec::with_capacity(44 + pcm.len());
                out.extend_from_slice(b"RIFF");
                out.extend_from_slice(&(pcm.len() as u32 + 36).to_le_bytes());
                out.extend_from_slice(b"WAVEfmt ");
                out.extend_from_slice(&16u32.to_le_bytes());
                out.extend_from_slice(&1u16.to_le_bytes());
                out.extend_from_slice(&u16::from(self.channels).to_le_bytes());
                out.extend_from_slice(&self.sample_rate.to_le_bytes());
                out.extend_from_slice(
                    &(self.sample_rate * u32::from(self.channels) * 2).to_le_bytes(),
                );
                out.extend_from_slice(&(u16::from(self.channels) * 2).to_le_bytes());
                out.extend_from_slice(&16u16.to_le_bytes());
                out.extend_from_slice(b"data");
                out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
                out.extend_from_slice(pcm);
                return Ok(out);
            }
            offset += 8 + n + n % 2;
        }
        bail!("WAV data unavailable")
    }
    pub(crate) fn validate_transport_wav(&self, bytes: &[u8]) -> Result<()> {
        self.validate_wav(bytes)?;
        if &bytes[12..16] != b"fmt "
            || u32::from_le_bytes(bytes[16..20].try_into()?) != 16
            || &bytes[36..40] != b"data"
            || u32::from_le_bytes(bytes[40..44].try_into()?) as usize != bytes.len() - 44
        {
            bail!("Peer audio must use canonical PCM16 WAV")
        }
        Ok(())
    }
}
