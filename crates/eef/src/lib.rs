//! Native EEF coordinator.

pub mod adapters;
pub mod api;
pub mod assistant;
pub mod brain;
pub mod capability;
pub mod config;
pub mod event;
pub mod firmware;
pub mod jobs;
pub mod memory;
pub mod model;
pub mod runtime;
pub mod task;
pub mod update;
pub mod world;

pub use runtime::Runtime;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
