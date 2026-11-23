//! Optional serving advertisement and an authenticated gateway snapshot.
//! Neither is a policy allowance, model reservation or execution authority.
use crate::{grants::GrantPolicy, network::PeerModel};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Advertisement {
    pub schema_version: u32,
    pub permission_revision: u64,
}
impl Advertisement {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "Unsupported interpreter service advertisement"
        );
        Ok(())
    }
}

/// Optional registration hint, separate from the published service shape so
/// older coordinators can continue accepting new nodes. Never grants authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskSupport {
    pub schema_version: u32,
    pub versions: Vec<u32>,
}
impl TaskSupport {
    pub fn current() -> Self {
        Self {
            schema_version: 1,
            versions: vec![
                crate::interpreter_task::LEGACY_VERSION,
                crate::interpreter_task::METHOD_VERSION,
            ],
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1 && !self.versions.is_empty() && self.versions.len() <= 16,
            "Invalid interpreter task support advertisement"
        );
        let mut versions = std::collections::HashSet::new();
        ensure!(
            self.versions.iter().all(|v| *v > 0 && versions.insert(*v)),
            "Invalid or duplicate interpreter task versions"
        );
        Ok(())
    }
    pub fn supports(&self, version: u32) -> bool {
        self.validate().is_ok() && self.versions.contains(&version)
    }
}

/// Constructed only from one live independently authenticated gateway route.
/// It is intentionally not deserializable and cannot authorize a later send.
#[derive(Clone, Debug)]
pub struct Session {
    pub(crate) fresh_until: std::time::Instant,
    pub(crate) node_id: String,
    pub(crate) runtime_id: String,
    pub(crate) session_id: String,
    pub(crate) issuer_id: String,
    pub(crate) limits: GrantPolicy,
    pub(crate) serving: Option<Advertisement>,
    pub(crate) task_support: Option<TaskSupport>,
    pub(crate) models: Vec<PeerModel>,
}
impl Session {
    pub fn is_fresh(&self) -> bool {
        std::time::Instant::now() < self.fresh_until
    }
    pub fn freshness_deadline(&self) -> std::time::Instant {
        self.fresh_until
    }
    pub fn node_id(&self) -> &str {
        &self.node_id
    }
    pub fn runtime_id(&self) -> &str {
        &self.runtime_id
    }
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    pub fn issuer_id(&self) -> &str {
        &self.issuer_id
    }
    pub fn limits(&self) -> &GrantPolicy {
        &self.limits
    }
    pub fn permission_revision(&self) -> Option<u64> {
        self.serving.as_ref().map(|a| a.permission_revision)
    }
    pub fn supports_task_version(&self, version: u32) -> bool {
        self.is_fresh()
            && self.serving.as_ref().is_some_and(|a| a.validate().is_ok())
            && self
                .task_support
                .as_ref()
                .map_or(version == crate::interpreter_task::LEGACY_VERSION, |a| {
                    a.supports(version)
                })
    }
    pub fn same_connection(&self, other: &Self) -> bool {
        self.node_id == other.node_id
            && self.runtime_id == other.runtime_id
            && self.session_id == other.session_id
            && self.issuer_id == other.issuer_id
    }
    pub fn serves(&self, backend: &str, model: &str) -> bool {
        if !self.is_fresh() || self.serving.is_none() || !matches!(backend, "ollama" | "llamacpp") {
            return false;
        }
        // Ambiguous duplicate advertisements cannot select an arbitrary entry.
        let matches: Vec<_> = self
            .models
            .iter()
            .filter(|m| m.model_id == model && m.backend.as_deref() == Some(backend))
            .collect();
        if matches.len() != 1 {
            return false;
        }
        let metadata = matches[0].normalized_metadata();
        metadata.validate().is_ok()
            && metadata.supports("llm.infer")
            && metadata.routable()
            && metadata.input_modalities.iter().any(|m| m == "text")
            && metadata.output_modalities.iter().any(|m| m == "text")
            && metadata
                .roles
                .as_ref()
                .is_some_and(|r| r.iter().any(|r| r == "request_interpreter"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn task_support_is_bounded_explicit_and_separate_from_the_legacy_service_shape() {
        let current = TaskSupport::current();
        assert!(current.supports(1) && current.supports(2));
        assert!(!current.supports(0) && !current.supports(3));
        for versions in [vec![], vec![0], vec![1, 1], (1..=17).collect()] {
            assert!(
                TaskSupport {
                    schema_version: 1,
                    versions
                }
                .validate()
                .is_err()
            );
        }
        let future = TaskSupport {
            schema_version: 1,
            versions: vec![99],
        };
        future.validate().unwrap();
        assert!(!future.supports(1) && !future.supports(2));
        assert!(
            serde_json::from_value::<TaskSupport>(
                json!({"schema_version":1,"versions":[1,2],"execution_authorized":true})
            )
            .is_err()
        );
        assert_eq!(
            serde_json::to_value(Advertisement {
                schema_version: 1,
                permission_revision: 4
            })
            .unwrap(),
            json!({"schema_version":1,"permission_revision":4}),
            "Keep the published strict service shape unchanged"
        );
    }
    #[test]
    fn service_advertisements_never_infer_roles_or_choose_duplicate_or_unavailable_models() {
        assert!(
            serde_json::from_value::<Advertisement>(
                json!({"schema_version":1,"permission_revision":0,"execution_authorized":true})
            )
            .is_err()
        );
        assert!(
            Advertisement {
                schema_version: 2,
                permission_revision: 0
            }
            .validate()
            .is_err()
        );
        let mut metadata = crate::model_metadata::ModelMetadata::from_legacy(Some("text"));
        metadata.roles = Some(vec!["request_interpreter".into()]);
        let model = PeerModel {
            model_id: "chosen".into(),
            backend: Some("ollama".into()),
            modality: Some("text".into()),
            model_metadata: Some(metadata.clone()),
            cpu_threads: None,
            job_memory_limit_mib: None,
            last_successful_owned_load: None,
            last_successful_owned_decode: None,
        };
        let mut session = Session {
            fresh_until: std::time::Instant::now() + std::time::Duration::from_secs(30),
            node_id: "owner".into(),
            runtime_id: uuid::Uuid::new_v4().to_string(),
            session_id: uuid::Uuid::new_v4().to_string(),
            issuer_id: "a".repeat(64),
            limits: Default::default(),
            serving: Some(Advertisement {
                schema_version: 1,
                permission_revision: 4,
            }),
            task_support: Some(TaskSupport::current()),
            models: vec![model.clone()],
        };
        assert!(session.serves("ollama", "chosen"));
        assert!(session.supports_task_version(1) && session.supports_task_version(2));
        let mut legacy = session.clone();
        legacy.task_support = None;
        assert!(legacy.supports_task_version(1));
        assert!(!legacy.supports_task_version(2));
        legacy.task_support = Some(TaskSupport {
            schema_version: 1,
            versions: vec![2],
        });
        assert!(!legacy.supports_task_version(1));
        assert!(legacy.supports_task_version(2));
        let mut expired = session.clone();
        expired.fresh_until = std::time::Instant::now();
        assert!(!expired.is_fresh());
        assert!(!expired.supports_task_version(1) && !expired.supports_task_version(2));
        assert!(!expired.serves("ollama", "chosen"));
        assert!(!session.serves("llamacpp", "chosen"));
        assert_eq!(session.permission_revision(), Some(4));
        session.models.push(model.clone());
        assert!(!session.serves("ollama", "chosen"));
        session.models.pop();
        session.serving = None;
        assert!(!session.supports_task_version(1) && !session.supports_task_version(2));
        assert!(!session.serves("ollama", "chosen"));
        session.serving = Some(Advertisement {
            schema_version: 1,
            permission_revision: 4,
        });
        for problem in [
            "role",
            "input",
            "output",
            "unavailable",
            "loading",
            "legacy",
        ] {
            let mut changed = metadata.clone();
            match problem {
                "role" => changed.roles = None,
                "input" => changed.input_modalities = vec!["image".into()],
                "output" => changed.output_modalities.clear(),
                "unavailable" => {
                    changed.availability =
                        Some(crate::model_metadata::ModelAvailability::Unavailable)
                }
                "loading" => {
                    changed.lifecycle = Some(crate::model_metadata::ModelLifecycle::Loading)
                }
                "legacy" => {}
                _ => unreachable!(),
            }
            session.models[0].model_metadata = (problem != "legacy").then_some(changed);
            assert!(!session.serves("ollama", "chosen"), "problem={problem}");
        }
    }
}
