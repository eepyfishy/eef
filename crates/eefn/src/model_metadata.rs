//! Bounded model advertisements, not execution grants or measured resource usage.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelMetadata {
    pub schema_version: u32,
    pub capabilities: Vec<String>,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    #[serde(default)]
    pub roles: Option<Vec<String>>,
    #[serde(default)]
    pub lifecycle: Option<ModelLifecycle>,
    #[serde(default)]
    pub availability: Option<ModelAvailability>,
    #[serde(default)]
    pub resource_estimates: ResourceEstimates,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelLifecycle {
    Unloaded,
    Loading,
    Ready,
    Active,
    WarmIdle,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAvailability {
    Available,
    Unavailable,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceEstimates {
    pub ram_mb: Option<u64>,
    pub vram_mb: Option<u64>,
    pub size_bytes: Option<u64>,
}

impl ModelMetadata {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            bail!("unsupported model metadata schema")
        }
        for items in [
            &self.capabilities,
            &self.input_modalities,
            &self.output_modalities,
        ]
        .into_iter()
        .chain(self.roles.iter())
        {
            let mut seen = std::collections::BTreeSet::new();
            if items.len() > 32 {
                bail!("model metadata list exceeds 32 entries")
            }
            for item in items {
                if item.is_empty()
                    || item.len() > 64
                    || !item
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                    || !seen.insert(item)
                {
                    bail!("model metadata identifiers must be unique bounded ASCII identifiers")
                }
            }
        }
        Ok(())
    }

    /// Compatibility hints from explicit owner/backend modality, never model names.
    pub fn from_legacy(modality: Option<&str>) -> Self {
        let (capabilities, inputs, outputs) = match modality {
            Some("text") => (vec!["llm.infer"], vec!["text"], vec!["text"]),
            Some("vlm") => (
                vec!["llm.infer", "vlm.analyze"],
                vec!["text", "image"],
                vec!["text"],
            ),
            _ => (vec![], vec![], vec![]),
        };
        Self {
            schema_version: 1,
            capabilities: capabilities.into_iter().map(str::to_owned).collect(),
            input_modalities: inputs.into_iter().map(str::to_owned).collect(),
            output_modalities: outputs.into_iter().map(str::to_owned).collect(),
            roles: None,
            lifecycle: None,
            availability: None,
            resource_estimates: ResourceEstimates::default(),
        }
    }

    pub fn supports(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|value| value == capability)
    }

    pub fn routable(&self) -> bool {
        self.availability != Some(ModelAvailability::Unavailable)
            && !matches!(
                self.lifecycle,
                Some(ModelLifecycle::Loading | ModelLifecycle::Error)
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_unknowns_and_multiple_capabilities_round_trip() {
        let legacy = ModelMetadata::from_legacy(Some("vlm"));
        assert!(legacy.supports("llm.infer") && legacy.supports("vlm.analyze"));
        assert!(legacy.lifecycle.is_none());
        assert!(legacy.resource_estimates.ram_mb.is_none());
        assert!(ModelMetadata::from_legacy(None).capabilities.is_empty());
        let mut general = legacy;
        general.capabilities = vec!["ocr".into(), "embedding".into()];
        general.roles = Some(vec!["request_interpreter".into()]);
        general.validate().unwrap();
        let decoded: ModelMetadata =
            serde_json::from_value(serde_json::to_value(&general).unwrap()).unwrap();
        assert_eq!(decoded, general);
        assert!(!general.supports("llm.infer"));
    }

    #[test]
    fn invalid_metadata_is_rejected_not_silently_downgraded() {
        let base = serde_json::to_value(ModelMetadata::from_legacy(Some("text"))).unwrap();
        for (key, value) in [
            ("schema_version", json!(2)),
            ("capabilities", json!(["llm.infer", "llm.infer"])),
            ("roles", json!(["bad role"])),
            ("input_modalities", json!(vec!["x"; 33])),
        ] {
            let mut raw = base.clone();
            raw[key] = value;
            assert!(
                serde_json::from_value::<ModelMetadata>(raw)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        for (key, value) in [
            ("lifecycle", json!("magic")),
            ("private_path", json!("secret")),
            ("capabilities", json!(null)),
            ("resource_estimates", json!({"ram_mb":-1})),
        ] {
            let mut raw = base.clone();
            raw[key] = value;
            assert!(serde_json::from_value::<ModelMetadata>(raw).is_err());
        }
    }

    #[test]
    fn reported_unavailability_blocks_routing_without_inventing_readiness() {
        let mut m = ModelMetadata::from_legacy(None);
        assert!(m.routable());
        for state in [ModelLifecycle::Loading, ModelLifecycle::Error] {
            m.lifecycle = Some(state);
            assert!(!m.routable());
        }
        m.lifecycle = Some(ModelLifecycle::Unloaded);
        assert!(m.routable());
        m.availability = Some(ModelAvailability::Unavailable);
        assert!(!m.routable());
    }
}
