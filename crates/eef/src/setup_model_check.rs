//! One-shot owner consent for an exact model after setup, never a default or retry.
use anyhow::{Result, bail};
use eefn::setup_check::ModelRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[path = "setup_model_check_runtime.rs"]
mod runtime;
pub(crate) use runtime::Manager;
#[cfg(test)]
#[path = "setup_model_check_tests.rs"]
pub(crate) mod tests;

pub(crate) const RECORD_BYTES: usize = 4096;
pub const MAX_WAIT_MS: u64 = 300_000;

pub(crate) fn validate_settings(value: Option<&Value>) -> Result<usize> {
    if value.is_some_and(|value| !value.is_object()) {
        bail!("setup_model_checks settings must be an object")
    }
    #[derive(Deserialize)]
    #[serde(default, deny_unknown_fields)]
    struct Limits {
        max_active: u64,
    }
    impl Default for Limits {
        fn default() -> Self {
            Self { max_active: 2 }
        }
    }
    let limits: Limits =
        serde_json::from_value(value.cloned().unwrap_or_else(|| serde_json::json!({})))?;
    if !(1..=8).contains(&limits.max_active) {
        bail!("setup_model_checks.max_active must be between 1 and 8")
    }
    Ok(limits.max_active as usize)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub schema_version: u32,
    pub node_id: String,
    pub operation_id: String,
    pub model: ModelRef,
    pub max_wait_ms: u64,
}
impl Definition {
    pub fn validate(&self) -> Result<()> {
        eefn::network::validate_remote_node_id(&self.node_id)?;
        eefn::automation_occurrence::uuid(&self.operation_id)?;
        eefn::model_selection::validate_id(&self.model.model_id)?;
        if self.schema_version != 1 || !(1000..=MAX_WAIT_MS).contains(&self.max_wait_ms) {
            bail!("Unsupported or unbounded setup model check")
        }
        Ok(())
    }
    pub(crate) fn id(&self) -> Result<String> {
        self.validate()?;
        eefn::model_verification::receipt_id(&self.node_id, &self.operation_id, "source", None)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Waiting,
    Launching,
    Finished,
    Cancelled,
    Expired,
    Unknown,
    /// Derived from missing live ownership, never saved as fresh authority.
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub state: crate::model_verification::State,
    pub node_runtime_id: Option<String>,
    pub job_id: Option<String>,
    pub grant_id: Option<String>,
    pub historical: bool,
    pub outcome_unknown: bool,
    pub inference_returned_text: bool,
}
impl Outcome {
    fn validate(&self) -> Result<()> {
        for value in [&self.node_runtime_id, &self.grant_id]
            .into_iter()
            .flatten()
        {
            eefn::automation_occurrence::uuid(value)?;
        }
        if let Some(job) = &self.job_id {
            eefn::request_drafts::validate_id(job)?;
        }
        if self.inference_returned_text
            != (self.state == crate::model_verification::State::ReturnedText)
            || (self.inference_returned_text
                && (self.outcome_unknown
                    || self.node_runtime_id.is_none()
                    || self.job_id.is_none()
                    || self.grant_id.is_none()))
        {
            bail!("Unconfirmed setup check outcome")
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub schema_version: u32,
    pub definition: Definition,
    pub coordinator_runtime_id: String,
    pub created_at_ms: u64,
    pub state: State,
    pub outcome: Option<Outcome>,
}
impl Record {
    pub(crate) fn validate(&self) -> Result<()> {
        self.definition.validate()?;
        eefn::automation_occurrence::uuid(&self.coordinator_runtime_id)?;
        if self.schema_version != 1
            || self.created_at_ms == 0
            || self.state == State::Interrupted
            || (self.state == State::Finished && self.outcome.is_none())
            || (self.state == State::Finished
                && self.outcome.as_ref().is_some_and(|o| {
                    o.outcome_unknown
                        || !matches!(
                            o.state,
                            crate::model_verification::State::ReturnedText
                                | crate::model_verification::State::NoText
                                | crate::model_verification::State::Failed
                        )
                }))
            || (self.outcome.is_some() && !matches!(self.state, State::Finished | State::Unknown))
        {
            bail!("Damaged or future setup check; retain without replay")
        }
        if let Some(outcome) = &self.outcome {
            outcome.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Arm {
        model: ModelRef,
        max_wait_ms: u64,
        approve_inference: bool,
    },
    Inspect {},
    /// Only fences waiting. It never claims to terminate admitted inference.
    CancelWait {},
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub node_id: String,
    pub operation_id: String,
    pub command: Command,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            bail!("Unsupported setup model check request")
        }
        eefn::automation_occurrence::uuid(&self.expected_runtime_id)?;
        eefn::network::validate_remote_node_id(&self.node_id)?;
        eefn::automation_occurrence::uuid(&self.operation_id)?;
        if let Command::Arm {
            approve_inference, ..
        } = &self.command
        {
            if !approve_inference {
                bail!("Separate explicit inference consent is required")
            }
            self.definition()?.validate()?;
        }
        Ok(())
    }
    pub(crate) fn definition(&self) -> Result<Definition> {
        let Command::Arm {
            model, max_wait_ms, ..
        } = &self.command
        else {
            bail!("Only arm defines setup-check consent")
        };
        Ok(Definition {
            schema_version: 1,
            node_id: self.node_id.clone(),
            operation_id: self.operation_id.clone(),
            model: model.clone(),
            max_wait_ms: *max_wait_ms,
        })
    }
}

#[derive(Debug, clap::Subcommand)]
pub enum Action {
    /// Consent to one fixed check of this exact model once setup is applied.
    Arm {
        #[arg(long, value_parser = ["ollama", "llamacpp"])]
        backend: String,
        #[arg(long)]
        model_id: String,
        #[arg(long, required = true)]
        allow_inference: bool,
        #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..=300))]
        wait_seconds: u64,
        #[arg(long)]
        operation_id: Option<String>,
    },
    Inspect {
        #[arg(long)]
        operation_id: String,
    },
    CancelWait {
        #[arg(long)]
        operation_id: String,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema_version: u32,
    pub report_type: String,
    pub coordinator_runtime_id: String,
    pub node_id: String,
    pub operation_id: String,
    pub command: Command,
    pub success: bool,
    pub record: Option<Record>,
    pub effective_state: Option<State>,
    pub worker_active: bool,
    pub waiting_reason: Option<String>,
    pub consent_created: bool,
    pub waiting_cancelled: bool,
    pub reused: bool,
    pub automatically_retried: bool,
    pub resumed: bool,
    pub model_selection_changed: bool,
    pub model_quality_approved: bool,
    pub current_setup_verified: bool,
}
pub fn confirmed(value: &Value, request: &Request) -> bool {
    let Ok(report) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    if request.validate().is_err()
        || report.schema_version != 1
        || report.report_type != "coordinator_setup_model_check"
        || report.coordinator_runtime_id != request.expected_runtime_id
        || report.node_id != request.node_id
        || report.operation_id != request.operation_id
        || report.command != request.command
        || !report.success
        || report.automatically_retried
        || report.resumed
        || report.model_selection_changed
        || report.model_quality_approved
        || report.current_setup_verified
        || (report.consent_created && !matches!(request.command, Command::Arm { .. }))
        || (report.waiting_cancelled && !matches!(request.command, Command::CancelWait {}))
        || (report.consent_created && report.reused)
    {
        return false;
    }
    match &report.record {
        None => {
            !matches!(request.command, Command::Arm { .. })
                && report.effective_state.is_none()
                && report.waiting_reason.is_none()
                && !report.worker_active
                && !report.consent_created
                && !report.waiting_cancelled
                && !report.reused
        }
        Some(record) => {
            record.validate().is_ok()
                && record.definition.node_id == request.node_id
                && record.definition.operation_id == request.operation_id
                && (!matches!(request.command, Command::Arm { .. })
                    || request
                        .definition()
                        .is_ok_and(|definition| definition == record.definition))
                && report.effective_state
                    == Some(effective_state(
                        record,
                        &request.expected_runtime_id,
                        report.worker_active,
                    ))
                && (!report.worker_active
                    || (record.coordinator_runtime_id == request.expected_runtime_id
                        && matches!(record.state, State::Waiting | State::Launching)))
                && (report.waiting_reason.is_none()
                    || (report.worker_active && record.state == State::Waiting))
                && (!report.reused || matches!(request.command, Command::Arm { .. }))
                && (!matches!(request.command, Command::Arm { .. })
                    || (report.consent_created != report.reused))
                && (!(report.worker_active && record.state == State::Waiting)
                    || report.waiting_reason.is_some())
                && report
                    .waiting_reason
                    .as_deref()
                    .is_none_or(runtime::valid_reason)
                && (!report.waiting_cancelled || record.state == State::Cancelled)
                && (!report.consent_created
                    || record.coordinator_runtime_id == request.expected_runtime_id)
        }
    }
}

pub(crate) fn effective_state(record: &Record, runtime: &str, active: bool) -> State {
    match record.state {
        State::Waiting if !active || record.coordinator_runtime_id != runtime => State::Interrupted,
        State::Launching if !active || record.coordinator_runtime_id != runtime => State::Unknown,
        state => state,
    }
}

pub fn render(value: &Value) -> String {
    let Ok(report) = serde_json::from_value::<Report>(value.clone()) else {
        return "Setup model check unconfirmed; inspect its operation ID before starting another.\n".into();
    };
    let request = Request {
        schema_version: 1,
        expected_runtime_id: report.coordinator_runtime_id.clone(),
        node_id: report.node_id.clone(),
        operation_id: report.operation_id.clone(),
        command: report.command.clone(),
    };
    if !confirmed(value, &request) {
        return "Setup model check unconfirmed; inspect its operation ID before starting another.\n"
            .into();
    }
    let detail = match report.effective_state {
        None => {
            "No retained consent was found; this is not proof that no earlier request was accepted."
        }
        Some(State::Waiting) => {
            "Waiting for the exact model's applied setup and permissions; no model is selected or permission enabled by this check."
        }
        Some(State::Launching) => {
            "The one fixed check may be running. Cancelling the wait no longer stops admitted inference."
        }
        Some(State::Finished) => match report
            .record
            .as_ref()
            .and_then(|r| r.outcome.as_ref())
            .map(|o| o.state)
        {
            Some(crate::model_verification::State::ReturnedText) => {
                "The model answered the fixed setup check. This is historical operation evidence, not model quality or current health approval."
            }
            Some(crate::model_verification::State::NoText) => {
                "The check completed without model text; successful inference has not been verified."
            }
            _ => {
                "The fixed setup check failed. Inspect the original operation; no automatic retry was requested."
            }
        },
        Some(State::Cancelled) => {
            "Waiting cancelled before the launch claim; this consent cannot start a check."
        }
        Some(State::Expired) => {
            "Waiting expired without a launch claim. Reusing this operation does not retry."
        }
        Some(State::Unknown) => {
            "Outcome unknown; inspect the original model verification. No automatic retry or stop proof."
        }
        Some(State::Interrupted) => {
            "The original worker is no longer owned. This consent will not resume after restart."
        }
    };
    format!(
        "{detail}\nOperation: {}\nInspect inference: eef node verify-model --node {} inspect --operation-id {}\n",
        report.operation_id, report.node_id, report.operation_id
    )
}
