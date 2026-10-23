//! Read-only setup guidance from the existing model-setup journal, not a runner.
use super::NextStep;
use crate::{
    model_bootstrap::Saved,
    model_setup::{Phase, State, Status},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize)]
#[serde(tag = "availability", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Observation {
    Observed { policy: Saved, status: Status },
    Unavailable {},
}
impl Observation {
    pub(super) fn read(service: &crate::NodeService, config: &Value, live: &Value) -> Self {
        let Ok(policy) = Saved::from_config(config) else {
            return Self::Unavailable {};
        };
        let status = crate::model_setup::status(service, config, &policy, live);
        if status.validate(&service.node_id, &policy).is_err() {
            return Self::Unavailable {};
        }
        Self::Observed { policy, status }
    }
    pub(super) fn valid(&self, node: &str, pending_restart: Option<bool>) -> bool {
        match self {
            Self::Unavailable {} => true,
            Self::Observed { policy, status } => {
                policy.validate().is_ok()
                    && status.validate(node, policy).is_ok()
                    && (!status.restart_required || pending_restart == Some(true))
            }
        }
    }
    pub(super) fn next_step(&self) -> Option<NextStep> {
        match self {
            Self::Unavailable {} => Some(NextStep::InspectModelSetup),
            Self::Observed { status, .. } => match status.state {
                State::AwaitingApproval => Some(NextStep::ApproveModelSetup),
                State::NoApprovedDefault => Some(NextStep::ChooseModelSetup),
                State::Waiting | State::CatalogChanged | State::EvidenceUnavailable => {
                    Some(NextStep::InspectModelSetup)
                }
                State::Attempt
                    if status.attempt.as_ref().is_some_and(|a| {
                        !matches!(a.phase, Phase::Configured | Phase::RuntimeConfigured)
                    }) =>
                {
                    Some(NextStep::InspectModelSetup)
                }
                _ => None,
            },
        }
    }
    pub(super) fn defers_interpreter_choice(&self) -> bool {
        self.next_step().is_some()
    }
    pub(super) fn defers_restart(&self, runtime: &str) -> bool {
        matches!(self, Self::Observed { status, .. }
            if status.state == State::Attempt && status.current_policy_attempt
                && status.attempt.as_ref().is_some_and(|a| a.runtime_id == runtime
                    && matches!(a.phase, Phase::Approved | Phase::Inspecting | Phase::Downloading | Phase::Verifying | Phase::Committing | Phase::RestartRequested | Phase::RestartDraining)))
    }
    pub(super) fn render(&self, runtime: &str) -> String {
        let Self::Observed { status, .. } = self else {
            return "Model setup: status unavailable; not treated as disabled, running or safe to retry.\n".into();
        };
        let label = match status.state {
            State::Disabled => "automation disabled by the current preference",
            State::Preserved => "current preference preserves existing model choices",
            State::Managed => "models managed outside automatic setup",
            State::AwaitingApproval => "your selected model needs one explicit setup approval",
            State::NoApprovedDefault => {
                "no project default is approved; choose an installed or owner-selected catalog model"
            }
            State::CatalogChanged => {
                "your pinned catalog model changed or is unavailable; review is required"
            }
            State::ExistingSetup => {
                "existing model choices are preserved; no automatic replacement"
            }
            State::Waiting => "automatic setup is permitted, but no current attempt is retained",
            State::EvidenceUnavailable => {
                "retained setup evidence unavailable; no retry or repair is inferred"
            }
            State::Attempt => "current policy has a retained setup attempt",
        };
        let mut text = format!("Model setup: {label}.\n");
        if let Some(attempt) = &status.attempt {
            let phase = match attempt.phase {
                Phase::Approved => "approved; provider inspection not yet recorded",
                Phase::Inspecting => "provider inspection checkpoint",
                Phase::Downloading => {
                    "download checkpoint; use the exact installer receipt for progress"
                }
                Phase::Verifying => "verification checkpoint",
                Phase::Committing => {
                    "configuration commit checkpoint; final outcome not yet recorded"
                }
                Phase::Configured => "model settings saved; activation and inference are separate",
                Phase::RestartRequested => {
                    "automatic runtime restart request retained; not completion proof"
                }
                Phase::RestartDraining => "runtime retirement checkpoint; not physical-stop proof",
                Phase::RuntimeConfigured => {
                    "replacement runtime configuration observed; inference and quality untested"
                }
                Phase::ApplyBlocked => "automatic activation blocked; inspect pending settings",
                Phase::Stopped => "setup stopped; prior provider effects may remain",
                Phase::Failed => "setup failed; inspect its receipt before any new decision",
                Phase::Unknown => "setup outcome unknown; not replayed automatically",
            };
            text.push_str(&format!("Retained attempt {}: {phase}.\n", attempt.id));
            if !status.current_policy_attempt {
                text.push_str(
                    "This attempt belongs to an earlier policy, not current setup progress.\n",
                );
            }
            if attempt.runtime_id != runtime {
                text.push_str("This attempt originated in an earlier runtime; it is not proof of a current worker.\n");
            }
            if attempt.download_admitted {
                text.push_str(&format!(
                    "Inspect transfer evidence: eefn models download-status --id {}\n",
                    attempt.id
                ));
            }
            text.push_str("Retained checkpoints do not prove a live worker, successful inference or model quality.\n");
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model_bootstrap::{CatalogPin, Mode},
        model_selection::Backend,
        model_setup::{ActivationEvidence, Attempt, Reason},
    };
    use serde_json::json;

    fn policy(mode: Mode, restart: bool) -> Saved {
        Saved::from_config(&json!({"model_bootstrap":{"mode":mode,"automation":true,"restart_after_setup":restart,"catalog":CatalogPin{
            schema_version:1, model_id:"fixture".into(), definition_sha256:"a".repeat(64), artifact_sha256:"b".repeat(64),artifact_bytes:32,
            source_uri_sha256:"c".repeat(64),source_page_sha256:"d".repeat(64),declared_license:"test-only".into()
        }}})).unwrap()
    }
    fn observation(phase: Phase) -> Observation {
        let restart = matches!(
            phase,
            Phase::RestartRequested
                | Phase::RestartDraining
                | Phase::RuntimeConfigured
                | Phase::ApplyBlocked
        );
        let policy = policy(Mode::Prompt, restart);
        let reason = match phase {
            Phase::Failed => Some(Reason::TransferFailed),
            Phase::Unknown => Some(Reason::RuntimeInterrupted),
            Phase::Stopped => Some(Reason::StateChanged),
            Phase::ApplyBlocked => Some(Reason::PendingSettings),
            _ => None,
        };
        let activation = matches!(
            phase,
            Phase::RestartRequested | Phase::RestartDraining | Phase::RuntimeConfigured
        )
        .then(|| ActivationEvidence {
            models_sha256: "e".repeat(64),
            replacement_runtime_id: (phase == Phase::RuntimeConfigured)
                .then(|| uuid::Uuid::new_v4().to_string()),
        });
        let attempt = Attempt {
            schema_version: 1,
            node_id: "node".into(),
            id: uuid::Uuid::new_v4().to_string(),
            runtime_id: uuid::Uuid::new_v4().to_string(),
            saved: policy.clone(),
            models_sha256: "f".repeat(64),
            backend: Backend::Ollama,
            phase,
            reason,
            download_admitted: phase != Phase::Approved,
            owner_approved: true,
            activation,
            created_at_ms: 1,
            updated_at_ms: 2,
        };
        let status = Status {
            schema_version: 1,
            node_id: "node".into(),
            policy_digest: policy.digest.clone(),
            state: State::Attempt,
            attempt: Some(attempt),
            current_policy_attempt: true,
            restart_required: restart && phase != Phase::RuntimeConfigured,
            readiness_verified: false,
            automatically_retried: false,
        };
        Observation::Observed { policy, status }
    }
    #[test]
    fn retained_phases_give_guidance_not_worker_or_readiness_claims() {
        for phase in [
            Phase::Approved,
            Phase::Inspecting,
            Phase::Downloading,
            Phase::Verifying,
            Phase::Committing,
            Phase::Configured,
            Phase::RestartRequested,
            Phase::RestartDraining,
            Phase::RuntimeConfigured,
            Phase::ApplyBlocked,
            Phase::Stopped,
            Phase::Failed,
            Phase::Unknown,
        ] {
            let observed = observation(phase);
            assert!(observed.valid("node", Some(true)), "{phase:?}");
            let text = observed.render("other-runtime");
            assert!(text.contains("earlier runtime"));
            assert!(text.contains("do not prove a live worker"));
            assert!(!observed.defers_restart("other-runtime"));
            let Observation::Observed { status, .. } = &observed else {
                unreachable!()
            };
            assert_eq!(
                observed.defers_restart(&status.attempt.as_ref().unwrap().runtime_id),
                matches!(
                    phase,
                    Phase::Approved
                        | Phase::Inspecting
                        | Phase::Downloading
                        | Phase::Verifying
                        | Phase::Committing
                        | Phase::RestartRequested
                        | Phase::RestartDraining
                )
            );
            assert_eq!(
                observed.next_step(),
                if matches!(phase, Phase::Configured | Phase::RuntimeConfigured) {
                    None
                } else {
                    Some(NextStep::InspectModelSetup)
                }
            );
        }
    }
    #[test]
    fn preferences_separate_consent_defaults_and_unavailable_evidence() {
        for (state, expected) in [
            (State::AwaitingApproval, Some(NextStep::ApproveModelSetup)),
            (State::Waiting, Some(NextStep::InspectModelSetup)),
            (State::NoApprovedDefault, Some(NextStep::ChooseModelSetup)),
            (State::CatalogChanged, Some(NextStep::InspectModelSetup)),
            (
                State::EvidenceUnavailable,
                Some(NextStep::InspectModelSetup),
            ),
            (State::ExistingSetup, None),
            (State::Disabled, None),
            (State::Preserved, None),
            (State::Managed, None),
        ] {
            let mut policy = policy(
                if state == State::AwaitingApproval {
                    Mode::Prompt
                } else {
                    Mode::Auto
                },
                false,
            );
            match state {
                State::NoApprovedDefault => policy.policy.catalog = None,
                State::Disabled => policy.policy.automation = false,
                State::Preserved => policy.policy.mode = Mode::Preserve,
                State::Managed => policy.policy.mode = Mode::Managed,
                _ => {}
            }
            policy = Saved::from_config(&json!({"model_bootstrap":policy.policy})).unwrap();
            let observed = Observation::Observed {
                status: Status {
                    schema_version: 1,
                    node_id: "node".into(),
                    policy_digest: policy.digest.clone(),
                    state,
                    attempt: None,
                    current_policy_attempt: false,
                    restart_required: false,
                    readiness_verified: false,
                    automatically_retried: false,
                },
                policy,
            };
            assert!(observed.valid("node", None), "{state:?}");
            assert_eq!(observed.next_step(), expected, "{state:?}");
            assert_eq!(observed.defers_interpreter_choice(), expected.is_some());
        }
    }
    #[test]
    fn corrupted_future_and_positive_health_claims_are_not_confirmed() {
        let value = serde_json::to_value(observation(Phase::Downloading)).unwrap();
        for (pointer, replacement) in [
            ("/status/readiness_verified", json!(true)),
            ("/status/automatically_retried", json!(true)),
            ("/status/current_policy_attempt", json!(false)),
            ("/status/node_id", json!("other")),
            ("/status/policy_digest", json!("0".repeat(64))),
            ("/status/schema_version", json!(99)),
            ("/policy/digest", json!("0".repeat(64))),
        ] {
            let mut bad = value.clone();
            *bad.pointer_mut(pointer).unwrap() = replacement;
            let observed: Observation = serde_json::from_value(bad).unwrap();
            assert!(!observed.valid("node", None), "{pointer}");
        }
        let mut future = value;
        future["extra_private_body"] = json!("not rendered");
        assert!(serde_json::from_value::<Observation>(future).is_err());
        let observed = observation(Phase::RestartRequested);
        assert!(!observed.valid("node", Some(false)));
    }
    #[test]
    fn prior_policy_attempt_is_history_not_current_restart_or_progress() {
        let mut observed = observation(Phase::Downloading);
        let Observation::Observed { status, policy } = &mut observed else {
            unreachable!()
        };
        *policy = Saved::from_config(
            &json!({"model_bootstrap":policy.policy,"model_bootstrap_revision":1}),
        )
        .unwrap();
        status.policy_digest = policy.digest.clone();
        status.current_policy_attempt = false;
        status.state = State::AwaitingApproval;
        assert!(observed.valid("node", None));
        assert!(
            observed
                .render("another-runtime")
                .contains("earlier policy, not current setup progress")
        );
        assert_eq!(observed.next_step(), Some(NextStep::ApproveModelSetup));
        assert!(!observed.defers_restart("another-runtime"));
    }
}
