//! Owner-only bounded reusable-approval contracts. Saving is not occurrence admission.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MAX_RESERVED_EXECUTION_MS: u64 = 86_400_000;
pub const MAX_RESERVED_RESULT_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, clap::Subcommand)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum PolicyCommand {
    /// Inspect private retained policies. Never activate or renew them.
    List {
        #[arg(long)]
        definition: Option<String>,
        #[arg(long, default_value_t = 25)]
        limit: u16,
        #[arg(long)]
        after: Option<String>,
    },
    Show {
        id: String,
    },
    /// Record bounded owner approval of one exact disabled definition.
    Approve {
        #[arg(long)]
        definition: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        definition_sha256: String,
        /// Last retained generation for this definition (zero before its first approval).
        #[arg(long)]
        generation: u64,
        #[arg(long)]
        operation_id: String,
        #[arg(long)]
        valid_for_ms: u64,
        #[arg(long)]
        max_runs: u16,
        #[arg(long)]
        max_execution_ms_per_task: u64,
        #[arg(long)]
        max_result_bytes_per_task: usize,
        /// Aggregate worst-case granted duration, not a measured CPU/RAM quota.
        #[arg(long)]
        max_reserved_execution_ms: u64,
        /// Aggregate worst-case result allowances, not physical disk reservation.
        #[arg(long)]
        max_reserved_result_bytes: u64,
    },
    /// Revoke without erasing history or claiming remote effects stopped.
    Revoke {
        id: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        operation_id: String,
    },
}
impl PolicyCommand {
    pub fn receipt(&self) -> Option<&str> {
        match self {
            Self::Approve { operation_id, .. } | Self::Revoke { operation_id, .. } => {
                Some(operation_id)
            }
            _ => None,
        }
    }
    pub fn limits(&self) -> Option<crate::grants::GrantLimits> {
        if let Self::Approve {
            max_execution_ms_per_task,
            max_result_bytes_per_task,
            ..
        } = self
        {
            Some(crate::grants::GrantLimits {
                max_execution_ms: *max_execution_ms_per_task,
                max_result_bytes: *max_result_bytes_per_task,
            })
        } else {
            None
        }
    }
    pub fn validate(&self) -> Result<()> {
        if let Some(id) = self.receipt() {
            crate::job_commands::validate_operation_id(id)?;
            if uuid::Uuid::parse_str(id)?.is_nil() {
                bail!("Policy receipts cannot be nil");
            }
        }
        match self {
            Self::List {
                definition,
                limit,
                after,
            } => {
                if !(1..=100).contains(limit) {
                    bail!("Policy page limit must be 1-100");
                }
                for id in definition.iter().chain(after) {
                    crate::job_commands::validate_operation_id(id)?;
                }
            }
            Self::Show { id } | Self::Revoke { id, .. } => {
                crate::job_commands::validate_operation_id(id)?;
            }
            Self::Approve {
                definition,
                revision,
                definition_sha256,
                generation,
                valid_for_ms,
                max_runs,
                max_execution_ms_per_task,
                max_result_bytes_per_task,
                max_reserved_execution_ms,
                max_reserved_result_bytes,
                ..
            } => {
                crate::job_commands::validate_operation_id(definition)?;
                if *revision != 1
                    || *generation == u64::MAX
                    || !crate::request_result::valid_digest(definition_sha256)
                    || !(1_000..=86_400_000).contains(valid_for_ms)
                    || !(1..=128).contains(max_runs)
                    || !(1..=crate::grants::MAX_DURATION_MS).contains(max_execution_ms_per_task)
                    || !(1024..=1024 * 1024).contains(max_result_bytes_per_task)
                    || valid_for_ms < max_execution_ms_per_task
                    || !(1..=MAX_RESERVED_EXECUTION_MS).contains(max_reserved_execution_ms)
                    || !(1024..=MAX_RESERVED_RESULT_BYTES).contains(max_reserved_result_bytes)
                {
                    bail!("Invalid exact definition, generation or bounded approval options");
                }
            }
        }
        if matches!(self, Self::Revoke {revision,..} if *revision!=1) {
            bail!("Revocation requires approval revision one");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedOperation {
    pub capability: String,
    pub action: String,
    pub params: Value,
    pub constraints: Value,
    pub dependencies: Vec<usize>,
    pub timeout_ms: u64,
    pub executor_node: String,
    pub resource_id: Option<String>,
    pub model_id: Option<String>,
    pub backend: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixedScope {
    pub owner_context: crate::context::RequestContext,
    pub input: crate::request_planning::PlanInput,
    pub operations: Vec<ScopedOperation>,
}
impl FixedScope {
    pub fn validate(&self) -> Result<()> {
        crate::job_commands::validate_operation_id(self.owner_context.request_id())?;
        crate::network::validate_remote_node_id(self.owner_context.origin_node())?;
        self.input.validate()?;
        if self.operations.is_empty()
            || self.operations.len() > 128
            || serde_json::to_vec(self)?.len() > 1024 * 1024
        {
            bail!("Invalid bounded fixed-operation scope");
        }
        for (index, operation) in self.operations.iter().enumerate() {
            crate::network::validate_remote_node_id(&operation.executor_node)?;
            if operation.dependencies != (0..index).collect::<Vec<_>>()
                || operation.timeout_ms == 0
                || !operation.params.is_object()
                || !operation.constraints.is_object()
                || operation.constraints["node_id"] != operation.executor_node
            {
                bail!("Invalid fixed operation binding/dependencies");
            }
        }
        Ok(())
    }
    pub fn run_allowance(&self, limits: &crate::grants::GrantLimits) -> Result<(u64, u64)> {
        self.validate()?;
        let count = u64::try_from(self.operations.len())?;
        Ok((
            count
                .checked_mul(limits.max_execution_ms)
                .ok_or_else(|| anyhow::anyhow!("Duration overflow"))?,
            count
                .checked_mul(u64::try_from(limits.max_result_bytes)?)
                .ok_or_else(|| anyhow::anyhow!("Result allowance overflow"))?,
        ))
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revocation {
    pub operation_id: String,
    pub source_revision: u64,
    pub revoked_at_ms: u64,
    pub runtime_id: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authorization {
    pub schema_version: u32,
    pub id: String,
    pub creation: PolicyCommand,
    pub generation: u64,
    pub runtime_id: String,
    pub issuer_id: String,
    pub approved_at_ms: u64,
    pub expires_at_ms: u64,
    pub scope: FixedScope,
    pub scope_sha256: String,
    pub authorization_sha256: String,
    pub revision: u64,
    pub state: String,
    pub revocation: Option<Revocation>,
}
impl Authorization {
    /// Eligibility metadata only: no reservation, consumption, grant or dispatch.
    pub fn current_at(
        &self,
        runtime: &str,
        issuer: &str,
        generation: u64,
        definition: &crate::automation::Definition,
        now: u64,
    ) -> bool {
        let PolicyCommand::Approve {
            definition: id,
            definition_sha256,
            ..
        } = &self.creation
        else {
            return false;
        };
        self.validate().is_ok()
            && definition.validate().is_ok()
            && self.revocation.is_none()
            && definition.state == "disabled"
            && definition.source.is_some()
            && definition.id == *id
            && definition.definition_sha256 == *definition_sha256
            && self.runtime_id == runtime
            && self.issuer_id == issuer
            && self.generation == generation
            && now >= self.approved_at_ms
            && now < self.expires_at_ms
    }
    pub fn digest(&self) -> Result<String> {
        crate::grants::definition_digest(
            &json!({"schema_version":self.schema_version,"id":self.id,"creation":self.creation,
            "generation":self.generation,"runtime_id":self.runtime_id,"issuer_id":self.issuer_id,"approved_at_ms":self.approved_at_ms,
            "expires_at_ms":self.expires_at_ms,"scope":self.scope,"scope_sha256":self.scope_sha256}),
        )
    }
    pub fn validate(&self) -> Result<()> {
        self.creation.validate()?;
        self.scope.validate()?;
        let PolicyCommand::Approve {
            operation_id,
            generation,
            valid_for_ms,
            max_reserved_execution_ms,
            max_reserved_result_bytes,
            ..
        } = &self.creation
        else {
            bail!("Authorization lacks owner approval command");
        };
        crate::job_commands::validate_operation_id(&self.runtime_id)?;
        let limits = self.creation.limits().unwrap();
        let (duration, results) = self.scope.run_allowance(&limits)?;
        if self.schema_version != 1
            || self.id != *operation_id
            || generation.checked_add(1) != Some(self.generation)
            || !crate::request_result::valid_digest(&self.issuer_id)
            || self.approved_at_ms == 0
            || self.approved_at_ms.checked_add(*valid_for_ms) != Some(self.expires_at_ms)
            || crate::grants::definition_digest(&json!(self.scope))? != self.scope_sha256
            || self.digest()? != self.authorization_sha256
            || duration > *max_reserved_execution_ms
            || results > *max_reserved_result_bytes
            || self
                .scope
                .operations
                .iter()
                .any(|o| limits.max_execution_ms > o.timeout_ms)
        {
            bail!("Damaged or unbounded reusable approval; retained without activation");
        }
        match (&self.revocation, self.revision, self.state.as_str()) {
            (None, 1, "approved") => {}
            (Some(r), 2, "revoked") => {
                crate::job_commands::validate_operation_id(&r.operation_id)?;
                crate::job_commands::validate_operation_id(&r.runtime_id)?;
                if r.operation_id == self.id || r.source_revision != 1 || r.revoked_at_ms == 0 {
                    bail!("Invalid retained policy revocation");
                }
            }
            _ => bail!("Unsupported authorization lifecycle; no automatic activation"),
        }
        Ok(())
    }
    pub fn summary(&self) -> Value {
        json!({"id":self.id,"creation":self.creation,"generation":self.generation,"runtime_id":self.runtime_id,
            "issuer_id":self.issuer_id,"owner_node":self.scope.owner_context.origin_node(),"approved_at_ms":self.approved_at_ms,
            "expires_at_ms":self.expires_at_ms,"scope_sha256":self.scope_sha256,"authorization_sha256":self.authorization_sha256,
            "revision":self.revision,"state":self.state,"timers_started":false,"execution_requested":false})
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyRequest {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub command: PolicyCommand,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyReport {
    pub schema_version: u32,
    pub report_type: String,
    pub runtime_id: String,
    pub command: PolicyCommand,
    pub authorization: Option<Authorization>,
    pub items: Vec<Value>,
    pub next_after: Option<String>,
    pub reused: bool,
    pub occurrence_admission_available: bool,
    pub timers_started: bool,
    pub execution_requested: bool,
    pub dispatched: bool,
    pub automatically_retried: bool,
}
pub fn confirmed(value: &Value, command: &PolicyCommand, runtime: &str) -> bool {
    let Ok(report) = serde_json::from_value::<PolicyReport>(value.clone()) else {
        return false;
    };
    if command.validate().is_err()
        || report.schema_version != 1
        || report.report_type != "automation_authorization"
        || report.runtime_id != runtime
        || report.command != *command
        || !report.occurrence_admission_available
        || report.timers_started
        || report.execution_requested
        || report.dispatched
        || report.automatically_retried
    {
        return false;
    }
    if let PolicyCommand::List {
        definition,
        limit,
        after,
    } = command
    {
        if report.authorization.is_some() || report.reused || report.items.len() > *limit as usize {
            return false;
        }
        let mut previous = after.as_deref();
        for item in &report.items {
            let Some(id) = item["id"].as_str() else {
                return false;
            };
            // Reconstruct a strict summary from an independently validated
            // retained authorization would need its private scope. Instead the
            // public summary is parsed by its own strict, bounded contract.
            if !confirmed_summary(item, definition.as_deref()) || previous.is_some_and(|p| id <= p)
            {
                return false;
            }
            previous = Some(id);
        }
        return report
            .next_after
            .as_deref()
            .is_none_or(|id| report.items.len() == *limit as usize && Some(id) == previous);
    }
    let Some(record) = &report.authorization else {
        return false;
    };
    if record.validate().is_err() || !report.items.is_empty() || report.next_after.is_some() {
        return false;
    }
    match command {
        PolicyCommand::Approve { .. } => record.creation == *command,
        PolicyCommand::Show { id } => record.id == *id && !report.reused,
        PolicyCommand::Revoke {
            id,
            revision,
            operation_id,
        } => {
            record.id == *id
                && record.revocation.as_ref().is_some_and(|r| {
                    r.source_revision == *revision && r.operation_id == *operation_id
                })
        }
        _ => false,
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Summary {
    id: String,
    creation: PolicyCommand,
    generation: u64,
    runtime_id: String,
    issuer_id: String,
    owner_node: String,
    approved_at_ms: u64,
    expires_at_ms: u64,
    scope_sha256: String,
    authorization_sha256: String,
    revision: u64,
    state: String,
    timers_started: bool,
    execution_requested: bool,
}
fn confirmed_summary(value: &Value, definition: Option<&str>) -> bool {
    let Ok(s) = serde_json::from_value::<Summary>(value.clone()) else {
        return false;
    };
    let PolicyCommand::Approve {
        operation_id,
        definition: d,
        generation,
        valid_for_ms,
        ..
    } = &s.creation
    else {
        return false;
    };
    s.creation.validate().is_ok()
        && s.id == *operation_id
        && definition.is_none_or(|id| id == d)
        && generation.checked_add(1) == Some(s.generation)
        && crate::job_commands::validate_operation_id(&s.runtime_id).is_ok()
        && crate::network::validate_remote_node_id(&s.owner_node).is_ok()
        && crate::request_result::valid_digest(&s.issuer_id)
        && crate::request_result::valid_digest(&s.scope_sha256)
        && crate::request_result::valid_digest(&s.authorization_sha256)
        && s.approved_at_ms > 0
        && s.approved_at_ms.checked_add(*valid_for_ms) == Some(s.expires_at_ms)
        && matches!(
            (s.revision, s.state.as_str()),
            (1, "approved") | (2, "revoked")
        )
        && !s.timers_started
        && !s.execution_requested
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (crate::automation::Definition, Authorization) {
        let context = crate::context::RequestContext::new(
            uuid::Uuid::new_v4().to_string(),
            "origin".into(),
            vec!["Room".into()],
        )
        .unwrap();
        let job = uuid::Uuid::new_v4().simple().to_string();
        let creation = crate::automation::AutomationCommand::Define {
            operation_id: uuid::Uuid::new_v4().to_string(),
            from_job: job.clone(),
            revision: 2,
            plan_sha256: "a".repeat(64),
            name: "Status".into(),
            every_ms: 1000,
            max_runs: 3,
            valid_for_ms: 10000,
        };
        let source = json!({"plan_id":job,"revision":2,"status":"awaiting_approval","planning":{"plan_sha256":"a".repeat(64)},"goal":{"request_context":context}});
        let mut definition = crate::automation::Definition {
            schema_version: 1,
            id: creation.receipt().unwrap().into(),
            owner_context: context.clone(),
            created_at_ms: 1,
            creation,
            source_sha256: crate::grants::definition_digest(&source).unwrap(),
            definition_sha256: String::new(),
            revision: 1,
            state: "disabled".into(),
            source: Some(source),
            removal: None,
        };
        definition.definition_sha256 = definition.digest().unwrap();
        let command = PolicyCommand::Approve {
            definition: definition.id.clone(),
            revision: 1,
            definition_sha256: definition.definition_sha256.clone(),
            generation: 0,
            operation_id: uuid::Uuid::new_v4().to_string(),
            valid_for_ms: 10000,
            max_runs: 3,
            max_execution_ms_per_task: 1000,
            max_result_bytes_per_task: 1024,
            max_reserved_execution_ms: 3000,
            max_reserved_result_bytes: 3072,
        };
        let scope = FixedScope {
            owner_context: context,
            input: serde_json::from_value(
                json!({"schema_version":1,"template":"system_info","params":{},"constraints":{}}),
            )
            .unwrap(),
            operations: vec![ScopedOperation {
                capability: "system.info".into(),
                action: "run".into(),
                params: json!({}),
                constraints: json!({"node_id":"executor"}),
                dependencies: vec![],
                timeout_ms: 30000,
                executor_node: "executor".into(),
                resource_id: None,
                model_id: None,
                backend: None,
            }],
        };
        let mut record = Authorization {
            schema_version: 1,
            id: command.receipt().unwrap().into(),
            creation: command,
            generation: 1,
            runtime_id: uuid::Uuid::new_v4().to_string(),
            issuer_id: "a".repeat(64),
            approved_at_ms: 2,
            expires_at_ms: 10002,
            scope_sha256: crate::grants::definition_digest(&json!(scope)).unwrap(),
            scope,
            authorization_sha256: String::new(),
            revision: 1,
            state: "approved".into(),
            revocation: None,
        };
        record.authorization_sha256 = record.digest().unwrap();
        (definition, record)
    }
    #[test]
    fn approval_bounds_and_authority_fields_are_strict() {
        let (_definition, record) = fixture();
        record.validate().unwrap();
        for (pointer, value) in [
            ("/valid_for_ms", json!(86400001)),
            ("/max_runs", json!(0)),
            ("/revision", json!(2)),
            ("/generation", json!(u64::MAX)),
            ("/max_execution_ms_per_task", json!(0)),
            ("/max_result_bytes_per_task", json!(1023)),
            (
                "/max_reserved_execution_ms",
                json!(MAX_RESERVED_EXECUTION_MS + 1),
            ),
            (
                "/max_reserved_result_bytes",
                json!(MAX_RESERVED_RESULT_BYTES + 1),
            ),
        ] {
            let mut bad = json!(record.creation);
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(
                serde_json::from_value::<PolicyCommand>(bad)
                    .unwrap()
                    .validate()
                    .is_err(),
                "{pointer}"
            );
        }
        let mut bad = json!(record.creation);
        bad["permissions"] = json!("everything");
        assert!(serde_json::from_value::<PolicyCommand>(bad).is_err());
        let mut bad = json!(record);
        bad["consumed_runs"] = json!(1);
        assert!(serde_json::from_value::<Authorization>(bad).is_err());
    }
    #[test]
    fn eligibility_is_fenced_by_time_runtime_issuer_generation_revocation_and_definition() {
        let (mut definition, mut record) = fixture();
        assert!(record.current_at(&record.runtime_id, &record.issuer_id, 1, &definition, 2));
        for (runtime, issuer, generation, now) in [
            ("changed".into(), record.issuer_id.clone(), 1, 2),
            (record.runtime_id.clone(), "b".repeat(64), 1, 2),
            (record.runtime_id.clone(), record.issuer_id.clone(), 2, 2),
            (record.runtime_id.clone(), record.issuer_id.clone(), 1, 1),
            (
                record.runtime_id.clone(),
                record.issuer_id.clone(),
                1,
                record.expires_at_ms,
            ),
        ] {
            assert!(!record.current_at(&runtime, &issuer, generation, &definition, now));
        }
        record.revocation = Some(Revocation {
            operation_id: uuid::Uuid::new_v4().to_string(),
            source_revision: 1,
            revoked_at_ms: 1,
            runtime_id: record.runtime_id.clone(),
        });
        record.revision = 2;
        record.state = "revoked".into();
        record.validate().unwrap();
        assert!(!record.current_at(&record.runtime_id, &record.issuer_id, 1, &definition, 2));
        definition.source = None;
        definition.state = "removed".into();
        definition.revision = 2;
        definition.removal = Some(crate::automation::RemovalReceipt {
            operation_id: uuid::Uuid::new_v4().to_string(),
            source_revision: 1,
        });
        definition.validate().unwrap();
        record.revocation = None;
        record.revision = 1;
        record.state = "approved".into();
        assert!(!record.current_at(&record.runtime_id, &record.issuer_id, 1, &definition, 2));
    }
    #[test]
    fn replies_are_correlated_strict_and_cannot_claim_admission_or_execution() {
        let (_definition, record) = fixture();
        let command = record.creation.clone();
        let runtime = record.runtime_id.clone();
        let report = PolicyReport {
            schema_version: 1,
            report_type: "automation_authorization".into(),
            runtime_id: runtime.clone(),
            command: command.clone(),
            authorization: Some(record.clone()),
            items: vec![],
            next_after: None,
            reused: false,
            occurrence_admission_available: true,
            timers_started: false,
            execution_requested: false,
            dispatched: false,
            automatically_retried: false,
        };
        let value = json!(report);
        assert!(confirmed(&value, &command, &runtime));
        for (pointer, replacement) in [
            ("/runtime_id", json!(uuid::Uuid::new_v4().to_string())),
            ("/occurrence_admission_available", json!(false)),
            ("/timers_started", json!(true)),
            ("/execution_requested", json!(true)),
            (
                "/authorization/scope/operations/0/params",
                json!({"invented":true}),
            ),
        ] {
            let mut bad = value.clone();
            *bad.pointer_mut(pointer).unwrap() = replacement;
            assert!(!confirmed(&bad, &command, &runtime), "{pointer}");
        }
        let command = PolicyCommand::List {
            definition: None,
            limit: 1,
            after: None,
        };
        let value = json!(PolicyReport {
            command: command.clone(),
            authorization: None,
            items: vec![record.summary()],
            ..report
        });
        assert!(confirmed(&value, &command, &runtime));
        let mut bad = value.clone();
        bad["items"][0]["private_params"] = json!({});
        assert!(!confirmed(&bad, &command, &runtime));
        let mut bad = value;
        bad["items"][0]["id"] = json!(uuid::Uuid::new_v4().to_string());
        assert!(!confirmed(&bad, &command, &runtime));
    }
}
