//! Node-local connection controls. No browser, interpreter or second connection.
use clap::Subcommand;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, Subcommand)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConnectionCommand {
    Show {},
    /// Inspect/save request capacity; explicitly restart later to apply changes.
    Limits {
        #[arg(long)]
        work: Option<usize>,
        #[arg(long)]
        control: Option<usize>,
    },
    /// Disable the outgoing EEF connection; local command services remain available.
    Pause {},
    /// Enable connections using existing owner-configured endpoints and trust.
    Resume {},
    /// Explicitly switch to automatic same-user local EEF pairing.
    PairLocal {},
    /// Apply a PRIVATE exact-node invitation file; never changes feature permissions.
    Pair {
        #[arg(long)]
        code_file: std::path::PathBuf,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub command: ConnectionCommand,
}
