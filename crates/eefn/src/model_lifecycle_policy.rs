//! Saved/applied owned-model policy. No browser, inference or restart controller.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum StartupMode {
    #[default]
    Eager,
    Manual,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModelLifecyclePolicy {
    pub startup: StartupMode,
    pub idle_unload_seconds: Option<u64>,
    pub queue_capacity: usize,
    pub receipt_capacity: usize,
    pub load_timeout_seconds: u64,
    pub termination_wait_seconds: u64,
}
impl Default for ModelLifecyclePolicy {
    fn default() -> Self {
        Self {
            startup: StartupMode::Eager,
            idle_unload_seconds: None,
            queue_capacity: 8,
            receipt_capacity: 256,
            load_timeout_seconds: 300,
            termination_wait_seconds: 5,
        }
    }
}
impl ModelLifecyclePolicy {
    pub fn validate(&self) -> Result<()> {
        if !(1..=64).contains(&self.queue_capacity)
            || !(1..=4096).contains(&self.receipt_capacity)
            || self.receipt_capacity < self.queue_capacity
            || !(1..=3600).contains(&self.load_timeout_seconds)
            || !(1..=30).contains(&self.termination_wait_seconds)
            || self
                .idle_unload_seconds
                .is_some_and(|n| !(1..=86400).contains(&n))
        {
            bail!("Invalid owned-model lifecycle policy bounds")
        }
        Ok(())
    }
    pub(crate) fn from_config(config: &Value) -> Result<Self> {
        Self::revision_from_config(config)?;
        let policy: Self = serde_json::from_value(
            config
                .get("model_lifecycle")
                .cloned()
                .unwrap_or_else(|| json!({})),
        )?;
        policy.validate()?;
        Ok(policy)
    }
    /// Content fingerprint only; command fences use the persisted revision too.
    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }
    pub(crate) fn revision_from_config(config: &Value) -> Result<u64> {
        match config.get("model_lifecycle_revision") {
            None => Ok(0),
            Some(value) => value.as_u64().ok_or_else(|| {
                anyhow::anyhow!("Invalid saved model policy revision; retain configuration")
            }),
        }
    }
    pub fn versioned_digest(&self, revision: u64) -> Result<String> {
        self.validate()?;
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(
                &json!({"schema_version":1,"revision":revision,"policy":self})
            )?)
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, clap::Subcommand)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum PolicyCommand {
    Show {},
    /// Save future runtime policy; no automatic restart or inference.
    Set {
        #[arg(long)]
        expected_digest: String,
        #[arg(long, value_enum)]
        startup: Option<StartupMode>,
        /// Zero disables idle unloading; otherwise 1-86400 seconds.
        #[arg(long)]
        idle_unload_seconds: Option<u64>,
        #[arg(long)]
        queue_capacity: Option<usize>,
        #[arg(long)]
        receipt_capacity: Option<usize>,
        #[arg(long)]
        load_timeout_seconds: Option<u64>,
        #[arg(long)]
        termination_wait_seconds: Option<u64>,
    },
}
#[derive(Debug, clap::Args)]
pub struct PolicyArgs {
    /// Exact runtime for a running-node mutation; omit only for locked offline setup.
    #[arg(long, global = true)]
    pub runtime: Option<String>,
    #[command(subcommand)]
    pub command: PolicyCommand,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub expected_runtime_id: Option<String>,
    pub command: PolicyCommand,
}
impl PolicyRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            bail!("Unsupported model lifecycle policy schema")
        }
        crate::network::validate_remote_node_id(&self.expected_node_id)?;
        if let Some(id) = &self.expected_runtime_id {
            crate::automation_occurrence::uuid(id)?;
        }
        if let PolicyCommand::Set {
            expected_digest,
            startup,
            idle_unload_seconds,
            queue_capacity,
            receipt_capacity,
            load_timeout_seconds,
            termination_wait_seconds,
        } = &self.command
        {
            if expected_digest.len() != 64
                || !expected_digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                bail!("Exact lowercase policy digest required")
            }
            if startup.is_none()
                && idle_unload_seconds.is_none()
                && queue_capacity.is_none()
                && receipt_capacity.is_none()
                && load_timeout_seconds.is_none()
                && termination_wait_seconds.is_none()
            {
                bail!("Choose at least one lifecycle policy field")
            }
            // Validate scalar choices before consulting/mutating configuration.
            if idle_unload_seconds.is_some_and(|n| n > 86400)
                || queue_capacity.is_some_and(|n| !(1..=64).contains(&n))
                || receipt_capacity.is_some_and(|n| !(1..=4096).contains(&n))
                || load_timeout_seconds.is_some_and(|n| !(1..=3600).contains(&n))
                || termination_wait_seconds.is_some_and(|n| !(1..=30).contains(&n))
            {
                bail!("Invalid lifecycle policy choice")
            }
        }
        Ok(())
    }
}
impl PolicyCommand {
    fn apply(&self, current: &ModelLifecyclePolicy, revision: u64) -> Result<ModelLifecyclePolicy> {
        let mut next = current.clone();
        if let Self::Set {
            expected_digest,
            startup,
            idle_unload_seconds,
            queue_capacity,
            receipt_capacity,
            load_timeout_seconds,
            termination_wait_seconds,
        } = self
        {
            if *expected_digest != current.versioned_digest(revision)? {
                bail!("Saved model policy changed; inspect before a new owner decision")
            }
            if let Some(value) = startup {
                next.startup = *value;
            }
            if let Some(value) = idle_unload_seconds {
                next.idle_unload_seconds = (*value != 0).then_some(*value);
            }
            if let Some(value) = queue_capacity {
                next.queue_capacity = *value;
            }
            if let Some(value) = receipt_capacity {
                next.receipt_capacity = *value;
            }
            if let Some(value) = load_timeout_seconds {
                next.load_timeout_seconds = *value;
            }
            if let Some(value) = termination_wait_seconds {
                next.termination_wait_seconds = *value;
            }
        }
        next.validate()?;
        Ok(next)
    }
}
impl crate::NodeService {
    /// `offline` is adapter-owned, never a deserialized permission. CLI holds the
    /// config's exclusive instance lock; HTTP/RPC always pass false.
    pub fn model_lifecycle_policy_command(
        &self,
        request: PolicyRequest,
        remote: bool,
        offline: bool,
    ) -> Result<Value> {
        request.validate()?;
        if request.expected_node_id != self.node_id {
            bail!("Model policy node identity changed")
        }
        let bound = self.model_runtime.lock().unwrap();
        let runtime = bound.as_ref().filter(|r| !r.is_closed());
        if offline && runtime.is_some() {
            bail!("Offline model policy cannot control a running runtime")
        }
        if !offline && runtime.is_none() {
            bail!("Model policy runtime unavailable")
        }
        let runtime_id = runtime.map(|r| r.id().to_owned());
        if request
            .expected_runtime_id
            .as_ref()
            .is_some_and(|id| Some(id) != runtime_id.as_ref())
            || (!offline
                && matches!(request.command, PolicyCommand::Set { .. })
                && request.expected_runtime_id.is_none())
        {
            bail!("Exact current runtime required; no automatic refresh")
        }
        let applied = runtime.map(|r| r.policy().clone());
        let applied_revision = runtime.map(|r| r.policy_revision());
        let result = self.inspect_grant_configuration(|config| {
            if runtime.is_some_and(|r|r.is_closed()) { bail!("Model policy runtime closed before application") }
            if config["node_id"] != self.node_id { bail!("Saved node identity changed") }
            if remote && matches!(request.command, PolicyCommand::Set {..})
                && config.pointer("/management/allow_remote") != Some(&json!(true)) {
                bail!("Model policy requires current local owner management approval")
            }
            let before = ModelLifecyclePolicy::from_config(config)?;
            let before_revision=ModelLifecyclePolicy::revision_from_config(config)?;
            let saved = request.command.apply(&before,before_revision)?;
            let changed = saved != before;
            let saved_revision=if changed {before_revision.checked_add(1).ok_or_else(||anyhow::anyhow!("Model policy revision exhausted; no wrap or reset"))?}else{before_revision};
            if changed {
                let mut next = config.clone();
                next["model_lifecycle"] = json!(saved);
                self.save_config_locked(next)?;
                self.live.lock().unwrap()["pending_restart"] = json!(true);
            }
            Ok(json!({"schema_version":1,"report_type":"model_lifecycle_policy","node_id":self.node_id,
                "runtime_id":runtime_id,"command":request.command,"saved_policy":saved,"saved_revision":saved_revision,"saved_digest":saved.versioned_digest(saved_revision)?,
                "applied_policy":applied,"applied_revision":applied_revision,"changed":changed,"restart_required":applied.as_ref().is_some_and(|p|*p!=saved || applied_revision!=Some(saved_revision)),
                "execution_authorized":false,"model_inference_requested":false,"restart_requested":false,"automatically_retried":false,
                "note":"Saved policy applies on explicit runtime restart (or next offline start). A saved mismatch prevents new automatic idle unloads immediately. No selection, download, inference, provider switch or restart was sent."}))
        })?;
        if !confirmed(&result, &request) {
            bail!("Model policy report not confirmed; inspect saved policy")
        }
        Ok(result)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    node_id: String,
    runtime_id: Option<String>,
    command: PolicyCommand,
    saved_policy: ModelLifecyclePolicy,
    saved_digest: String,
    saved_revision: u64,
    applied_policy: Option<ModelLifecyclePolicy>,
    applied_revision: Option<u64>,
    changed: bool,
    restart_required: bool,
    execution_authorized: bool,
    model_inference_requested: bool,
    restart_requested: bool,
    automatically_retried: bool,
    note: String,
}
pub fn confirmed(value: &Value, request: &PolicyRequest) -> bool {
    let Ok(r) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    if request.validate().is_err()
        || r.schema_version != 1
        || r.report_type != "model_lifecycle_policy"
        || r.node_id != request.expected_node_id
        || r.command != request.command
        || r.execution_authorized
        || r.model_inference_requested
        || r.restart_requested
        || r.automatically_retried
        || r.note.len() > 1024
        || r.saved_policy.validate().is_err()
        || r.saved_policy
            .versioned_digest(r.saved_revision)
            .ok()
            .as_ref()
            != Some(&r.saved_digest)
        || r.applied_policy
            .as_ref()
            .is_some_and(|p| p.validate().is_err())
        || r.runtime_id
            .as_ref()
            .is_some_and(|id| crate::automation_occurrence::uuid(id).is_err())
        || request
            .expected_runtime_id
            .as_ref()
            .is_some_and(|id| Some(id) != r.runtime_id.as_ref())
        || r.runtime_id.is_some() != r.applied_policy.is_some()
        || r.runtime_id.is_some() != r.applied_revision.is_some()
        || r.restart_required
            != r.applied_policy.as_ref().is_some_and(|p| {
                *p != r.saved_policy || r.applied_revision != Some(r.saved_revision)
            })
    {
        return false;
    }
    if matches!(request.command, PolicyCommand::Show {}) && r.changed {
        return false;
    }
    if let PolicyCommand::Set {
        startup,
        idle_unload_seconds,
        queue_capacity,
        receipt_capacity,
        load_timeout_seconds,
        termination_wait_seconds,
        ..
    } = &request.command
    {
        if startup.is_some_and(|v| v != r.saved_policy.startup)
            || idle_unload_seconds
                .is_some_and(|v| (v != 0).then_some(v) != r.saved_policy.idle_unload_seconds)
            || queue_capacity.is_some_and(|v| v != r.saved_policy.queue_capacity)
            || receipt_capacity.is_some_and(|v| v != r.saved_policy.receipt_capacity)
            || load_timeout_seconds.is_some_and(|v| v != r.saved_policy.load_timeout_seconds)
            || termination_wait_seconds
                .is_some_and(|v| v != r.saved_policy.termination_wait_seconds)
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;
    fn command(digest: String) -> PolicyCommand {
        PolicyCommand::Set {
            expected_digest: digest,
            startup: Some(StartupMode::Manual),
            idle_unload_seconds: Some(3),
            queue_capacity: Some(2),
            receipt_capacity: Some(16),
            load_timeout_seconds: Some(8),
            termination_wait_seconds: Some(2),
        }
    }
    fn request(command: PolicyCommand, runtime: Option<String>) -> PolicyRequest {
        PolicyRequest {
            schema_version: 1,
            expected_node_id: "owner".into(),
            expected_runtime_id: runtime,
            command,
        }
    }
    fn fixture() -> (tempfile::TempDir, std::sync::Arc<crate::NodeService>) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node.json");
        crate::setup::write_json(&path,&json!({"node_id":"owner","models":{"provider":"llamacpp"},"management":{"allow_remote":false},"unrelated":"retain"})).unwrap();
        let service = crate::NodeService::new(path, "owner".into());
        (directory, service)
    }
    #[test]
    fn defaults_strict_bounds_and_canonical_digest_preserve_older_configs() {
        let old = ModelLifecyclePolicy::from_config(&json!({})).unwrap();
        assert_eq!(old, ModelLifecyclePolicy::default());
        assert_eq!(old.startup, StartupMode::Eager);
        assert_eq!(old.idle_unload_seconds, None);
        assert_eq!(
            old.digest().unwrap(),
            ModelLifecyclePolicy::from_config(&json!({"model_lifecycle":{}}))
                .unwrap()
                .digest()
                .unwrap()
        );
        for value in [
            json!(null),
            json!({"queue_capacity":0}),
            json!({"receipt_capacity":4097}),
            json!({"queue_capacity":4,"receipt_capacity":2}),
            json!({"load_timeout_seconds":0}),
            json!({"termination_wait_seconds":31}),
            json!({"idle_unload_seconds":0}),
            json!({"startup":"on_demand"}),
            json!({"force":true}),
        ] {
            assert!(ModelLifecyclePolicy::from_config(&json!({"model_lifecycle":value})).is_err());
        }
        let next = command(old.versioned_digest(0).unwrap())
            .apply(&old, 0)
            .unwrap();
        assert_eq!(next.startup, StartupMode::Manual);
        let mut reset = command(next.versioned_digest(1).unwrap());
        if let PolicyCommand::Set {
            idle_unload_seconds,
            ..
        } = &mut reset
        {
            *idle_unload_seconds = Some(0);
        }
        assert_eq!(reset.apply(&next, 1).unwrap().idle_unload_seconds, None);
    }
    #[test]
    fn offline_policy_cas_preserves_config_and_never_restarts_or_replays() {
        let (_dir, service) = fixture();
        let show = request(PolicyCommand::Show {}, None);
        let initial = service
            .model_lifecycle_policy_command(show.clone(), false, true)
            .unwrap();
        assert!(confirmed(&initial, &show));
        let set = request(
            command(initial["saved_digest"].as_str().unwrap().into()),
            None,
        );
        let result = service
            .model_lifecycle_policy_command(set.clone(), false, true)
            .unwrap();
        assert!(confirmed(&result, &set));
        assert_eq!(result["changed"], true);
        assert_eq!(result["runtime_id"], Value::Null);
        assert_eq!(result["restart_requested"], false);
        assert_eq!(service.read_config().unwrap()["unrelated"], "retain");
        let bytes = std::fs::read(&service.config_path).unwrap();
        assert!(
            service
                .model_lifecycle_policy_command(set, false, true)
                .is_err()
        );
        assert_eq!(std::fs::read(&service.config_path).unwrap(), bytes);
        assert!(!service.restart.notified().now_or_never().is_some());
    }
    #[tokio::test]
    async fn running_policy_is_fenced_owner_approved_saved_not_applied_and_status_readonly() {
        let (_dir, service) = fixture();
        let engine = std::sync::Arc::new(
            crate::NodeEngine::new(false, vec![], std::env::temp_dir())
                .unwrap()
                .with_service(service.clone()),
        );
        let id = uuid::Uuid::new_v4().to_string();
        let guard = service.bind_model_runtime(&engine, id.clone()).unwrap();
        let show = request(PolicyCommand::Show {}, None);
        let initial = service
            .model_lifecycle_policy_command(show.clone(), true, false)
            .unwrap();
        let mut set = request(
            command(initial["saved_digest"].as_str().unwrap().into()),
            Some(id.clone()),
        );
        assert!(
            service
                .model_lifecycle_policy_command(set.clone(), true, false)
                .is_err()
        );
        assert!(
            service
                .remote("model_lifecycle_policy_status", json!(set))
                .await
                .is_err()
        );
        set.expected_runtime_id = Some(uuid::Uuid::new_v4().to_string());
        assert!(
            service
                .model_lifecycle_policy_command(set.clone(), false, false)
                .is_err()
        );
        set.expected_runtime_id = Some(id);
        let saved = service
            .model_lifecycle_policy_command(set.clone(), false, false)
            .unwrap();
        assert!(confirmed(&saved, &set));
        assert_eq!(saved["restart_required"], true);
        assert_eq!(saved["applied_policy"]["startup"], "eager");
        assert_eq!(saved["saved_policy"]["startup"], "manual");
        assert_eq!(guard.policy().startup, StartupMode::Eager);
        assert!(
            service
                .model_lifecycle_policy_command(show.clone(), true, false)
                .is_ok()
        );
        assert!(
            service
                .model_lifecycle_policy_command(show.clone(), false, true)
                .is_err()
        );
        drop(guard);
        assert!(
            service
                .model_lifecycle_policy_command(show, false, false)
                .is_err()
        );
    }
    #[test]
    fn extra_authority_digest_or_requested_choice_substitution_never_confirms() {
        let (_dir, service) = fixture();
        let initial = service
            .model_lifecycle_policy_command(request(PolicyCommand::Show {}, None), false, true)
            .unwrap();
        let req = request(
            command(initial["saved_digest"].as_str().unwrap().into()),
            None,
        );
        let good = service
            .model_lifecycle_policy_command(req.clone(), false, true)
            .unwrap();
        assert!(confirmed(&good, &req));
        for field in [
            "execution_authorized",
            "model_inference_requested",
            "restart_requested",
            "automatically_retried",
        ] {
            let mut bad = good.clone();
            bad[field] = json!(true);
            assert!(!confirmed(&bad, &req));
        }
        let mut bad = good.clone();
        bad["extra_scope"] = json!(true);
        assert!(!confirmed(&bad, &req));
        let mut bad = good.clone();
        bad["saved_digest"] = json!("0".repeat(64));
        assert!(!confirmed(&bad, &req));
        let mut bad = good;
        bad["saved_policy"]["startup"] = json!("eager");
        bad["saved_digest"] = json!(
            serde_json::from_value::<ModelLifecyclePolicy>(bad["saved_policy"].clone())
                .unwrap()
                .versioned_digest(bad["saved_revision"].as_u64().unwrap())
                .unwrap()
        );
        assert!(!confirmed(&bad, &req));
    }
    #[test]
    fn generic_content_restore_preserves_revision_and_cannot_revive_old_cas() {
        let (_dir, service) = fixture();
        let before = service.read_config().unwrap();
        let initial = service
            .model_lifecycle_policy_command(request(PolicyCommand::Show {}, None), false, true)
            .unwrap();
        let old = request(
            command(initial["saved_digest"].as_str().unwrap().into()),
            None,
        );
        let saved = service
            .model_lifecycle_policy_command(old.clone(), false, true)
            .unwrap();
        assert_eq!(saved["saved_revision"], 1);
        let mut restore = before;
        restore["model_lifecycle_revision"] = json!(0);
        service.save_config(restore).unwrap();
        let restored = service
            .model_lifecycle_policy_command(request(PolicyCommand::Show {}, None), false, true)
            .unwrap();
        assert_eq!(restored["saved_policy"], initial["saved_policy"]);
        assert_eq!(restored["saved_revision"], 2);
        assert_ne!(restored["saved_digest"], initial["saved_digest"]);
        assert!(
            service
                .model_lifecycle_policy_command(old, false, true)
                .is_err()
        );
        let mut unchanged = service.read_config().unwrap();
        unchanged["model_lifecycle_revision"] = json!(0);
        service.save_config(unchanged).unwrap();
        assert_eq!(
            ModelLifecyclePolicy::revision_from_config(&service.read_config().unwrap()).unwrap(),
            2
        );
    }
    #[test]
    fn invalid_or_exhausted_revision_is_retained_without_default_reset_or_wrap() {
        for revision in [json!(null), json!(-1), json!("1"), json!(u64::MAX)] {
            let (_dir, service) = fixture();
            let mut config = service.read_config().unwrap();
            config["model_lifecycle_revision"] = revision.clone();
            crate::setup::write_json(&service.config_path, &config).unwrap();
            let before = std::fs::read(&service.config_path).unwrap();
            if revision == json!(u64::MAX) {
                let shown = service
                    .model_lifecycle_policy_command(
                        request(PolicyCommand::Show {}, None),
                        false,
                        true,
                    )
                    .unwrap();
                assert!(
                    service
                        .model_lifecycle_policy_command(
                            request(
                                command(shown["saved_digest"].as_str().unwrap().into()),
                                None
                            ),
                            false,
                            true
                        )
                        .is_err()
                );
            } else {
                assert!(
                    service
                        .model_lifecycle_policy_command(
                            request(PolicyCommand::Show {}, None),
                            false,
                            true
                        )
                        .is_err()
                );
            }
            assert_eq!(std::fs::read(&service.config_path).unwrap(), before);
        }
    }
}
