//! Data-only model-service task inputs. Hashes are not authorization or attestations.
//! The issuer must match the retained source and policy before issuing a grant;
//! the receiver must check that grant, serving permission, model and completion.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MAX_SNAPSHOT_BYTES: usize = 64 * 1024;
pub const MAX_TASK_BYTES: usize = 128 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    Initial {
        input: crate::request_intent::Input,
    },
    Clarification {
        draft: Box<crate::request_drafts::DraftRecord>,
    },
    Method {
        draft: Box<crate::request_drafts::DraftRecord>,
    },
}
impl Source {
    pub fn max_output_tokens(&self) -> u64 {
        match self {
            Self::Initial { .. } | Self::Clarification { .. } => {
                crate::interpretation::MAX_OUTPUT_TOKENS
            }
            Self::Method { .. } => crate::request_preparation::MAX_OUTPUT_TOKENS,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema_version: u32,
    pub job_id: String,
    /// Retained journal revision being interpreted, not a new executor assignment.
    pub revision: u64,
    pub operation_id: String,
    #[serde(deserialize_with = "strict_context")]
    pub request_context: crate::context::RequestContext,
    pub allowed_capabilities: Vec<String>,
    pub source: Source,
}
fn strict_context<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<crate::context::RequestContext, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Context {
        request_id: String,
        origin_node: String,
        origin_area: crate::context::AreaPath,
    }
    let value = Context::deserialize(d)?;
    crate::context::RequestContext::new(value.request_id, value.origin_node, value.origin_area)
        .map_err(serde::de::Error::custom)
}

/// Reconstructed locally from typed retained data; never caller-supplied messages.
/// Applied node sampling preferences remain separate from this input definition.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InferenceSpec {
    pub messages: Value,
    pub output_schema: Option<Value>,
    pub max_output_tokens: u64,
}
impl Snapshot {
    fn spec(&self, max_output_tokens: u64) -> Result<InferenceSpec> {
        ensure!(
            self.schema_version == 1,
            "Unsupported interpreter source version"
        );
        crate::request_drafts::validate_id(&self.job_id)?;
        crate::job_commands::validate_operation_id(&self.operation_id)?;
        ensure!(
            self.revision > 0 && self.revision != u64::MAX,
            "Invalid interpreter source revision"
        );
        ensure!(
            serde_json::to_vec(self)?.len() <= MAX_SNAPSHOT_BYTES,
            "Interpreter source exceeds snapshot budget"
        );
        ensure!(
            (1..=self.source.max_output_tokens()).contains(&max_output_tokens),
            "Invalid interpreter token allowance"
        );
        let (messages, output_schema) = match &self.source {
            Source::Initial { input } => {
                input.validate()?;
                (
                    crate::interpretation::messages(
                        &input.original_text,
                        &self.allowed_capabilities,
                    )?,
                    Some(crate::interpretation::output_schema(
                        &self.allowed_capabilities,
                    )?),
                )
            }
            Source::Clarification { draft } | Source::Method { draft } => {
                draft.validate()?;
                ensure!(
                    self.revision >= (draft.proposals.len() + draft.answers.len()) as u64,
                    "Interpreter source revision predates history"
                );
                if matches!(&self.source, Source::Clarification { .. }) {
                    (
                        crate::interpretation::clarification_messages(
                            draft,
                            &self.allowed_capabilities,
                        )?,
                        Some(crate::interpretation::output_schema(
                            &self.allowed_capabilities,
                        )?),
                    )
                } else {
                    (
                        crate::request_preparation::messages(draft, &self.allowed_capabilities)?,
                        None,
                    )
                }
            }
        };
        ensure!(
            serde_json::to_vec(&messages)?.len() <= crate::request_preparation::MAX_PROMPT_BYTES,
            "Interpreter prompt exceeds budget; history was not truncated"
        );
        Ok(InferenceSpec {
            messages,
            output_schema,
            max_output_tokens,
        })
    }
    fn digest(&self) -> Result<String> {
        crate::grants::definition_digest(
            &json!({"domain":"eef.interpreter.source.v1","snapshot":self}),
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub schema_version: u32,
    pub snapshot: Snapshot,
    pub snapshot_sha256: String,
    /// Detects differing prompt/template/schema/token definitions before inference.
    pub inference_spec_sha256: String,
    pub max_output_tokens: u64,
}
fn spec_digest(spec: &InferenceSpec) -> Result<String> {
    crate::grants::definition_digest(
        &json!({"domain":"eef.interpreter.inference-input.v1","spec":spec}),
    )
}
impl Task {
    pub fn new(snapshot: Snapshot, max_output_tokens: u64) -> Result<Self> {
        let spec = snapshot.spec(max_output_tokens)?;
        let task = Self {
            schema_version: 1,
            snapshot_sha256: snapshot.digest()?,
            inference_spec_sha256: spec_digest(&spec)?,
            snapshot,
            max_output_tokens,
        };
        task.validate()?;
        Ok(task)
    }
    /// Self-consistency only. The grant must pin the entire exact task, and its
    /// issuer must independently establish journal ownership and policy authority.
    pub fn validate(&self) -> Result<InferenceSpec> {
        ensure!(
            self.schema_version == 1,
            "Unsupported interpreter task version"
        );
        ensure!(
            serde_json::to_vec(self)?.len() <= MAX_TASK_BYTES,
            "Interpreter task exceeds encoded budget"
        );
        crate::request_interpretation::digest(&self.snapshot_sha256)?;
        crate::request_interpretation::digest(&self.inference_spec_sha256)?;
        let spec = self.snapshot.spec(self.max_output_tokens)?;
        ensure!(
            self.snapshot.digest()? == self.snapshot_sha256,
            "Interpreter source definition changed"
        );
        ensure!(
            spec_digest(&spec)? == self.inference_spec_sha256,
            "Interpreter prompt definition changed"
        );
        Ok(spec)
    }
    /// Validate only completed provider output after the caller checks completion
    /// and exact issued model/session binding. Attribution here is not attestation.
    pub fn candidate(&self, output: &[u8], backend: &str, model: &str) -> Result<Candidate> {
        self.validate()?;
        ensure!(
            matches!(backend, "ollama" | "llamacpp"),
            "Unsupported interpreter backend"
        );
        crate::model_selection::validate_id(model)?;
        let snapshot = &self.snapshot;
        match &snapshot.source {
            Source::Initial { input } => crate::interpretation::validate(
                &input.original_text,
                output,
                &snapshot.allowed_capabilities,
            )
            .map(Candidate::Initial)
            .map_err(|_| anyhow::anyhow!("Interpreter candidate rejected")),
            Source::Clarification { draft } => crate::interpretation::validate(
                &draft.original_text,
                output,
                &snapshot.allowed_capabilities,
            )
            .map(Candidate::Clarification)
            .map_err(|_| anyhow::anyhow!("Interpreter candidate rejected")),
            Source::Method { draft } => crate::request_preparation::PreparedMethod::from_selected(
                &snapshot.job_id,
                snapshot.revision,
                draft,
                &snapshot.allowed_capabilities,
                output,
                backend,
                model,
            )
            .map(Candidate::Method)
            .map_err(|_| anyhow::anyhow!("Interpreter method candidate rejected")),
        }
    }
}

/// Cannot be deserialized into authority. All payloads retain their existing
/// untrusted-output/non-execution markers and need normal coordinator validation.
#[derive(Serialize)]
#[serde(tag = "stage", content = "candidate", rename_all = "snake_case")]
pub enum Candidate {
    Initial(crate::interpretation::ValidatedInterpretation),
    Clarification(crate::interpretation::ValidatedInterpretation),
    Method(crate::request_preparation::PreparedMethod),
}

#[cfg(test)]
#[path = "interpreter_task_tests.rs"]
mod tests;
