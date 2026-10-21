//! EEF node library.
//!
//! The wire format is byte-compatible with the Python 0.1 implementation:
//! newline-framed JSON encrypted with AES-256-GCM and the same HKDF inputs.

pub mod api;
mod artifact_transfer;
#[path = "../../../shared/atomic_file.rs"]
pub mod atomic_file;
pub mod audio_submission;
pub mod authentication;
pub mod automation;
pub mod automation_occurrence;
pub mod automation_policy;
pub mod bundle_selection;
pub mod client;
pub mod connection_commands;
pub mod context;
pub mod credential_pairing;
pub mod crypto;
pub mod dashboard;
mod download_receipts;
pub mod engine;
pub mod enrollment;
pub mod execution_boundary;
pub mod firmware;
pub mod grant_time;
pub mod grants;
pub mod inference_budget;
mod installed_selection;
pub mod interpretation;
pub mod interpreter_sampling;
pub mod job_accounting;
pub mod job_commands;
pub mod local_commands;
pub mod local_enrollment;
pub mod local_setup;
pub mod model_activation;
pub mod model_bootstrap;
pub mod model_cli;
pub mod model_downloads;
pub mod model_lifecycle;
pub mod model_lifecycle_policy;
pub mod model_manager;
pub mod model_manifest;
pub mod model_memory;
pub mod model_metadata;
mod model_response;
pub mod model_selection;
pub mod model_server;
pub mod model_setup;
pub mod network;
pub mod package_repair;
pub mod peer;
pub mod permission_commands;
pub mod pipeline;
pub mod pipeline_progress;
mod pipeline_runtime;
mod process_lifetime;
pub mod protocol;
pub mod python;
pub mod python_policy;
pub mod registration;
pub mod relay_setup;
pub mod request_abandon;
pub mod request_drafts;
pub mod request_execution;
pub mod request_input;
pub mod request_limits;
pub mod request_planning;
pub mod request_preparation;
pub mod request_preview;
pub mod request_receipt;
pub mod request_recovery;
pub mod request_resolution;
pub mod request_response;
pub mod request_result;
pub mod request_stop_receipt;
pub mod request_submission;
pub mod runtime_layout;
pub mod server;
pub mod service;
mod session_output;
#[cfg(test)]
mod settings_storage_tests;
pub mod setup;
pub mod setup_check;
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
