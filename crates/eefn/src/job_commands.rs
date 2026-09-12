//! Deterministic node job commands over the existing authenticated submission path.
use anyhow::{Result, bail};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize, Subcommand)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum JobCommand {
    /// List this origin node's jobs, not every job on the coordinator.
    List {},
    /// Find retained jobs created by this node with a command receipt ID.
    Find {
        #[arg(long)]
        operation_id: String,
    },
    Get {
        id: String,
    },
    /// Explicitly retrieve bounded text-model output, not files or media.
    Output {
        id: String,
    },
    Pause {
        id: String,
    },
    Resume {
        id: String,
    },
    Stop {
        id: String,
    },
    /// Remove finished history only; does not stop active work.
    Remove {
        id: String,
    },
    /// Submit a durable text generation job, without natural-language planning.
    GenerateText {
        #[arg(long)]
        prompt: String,
        #[arg(long)]
        role: Option<String>,
        #[arg(long)]
        target_node: Option<String>,
        #[arg(long)]
        backend: Option<String>,
        #[arg(long)]
        model: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    /// Optional for older local clients. Correlation only, never a retry token.
    #[serde(default)]
    pub operation_id: Option<String>,
    pub command: JobCommand,
}

pub fn validate_operation_id(id: &str) -> Result<()> {
    if id.len() != 36 || uuid::Uuid::parse_str(id)?.to_string() != id {
        bail!("operation ID must be a canonical lowercase hyphenated UUID")
    }
    Ok(())
}

impl JobCommand {
    pub fn mutates(&self) -> bool {
        !matches!(
            self,
            Self::List {} | Self::Find { .. } | Self::Get { .. } | Self::Output { .. }
        )
    }

    pub fn submission(&self) -> Result<(&'static str, Value)> {
        let identified = |kind, id: &str| {
            if id.is_empty()
                || id.len() > 128
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                bail!("job ID must be 1-128 ASCII letters, digits, hyphens or underscores")
            }
            Ok((kind, json!({"id":id})))
        };
        match self {
            Self::List {} => Ok(("jobs.list", json!({}))),
            Self::Find { operation_id } => {
                validate_operation_id(operation_id)?;
                Ok(("jobs.find", json!({"operation_id":operation_id})))
            }
            Self::Get { id } => identified("jobs.get", id),
            Self::Output { id } => identified("jobs.output", id),
            Self::Pause { id } => identified("jobs.pause", id),
            Self::Resume { id } => identified("jobs.resume", id),
            Self::Stop { id } => identified("jobs.stop", id),
            Self::Remove { id } => identified("jobs.remove", id),
            Self::GenerateText {
                prompt,
                role,
                target_node,
                backend,
                model,
            } => {
                if prompt.trim().is_empty() || prompt.len() > 32 * 1024 {
                    bail!("prompt must contain 1-32768 bytes")
                }
                let mut params = json!({"prompt":prompt});
                let mut constraints = json!({});
                if let Some(id) = target_node {
                    crate::network::validate_node_id(id)?;
                    constraints["node_id"] = json!(id);
                }
                if let Some(id) = model {
                    crate::model_selection::validate_id(id)?;
                    params["model_id"] = json!(id);
                }
                for (key, value) in [("model_role", role), ("backend", backend)] {
                    if let Some(value) = value {
                        if value.is_empty()
                            || value.len() > 64
                            || !value
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                        {
                            bail!("role/backend must be a bounded ASCII identifier")
                        }
                        constraints[key] = json!(value);
                    }
                }
                Ok((
                    "jobs.create",
                    json!({"description":"Text generation","template":"generate_text","params":params,"constraints":constraints}),
                ))
            }
        }
    }
}

impl crate::NodeService {
    pub async fn job_command(&self, request: JobRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_node_id != self.node_id {
            bail!("job command schema or target node does not match this instance")
        }
        let (kind, payload) = request.command.submission()?;
        let operation_id = request
            .operation_id
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        validate_operation_id(&operation_id)?;
        let mut result = json!({"schema_version":1,"success":false,"report_type":"node_job_command",
            "node_id":self.node_id,"operation_id":operation_id,
            "acknowledged":false,"outcome_unknown":false});
        match self
            .submissions
            .request_correlated(kind, payload, Some(operation_id.clone()))
            .await
        {
            Ok(data) => {
                if kind == "jobs.create" {
                    result["creation_correlated"] =
                        json!(data["request_context"]["request_id"] == operation_id);
                }
                result["success"] = json!(true);
                result["acknowledged"] = json!(true);
                result["data"] = data;
                result["note"] = json!(
                    "Coordinator acknowledged the command. Inspect job status for completion; acknowledgement is not completed work."
                );
            }
            Err(error)
                if error
                    .downcast_ref::<crate::submission::SubmissionFailure>()
                    .is_some() =>
            {
                result["error_code"] = json!(match error
                    .downcast_ref::<crate::submission::SubmissionFailure>()
                    .unwrap()
                {
                    crate::submission::SubmissionFailure::NotSent(_) => "not_sent",
                    crate::submission::SubmissionFailure::Rejected(_) => "job_rejected",
                });
                // A remote error can follow a checkpoint failure, not a rollback.
                result["outcome_unknown"] = json!(
                    request.command.mutates()
                        && matches!(
                            error.downcast_ref::<crate::submission::SubmissionFailure>(),
                            Some(crate::submission::SubmissionFailure::Rejected(_))
                        )
                );
                result["error"] = json!(error.to_string());
            }
            Err(_) => {
                result["error_code"] = json!("job_command_unconfirmed");
                result["outcome_unknown"] = json!(request.command.mutates());
                result["note"] = json!(
                    "Coordinator did not confirm the command. Check connection and inspect this node's job list/status before retrying; no automatic replay was sent."
                );
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_jobs_cannot_supply_origin_or_arbitrary_plan_authority() {
        for id in [
            "",
            "../other",
            "00112233445566778899aabbccddeeff",
            "00112233-4455-6677-8899-AABBCCDDEEFF",
        ] {
            assert!(validate_operation_id(id).is_err());
        }
        assert!(
            !JobCommand::Find {
                operation_id: uuid::Uuid::new_v4().to_string()
            }
            .mutates()
        );
        assert!(
            serde_json::from_value::<JobCommand>(
                json!({"operation":"list","origin_node":"forged"})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<JobCommand>(
                json!({"operation":"generate_text","prompt":"hi","request_context":{}})
            )
            .is_err()
        );
        assert!(
            JobCommand::Get {
                id: "../other".into()
            }
            .submission()
            .is_err()
        );
        let command = JobCommand::GenerateText {
            prompt: "original input".into(),
            role: Some("request_interpreter".into()),
            target_node: Some("target".into()),
            backend: Some("ollama".into()),
            model: Some("fixture".into()),
        };
        let (kind, value) = command.submission().unwrap();
        assert_eq!(kind, "jobs.create");
        assert!(value.get("request_context").is_none());
        assert_eq!(value["params"]["prompt"], "original input");
        assert_eq!(value["params"]["model_id"], "fixture");
        assert_eq!(value["constraints"]["node_id"], "target");
        assert_eq!(value["constraints"]["model_role"], "request_interpreter");
    }
    #[tokio::test]
    async fn jobs_use_existing_mailbox_once_and_require_instance_identity() {
        let dir = tempfile::tempdir().unwrap();
        let node = crate::NodeService::new(dir.path().join("node.json"), "origin".into());
        let mut receiver = node.submissions.connect();
        let request = JobRequest {
            schema_version: 1,
            expected_node_id: "origin".into(),
            operation_id: None,
            command: JobCommand::List {},
        };
        let mut wrong = request.clone();
        wrong.expected_node_id = "other".into();
        assert!(node.job_command(wrong).await.is_err());
        let mut invalid = request.clone();
        invalid.operation_id = Some("not-a-receipt".into());
        assert!(node.job_command(invalid).await.is_err());
        assert!(receiver.try_recv().is_err());
        let copy = node.clone();
        let command = tokio::spawn(async move { copy.job_command(request).await.unwrap() });
        let submission = receiver.recv().await.unwrap();
        assert_eq!(submission.kind, "jobs.list");
        let operation_id = submission.operation_id.clone().unwrap();
        assert_ne!(submission.id, operation_id);
        submission.reply.send(Ok(json!({"jobs":[]}))).unwrap();
        let report = command.await.unwrap();
        assert_eq!(report["operation_id"], operation_id);
        assert_eq!(report["data"]["jobs"], json!([]));
        let copy = node.clone();
        let command = tokio::spawn(async move {
            copy.job_command(JobRequest {
                schema_version: 1,
                expected_node_id: "origin".into(),
                operation_id: None,
                command: JobCommand::Stop {
                    id: "job-id".into(),
                },
            })
            .await
            .unwrap()
        });
        let submission = receiver.recv().await.unwrap();
        assert_eq!(submission.kind, "jobs.stop");
        drop(submission.reply);
        let result = command.await.unwrap();
        assert_eq!(result["outcome_unknown"], true);
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn disconnected_job_command_is_known_not_sent() {
        let dir = tempfile::tempdir().unwrap();
        let node = crate::NodeService::new(dir.path().join("node.json"), "origin".into());
        let result = node
            .job_command(JobRequest {
                schema_version: 1,
                expected_node_id: "origin".into(),
                operation_id: None,
                command: JobCommand::Stop { id: "job".into() },
            })
            .await
            .unwrap();
        assert_eq!(result["error_code"], "not_sent");
        assert_eq!(result["outcome_unknown"], false);
    }
}
