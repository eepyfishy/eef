//! Explicit owner policy for a bounded interpreter service, never a tool approval.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

pub const MAX_RECORD_BYTES: usize = 16 * 1024;
pub const REVOCATION_RESERVE: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub origin_nodes: Vec<String>,
    pub provider_nodes: Vec<String>,
}
impl Scope {
    pub fn validate(&self) -> Result<()> {
        for nodes in [&self.origin_nodes, &self.provider_nodes] {
            ensure!(
                (1..=128).contains(&nodes.len()),
                "Explicit origin/provider node lists are required"
            );
            let mut seen = HashSet::new();
            for node in nodes {
                crate::network::validate_remote_node_id(node)?;
                ensure!(seen.insert(node), "Duplicate interpreter policy node");
            }
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= 8192,
            "Interpreter policy scope exceeds its budget"
        );
        Ok(())
    }
    pub fn permits(
        &self,
        origin: &str,
        provider: &str,
        origin_private: bool,
        provider_private: bool,
    ) -> Result<()> {
        self.validate()?;
        ensure!(
            origin_private && provider_private,
            "Interpreter service requires independently authenticated nodes"
        );
        ensure!(
            self.origin_nodes.iter().any(|n| n == origin)
                && self.provider_nodes.iter().any(|n| n == provider),
            "Interpreter service origin/provider is outside policy"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub max_task_bytes: usize,
    pub max_output_tokens: u64,
    pub max_execution_ms: u64,
    pub max_result_bytes: usize,
    pub max_in_flight: u16,
    pub max_in_flight_per_origin: u16,
    pub max_in_flight_per_provider: u16,
    pub max_calls: u32,
    pub max_reserved_output_tokens: u64,
    pub max_reserved_execution_ms: u64,
    pub max_reserved_result_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_task_bytes: 48 * 1024,
            max_output_tokens: 1024,
            max_execution_ms: 30_000,
            max_result_bytes: 64 * 1024,
            max_in_flight: 2,
            max_in_flight_per_origin: 1,
            max_in_flight_per_provider: 1,
            max_calls: 100,
            max_reserved_output_tokens: 102_400,
            max_reserved_execution_ms: 3_000_000,
            max_reserved_result_bytes: 6_553_600,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=64 * 1024).contains(&self.max_task_bytes)
                && (1..=crate::request_preparation::MAX_OUTPUT_TOKENS)
                    .contains(&self.max_output_tokens)
                && (1..=crate::grants::MAX_DURATION_MS).contains(&self.max_execution_ms)
                && (1024..=1024 * 1024).contains(&self.max_result_bytes)
                && (1..=64).contains(&self.max_in_flight)
                && (1..=self.max_in_flight).contains(&self.max_in_flight_per_origin)
                && (1..=self.max_in_flight).contains(&self.max_in_flight_per_provider)
                && (1..=1_000_000).contains(&self.max_calls)
                && (self.max_output_tokens..=1_024_000_000)
                    .contains(&self.max_reserved_output_tokens)
                && (self.max_execution_ms..=86_400_000_000)
                    .contains(&self.max_reserved_execution_ms)
                && (self.max_result_bytes as u64..=128 * 1024 * 1024 * 1024)
                    .contains(&self.max_reserved_result_bytes),
            "Invalid interpreter per-call, concurrency or aggregate limits"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Show {
        id: Option<String>,
    },
    List {
        limit: u16,
        after_generation: Option<u64>,
    },
    Approve {
        operation_id: String,
        generation: u64,
        scope: Scope,
        limits: Limits,
        /// None is time-unlimited but still bounded by aggregate allowances.
        valid_for_ms: Option<u64>,
    },
    Revoke {
        id: String,
        revision: u64,
        operation_id: String,
    },
}
pub fn uuid(value: &str) -> Result<()> {
    crate::automation_occurrence::uuid(value)?;
    ensure!(
        !uuid::Uuid::parse_str(value)?.is_nil(),
        "Nil interpreter policy identity"
    );
    Ok(())
}
impl Command {
    pub fn receipt(&self) -> Option<&str> {
        match self {
            Self::Approve { operation_id, .. } | Self::Revoke { operation_id, .. } => {
                Some(operation_id)
            }
            _ => None,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if let Some(receipt) = self.receipt() {
            uuid(receipt)?;
        }
        match self {
            Self::Show { id } => {
                if let Some(id) = id {
                    uuid(id)?;
                }
            }
            Self::List {
                limit,
                after_generation,
            } => ensure!(
                (1..=100).contains(limit) && after_generation.is_none_or(|n| n <= i64::MAX as u64),
                "Invalid interpreter policy page"
            ),
            Self::Approve {
                generation,
                scope,
                limits,
                valid_for_ms,
                ..
            } => {
                ensure!(
                    *generation <= i64::MAX as u64,
                    "Invalid interpreter policy generation"
                );
                scope.validate()?;
                limits.validate()?;
                ensure!(
                    valid_for_ms.is_none_or(|n| (1000..=31_536_000_000).contains(&n)),
                    "Invalid interpreter policy validity"
                );
            }
            Self::Revoke { id, revision, .. } => {
                uuid(id)?;
                ensure!(
                    (1..=2).contains(revision),
                    "Invalid interpreter policy revision"
                );
            }
        }
        Ok(())
    }
    /// Reusing a receipt is read-only even after another generation was saved.
    pub fn same_approval(&self, other: &Self) -> bool {
        matches!((self,other),(Self::Approve{operation_id:a,scope:b,limits:c,valid_for_ms:d,..},
            Self::Approve{operation_id:x,scope:y,limits:z,valid_for_ms:w,..}) if (a,b,c,d)==(x,y,z,w))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revocation {
    pub operation_id: String,
    pub source_revision: u64,
    pub runtime_id: String,
    pub at_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authorization {
    pub schema_version: u32,
    pub id: String,
    pub generation: u64,
    pub creation: Command,
    pub runtime_id: String,
    pub issuer_id: String,
    pub approved_at_ms: u64,
    pub expires_at_ms: Option<u64>,
    pub authorization_sha256: String,
    pub revision: u64,
    pub revocation: Option<Revocation>,
}
impl Authorization {
    pub fn digest(&self) -> Result<String> {
        let mut value = serde_json::to_value(self)?;
        let object = value
            .as_object_mut()
            .context("Invalid interpreter policy record")?;
        for name in ["authorization_sha256", "revision", "revocation"] {
            object.remove(name);
        }
        crate::grants::definition_digest(
            &json!({"domain":"eef.interpreter.policy.v1","authorization":value}),
        )
    }
    pub fn validate(&self) -> Result<()> {
        uuid(&self.id)?;
        uuid(&self.runtime_id)?;
        crate::request_interpretation::digest(&self.issuer_id)?;
        self.creation.validate()?;
        let Command::Approve {
            operation_id,
            generation,
            valid_for_ms,
            ..
        } = &self.creation
        else {
            anyhow::bail!("Interpreter policy has no approval command")
        };
        let expiry = valid_for_ms
            .map(|n| {
                self.approved_at_ms
                    .checked_add(n)
                    .context("Interpreter policy time overflow")
            })
            .transpose()?;
        ensure!(
            self.schema_version == 1
                && self.approved_at_ms > 0
                && self.id == *operation_id
                && self.generation
                    == generation
                        .checked_add(1)
                        .context("Policy generation overflow")?
                && self.expires_at_ms == expiry
                && self.authorization_sha256 == self.digest()?
                && self.revision == (if self.revocation.is_some() { 2 } else { 1 })
                && serde_json::to_vec(self)?.len() <= MAX_RECORD_BYTES,
            "Invalid retained interpreter policy"
        );
        if let Some(revocation) = &self.revocation {
            uuid(&revocation.operation_id)?;
            uuid(&revocation.runtime_id)?;
            ensure!(
                revocation.operation_id != self.id
                    && revocation.source_revision == 1
                    && revocation.at_ms > 0,
                "Invalid interpreter policy revocation"
            );
        }
        Ok(())
    }
    pub fn active(&self, generation: u64, issuer: &str, now: u64) -> bool {
        self.validate().is_ok()
            && self.generation == generation
            && self.issuer_id == issuer
            && self.revocation.is_none()
            && now >= self.approved_at_ms
            && self.expires_at_ms.is_none_or(|end| now < end)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub command: Command,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "Unsupported interpreter policy schema"
        );
        uuid(&self.expected_runtime_id)?;
        self.command.validate()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub id: String,
    pub generation: u64,
    pub revision: u64,
    pub revoked: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub runtime_id: String,
    pub command: Command,
    pub current_generation: u64,
    pub authorization: Option<Authorization>,
    pub items: Vec<Summary>,
    pub next_after: Option<u64>,
    pub reused: bool,
    pub changed: bool,
    pub grants_issued: bool,
    pub model_inference_requested: bool,
    pub automatically_retried: bool,
    pub allowance_reservation_available: bool,
    pub automatic_delegation_available: bool,
}
pub fn confirmed(value: &Value, command: &Command, runtime: &str) -> bool {
    let Ok(report) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    if command.validate().is_err()
        || uuid(runtime).is_err()
        || report.schema_version != 1
        || report.report_type != "coordinator_interpreter_policy"
        || !report.success
        || report.runtime_id != runtime
        || report.command != *command
        || report.current_generation > i64::MAX as u64
        || report.grants_issued
        || report.model_inference_requested
        || report.automatically_retried
        || report.allowance_reservation_available
        || report.automatic_delegation_available
        || report.reused && report.changed
    {
        return false;
    }
    if let Some(record) = &report.authorization {
        if record.validate().is_err() || record.generation > report.current_generation {
            return false;
        }
    }
    match command {
        Command::List {
            limit,
            after_generation,
        } => {
            let mut previous = after_generation.unwrap_or(0);
            if report.authorization.is_some()
                || report.changed
                || report.reused
                || report.items.len() > *limit as usize
            {
                return false;
            }
            for item in &report.items {
                if uuid(&item.id).is_err()
                    || item.generation <= previous
                    || item.generation > report.current_generation
                    || item.revision != (if item.revoked { 2 } else { 1 })
                {
                    return false;
                }
                previous = item.generation;
            }
            report
                .next_after
                .is_none_or(|next| report.items.len() == *limit as usize && previous == next)
        }
        Command::Show { id } => {
            !report.changed
                && !report.reused
                && report.items.is_empty()
                && report.next_after.is_none()
                && match (&report.authorization, id) {
                    (Some(record), Some(id)) => record.id == *id,
                    (Some(record), None) => record.generation == report.current_generation,
                    (None, None) => report.current_generation == 0,
                    _ => false,
                }
        }
        Command::Approve { generation, .. } => {
            report.items.is_empty()
                && report.next_after.is_none()
                && report.authorization.as_ref().is_some_and(|record| {
                    record.creation.same_approval(command)
                        && (report.reused
                            || record.creation == *command
                                && record.runtime_id == runtime
                                && record.generation == generation.saturating_add(1)
                                && record.generation == report.current_generation
                                && record.revocation.is_none())
                })
                && report.changed != report.reused
        }
        Command::Revoke {
            id,
            revision,
            operation_id,
        } => {
            report.items.is_empty()
                && report.next_after.is_none()
                && report.authorization.as_ref().is_some_and(|record| {
                    record.id == *id
                        && record.revocation.as_ref().is_some_and(|revocation| {
                            report.reused
                                || revocation.operation_id == *operation_id
                                    && revocation.source_revision == *revision
                                    && revocation.runtime_id == runtime
                        })
                })
                && report.changed != report.reused
        }
    }
}

#[cfg(test)]
#[path = "interpreter_policy_tests.rs"]
mod tests;
