//! EEF node library.
//!
//! The wire format is byte-compatible with the Python 0.1 implementation:
//! newline-framed JSON encrypted with AES-256-GCM and the same HKDF inputs.

pub mod api;
pub mod client;
pub mod connection_commands;
pub mod context;
pub mod crypto;
pub mod dashboard;
pub mod engine;
pub mod firmware;
pub mod interpretation;
pub mod job_commands;
pub mod local_commands;
pub mod model_cli;
pub mod model_downloads;
pub mod model_manager;
pub mod model_manifest;
pub mod model_metadata;
pub mod model_selection;
pub mod model_server;
pub mod network;
pub mod protocol;
pub mod python;
pub mod server;
pub mod service;
pub mod setup;
pub mod startup;
pub mod submission;
pub mod updater;

pub use client::{
    CoordinatorEndpoint, NodeClient, NodeClientConfig, SelectedModel, current_load,
    discover_models, installed_ollama_models, system_specs,
};
pub use crypto::{CryptoError, NodeCrypto, derive_key};
pub use dashboard::NodeDashboard;
pub use engine::{CAPABILITIES, NodeEngine, NodePolicy};
pub use model_server::{ModelServer, ModelSlot};
pub use protocol::PROTOCOL_VERSION;
pub use python::{PythonPlugin, PythonRuntime};
pub use server::{DEFAULT_PORT, NodeEvent, NodeOfflineError, NodeServer};
pub use service::NodeService;
pub use startup::{StartupStatus, set_startup, startup_status};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
