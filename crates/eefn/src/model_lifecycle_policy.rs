//! Saved/applied model policy. No browser, inference or restart controller.
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
    OnDemand,
}

/// Request hint only: a backend/model may not support switching thinking.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingMode {
    #[default]
    Inherit,
    Disabled,
    Enabled,
}
impl ThinkingMode {
    fn is_inherit(&self) -> bool {
        *self == Self::Inherit
    }
    pub(crate) fn hint(self) -> Option<bool> {
        match self {
            Self::Inherit => None,
            Self::Disabled => Some(false),
            Self::Enabled => Some(true),
        }
    }
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
    /// Shared Ollama request hint: null inherits backend policy; -1 is explicit
    /// indefinite retention. This does not confer ownership of service/models.
    pub ollama_keep_alive_seconds: Option<i64>,
    /// Applied request option, not a RAM quota or a shared-service setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ollama_context_tokens: Option<u32>,
    /// Applied owner request preference, not proof of backend thinking state.
    /// Omit inherit to preserve pre-extension policy digests and wire reports.
    #[serde(skip_serializing_if = "ThinkingMode::is_inherit")]
    pub thinking_mode: ThinkingMode,
    #[serde(skip_serializing_if = "crate::interpreter_sampling::InterpreterSampling::is_default")]
    pub interpreter_sampling: crate::interpreter_sampling::InterpreterSampling,
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
            ollama_keep_alive_seconds: None,
            ollama_context_tokens: None,
            thinking_mode: ThinkingMode::Inherit,
            interpreter_sampling: Default::default(),
        }
    }
}
impl ModelLifecyclePolicy {
    pub fn validate(&self) -> Result<()> {
        self.interpreter_sampling.validate()?;
        if !(1..=64).contains(&self.queue_capacity)
            || !(1..=4096).contains(&self.receipt_capacity)
            || self.receipt_capacity < self.queue_capacity
            || !(1..=3600).contains(&self.load_timeout_seconds)
            || !(1..=30).contains(&self.termination_wait_seconds)
            || self
                .idle_unload_seconds
                .is_some_and(|n| !(1..=86400).contains(&n))
            || self
                .ollama_keep_alive_seconds
                .is_some_and(|n| !(-1..=86400).contains(&n))
            || self
                .ollama_context_tokens
                .is_some_and(|n| !(512..=1_048_576).contains(&n))
        {
            bail!("Invalid model lifecycle policy bounds")
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
        /// Ollama retention hint for future inference: -1 indefinite, 0 after
        /// response, or 1-86400 seconds. Does not stop the shared service.
        #[arg(
            long,
            allow_hyphen_values = true,
            conflicts_with = "ollama_keep_alive_default"
        )]
        ollama_keep_alive_seconds: Option<i64>,
        /// Omit the Ollama keep_alive hint and inherit its server policy.
        #[arg(long, conflicts_with = "ollama_keep_alive_seconds")]
        #[serde(default)]
        ollama_keep_alive_default: bool,
        /// Save Ollama num_ctx for EEF-owned requests; restart this node to apply.
        #[arg(long, conflicts_with = "ollama_context_default", value_parser=clap::value_parser!(u32).range(512..=1_048_576))]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ollama_context_tokens: Option<u32>,
        /// Omit num_ctx and inherit the model/server's context preference.
        #[arg(long, conflicts_with = "ollama_context_tokens")]
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        ollama_context_default: bool,
        /// Thinking request hint for future inference; inherit omits the hint.
        #[arg(long, value_enum)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thinking_mode: Option<ThinkingMode>,
        #[command(flatten)]
        #[serde(
            default,
            skip_serializing_if = "crate::interpreter_sampling::SamplingPatch::is_empty"
        )]
        interpreter_sampling: crate::interpreter_sampling::SamplingPatch,
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
            ollama_keep_alive_seconds,
            ollama_keep_alive_default,
            ollama_context_tokens,
            ollama_context_default,
            thinking_mode,
            interpreter_sampling,
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
                && ollama_keep_alive_seconds.is_none()
                && !ollama_keep_alive_default
                && ollama_context_tokens.is_none()
                && !ollama_context_default
                && thinking_mode.is_none()
                && interpreter_sampling.is_empty()
            {
                bail!("Choose at least one lifecycle policy field")
            }
            // Validate scalar choices before consulting/mutating configuration.
            interpreter_sampling.validate()?;
            if idle_unload_seconds.is_some_and(|n| n > 86400)
                || queue_capacity.is_some_and(|n| !(1..=64).contains(&n))
                || receipt_capacity.is_some_and(|n| !(1..=4096).contains(&n))
                || load_timeout_seconds.is_some_and(|n| !(1..=3600).contains(&n))
                || termination_wait_seconds.is_some_and(|n| !(1..=30).contains(&n))
                || ollama_keep_alive_seconds.is_some_and(|n| !(-1..=86400).contains(&n))
                || (*ollama_keep_alive_default && ollama_keep_alive_seconds.is_some())
                || ollama_context_tokens.is_some_and(|n| !(512..=1_048_576).contains(&n))
                || (*ollama_context_default && ollama_context_tokens.is_some())
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
            ollama_keep_alive_seconds,
            ollama_keep_alive_default,
            ollama_context_tokens,
            ollama_context_default,
            thinking_mode,
            interpreter_sampling,
        } = self
        {
            if *expected_digest != current.versioned_digest(revision)? {
                bail!("Saved model policy changed; inspect before a new owner decision")
            }
            if *ollama_context_default && ollama_context_tokens.is_some() {
                bail!("Choose Ollama context tokens or inherited context, not both")
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
            if *ollama_keep_alive_default {
                next.ollama_keep_alive_seconds = None;
            } else if let Some(value) = ollama_keep_alive_seconds {
                next.ollama_keep_alive_seconds = Some(*value);
            }
            if let Some(value) = thinking_mode {
                next.thinking_mode = *value;
            }
            if *ollama_context_default {
                next.ollama_context_tokens = None;
            } else if let Some(value) = ollama_context_tokens {
                next.ollama_context_tokens = Some(*value);
            }
            next.interpreter_sampling = interpreter_sampling.apply(&next.interpreter_sampling)?;
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
        ollama_keep_alive_seconds,
        ollama_keep_alive_default,
        ollama_context_tokens,
        ollama_context_default,
        thinking_mode,
        interpreter_sampling,
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
            || ollama_keep_alive_seconds
                .is_some_and(|v| Some(v) != r.saved_policy.ollama_keep_alive_seconds)
            || (*ollama_keep_alive_default && r.saved_policy.ollama_keep_alive_seconds.is_some())
            || ollama_context_tokens
                .is_some_and(|v| Some(v) != r.saved_policy.ollama_context_tokens)
            || (*ollama_context_default && r.saved_policy.ollama_context_tokens.is_some())
            || thinking_mode.is_some_and(|v| v != r.saved_policy.thinking_mode)
            || !interpreter_sampling.matches(&r.saved_policy.interpreter_sampling)
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
            ollama_keep_alive_seconds: None,
            ollama_keep_alive_default: false,
            ollama_context_tokens: None,
            ollama_context_default: false,
            thinking_mode: None,
            interpreter_sampling: Default::default(),
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
            json!({"startup":"unsupported"}),
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
    fn inherited_thinking_preserves_prior_policy_serialization_and_digests() {
        let old = json!({"startup":"eager","idle_unload_seconds":null,"queue_capacity":8,
            "receipt_capacity":256,"load_timeout_seconds":300,"termination_wait_seconds":5,
            "ollama_keep_alive_seconds":null});
        let policy: ModelLifecyclePolicy = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(policy.thinking_mode, ThinkingMode::Inherit);
        assert_eq!(policy.ollama_context_tokens, None);
        assert_eq!(json!(policy), old);
        let prior_digest = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&json!({"schema_version":1,"revision":17,"policy":old}))
                    .unwrap()
            )
        );
        assert_eq!(policy.versioned_digest(17).unwrap(), prior_digest);
        let mut explicit = old;
        explicit["thinking_mode"] = json!("inherit");
        let explicit: ModelLifecyclePolicy = serde_json::from_value(explicit).unwrap();
        assert_eq!(explicit, policy);
        assert_eq!(explicit.versioned_digest(17).unwrap(), prior_digest);
    }
    #[test]
    fn ollama_context_choices_are_bounded_explicit_reset_and_exact_report_confirmation() {
        let original = ModelLifecyclePolicy::default();
        for tokens in [512, 4096, 1_048_576] {
            let command:PolicyCommand=serde_json::from_value(json!({"operation":"set",
                "expected_digest":original.versioned_digest(0).unwrap(),"ollama_context_tokens":tokens})).unwrap();
            request(command.clone(), None).validate().unwrap();
            let next = command.apply(&original, 0).unwrap();
            assert_eq!(next.ollama_context_tokens, Some(tokens));
            let reset: PolicyCommand = serde_json::from_value(json!({"operation":"set",
                "expected_digest":next.versioned_digest(1).unwrap(),"ollama_context_default":true}))
            .unwrap();
            assert_eq!(reset.apply(&next, 1).unwrap().ollama_context_tokens, None);
        }
        for raw in [
            json!({"ollama_context_tokens":0}),
            json!({"ollama_context_tokens":511}),
            json!({"ollama_context_tokens":1_048_577}),
            json!({"ollama_context_tokens":4096,"ollama_context_default":true}),
        ] {
            let mut value =
                json!({"operation":"set","expected_digest":original.versioned_digest(0).unwrap()});
            value
                .as_object_mut()
                .unwrap()
                .extend(raw.as_object().unwrap().clone());
            let command: PolicyCommand = serde_json::from_value(value).unwrap();
            assert!(request(command.clone(), None).validate().is_err());
            assert!(command.apply(&original, 0).is_err());
        }
    }
    #[test]
    fn ollama_context_report_cannot_substitute_another_valid_value() {
        let (_directory, service) = fixture();
        let shown = service
            .model_lifecycle_policy_command(request(PolicyCommand::Show {}, None), false, true)
            .unwrap();
        let command:PolicyCommand=serde_json::from_value(json!({"operation":"set","expected_digest":shown["saved_digest"],"ollama_context_tokens":4096})).unwrap();
        let req = request(command, None);
        let good = service
            .model_lifecycle_policy_command(req.clone(), false, true)
            .unwrap();
        assert!(confirmed(&good, &req));
        assert_eq!(good["saved_policy"]["ollama_context_tokens"], 4096);
        for tokens in [Value::Null, json!(2048), json!(8192)] {
            let mut changed = good.clone();
            changed["saved_policy"]["ollama_context_tokens"] = tokens;
            let policy: ModelLifecyclePolicy =
                serde_json::from_value(changed["saved_policy"].clone()).unwrap();
            changed["saved_digest"] = json!(
                policy
                    .versioned_digest(changed["saved_revision"].as_u64().unwrap())
                    .unwrap()
            );
            assert!(!confirmed(&changed, &req));
        }
    }
    #[test]
    fn ollama_context_cli_requires_explicit_bounded_choice_or_reset() {
        use clap::Parser;
        #[derive(clap::Parser)]
        struct Cli {
            #[command(flatten)]
            args: PolicyArgs,
        }
        let digest = "a".repeat(64);
        for extras in [
            vec!["--ollama-context-tokens", "4096"],
            vec!["--ollama-context-default"],
        ] {
            let mut argv = vec!["policy", "set", "--expected-digest", digest.as_str()];
            argv.extend(extras);
            let parsed = Cli::try_parse_from(argv).unwrap();
            request(parsed.args.command, None).validate().unwrap();
        }
        for extras in [
            vec!["--ollama-context-tokens", "511"],
            vec!["--ollama-context-tokens", "1048577"],
            vec![
                "--ollama-context-tokens",
                "4096",
                "--ollama-context-default",
            ],
            vec!["--ollama-context-tokens"],
        ] {
            let mut argv = vec!["policy", "set", "--expected-digest", digest.as_str()];
            argv.extend(extras);
            assert!(Cli::try_parse_from(argv).is_err());
        }
        let legacy: PolicyCommand = serde_json::from_value(
            json!({"operation":"set","expected_digest":digest,"startup":"manual"}),
        )
        .unwrap();
        let value = serde_json::to_value(legacy).unwrap();
        assert!(value.get("ollama_context_tokens").is_none());
        assert!(value.get("ollama_context_default").is_none());
    }
    #[test]
    fn thinking_only_set_reset_are_strict_and_revision_fenced() {
        let old = ModelLifecyclePolicy::default();
        for mode in [ThinkingMode::Disabled, ThinkingMode::Enabled] {
            let raw = json!({"operation":"set","expected_digest":old.versioned_digest(0).unwrap(),"thinking_mode":mode});
            let set: PolicyCommand = serde_json::from_value(raw.clone()).unwrap();
            request(set.clone(), None).validate().unwrap();
            let next = set.apply(&old, 0).unwrap();
            assert_eq!(next.thinking_mode, mode);
            assert!(set.apply(&next, 1).is_err());
            let reset: PolicyCommand = serde_json::from_value(json!({"operation":"set",
                "expected_digest":next.versioned_digest(1).unwrap(),"thinking_mode":"inherit"}))
            .unwrap();
            request(reset.clone(), None).validate().unwrap();
            assert_eq!(reset.apply(&next, 1).unwrap(), old);
        }
        for invalid in [
            json!(true),
            json!(false),
            json!(1),
            json!("none"),
            json!({}),
        ] {
            assert!(
                serde_json::from_value::<PolicyCommand>(json!({"operation":"set",
                "expected_digest":old.versioned_digest(0).unwrap(),"thinking_mode":invalid}))
                .is_err()
            );
            assert!(
                ModelLifecyclePolicy::from_config(
                    &json!({"model_lifecycle":{"thinking_mode":invalid}})
                )
                .is_err()
            );
        }
        let empty: PolicyCommand = serde_json::from_value(json!({"operation":"set",
            "expected_digest":old.versioned_digest(0).unwrap(),"thinking_mode":null}))
        .unwrap();
        assert!(request(empty, None).validate().is_err());
    }
    #[test]
    fn substituted_thinking_report_cannot_confirm_even_with_recomputed_digest() {
        let (_directory, service) = fixture();
        let show = service
            .model_lifecycle_policy_command(request(PolicyCommand::Show {}, None), false, true)
            .unwrap();
        let command = serde_json::from_value(json!({"operation":"set",
            "expected_digest":show["saved_digest"],"thinking_mode":"disabled"}))
        .unwrap();
        let req = request(command, None);
        let good = service
            .model_lifecycle_policy_command(req.clone(), false, true)
            .unwrap();
        assert!(confirmed(&good, &req));
        for mode in [ThinkingMode::Enabled, ThinkingMode::Inherit] {
            let mut bad = good.clone();
            bad["saved_policy"]["thinking_mode"] = json!(mode);
            let policy: ModelLifecyclePolicy =
                serde_json::from_value(bad["saved_policy"].clone()).unwrap();
            bad["saved_digest"] = json!(
                policy
                    .versioned_digest(bad["saved_revision"].as_u64().unwrap())
                    .unwrap()
            );
            assert!(!confirmed(&bad, &req));
        }
        assert_eq!(service.read_config().unwrap()["unrelated"], "retain");
    }
    #[test]
    fn sampling_only_policy_set_reset_and_report_substitution_are_fenced() {
        let (_directory, service) = fixture();
        let before = service
            .model_lifecycle_policy_command(request(PolicyCommand::Show {}, None), false, true)
            .unwrap();
        let command:PolicyCommand=serde_json::from_value(json!({"operation":"set","expected_digest":before["saved_digest"],
            "interpreter_sampling":{"temperature_milli":1000,"top_p_milli":1000,"top_k":20,"min_p_milli":0,
                "presence_penalty_milli":2000,"repeat_penalty_milli":1000,"seed":42}})).unwrap();
        let req = request(command.clone(), None);
        req.validate().unwrap();
        let good = service
            .model_lifecycle_policy_command(req.clone(), false, true)
            .unwrap();
        assert!(confirmed(&good, &req));
        assert_eq!(good["saved_policy"]["interpreter_sampling"]["seed"], 42);
        assert_eq!(good["applied_policy"], Value::Null);
        assert_eq!(good["restart_requested"], false);
        for (key, value) in [
            ("temperature_milli", json!(0)),
            ("top_p_milli", json!(500)),
            ("top_k", json!(40)),
            ("min_p_milli", json!(100)),
            ("presence_penalty_milli", json!(0)),
            ("repeat_penalty_milli", json!(1100)),
            ("seed", json!(1)),
        ] {
            let mut bad = good.clone();
            bad["saved_policy"]["interpreter_sampling"][key] = value;
            let substituted: ModelLifecyclePolicy =
                serde_json::from_value(bad["saved_policy"].clone()).unwrap();
            bad["saved_digest"] = json!(
                substituted
                    .versioned_digest(bad["saved_revision"].as_u64().unwrap())
                    .unwrap()
            );
            assert!(!confirmed(&bad, &req));
        }
        assert!(
            service
                .model_lifecycle_policy_command(request(command, None), false, true)
                .is_err()
        );
        let reset: PolicyCommand = serde_json::from_value(
            json!({"operation":"set","expected_digest":good["saved_digest"],
            "interpreter_sampling":{"reset":true}}),
        )
        .unwrap();
        let req = request(reset, None);
        let reset = service
            .model_lifecycle_policy_command(req.clone(), false, true)
            .unwrap();
        assert!(confirmed(&reset, &req));
        assert!(reset["saved_policy"].get("interpreter_sampling").is_none());
        assert_eq!(service.read_config().unwrap()["unrelated"], "retain");
    }
    #[test]
    fn invalid_sampling_wire_choices_fail_before_config_changes() {
        let (_directory, service) = fixture();
        let before = service.read_config().unwrap();
        let digest = ModelLifecyclePolicy::default().versioned_digest(0).unwrap();
        for raw in [
            json!({}),
            json!({"top_p_milli":0}),
            json!({"reset":true,"seed":42}),
            json!({"seed":u32::MAX}),
        ] {
            let command = serde_json::from_value(
                json!({"operation":"set","expected_digest":digest,"interpreter_sampling":raw}),
            )
            .unwrap();
            assert!(
                service
                    .model_lifecycle_policy_command(request(command, None), false, true)
                    .is_err()
            );
            assert_eq!(service.read_config().unwrap(), before);
        }
        assert!(
            serde_json::from_value::<PolicyCommand>(
                json!({"operation":"set","expected_digest":digest,"interpreter_sampling":null})
            )
            .is_err()
        );
    }
    #[test]
    fn ollama_preferences_normalize_legacy_policy_and_enforce_bounds() {
        assert_eq!(
            ModelLifecyclePolicy::from_config(&json!({}))
                .unwrap()
                .ollama_keep_alive_seconds,
            None
        );
        for seconds in [json!(null), json!(-1), json!(0), json!(1), json!(86400)] {
            let policy = ModelLifecyclePolicy::from_config(
                &json!({"model_lifecycle":{"ollama_keep_alive_seconds":seconds}}),
            )
            .unwrap();
            assert_eq!(json!(policy.ollama_keep_alive_seconds), seconds);
        }
        for seconds in [
            json!(-2),
            json!(86401),
            json!("120"),
            json!(1.5),
            json!(true),
        ] {
            assert!(
                ModelLifecyclePolicy::from_config(
                    &json!({"model_lifecycle":{"ollama_keep_alive_seconds":seconds}})
                )
                .is_err()
            );
        }
    }
    #[test]
    fn ollama_only_set_reset_and_conflicting_wire_choices_are_validated() {
        let old = ModelLifecyclePolicy::default();
        let raw = json!({"operation":"set","expected_digest":old.versioned_digest(0).unwrap(),"ollama_keep_alive_seconds":120});
        let set: PolicyCommand = serde_json::from_value(raw.clone()).unwrap();
        request(set.clone(), None).validate().unwrap();
        let saved = set.apply(&old, 0).unwrap();
        assert_eq!(saved.ollama_keep_alive_seconds, Some(120));
        assert_eq!(saved.startup, old.startup);
        let mut reset = raw.clone();
        reset["expected_digest"] = json!(saved.versioned_digest(1).unwrap());
        reset["ollama_keep_alive_seconds"] = Value::Null;
        reset["ollama_keep_alive_default"] = json!(true);
        let reset: PolicyCommand = serde_json::from_value(reset).unwrap();
        request(reset.clone(), None).validate().unwrap();
        assert_eq!(reset.apply(&saved, 1).unwrap(), old);
        for (seconds, clear) in [
            (Some(-2), false),
            (Some(86401), false),
            (Some(120), true),
            (None, false),
        ] {
            let mut bad = raw.clone();
            bad["ollama_keep_alive_seconds"] = json!(seconds);
            bad["ollama_keep_alive_default"] = json!(clear);
            let bad: PolicyCommand = serde_json::from_value(bad).unwrap();
            assert!(request(bad, None).validate().is_err());
        }
    }
    #[test]
    fn ollama_saved_preference_substitution_cannot_confirm_even_with_recomputed_digest() {
        let (_directory, service) = fixture();
        let show = service
            .model_lifecycle_policy_command(request(PolicyCommand::Show {}, None), false, true)
            .unwrap();
        let cmd:PolicyCommand=serde_json::from_value(json!({"operation":"set","expected_digest":show["saved_digest"],"ollama_keep_alive_seconds":120})).unwrap();
        let req = request(cmd, None);
        let good = service
            .model_lifecycle_policy_command(req.clone(), false, true)
            .unwrap();
        assert!(confirmed(&good, &req));
        for seconds in [json!(null), json!(-1), json!(0), json!(121)] {
            let mut bad = good.clone();
            bad["saved_policy"]["ollama_keep_alive_seconds"] = seconds;
            let substituted: ModelLifecyclePolicy =
                serde_json::from_value(bad["saved_policy"].clone()).unwrap();
            bad["saved_digest"] = json!(
                substituted
                    .versioned_digest(bad["saved_revision"].as_u64().unwrap())
                    .unwrap()
            );
            assert!(!confirmed(&bad, &req));
        }
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
