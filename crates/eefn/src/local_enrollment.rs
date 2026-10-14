//! Explicit same-PC private enrollment, with no write retries or browser owner.
use crate::{enrollment as contract, setup_check::CheckRequest};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const NOTE: &str = "Enrollment is not atomic or a durable deduplication receipt. Inspect setup/connection after partial or unknown outcomes before another explicit command. No feature permissions, model downloads, workload or automatic write retry was requested. App restart requests apply other pending settings too.";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub expected_runtime_id: String,
    pub request_id: String,
    pub expected_coordinator_runtime_id: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub expected_fingerprint: Option<String>,
    pub allow_coordinator_restart: bool,
}
impl Request {
    pub fn check(&self) -> CheckRequest {
        CheckRequest {
            schema_version: self.schema_version,
            expected_node_id: self.expected_node_id.clone(),
            expected_runtime_id: self.expected_runtime_id.clone(),
            request_id: self.request_id.clone(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        self.check().validate()?;
        if !contract::runtime_valid(&self.expected_coordinator_runtime_id)
            || self
                .expected_fingerprint
                .as_ref()
                .is_some_and(|fp| !contract::hash_valid(fp))
        {
            bail!("invalid enrollment coordinator context")
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    Busy,
    InvalidTarget,
    MissingCoordinator,
    StalePlan,
    RestartConsentRequired,
    Revoked,
    OtherTrust,
    CoordinatorPreparationUnconfirmed,
    CoordinatorRestartUnconfirmed,
    CoordinatorStateUnconfirmed,
    NodeStateChanged,
    TimedOut,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub node_id: String,
    pub previous_runtime_id: String,
    pub request_id: String,
    pub coordinator_prepare_attempted: bool,
    pub coordinator_prepared: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    pub coordinator_changed: Option<bool>,
    pub coordinator_restart_attempted: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    pub coordinator_restart_requested: Option<bool>,
    pub coordinator_applied_verified: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    pub node_saved: Option<bool>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub node_restart_requested: Option<bool>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub connection_enabled: Option<bool>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub error_code: Option<Failure>,
    pub outcome_unknown: bool,
    pub registration_verified: bool,
    pub execution_authorized: bool,
    pub automatically_retried: bool,
    pub note: String,
}
impl Report {
    pub fn new(request: &Request) -> Self {
        Self {
            schema_version: 1,
            report_type: "node_local_enrollment".into(),
            success: false,
            node_id: request.expected_node_id.clone(),
            previous_runtime_id: request.expected_runtime_id.clone(),
            request_id: request.request_id.clone(),
            coordinator_prepare_attempted: false,
            coordinator_prepared: false,
            coordinator_changed: Some(false),
            coordinator_restart_attempted: false,
            coordinator_restart_requested: Some(false),
            coordinator_applied_verified: false,
            node_saved: Some(false),
            node_restart_requested: Some(false),
            connection_enabled: None,
            error_code: None,
            outcome_unknown: false,
            registration_verified: false,
            execution_authorized: false,
            automatically_retried: false,
            note: NOTE.into(),
        }
    }
    fn valid(&self, request: &Request) -> bool {
        request.validate().is_ok()
            && self.schema_version == 1
            && self.report_type == "node_local_enrollment"
            && self.node_id == request.expected_node_id
            && self.previous_runtime_id == request.expected_runtime_id
            && self.request_id == request.request_id
            && self.note == NOTE
            && !self.registration_verified
            && !self.execution_authorized
            && !self.automatically_retried
            && self.outcome_unknown
                == (self.coordinator_changed.is_none()
                    || self.coordinator_restart_requested.is_none()
                    || self.node_saved.is_none()
                    || (self.coordinator_restart_attempted && !self.coordinator_applied_verified))
            && (!self.coordinator_prepared || self.coordinator_prepare_attempted)
            && (!self.coordinator_restart_attempted || self.coordinator_prepared)
            && (self.coordinator_restart_requested != Some(true)
                || self.coordinator_restart_attempted)
            && (!self.coordinator_applied_verified || self.coordinator_prepared)
            && (self.node_saved != Some(true)
                || (self.coordinator_applied_verified && self.node_restart_requested == Some(true)))
            && (self.node_restart_requested != Some(true) || self.node_saved == Some(true))
            && if self.success {
                self.error_code.is_none()
                    && !self.outcome_unknown
                    && self.node_saved == Some(true)
                    && self.connection_enabled.is_some()
                    && self.coordinator_changed.is_some()
                    && self.coordinator_restart_requested.is_some()
            } else {
                self.error_code.is_some()
            }
    }
    fn failed(&mut self, error: Failure) {
        self.error_code = Some(error);
        self.outcome_unknown = self.coordinator_changed.is_none()
            || self.coordinator_restart_requested.is_none()
            || self.node_saved.is_none()
            || (self.coordinator_restart_attempted && !self.coordinator_applied_verified);
    }
    pub fn lost(request: &Request) -> Self {
        let mut report = Self::new(request);
        // A lost node-owner reply cannot establish which phase was reached.
        report.coordinator_prepare_attempted = true;
        report.coordinator_changed = None;
        report.coordinator_restart_requested = None;
        report.node_saved = None;
        report.node_restart_requested = None;
        report.failed(Failure::CoordinatorPreparationUnconfirmed);
        report
    }
}
pub fn confirmed(value: &Value, request: &Request) -> bool {
    serde_json::from_value::<Report>(value.clone()).is_ok_and(|report| report.valid(request))
}
pub fn render(value: &Value) -> Result<String> {
    let report: Report = serde_json::from_value(value.clone())?;
    let text = if report.success {
        if report.connection_enabled == Some(false) {
            "Private enrollment saved. Node app restart requested; outgoing connections remain paused."
        } else {
            "Private enrollment saved. Node app restart requested; registration is not yet verified."
        }
    } else if report.outcome_unknown {
        "Enrollment outcome unconfirmed. Inspect setup and connection before another explicit attempt."
    } else {
        "Enrollment was not completed. Inspect setup and the error code before another explicit attempt."
    };
    Ok(format!(
        "{text}\nError: {:?}\n{}\n",
        report.error_code, NOTE
    ))
}
async fn body<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> std::result::Result<T, ()> {
    if !response.status().is_success() {
        return Err(());
    }
    let value = crate::model_manager::bounded_json(response)
        .await
        .map_err(|_| ())?;
    serde_json::from_value(value).map_err(|_| ())
}

impl crate::NodeService {
    pub async fn enroll_local(&self, request: Request) -> Result<Value> {
        request.validate()?;
        let mut report = Report::new(&request);
        let _permit = match self.local_enrollment.try_lock() {
            Ok(permit) => permit,
            Err(_) => {
                report.failed(Failure::Busy);
                return Ok(serde_json::to_value(report)?);
            }
        };
        let work = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            self.enroll_local_on(&request, &mut report),
        )
        .await;
        match work {
            Ok(Ok(())) => {}
            Ok(Err(error)) => report.failed(error),
            Err(_) => report.failed(Failure::TimedOut),
        }
        if !report.valid(&request) {
            bail!("local enrollment report unconfirmed")
        }
        Ok(serde_json::to_value(report)?)
    }
    async fn enroll_local_on(
        &self,
        request: &Request,
        report: &mut Report,
    ) -> std::result::Result<(), Failure> {
        let current = || {
            self.node_id == request.expected_node_id
                && self.live.lock().unwrap()["runtime_id"] == request.expected_runtime_id
        };
        if !current() {
            return Err(Failure::InvalidTarget);
        }
        let snapshot = self
            .inspect_grant_configuration(|config| {
                if config["node_id"] != self.node_id {
                    bail!("node identity changed")
                }
                Ok(contract::connection_snapshot(config))
            })
            .map_err(|_| Failure::InvalidTarget)?;
        if !matches!(
            snapshot["auth_mode"].as_str(),
            None | Some("legacy" | "node_credential")
        ) {
            return Err(Failure::OtherTrust);
        }
        let advertisement = crate::local_commands::owner_coordinator_path()
            .and_then(|path| {
                crate::local_commands::read_owner_coordinator(&path)
                    .ok()
                    .flatten()
            })
            .ok_or(Failure::MissingCoordinator)?;
        if advertisement.runtime_id != request.expected_coordinator_runtime_id {
            return Err(Failure::StalePlan);
        }
        let plan = crate::local_setup::inspect(&request.check(), Some(advertisement.clone()))
            .await
            .map_err(|_| Failure::StalePlan)?;
        if plan.coordinator_runtime_id.as_deref() != Some(&request.expected_coordinator_runtime_id)
            || plan.coordinator_fingerprint != request.expected_fingerprint
        {
            return Err(Failure::StalePlan);
        }
        if plan.state == crate::local_setup::State::Revoked {
            return Err(Failure::Revoked);
        }
        if plan.state == crate::local_setup::State::TrustIncomplete {
            return Err(Failure::OtherTrust);
        }
        if (plan.state == crate::local_setup::State::Unprovisioned
            || plan.coordinator_restart_required == Some(true))
            && !request.allow_coordinator_restart
        {
            return Err(Failure::RestartConsentRequired);
        }
        if snapshot["auth_mode"] == "node_credential" {
            let pin = snapshot["coordinator_certificate"]
                .as_str()
                .and_then(|certificate| crate::tls::certificate_fingerprint(certificate).ok());
            if pin.is_none() || pin != plan.coordinator_fingerprint {
                return Err(Failure::OtherTrust);
            }
        }
        if !current() {
            return Err(Failure::InvalidTarget);
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|_| Failure::CoordinatorPreparationUnconfirmed)?;
        let prepare = contract::PrepareRequest {
            schema_version: 1,
            expected_runtime_id: advertisement.runtime_id.clone(),
            node_id: self.node_id.clone(),
            address: advertisement.node_address,
            expected_fingerprint: plan.coordinator_fingerprint,
            allow_coordinator_restart: request.allow_coordinator_restart,
        };
        report.coordinator_prepare_attempted = true;
        report.coordinator_changed = None;
        let prepared: contract::Prepared = body(
            client
                .post(format!(
                    "http://{}/api/commands/setup/enrollment",
                    advertisement.owner_address
                ))
                .json(&prepare)
                .send()
                .await
                .map_err(|_| Failure::CoordinatorPreparationUnconfirmed)?,
        )
        .await
        .map_err(|_| Failure::CoordinatorPreparationUnconfirmed)?;
        prepared
            .validate(&prepare)
            .map_err(|_| Failure::CoordinatorPreparationUnconfirmed)?;
        report.coordinator_changed = Some(prepared.changed);
        report.coordinator_prepared = true;
        if prepared.restart_required {
            let restart = contract::RestartRequest {
                schema_version: 1,
                expected_runtime_id: prepared.runtime_id.clone(),
                expected_authentication_digest: prepared.authentication_digest.clone(),
            };
            report.coordinator_restart_attempted = true;
            report.coordinator_restart_requested = None;
            let ack: contract::RestartReport = body(
                client
                    .post(format!(
                        "http://{}/api/commands/setup/restart",
                        advertisement.owner_address
                    ))
                    .json(&restart)
                    .send()
                    .await
                    .map_err(|_| Failure::CoordinatorRestartUnconfirmed)?,
            )
            .await
            .map_err(|_| Failure::CoordinatorRestartUnconfirmed)?;
            if !ack.valid(&restart) {
                return Err(Failure::CoordinatorRestartUnconfirmed);
            }
            report.coordinator_restart_requested = Some(true);
        }
        let applied = async {
            loop {
                if !current() {
                    return Err(Failure::InvalidTarget);
                }
                if let Ok(response) = client
                    .get(format!(
                        "http://{}/api/commands/setup/enrollment",
                        advertisement.owner_address
                    ))
                    .send()
                    .await
                {
                    if let Ok(status) = body::<contract::Status>(response).await {
                        if !status.valid() {
                            return Err(Failure::CoordinatorStateUnconfirmed);
                        }
                        let runtime_matches = if prepared.restart_required {
                            status.runtime_id != prepared.runtime_id
                        } else {
                            status.runtime_id == prepared.runtime_id
                        };
                        if runtime_matches {
                            if status.saved_digest != prepared.authentication_digest
                                || status.applied_digest != prepared.authentication_digest
                                || status.fingerprint.as_deref() != Some(&prepared.fingerprint)
                            {
                                return Err(Failure::CoordinatorStateUnconfirmed);
                            }
                            return Ok(());
                        }
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(45), applied)
            .await
            .map_err(|_| Failure::CoordinatorStateUnconfirmed)??;
        report.coordinator_applied_verified = true;
        let invitation =
            crate::credential_pairing::Invitation::parse(&prepared.code, &self.node_id)
                .map_err(|_| Failure::CoordinatorPreparationUnconfirmed)?;
        report.node_saved = None;
        report.node_restart_requested = None;
        let enabled = self
            .save_credential_pair_checked(
                &invitation.address,
                &invitation.psk,
                &invitation.coordinator_certificate,
                Some(&request.expected_runtime_id),
                Some(&snapshot),
            )
            .map_err(|_| Failure::NodeStateChanged)?;
        report.node_saved = Some(true);
        report.node_restart_requested = Some(true);
        report.connection_enabled = Some(enabled);
        report.success = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn request() -> Request {
        Request {
            schema_version: 1,
            expected_node_id: "stable".into(),
            expected_runtime_id: uuid::Uuid::new_v4().to_string(),
            request_id: uuid::Uuid::new_v4().to_string(),
            expected_coordinator_runtime_id: uuid::Uuid::new_v4().to_string(),
            expected_fingerprint: None,
            allow_coordinator_restart: true,
        }
    }
    #[test]
    fn mutation_reports_reject_false_success_unknowns_and_secret_fields() {
        let request = request();
        let lost = serde_json::to_value(Report::lost(&request)).unwrap();
        assert!(confirmed(&lost, &request));
        assert_eq!(lost["coordinator_restart_requested"], Value::Null);
        for (field, value) in [
            ("success", json!(true)),
            ("outcome_unknown", json!(false)),
            ("registration_verified", json!(true)),
            ("execution_authorized", json!(true)),
            ("automatically_retried", json!(true)),
            ("code", json!("PRIVATE")),
            ("note", json!("PRIVATE")),
        ] {
            let mut bad = lost.clone();
            bad[field] = value;
            assert!(!confirmed(&bad, &request), "{field}");
        }
        let mut missing = lost;
        missing.as_object_mut().unwrap().remove("node_saved");
        assert!(!confirmed(&missing, &request));
        let mut wire = serde_json::to_value(&request).unwrap();
        wire["rotate"] = json!(true);
        assert!(serde_json::from_value::<Request>(wire).is_err());
    }
    #[tokio::test]
    async fn concurrent_enrollment_and_stale_targets_never_write_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let core = crate::NodeService::new(path.clone(), "stable".into());
        core.save_config(json!({"node_id":"stable","custom":42}))
            .unwrap();
        let request = request();
        core.live.lock().unwrap()["runtime_id"] = json!(request.expected_runtime_id);
        let before = std::fs::read(&path).unwrap();
        let permit = core.local_enrollment.lock().await;
        let busy = core.enroll_local(request.clone()).await.unwrap();
        assert_eq!(busy["error_code"], "busy");
        assert_eq!(busy["coordinator_prepare_attempted"], false);
        drop(permit);
        let mut stale = request;
        stale.expected_runtime_id = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            core.enroll_local(stale).await.unwrap()["error_code"],
            "invalid_target"
        );
        assert!(std::fs::read(&path).unwrap() == before);
    }
    #[test]
    fn pairing_cas_preserves_concurrent_connection_decisions_and_latest_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let core = crate::NodeService::new(path.clone(), "stable".into());
        core.save_config(json!({"node_id":"stable","connection_enabled":true,"permissions":{"remote_updates":false},"custom":42})).unwrap();
        let runtime = uuid::Uuid::new_v4().to_string();
        core.live.lock().unwrap()["runtime_id"] = json!(runtime);
        let snapshot = contract::connection_snapshot(&core.read_config().unwrap());
        let mut config = core.read_config().unwrap();
        config["connection_enabled"] = json!(false);
        core.save_config(config).unwrap();
        let before = std::fs::read(&path).unwrap();
        let tls = crate::tls::CoordinatorIdentity::generate().unwrap();
        assert!(
            core.save_credential_pair_checked(
                "127.0.0.1:12345",
                &crate::authentication::generate_credential(),
                &tls.certificate,
                Some(&runtime),
                Some(&snapshot)
            )
            .is_err()
        );
        assert!(std::fs::read(&path).unwrap() == before);
        let snapshot = contract::connection_snapshot(&core.read_config().unwrap());
        let mut config = core.read_config().unwrap();
        config["permissions"]["remote_updates"] = json!(true);
        core.save_config(config).unwrap();
        assert!(
            !core
                .save_credential_pair_checked(
                    "127.0.0.1:12345",
                    &crate::authentication::generate_credential(),
                    &tls.certificate,
                    Some(&runtime),
                    Some(&snapshot)
                )
                .unwrap()
        );
        assert_eq!(
            core.read_config().unwrap()["permissions"]["remote_updates"],
            true
        );
    }
}
