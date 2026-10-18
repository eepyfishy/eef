//! Experimental peer-channel building blocks, NOT execution/session authority.
//! No runtime, command, listener, permission, registration or grant integration.
//! A future caller must use existing approved jobs and consumed execution grants.
mod credentials;
pub mod scope;
mod transport;
pub use credentials::{Recipient, SealedMaterial, SealedPair, open, seal_pair};
pub use transport::{Acknowledgement, PeerChannel, PeerEndpoint};
#[cfg(test)]
mod tests;
