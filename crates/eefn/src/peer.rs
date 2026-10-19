//! Experimental peer-channel building blocks, NOT execution/session authority.
//! Includes consumed finite admission and opt-in NodeClient delivery bindings.
//! Native app startup does not enable these or supply media/file producers.
pub mod admission;
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
