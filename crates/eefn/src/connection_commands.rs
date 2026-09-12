//! Node-local connection controls. No browser, interpreter or second connection.
use clap::Subcommand;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, Subcommand)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConnectionCommand {
    Show {},
    /// Disable the outgoing EEF connection; local command services remain available.
    Pause {},
    /// Enable connections using existing owner-configured endpoints and trust.
    Resume {},
    /// Explicitly switch to automatic same-user local EEF pairing.
    PairLocal {},
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub command: ConnectionCommand,
}
