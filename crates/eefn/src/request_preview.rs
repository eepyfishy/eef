//! Local owner interpretation preview, never a job submission or execution grant.
use crate::{NodeEngine, NodeService};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{Semaphore, watch};

#[derive(Debug, clap::Subcommand)]
pub enum RequestAction {
    /// Inspect an untrusted proposal from the selected interpreter; never execute it.
    Preview {
        #[arg(long)]
        text: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub request_id: String,
    pub text: String,
}

#[derive(Clone)]
struct Binding {
    runtime_id: String,
    engine: Weak<NodeEngine>,
}

pub(crate) struct PreviewRuntime {
    binding: Mutex<Option<Binding>>,
    generation: watch::Sender<Option<String>>,
    capacity: Semaphore,
}
impl Default for PreviewRuntime {
    fn default() -> Self {
        Self {
            binding: Mutex::new(None),
            generation: watch::channel(None).0,
            capacity: Semaphore::new(1),
        }
    }
}

/// Dropped with run_node, including when its future is cancelled for restart.
pub struct PreviewGuard {
    runtime: Arc<PreviewRuntime>,
    runtime_id: String,
}
impl Drop for PreviewGuard {
    fn drop(&mut self) {
        let mut binding = self.runtime.binding.lock().unwrap();
        if binding
            .as_ref()
            .is_some_and(|b| b.runtime_id == self.runtime_id)
        {
            *binding = None;
            self.runtime.generation.send_replace(None);
        }
    }
}

impl NodeService {
    pub fn attach_preview_runtime(
        &self,
        engine: &Arc<NodeEngine>,
        runtime_id: String,
    ) -> PreviewGuard {
        let runtime = self.preview.clone();
        {
            let mut binding = runtime.binding.lock().unwrap();
            *binding = Some(Binding {
                runtime_id: runtime_id.clone(),
                engine: Arc::downgrade(engine),
            });
            runtime.generation.send_replace(Some(runtime_id.clone()));
        }
        PreviewGuard {
            runtime,
            runtime_id,
        }
    }

    pub async fn preview_request(&self, request: PreviewRequest) -> Value {
        let mut runtime_id = None;
        let result: Result<Value, &'static str> = async {
            if request.schema_version != 1 || request.expected_node_id != self.node_id
                || uuid::Uuid::parse_str(&request.request_id).ok().map(|id| id.to_string()).as_deref() != Some(&request.request_id) {
                return Err("invalid_preview_request");
            }
            // Validate user text before acquiring capacity or consulting a model.
            crate::interpretation::messages(&request.text, &[]).map_err(|_| "invalid_preview_input")?;
            let _permit = self.preview.capacity.try_acquire().map_err(|_| "interpreter_busy")?;
            let mut generation = self.preview.generation.subscribe();
            let binding = self.preview.binding.lock().unwrap().clone().ok_or("runtime_unavailable")?;
            runtime_id = Some(binding.runtime_id.clone());
            let engine = binding.engine.upgrade().ok_or("runtime_unavailable")?;
            if generation.borrow_and_update().as_ref() != Some(&binding.runtime_id) {
                return Err("runtime_changed");
            }
            let proposal = tokio::select! {
                biased;
                _ = generation.changed() => return Err("runtime_changed"),
                result = tokio::time::timeout(Duration::from_secs(20), engine.preview_interpretation(&request.text)) => {
                    result.map_err(|_| "interpretation_timeout")??
                },
            };
            if self.preview.binding.lock().unwrap().as_ref().map(|b| &b.runtime_id) != Some(&binding.runtime_id) {
                return Err("runtime_changed");
            }
            Ok(proposal)
        }.await;
        let mut report = match result {
            Ok(value) => json!({"success":true,"result":value}),
            Err(code) => json!({"success":false,"error_code":code,
                "error":"Preview unavailable or rejected. No job was submitted and no action was authorized; inspect model selection/status before retrying."}),
        };
        report["schema_version"] = json!(1);
        report["report_type"] = json!("request_preview");
        report["node_id"] = json!(self.node_id);
        report["request_id"] = json!(
            uuid::Uuid::parse_str(&request.request_id)
                .ok()
                .map(|id| id.to_string())
                .filter(|id| id == &request.request_id)
        );
        report["runtime_id"] = json!(runtime_id);
        report["execution_authorized"] = json!(false);
        report["dispatched"] = json!(false);
        report["automatically_retried"] = json!(false);
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> PreviewRequest {
        PreviewRequest {
            schema_version: 1,
            expected_node_id: "node".into(),
            request_id: uuid::Uuid::new_v4().to_string(),
            text: "Check this node".into(),
        }
    }
    #[test]
    fn preview_binding_does_not_keep_engine_service_cycles_alive() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("config.json"), "node".into());
        let weak = Arc::downgrade(&service);
        let engine = Arc::new(
            NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_service(service.clone()),
        );
        let guard = service.attach_preview_runtime(&engine, "runtime".into());
        drop(service);
        drop(engine);
        assert!(weak.upgrade().is_none());
        drop(guard);
    }
    #[tokio::test]
    async fn preview_validates_target_input_runtime_and_does_not_start_node() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("config.json"), "node".into());
        let mut wrong = request();
        wrong.expected_node_id = "other".into();
        assert_eq!(
            service.preview_request(wrong).await["error_code"],
            "invalid_preview_request"
        );
        let mut wrong = request();
        wrong.text = " ".into();
        assert_eq!(
            service.preview_request(wrong).await["error_code"],
            "invalid_preview_input"
        );
        assert_eq!(
            service.preview_request(request()).await["error_code"],
            "runtime_unavailable"
        );
        assert!(!dir.path().join("config.json").exists());
        let engine = Arc::new(NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let guard = service.attach_preview_runtime(&engine, "first".into());
        let report = service.preview_request(request()).await;
        assert_eq!(report["error_code"], "interpreter_unavailable");
        assert_eq!(report["execution_authorized"], false);
        let _permit = service.preview.capacity.acquire().await.unwrap();
        assert_eq!(
            service.preview_request(request()).await["error_code"],
            "interpreter_busy"
        );
        drop(_permit);
        let replacement = service.attach_preview_runtime(&engine, "second".into());
        drop(guard);
        assert_eq!(
            service.preview_request(request()).await["runtime_id"],
            "second"
        );
        drop(replacement);
        assert_eq!(
            service.preview_request(request()).await["error_code"],
            "runtime_unavailable"
        );
    }
}
