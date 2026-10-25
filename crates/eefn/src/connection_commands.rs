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
    /// Disable the outgoing eef connection; local command services remain available.
    Pause {},
    /// Enable connections using existing owner-configured endpoints and trust.
    Resume {},
    /// Explicitly switch to automatic same-user local eef pairing.
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

/// CLI input adapters are separate from the core command contract. In particular,
/// the service never reads its own stdin or accepts a secret command-line flag.
#[derive(Debug, Subcommand)]
pub enum ConnectionAction {
    /// Inspect saved connection settings and current runtime status.
    Show {},
    /// Inspect/save request capacity; restart explicitly to apply changes.
    Limits {
        #[arg(long)]
        work: Option<usize>,
        #[arg(long)]
        control: Option<usize>,
    },
    /// Disable outgoing connections while keeping local commands available.
    Pause {},
    /// Enable existing owner-configured connections and trust.
    Resume {},
    /// Explicitly use legacy same-user local pairing; not private provisioning.
    PairLocal {},
    /// Apply a private exact-node invitation from a protected file.
    Pair {
        #[arg(long)]
        code_file: std::path::PathBuf,
    },
    /// Read a PRIVATE invitation from piped stdin, never an argument or public file.
    PairStdin {},
}
impl ConnectionAction {
    pub fn command(&self) -> Option<ConnectionCommand> {
        Some(match self {
            Self::Show {} => ConnectionCommand::Show {},
            Self::Limits { work, control } => ConnectionCommand::Limits {
                work: *work,
                control: *control,
            },
            Self::Pause {} => ConnectionCommand::Pause {},
            Self::Resume {} => ConnectionCommand::Resume {},
            Self::PairLocal {} => ConnectionCommand::PairLocal {},
            Self::Pair { code_file } => ConnectionCommand::Pair {
                code_file: code_file.clone(),
            },
            Self::PairStdin {} => return None,
        })
    }
}
