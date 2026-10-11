//! Ordinary RPC is the deterministic control plane, not workload authority.
use anyhow::Result;

pub const ERROR_CODE: &str = "execution_grant_required";
#[derive(Debug, thiserror::Error)]
#[error("Workload execution requires an exact reviewed execution grant; no legacy RPC fallback")]
pub struct ExecutionGrantRequired;

pub fn ordinary_control(capability: &str) -> bool {
    matches!(capability, "system.ping" | "node.configure" | "node.update")
}
pub fn require_control(capability: &str) -> Result<()> {
    if !ordinary_control(capability) {
        return Err(ExecutionGrantRequired.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_wire_has_no_effectful_workload_or_model_alias() {
        for capability in crate::engine::CAPABILITIES {
            assert_eq!(
                ordinary_control(capability),
                matches!(
                    *capability,
                    "system.ping" | "node.configure" | "node.update"
                )
            );
        }
        for capability in [
            "filesystem",
            "llm.infer",
            "vlm.analyze",
            "system.info",
            "http.request",
            "camera.capture",
            "process.execute",
            "audio.play",
            "keyboard",
            "network.wol",
            "unknown",
            "System.ping",
            "node.configure.extra",
        ] {
            assert!(
                require_control(capability)
                    .unwrap_err()
                    .is::<ExecutionGrantRequired>()
            );
        }
    }
}
