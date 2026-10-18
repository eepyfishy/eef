//! Experimental peer-channel building blocks, NOT execution/session authority.
//! Includes library-only grant admission, not a runtime/command/listener or
//! registration integration. Paired jobs and application delivery remain open.
pub mod admission;
mod credentials;
pub mod plan;
pub mod scope;
mod transport;
pub use credentials::verify_envelope;
pub use credentials::{Recipient, SealedMaterial, SealedPair, open, seal_pair};
pub use transport::{Acknowledgement, PeerChannel, PeerEndpoint};
#[cfg(test)]
mod tests;
