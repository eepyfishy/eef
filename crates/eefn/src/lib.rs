//! EEF node library.
//!
//! The wire format is byte-compatible with the Python 0.1 implementation:
//! newline-framed JSON encrypted with AES-256-GCM and the same HKDF inputs.

pub mod api;
pub mod authentication;
pub mod client;
pub mod connection_commands;
pub mod context;
pub mod crypto;
pub mod dashboard;
pub mod engine;
pub mod execution_boundary;
pub mod firmware;
pub mod grant_time;
pub mod grants;
pub mod interpretation;
pub mod job_commands;
pub mod local_commands;
pub mod model_cli;
pub mod model_downloads;
pub mod model_manager;
pub mod model_manifest;
pub mod model_metadata;
mod model_response;
pub mod model_selection;
pub mod model_server;
pub mod network;
pub mod protocol;
pub mod python;
pub mod request_abandon;
pub mod request_drafts;
pub mod request_execution;
pub mod request_limits;
pub mod request_planning;
pub mod request_preparation;
pub mod request_preview;
pub mod request_receipt;
pub mod request_recovery;
pub mod request_resolution;
pub mod request_result;
pub mod request_submission;
pub mod server;
pub mod service;
pub mod setup;
pub mod startup;
pub mod submission;
pub mod tls;
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
