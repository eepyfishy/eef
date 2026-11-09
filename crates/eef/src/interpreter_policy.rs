//! Local-owner policy commands. No scheduler, inference, provider selection or UI.
use crate::Runtime;
use anyhow::{Context, Result, ensure};
use eefn::interpreter_policy::{Command, Limits, Report, Request, Scope};
use serde_json::Value;

#[derive(Debug, clap::Subcommand)]
pub enum Action {
    Show {
        id: Option<String>,
    },
    List {
        #[arg(long, default_value_t = 25)]
        limit: u16,
        #[arg(long)]
        after_generation: Option<u64>,
    },
    /// Save explicit bounded service approval; starts no model call.
    Approve(Options),
    /// Revoke a policy while keeping its receipts and history.
    Revoke {
        id: String,
        #[arg(long)]
        operation_id: Option<String>,
    },
}
#[derive(Debug, clap::Args)]
pub struct Options {
    #[arg(long = "origin", required = true)]
    pub origin_nodes: Vec<String>,
    #[arg(long = "provider", required = true)]
    pub provider_nodes: Vec<String>,
    #[arg(long)]
    pub operation_id: Option<String>,
    /// Omit for time-unlimited approval with finite aggregate allowances.
    #[arg(long)]
    pub valid_for_seconds: Option<u64>,
    #[arg(long)]
    pub max_task_bytes: Option<usize>,
    #[arg(long)]
    pub max_output_tokens: Option<u64>,
    #[arg(long)]
    pub max_execution_ms: Option<u64>,
    #[arg(long)]
    pub max_result_bytes: Option<usize>,
    #[arg(long)]
    pub max_in_flight: Option<u16>,
    #[arg(long)]
    pub max_in_flight_per_origin: Option<u16>,
    #[arg(long)]
    pub max_in_flight_per_provider: Option<u16>,
    #[arg(long)]
    pub max_calls: Option<u32>,
    #[arg(long)]
    pub max_reserved_output_tokens: Option<u64>,
    #[arg(long)]
    pub max_reserved_execution_ms: Option<u64>,
    #[arg(long)]
    pub max_reserved_result_bytes: Option<u64>,
}
impl Options {
    pub fn command(&self, generation: u64) -> Result<Command> {
        let mut limits = Limits::default();
        if let Some(n) = self.max_task_bytes {
            limits.max_task_bytes = n;
        }
        if let Some(n) = self.max_output_tokens {
            limits.max_output_tokens = n;
        }
        if let Some(n) = self.max_execution_ms {
            limits.max_execution_ms = n;
        }
        if let Some(n) = self.max_result_bytes {
            limits.max_result_bytes = n;
        }
        if let Some(n) = self.max_in_flight {
            limits.max_in_flight = n;
        }
        if let Some(n) = self.max_in_flight_per_origin {
            limits.max_in_flight_per_origin = n;
        }
        if let Some(n) = self.max_in_flight_per_provider {
            limits.max_in_flight_per_provider = n;
        }
        if let Some(n) = self.max_calls {
            limits.max_calls = n;
        }
        limits.max_reserved_output_tokens = self.max_reserved_output_tokens.unwrap_or(
            u64::from(limits.max_calls)
                .checked_mul(limits.max_output_tokens)
                .context("Policy token allowance overflow")?,
        );
        limits.max_reserved_execution_ms = self.max_reserved_execution_ms.unwrap_or(
            u64::from(limits.max_calls)
                .checked_mul(limits.max_execution_ms)
                .context("Policy time allowance overflow")?,
        );
        limits.max_reserved_result_bytes = self.max_reserved_result_bytes.unwrap_or(
            u64::from(limits.max_calls)
                .checked_mul(limits.max_result_bytes as u64)
                .context("Policy result allowance overflow")?,
        );
        let command = Command::Approve {
            operation_id: self
                .operation_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            generation,
            scope: Scope {
                origin_nodes: self.origin_nodes.clone(),
                provider_nodes: self.provider_nodes.clone(),
            },
            limits,
            valid_for_ms: self
                .valid_for_seconds
                .map(|n| n.checked_mul(1000).context("Policy validity overflow"))
                .transpose()?,
        };
        command.validate()?;
        Ok(command)
    }
}

impl Runtime {
    pub fn interpreter_policy_command(&self, request: Request) -> Result<Value> {
        request.validate()?;
        ensure!(
            request.expected_runtime_id == self.instance_id,
            "Coordinator runtime changed before interpreter policy command"
        );
        let command = request.command;
        // Inspection/revocation and old receipt reuse need no current provider,
        // model or applied issuer; they never repair/renew authority on restart.
        let new_approval = if let Command::Approve { operation_id, .. } = &command {
            self.engine
                .store
                .interpreter_policy_receipt(operation_id)?
                .is_none()
        } else {
            false
        };
        if !new_approval {
            return self.engine.store.interpreter_policy_command(
                command,
                &self.instance_id,
                None,
                eefn::protocol::now_ms(),
            );
        }
        let applied = self.node_server.authentication_policy();
        // Hold the existing configuration fence through the durable checkpoint;
        // saved credential replacement cannot race approval under an old issuer.
        self.config.edit_for_restart(|saved| {
            let settings = eefn::authentication::AuthenticationPolicy::parse(
                saved.pointer("/node/authentication"),
            )?;
            let issuer = current_issuer(applied, &settings)?;
            let value = self.engine.store.interpreter_policy_command(
                command.clone(),
                &self.instance_id,
                issuer.as_deref(),
                eefn::protocol::now_ms(),
            )?;
            Ok((value, false))
        })
    }
}

fn current_issuer(
    applied: &eefn::authentication::AuthenticationPolicy,
    saved: &eefn::authentication::AuthenticationPolicy,
) -> Result<Option<String>> {
    if applied != saved {
        return Ok(None);
    }
    applied
        .tls
        .as_ref()
        .map(|identity| identity.fingerprint())
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_credential_or_legacy_changes_cannot_approve_under_an_unchanged_certificate() {
        use eefn::authentication::{AuthenticationPolicy, generate_credential};
        let applied = AuthenticationPolicy {
            allow_legacy: false,
            credentials: std::collections::BTreeMap::from([(
                "input-node".into(),
                Some(generate_credential()),
            )]),
            tls: Some(eefn::tls::CoordinatorIdentity::generate().unwrap()),
        };
        let fingerprint = applied.tls.as_ref().unwrap().fingerprint().unwrap();
        assert_eq!(
            current_issuer(&applied, &applied).unwrap(),
            Some(fingerprint.clone())
        );
        for edit in ["rotate", "revoke", "add", "remove", "legacy"] {
            let mut saved = applied.clone();
            match edit {
                "rotate" => {
                    saved
                        .credentials
                        .insert("input-node".into(), Some(generate_credential()));
                }
                "revoke" => {
                    saved.credentials.insert("input-node".into(), None);
                }
                "add" => {
                    saved
                        .credentials
                        .insert("other-node".into(), Some(generate_credential()));
                }
                "remove" => {
                    saved.credentials.remove("input-node");
                }
                "legacy" => saved.allow_legacy = true,
                _ => unreachable!(),
            }
            saved.validate().unwrap();
            assert_eq!(
                saved.tls.as_ref().unwrap().fingerprint().unwrap(),
                fingerprint
            );
            assert_eq!(
                current_issuer(&applied, &saved).unwrap(),
                None,
                "edit={edit}"
            );
        }
        assert_eq!(
            current_issuer(
                &AuthenticationPolicy::default(),
                &AuthenticationPolicy::default()
            )
            .unwrap(),
            None
        );
    }
}

pub fn render(value: &Value) -> Result<String> {
    let report: Report = serde_json::from_value(value.clone())?;
    ensure!(
        eefn::interpreter_policy::confirmed(value, &report.command, &report.runtime_id),
        "Unconfirmed interpreter policy report"
    );
    let mut text = String::new();
    if let Some(record) = report.authorization {
        let Command::Approve { scope, limits, .. } = &record.creation else {
            unreachable!()
        };
        text.push_str(&format!(
            "Interpreter policy {}: generation {}, {}.\n",
            record.id,
            record.generation,
            if record.revocation.is_some() {
                "revoked"
            } else if record.generation != report.current_generation {
                "superseded"
            } else {
                "approval retained"
            }
        ));
        text.push_str(&format!(
            "Scope: {} input nodes; {} serving nodes. Calls: at most {}.\n",
            scope.origin_nodes.len(),
            scope.provider_nodes.len(),
            limits.max_calls
        ));
    } else if matches!(report.command, Command::List { .. }) {
        for item in report.items {
            text.push_str(&format!(
                "{}  generation {}  {}\n",
                item.id,
                item.generation,
                if item.revoked { "revoked" } else { "retained" }
            ));
        }
        if let Some(next) = report.next_after {
            text.push_str(&format!("More history: --after-generation {next}\n"));
        }
    } else {
        text.push_str("Interpreter sharing has no owner policy.\n");
    }
    text.push_str("Policy records only: no grant or model call was started. Automatic network interpretation is not available yet.\n");
    Ok(text)
}
