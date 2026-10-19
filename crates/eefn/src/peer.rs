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
#[cfg(test)]
mod tests;
