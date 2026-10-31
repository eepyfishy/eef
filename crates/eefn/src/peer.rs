//! Experimental peer-channel building blocks, NOT execution/session authority.
//! Includes consumed finite admission and opt-in NodeClient delivery bindings.
//! Native startup can opt into finite audio with local permission/resources.
//! No continuous stream, virtual device or file producer is supplied.
pub mod admission;
pub mod audio;
mod audio_runtime;
mod credentials;
pub mod plan;
pub mod runtime;
pub mod scope;
mod transport;
pub use credentials::verify_envelope;
pub use credentials::{Recipient, SealedMaterial, SealedPair, open, seal_pair};
pub use transport::{Acknowledgement, PeerChannel, PeerEndpoint};

/// Closed failure context for a consumed operation. It conveys no retry,
/// rollback or physical termination guarantee and contains no private payload.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Failure {
    Listen,
    Connect,
    Readiness,
    Channel,
    Capture,
    CaptureFormat,
    Transfer,
    Manifest,
    Payload,
    Playback,
}
impl Failure {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Listen => "peer_listener_unavailable",
            Self::Connect => "peer_connection_unavailable",
            Self::Readiness => "peer_readiness_unavailable",
            Self::Channel => "peer_channel_unavailable",
            Self::Capture => "peer_audio_capture_failed",
            Self::CaptureFormat => "peer_audio_capture_invalid",
            Self::Transfer => "peer_transfer_failed",
            Self::Manifest => "peer_audio_manifest_invalid",
            Self::Payload => "peer_payload_invalid",
            Self::Playback => "peer_audio_playback_failed",
        }
    }
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for Failure {}
#[cfg(test)]
mod tests;
