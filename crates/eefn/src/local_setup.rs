//! Read-only same-PC enrollment planning. Discovery metadata is not trust.
use crate::{local_commands::OwnerCoordinator, setup_check::CheckRequest};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const NOTE: &str = "Read-only local setup plan. Discovery is not trust or connectivity proof. No credential, permission, model or connection preference was changed; no pairing, restart, probe, download or workload was submitted.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    NotFound,
    Unavailable,
    Stale,
    Unprovisioned,
    Revoked,
    TrustIncomplete,
    Provisioned,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub node_id: String,
    pub runtime_id: String,
    pub request_id: String,
    pub state: State,
    pub coordinator_runtime_id: Option<String>,
    pub coordinator_fingerprint: Option<String>,
    pub coordinator_restart_required: Option<bool>,
    pub changed: bool,
    pub execution_authorized: bool,
    pub pairing_submitted: bool,
    pub restart_requested: bool,
    pub note: String,
}
impl Report {
    fn new(request: &CheckRequest) -> Self {
        Self {
            schema_version: 1,
            report_type: "node_local_setup_plan".into(),
            success: true,
            node_id: request.expected_node_id.clone(),
            runtime_id: request.expected_runtime_id.clone(),
            request_id: request.request_id.clone(),
            state: State::NotFound,
            coordinator_runtime_id: None,
            coordinator_fingerprint: None,
            coordinator_restart_required: None,
            changed: false,
            execution_authorized: false,
            pairing_submitted: false,
            restart_requested: false,
            note: NOTE.into(),
        }
    }
    fn valid(&self, request: &CheckRequest) -> bool {
        let fresh = !matches!(
            self.state,
            State::NotFound | State::Unavailable | State::Stale
        );
        let runtime_valid = self.coordinator_runtime_id.as_ref().is_some_and(|runtime| {
            uuid::Uuid::parse_str(runtime)
                .is_ok_and(|id| !id.is_nil() && id.to_string() == *runtime)
        });
        let fingerprint_valid = self
            .coordinator_fingerprint
            .as_ref()
            .is_none_or(|value| value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()));
        request.validate().is_ok()
            && self.schema_version == 1
            && self.report_type == "node_local_setup_plan"
            && self.success
            && self.node_id == request.expected_node_id
            && self.runtime_id == request.expected_runtime_id
            && self.request_id == request.request_id
            && !self.changed
            && !self.execution_authorized
            && !self.pairing_submitted
            && !self.restart_requested
            && self.note == NOTE
            && fingerprint_valid
            && if fresh {
                runtime_valid
                    && self.coordinator_restart_required.is_some()
                    && (self.state != State::Provisioned || self.coordinator_fingerprint.is_some())
                    && (self.state != State::TrustIncomplete
                        || self.coordinator_fingerprint.is_none())
            } else {
                self.coordinator_runtime_id.is_none()
                    && self.coordinator_fingerprint.is_none()
                    && self.coordinator_restart_required.is_none()
            }
    }
}
pub fn confirmed(value: &Value, request: &CheckRequest) -> bool {
    serde_json::from_value::<Report>(value.clone()).is_ok_and(|report| report.valid(request))
}
pub fn render(value: &Value) -> Result<String> {
    let report: Report = serde_json::from_value(value.clone())?;
    let request = CheckRequest {
        schema_version: 1,
        expected_node_id: report.node_id.clone(),
        expected_runtime_id: report.runtime_id.clone(),
        request_id: report.request_id.clone(),
    };
    if !report.valid(&request) {
        bail!("local setup plan unconfirmed")
    }
    let state = match report.state {
        State::NotFound => "No local coordinator advertisement. Start eef on this PC.",
        State::Unavailable => {
            "The advertised coordinator owner API is unavailable. No fallback was used."
        }
        State::Stale => "The local coordinator advertisement is stale. No trust was inferred.",
        State::Unprovisioned => {
            "Local coordinator found. This node needs explicit private enrollment."
        }
        State::Revoked => {
            "The coordinator owner revoked this node. Setup will not restore its credential."
        }
        State::TrustIncomplete => {
            "A saved node credential exists, but coordinator TLS trust is missing."
        }
        State::Provisioned => {
            "Saved private node trust exists. This does not prove pairing or registration."
        }
    };
    let pending = if report.coordinator_restart_required == Some(true) {
        "Coordinator trust changes are pending; applying them requires explicit restart.\n"
    } else {
        ""
    };
    Ok(format!(
        "{state}\n{pending}No settings changed. Private enrollment/apply automation remains unfinished.\n"
    ))
}

pub(crate) async fn inspect(
    request: &CheckRequest,
    advertisement: Option<OwnerCoordinator>,
) -> Result<Report> {
    let mut report = Report::new(request);
    let Some(advertisement) = advertisement else {
        return Ok(report);
    };
    advertisement.validate()?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    let response = match client
        .get(format!(
            "http://{}/api/commands/credentials",
            advertisement.owner_address
        ))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => response,
        _ => {
            report.state = State::Unavailable;
            return Ok(report);
        }
    };
    let value = crate::model_manager::bounded_json(response)
        .await
        .map_err(|_| anyhow::anyhow!("local coordinator inspection unconfirmed"))?;
    const FIELDS: &[&str] = &[
        "schema_version",
        "report_type",
        "success",
        "changed",
        "saved_policy",
        "applied_policy",
        "restart_required",
        "execution_authorized",
        "note",
        "runtime_id",
    ];
    if value.as_object().is_none_or(|object| {
        object.len() != FIELDS.len() || object.keys().any(|key| !FIELDS.contains(&key.as_str()))
    }) || value["schema_version"] != 1
        || value["report_type"] != "node_credentials"
        || value["success"] != true
        || value["changed"] != false
        || value["execution_authorized"] != false
        || !value["restart_required"].is_boolean()
        || !crate::authentication::valid_summary(&value["saved_policy"])
        || !crate::authentication::valid_summary(&value["applied_policy"])
    {
        bail!("local coordinator inspection unconfirmed")
    }
    if value["runtime_id"] != advertisement.runtime_id {
        report.state = State::Stale;
        return Ok(report);
    }
    let saved = &value["saved_policy"];
    let identity = saved["identities"]
        .as_array()
        .expect("validated summary")
        .iter()
        .find(|id| id["node_id"] == request.expected_node_id);
    report.coordinator_runtime_id = Some(advertisement.runtime_id);
    report.coordinator_fingerprint = saved["coordinator_fingerprint"].as_str().map(str::to_owned);
    report.coordinator_restart_required = value["restart_required"].as_bool();
    report.state = match identity.map(|id| id["state"].as_str().expect("validated state")) {
        None => State::Unprovisioned,
        Some("revoked") => State::Revoked,
        _ if report.coordinator_fingerprint.is_none() => State::TrustIncomplete,
        _ => State::Provisioned,
    };
    Ok(report)
}

impl crate::NodeService {
    pub async fn local_setup_plan(&self, request: CheckRequest) -> Result<Value> {
        request.validate()?;
        let check_runtime = || -> Result<()> {
            if request.expected_node_id != self.node_id
                || self.live.lock().unwrap()["runtime_id"] != request.expected_runtime_id
            {
                bail!("local setup plan target or runtime changed")
            }
            Ok(())
        };
        check_runtime()?;
        let advertisement = match crate::local_commands::owner_coordinator_path() {
            Some(path) => crate::local_commands::read_owner_coordinator(&path)?,
            None => None,
        };
        let report = inspect(&request, advertisement).await?;
        check_runtime()?;
        if !report.valid(&request) {
            bail!("local setup plan unconfirmed")
        }
        Ok(serde_json::to_value(report)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    fn request() -> CheckRequest {
        CheckRequest {
            schema_version: 1,
            expected_node_id: "stable".into(),
            expected_runtime_id: uuid::Uuid::new_v4().to_string(),
            request_id: uuid::Uuid::new_v4().to_string(),
        }
    }
    #[tokio::test]
    async fn planning_is_read_only_and_distinguishes_missing_stale_and_saved_trust() {
        let request = request();
        let absent = serde_json::to_value(inspect(&request, None).await.unwrap()).unwrap();
        assert!(confirmed(&absent, &request));
        assert_eq!(absent["state"], "not_found");
        let runtime = uuid::Uuid::new_v4().to_string();
        let summary = crate::authentication::AuthenticationPolicy::default().summary();
        let value = Arc::new(Mutex::new(
            json!({"schema_version":1,"report_type":"node_credentials",
            "success":true,"changed":false,"saved_policy":summary,"applied_policy":summary,
            "restart_required":false,"execution_authorized":false,"note":"not echoed","runtime_id":runtime}),
        ));
        let posts = Arc::new(AtomicUsize::new(0));
        let output = value.clone();
        let writes = posts.clone();
        let router = axum::Router::new().route(
            "/api/commands/credentials",
            axum::routing::get(move || {
                let snapshot = output.lock().unwrap().clone();
                async move { axum::Json(snapshot) }
            })
            .post(move || {
                writes.fetch_add(1, Ordering::SeqCst);
                async { axum::Json(json!({})) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let advertisement = OwnerCoordinator {
            schema_version: 1,
            owner_address: address,
            node_address: address,
            runtime_id: runtime.clone(),
        };
        for (state, identities, tls) in [
            (State::Unprovisioned, json!([]), false),
            (
                State::Revoked,
                json!([{"node_id":"stable","state":"revoked"}]),
                false,
            ),
            (
                State::TrustIncomplete,
                json!([{"node_id":"stable","state":"provisioned"}]),
                false,
            ),
            (
                State::Provisioned,
                json!([{"node_id":"stable","state":"provisioned"}]),
                true,
            ),
        ] {
            {
                let mut output = value.lock().unwrap();
                output["saved_policy"]["identities"] = identities;
                output["saved_policy"]["tls_configured"] = json!(tls);
                output["saved_policy"]["coordinator_fingerprint"] = if tls {
                    json!("a".repeat(64))
                } else {
                    Value::Null
                };
                output["restart_required"] = json!(true);
            }
            let report = inspect(&request, Some(advertisement.clone()))
                .await
                .unwrap();
            assert_eq!(report.state, state);
            assert!(report.valid(&request));
            let serialized = serde_json::to_value(report).unwrap();
            assert!(confirmed(&serialized, &request));
            assert!(!render(&serialized).unwrap().contains("not echoed"));
        }
        value.lock().unwrap()["runtime_id"] = json!(uuid::Uuid::new_v4().to_string());
        let stale = inspect(&request, Some(advertisement.clone()))
            .await
            .unwrap();
        assert_eq!(stale.state, State::Stale);
        assert!(stale.valid(&request));
        value.lock().unwrap()["runtime_id"] = json!(runtime);
        value.lock().unwrap()["code"] = json!("PRIVATE");
        let error = inspect(&request, Some(advertisement.clone()))
            .await
            .unwrap_err()
            .to_string();
        assert!(!error.contains("PRIVATE"));
        assert_eq!(posts.load(Ordering::SeqCst), 0);
        server.abort();
    }
    #[test]
    fn plan_confirmation_rejects_extra_authority_and_false_freshness() {
        let request = request();
        let value = serde_json::to_value(Report::new(&request)).unwrap();
        assert!(confirmed(&value, &request));
        for (field, entry) in [
            ("changed", json!(true)),
            ("pairing_submitted", json!(true)),
            ("restart_requested", json!(true)),
            ("execution_authorized", json!(true)),
            (
                "coordinator_runtime_id",
                json!(uuid::Uuid::new_v4().to_string()),
            ),
            ("note", json!("PRIVATE")),
            ("code", json!("PRIVATE")),
            ("state", json!("provisioned")),
        ] {
            let mut bad = value.clone();
            bad[field] = entry;
            assert!(!confirmed(&bad, &request), "{field}");
        }
    }
    #[tokio::test]
    async fn wrong_node_or_runtime_is_rejected_without_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let core = crate::NodeService::new(path.clone(), "stable".into());
        core.save_config(json!({"node_id":"stable","custom":42}))
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        let request = request();
        core.live.lock().unwrap()["runtime_id"] = json!(request.expected_runtime_id);
        let mut wrong = request.clone();
        wrong.expected_node_id = "other".into();
        assert!(core.local_setup_plan(wrong).await.is_err());
        let mut wrong = request;
        wrong.expected_runtime_id = uuid::Uuid::new_v4().to_string();
        assert!(core.local_setup_plan(wrong).await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
