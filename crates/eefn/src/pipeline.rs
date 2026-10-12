//! Finite local dataflow definition. Structural validation is not execution
//! authority. Compiler, parent grant and node runtime integration remain required.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_STEPS: usize = 16;
pub const MAX_DEFINITION_BYTES: usize = 64 * 1024;
pub const MAX_RETAINED_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinitePipeline {
    pub schema_version: u32,
    pub steps: Vec<PipelineStep>,
    /// Encoded retained data budget, not a physical memory or backend CPU quota.
    pub max_retained_bytes: usize,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineStep {
    pub id: String,
    pub capability: String,
    pub action: String,
    pub params: Value,
    #[serde(default)]
    pub resource_id: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub model_role: Option<String>,
    #[serde(default)]
    pub inputs: Vec<DataInput>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataInput {
    pub parameter: String,
    pub source_step: String,
    /// RFC 6901 pointer into an earlier result. It cannot select an operation,
    /// model, resource, URL, program, path or local permission.
    pub pointer: String,
    #[serde(default)]
    pub wrap_array: bool,
}
fn step_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        bail!("Pipeline step ID must be a bounded ASCII identifier")
    }
    Ok(())
}
impl FinitePipeline {
    pub fn validate(&self) -> Result<()> {
        self.validate_for_executor(None)
    }
    pub fn validate_for_executor(&self, executor: Option<&str>) -> Result<()> {
        if let Some(node) = executor {
            crate::network::validate_remote_node_id(node)?;
        }
        if self.schema_version != 1
            || self.steps.is_empty()
            || self.steps.len() > MAX_STEPS
            || !(1024..=MAX_RETAINED_BYTES).contains(&self.max_retained_bytes)
            || encoded_bytes(self, MAX_DEFINITION_BYTES).is_err()
        {
            bail!("Unsupported or unbounded finite pipeline definition")
        }
        let mut previous = BTreeSet::new();
        for step in &self.steps {
            step.validate(executor)?;
            if previous.contains(&step.id) {
                bail!("Duplicate pipeline step ID")
            }
            for input in &step.inputs {
                if !previous.contains(&input.source_step) {
                    bail!(
                        "Pipeline input must refer to a completed earlier step; no cycles or forward references"
                    )
                }
            }
            previous.insert(step.id.clone());
        }
        Ok(())
    }
    /// How many later inputs retain each result. The runtime can release a
    /// result once its final reference is consumed; the final step is returned.
    pub fn reference_counts(&self) -> Result<BTreeMap<String, usize>> {
        self.validate()?;
        let mut counts = self
            .steps
            .iter()
            .map(|s| (s.id.clone(), 0))
            .collect::<BTreeMap<_, _>>();
        for step in &self.steps {
            for input in &step.inputs {
                *counts.get_mut(&input.source_step).unwrap() += 1;
            }
        }
        Ok(counts)
    }
    pub fn resolve_step(&self, index: usize, results: &BTreeMap<String, Value>) -> Result<Value> {
        self.validate()?;
        self.steps
            .get(index)
            .context("Pipeline step index is out of bounds")?
            .resolve_with_limit(results, self.max_retained_bytes)
    }
}
impl PipelineStep {
    pub fn bound_params(&self) -> Result<Value> {
        if !self.params.is_object() || encoded_bytes(&self.params, MAX_DEFINITION_BYTES).is_err() {
            bail!("Invalid or oversized pipeline literal parameters")
        }
        let mut params = self.params.clone();
        let object = params
            .as_object_mut()
            .context("Pipeline step params must be an object")?;
        for (key, value) in [
            ("resource_id", &self.resource_id),
            ("model_id", &self.model_id),
            ("backend", &self.backend),
        ] {
            if let Some(value) = value {
                if object.get(key).is_some_and(|old| old != &json!(value)) {
                    bail!("Conflicting pipeline operation identity")
                }
                object.insert(key.into(), json!(value));
            }
        }
        Ok(params)
    }
    pub fn validate(&self, executor: Option<&str>) -> Result<()> {
        step_id(&self.id)?;
        let params = self.bound_params()?;
        crate::grants::OperationShape {
            capability: &self.capability,
            action: &self.action,
            params: &params,
            resource_id: self.resource_id.as_deref(),
            model_id: self.model_id.as_deref(),
            backend: self.backend.as_deref(),
            model_role: self.model_role.as_deref(),
        }
        .validate(executor)?;
        let keys = parameter_keys(&self.capability, &self.action)?;
        if !matches!(self.capability.as_str(), "llm.infer" | "vlm.analyze")
            && (self.params.get("model_id").is_some() || self.params.get("backend").is_some())
        {
            bail!("Non-model pipeline step cannot claim a model identity")
        }
        if self.params.as_object().unwrap().keys().any(|k| {
            !keys.contains(&k.as_str())
                && !matches!(k.as_str(), "resource_id" | "model_id" | "backend")
        }) {
            bail!("Unsupported pipeline step parameter")
        }
        if self.inputs.len() > 8 {
            bail!("Too many pipeline input bindings")
        }
        let mut targets = BTreeSet::new();
        for input in &self.inputs {
            step_id(&input.source_step)?;
            if !data_parameter(&self.capability, &self.action, &input.parameter)
                || !targets.insert(&input.parameter)
                || self.params.get(&input.parameter).is_some()
                || (input.wrap_array && input.parameter != "images")
                || !valid_pointer(&input.pointer)
            {
                bail!(
                    "Pipeline inputs can bind distinct data parameters only, without literal or authority overrides"
                )
            }
        }
        if matches!(self.capability.as_str(), "llm.infer" | "vlm.analyze") {
            let prompt = self.params.get("prompt").is_some()
                || self.inputs.iter().any(|i| i.parameter == "prompt");
            if prompt == self.params.get("messages").is_some() {
                bail!("Pipeline model step needs exactly one prompt or messages source")
            }
            validate_model_literals(&self.params, self.capability == "vlm.analyze", &self.inputs)?;
        }
        Ok(())
    }
    /// Resolve only licensed data fields. Missing/type-invalid output fails;
    /// it never changes the graph, operation identity or literal parameters.
    pub fn resolve(&self, results: &BTreeMap<String, Value>) -> Result<Value> {
        self.resolve_with_limit(results, MAX_RETAINED_BYTES)
    }
    pub fn resolve_with_limit(
        &self,
        results: &BTreeMap<String, Value>,
        limit: usize,
    ) -> Result<Value> {
        if !(1024..=MAX_RETAINED_BYTES).contains(&limit) {
            bail!("Invalid pipeline resolution data budget")
        }
        self.validate(None)?;
        let mut params = self.bound_params()?;
        let mut remaining = limit - encoded_bytes(&params, limit)?;
        for input in &self.inputs {
            let source = results
                .get(&input.source_step)
                .context("Pipeline source result is unavailable")?;
            let value = source
                .pointer(&input.pointer)
                .context("Pipeline source field is unavailable")?;
            let overhead = input.parameter.len() + 8;
            let budget = remaining
                .checked_sub(overhead)
                .context("Resolved pipeline input exceeds data budget")?;
            let bytes = encoded_bytes(value, budget)?;
            remaining = budget
                .checked_sub(bytes)
                .context("Resolved pipeline input exceeds data budget")?;
            let value = if input.wrap_array {
                json!([value])
            } else {
                value.clone()
            };
            validate_data_type(&input.parameter, &value)?;
            params[&input.parameter] = value;
        }
        if matches!(self.capability.as_str(), "llm.infer" | "vlm.analyze") {
            validate_model_literals(&params, self.capability == "vlm.analyze", &[])?;
        }
        Ok(params)
    }
}
fn validate_model_literals(params: &Value, vision: bool, inputs: &[DataInput]) -> Result<()> {
    if let Some(prompt) = params.get("prompt") {
        crate::interpretation::messages(
            prompt.as_str().context("Pipeline prompt must be text")?,
            &[],
        )?;
    }
    if let Some(messages) = params.get("messages") {
        let messages = messages
            .as_array()
            .context("Pipeline messages must be an array")?;
        if messages.is_empty() || messages.len() > 64 {
            bail!("Pipeline messages exceed bounds")
        }
        for message in messages {
            let object = message
                .as_object()
                .context("Pipeline message must be an object")?;
            if object
                .keys()
                .any(|key| !matches!(key.as_str(), "role" | "content"))
                || !matches!(
                    message["role"].as_str(),
                    Some("system" | "user" | "assistant")
                )
            {
                bail!("Unsupported pipeline message")
            }
            crate::interpretation::messages(
                message["content"]
                    .as_str()
                    .context("Pipeline message content must be text")?,
                &[],
            )?;
        }
    }
    if let Some(images) = params.get("images") {
        validate_data_type("images", images)?;
    }
    if vision && params.get("images").is_none() && !inputs.iter().any(|i| i.parameter == "images") {
        bail!("Pipeline vision step requires literal images or an earlier image binding")
    }
    if params
        .get("temperature")
        .is_some_and(|v| v.as_f64().is_none_or(|n| !(0.0..=2.0).contains(&n)))
        || params
            .get("max_tokens")
            .is_some_and(|v| v.as_u64().is_none_or(|n| n == 0 || n > 32768))
    {
        bail!("Invalid pipeline model generation bounds")
    }
    Ok(())
}
/// Count encoded data with a bounded writer; do not allocate an unbounded JSON
/// copy merely to discover that it exceeds the local data budget.
pub(crate) fn encoded_bytes(value: &impl Serialize, limit: usize) -> Result<usize> {
    struct Counter {
        bytes: usize,
        limit: usize,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let next = self
                .bytes
                .checked_add(buf.len())
                .filter(|n| *n <= self.limit)
                .ok_or_else(|| std::io::Error::other("Pipeline encoded data budget exceeded"))?;
            self.bytes = next;
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter { bytes: 0, limit };
    serde_json::to_writer(&mut counter, value)?;
    Ok(counter.bytes)
}
fn valid_pointer(pointer: &str) -> bool {
    if pointer.len() > 256
        || pointer.chars().any(char::is_control)
        || (!pointer.is_empty() && !pointer.starts_with('/'))
    {
        return false;
    }
    let mut bytes = pointer.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'~' && !matches!(bytes.next(), Some(b'0' | b'1')) {
            return false;
        }
    }
    true
}
fn data_parameter(cap: &str, action: &str, key: &str) -> bool {
    matches!(
        (cap, action, key),
        ("llm.infer" | "vlm.analyze", "run" | "analyze", "prompt")
            | ("vlm.analyze", "run" | "analyze", "images")
            | ("filesystem", "write", "content")
            | ("http.request", "send", "body")
            | ("stt.transcribe", "transcribe", "audio_base64")
            | ("audio.play", "play", "data_base64")
            | ("tts.speak", "speak", "text")
            | ("input.control", "type", "text")
    )
}
fn validate_data_type(key: &str, value: &Value) -> Result<()> {
    match key {
        "body" => {}
        "images" => {
            let values = value
                .as_array()
                .context("Pipeline images must be an array")?;
            if values.is_empty()
                || values.len() > 8
                || values.iter().any(|v| v.as_str().is_none_or(str::is_empty))
            {
                bail!("Invalid pipeline image data")
            }
        }
        _ => {
            value.as_str().context("Pipeline data must be text")?;
        }
    }
    Ok(())
}
fn parameter_keys(cap: &str, action: &str) -> Result<&'static [&'static str]> {
    for template in crate::request_planning::METHOD_TEMPLATES {
        for (candidate, operation, keys) in crate::request_planning::template_steps(template)? {
            if candidate == cap
                && (operation == action || (cap == "vlm.analyze" && action == "analyze"))
            {
                return Ok(keys);
            }
        }
    }
    // Existing grants also support these explicit input actions; they are not
    // model-selected templates and retain their static operation parameters.
    match (cap, action) {
        ("system.ping", "run") => Ok(&[]),
        ("input.control", "move") => Ok(&["x", "y", "duration"]),
        ("input.control", "click") => Ok(&["x", "y", "button", "clicks"]),
        ("input.control", "scroll") => Ok(&["amount"]),
        _ => bail!("No finite pipeline parameter contract for this operation"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pipeline() -> FinitePipeline {
        serde_json::from_value(json!({"schema_version":1,"max_retained_bytes":65536,"steps":[
            {"id":"capture","capability":"camera.capture","action":"capture","params":{},"resource_id":"node-a::camera"},
            {"id":"describe","capability":"vlm.analyze","action":"run","model_id":"selected","backend":"llamacpp",
                "params":{"prompt":"Describe this image."},"inputs":[{"parameter":"images","source_step":"capture","pointer":"/image","wrap_array":true}]},
            {"id":"speak","capability":"tts.speak","action":"speak","params":{},"inputs":[{"parameter":"text","source_step":"describe","pointer":"/content"}]}
        ]})).unwrap()
    }
    #[test]
    fn camera_model_audio_graph_has_fixed_identities_and_backward_data_only() {
        let definition = pipeline();
        definition.validate().unwrap();
        definition.validate_for_executor(Some("node-a")).unwrap();
        assert!(definition.validate_for_executor(Some("node-b")).is_err());
        assert_eq!(
            definition.reference_counts().unwrap(),
            BTreeMap::from([
                ("capture".into(), 1),
                ("describe".into(), 1),
                ("speak".into(), 0)
            ])
        );
        let params = definition.steps[1]
            .resolve(&BTreeMap::from([(
                "capture".into(),
                json!({"image":"synthetic base64","private":"must not be copied"}),
            )]))
            .unwrap();
        assert_eq!(
            params,
            json!({"model_id":"selected","backend":"llamacpp","prompt":"Describe this image.","images":["synthetic base64"]})
        );
        assert_eq!(
            definition.steps[2]
                .resolve(&BTreeMap::from([(
                    "describe".into(),
                    json!({"content":"Hello"})
                )]))
                .unwrap(),
            json!({"text":"Hello"})
        );
    }
    #[test]
    fn forward_self_missing_sources_duplicate_ids_and_cycles_are_refused() {
        for source in ["consume", "describe", "missing"] {
            let mut value = pipeline();
            value.steps[0] = serde_json::from_value(
                json!({"id":"consume","capability":"tts.speak","action":"speak","params":{}}),
            )
            .unwrap();
            value.steps[0].inputs = vec![DataInput {
                parameter: "text".into(),
                source_step: source.into(),
                pointer: "/image".into(),
                wrap_array: false,
            }];
            assert!(
                value
                    .validate()
                    .unwrap_err()
                    .to_string()
                    .contains("completed earlier step")
            );
        }
        let mut value = pipeline();
        value.steps[1].inputs[0].source_step = "describe".into();
        assert!(value.validate().is_err());
        let mut value = pipeline();
        value.steps[2].id = value.steps[1].id.clone();
        assert!(value.validate().is_err());
        let mut value = pipeline();
        value.steps[1].id = "a\nb".into();
        assert!(value.validate().is_err());
    }
    #[test]
    fn operation_model_resource_and_unknown_field_substitution_is_refused() {
        let base = json!(pipeline());
        for (path, value) in [
            ("/steps/0/capability", json!("pipeline.run")),
            ("/steps/0/capability", json!("node.configure")),
            ("/steps/0/action", json!("restart")),
            ("/steps/1/backend", json!("future")),
            ("/steps/1/model_id", Value::Null),
            ("/steps/1/model_role", json!("invalid role")),
            ("/steps/1/params/backend", json!("ollama")),
            ("/steps/1/params/request_context", json!({})),
            ("/steps/0/params/model_id", json!("fake")),
        ] {
            let mut raw = base.clone();
            // Missing object members are inserted without changing production schema.
            let (parent, key) = path.rsplit_once('/').unwrap();
            raw.pointer_mut(parent).unwrap()[key] = value;
            assert!(
                serde_json::from_value::<FinitePipeline>(raw)
                    .unwrap()
                    .validate()
                    .is_err(),
                "{path}"
            );
        }
        for (parent, key) in [
            ("", "authority"),
            ("/steps/0", "timeout_override"),
            ("/steps/1/inputs/0", "permission"),
        ] {
            let mut raw = base.clone();
            raw.pointer_mut(parent).unwrap()[key] = json!(true);
            assert!(serde_json::from_value::<FinitePipeline>(raw).is_err());
        }
    }
    #[test]
    fn data_cannot_rewrite_paths_programs_urls_roles_or_literal_inputs() {
        for (cap, action, key) in [
            ("filesystem", "write", "path"),
            ("http.request", "send", "url"),
            ("process.exec", "run", "program"),
            ("application.control", "launch", "args"),
            ("vlm.analyze", "run", "model_id"),
            ("vlm.analyze", "run", "backend"),
            ("vlm.analyze", "run", "resource_id"),
        ] {
            let mut step = pipeline().steps[1].clone();
            step.capability = cap.into();
            step.action = action.into();
            if cap != "vlm.analyze" {
                step.model_id = None;
                step.backend = None;
                step.model_role = None;
                step.params = json!({});
            }
            step.inputs[0].parameter = key.into();
            step.inputs[0].wrap_array = false;
            assert!(
                step.validate(None)
                    .unwrap_err()
                    .to_string()
                    .contains("data parameters")
            );
        }
        let mut definition = pipeline();
        definition.steps[1].params["images"] = json!(["literal"]);
        assert!(definition.validate().is_err());
        let mut definition = pipeline();
        let duplicate = definition.steps[1].inputs[0].clone();
        definition.steps[1].inputs.push(duplicate);
        assert!(definition.validate().is_err());
    }
    #[test]
    fn missing_or_wrong_type_results_never_resolve_or_change_definition() {
        let definition = pipeline();
        let original = json!(definition);
        for result in [
            json!({}),
            json!({"image":null}),
            json!({"image":123}),
            json!({"image":[]}),
            json!({"image":""}),
        ] {
            assert!(
                definition.steps[1]
                    .resolve(&BTreeMap::from([("capture".into(), result)]))
                    .is_err()
            );
        }
        assert!(definition.steps[1].resolve(&BTreeMap::new()).is_err());
        assert!(
            definition.steps[2]
                .resolve(&BTreeMap::from([(
                    "describe".into(),
                    json!({"content":{}})
                )]))
                .is_err()
        );
        assert_eq!(json!(definition), original);
    }
    #[test]
    fn escaped_pointers_are_strict_and_do_not_evaluate_interpolation() {
        let mut step = pipeline().steps[2].clone();
        step.inputs[0].pointer = "/a~1b/~0".into();
        let data = BTreeMap::from([(
            "describe".into(),
            json!({"a/b":{"~":"literal ${model} $()"}}),
        )]);
        assert_eq!(step.resolve(&data).unwrap()["text"], "literal ${model} $()");
        for pointer in ["relative", "/bad~", "/bad~2", "/new\nline"] {
            step.inputs[0].pointer = pointer.into();
            assert!(step.validate(None).is_err());
        }
    }
    #[test]
    fn encoded_retention_bounds_steps_definition_and_resolved_data() {
        let mut definition = pipeline();
        definition.schema_version = 2;
        assert!(definition.validate().is_err());
        for limit in [0, 1023, MAX_RETAINED_BYTES + 1, usize::MAX] {
            let mut value = pipeline();
            value.max_retained_bytes = limit;
            assert!(value.validate().is_err());
        }
        let mut value = pipeline();
        value.steps.clear();
        assert!(value.validate().is_err());
        let mut value = pipeline();
        value.steps = vec![value.steps[0].clone(); MAX_STEPS + 1];
        assert!(value.validate().is_err());
        let mut value = pipeline();
        value.steps[1].params["prompt"] = json!("x".repeat(MAX_DEFINITION_BYTES));
        assert!(value.validate().is_err());
        let step = pipeline().steps[2].clone();
        assert!(
            step.resolve(&BTreeMap::from([(
                "describe".into(),
                json!({"content":"x".repeat(MAX_RETAINED_BYTES)})
            )]))
            .is_err()
        );
        assert_eq!(
            encoded_bytes(&json!({"x":"é"}), 100).unwrap(),
            serde_json::to_vec(&json!({"x":"é"})).unwrap().len()
        );
        assert!(encoded_bytes(&json!("x".repeat(100)), 50).is_err());
        let mut oversized = pipeline().steps[2].clone();
        oversized.params = json!({"text":"x".repeat(MAX_DEFINITION_BYTES)});
        assert!(
            oversized.bound_params().is_err(),
            "standalone literal inputs are checked before cloning"
        );
        let mut limited = pipeline();
        limited.max_retained_bytes = 1024;
        assert!(
            limited
                .resolve_step(
                    2,
                    &BTreeMap::from([("describe".into(), json!({"content":"x".repeat(1024)}))])
                )
                .is_err()
        );
        assert!(limited.resolve_step(16, &BTreeMap::new()).is_err());
    }
    #[test]
    fn model_literal_types_images_roles_and_generation_bounds_are_not_silently_defaulted() {
        for (key, value) in [
            ("prompt", json!(true)),
            ("messages", json!("not an array")),
            ("temperature", json!(3)),
            ("max_tokens", json!(0)),
            ("images", json!([])),
        ] {
            let mut definition = pipeline();
            definition.steps[1].params[key] = value;
            assert!(definition.validate().is_err());
        }
        let mut definition = pipeline();
        definition.steps[1].inputs.clear();
        assert!(definition.validate().is_err());
        let mut definition = pipeline();
        definition.steps[1].params = json!({"messages":[{"role":"tool","content":"unapproved"}]});
        assert!(definition.validate().is_err());
        let click:PipelineStep=serde_json::from_value(json!({"id":"click","capability":"input.control","action":"click","params":{"interval":1}})).unwrap();
        assert!(
            click.validate(None).is_err(),
            "unsupported ignored adapter fields must not appear active"
        );
        let mut bound = pipeline().steps[1].clone();
        bound.params = json!({});
        bound.inputs.push(DataInput {
            parameter: "prompt".into(),
            source_step: "capture".into(),
            pointer: "/text".into(),
            wrap_array: false,
        });
        let data = BTreeMap::from([(
            "capture".into(),
            json!({"image":"synthetic","text":"x".repeat(crate::interpretation::MAX_INPUT_BYTES+1)}),
        )]);
        assert!(
            bound.resolve(&data).is_err(),
            "resolved model prompts keep the literal model bounds"
        );
    }
    #[test]
    fn process_audio_http_and_filesystem_bind_only_data_not_authority() {
        for (cap, action, params, key, data) in [
            (
                "filesystem",
                "write",
                json!({"path":"owner-fixed.txt"}),
                "content",
                json!("untrusted content"),
            ),
            (
                "http.request",
                "send",
                json!({"url":"https://owner.example","method":"POST"}),
                "body",
                json!({"data":"untrusted"}),
            ),
            (
                "audio.play",
                "play",
                json!({}),
                "data_base64",
                json!("synthetic"),
            ),
            (
                "stt.transcribe",
                "transcribe",
                json!({"filename":"recording.wav"}),
                "audio_base64",
                json!("synthetic"),
            ),
        ] {
            let step: PipelineStep = serde_json::from_value(
                json!({"id":"consume","capability":cap,"action":action,"params":params,
                "inputs":[{"parameter":key,"source_step":"source","pointer":"/value"}]}),
            )
            .unwrap();
            step.validate(None).unwrap();
            let resolved = step
                .resolve(&BTreeMap::from([("source".into(), json!({"value":data}))]))
                .unwrap();
            assert_eq!(resolved[key], data);
            for (k, v) in params.as_object().unwrap() {
                assert_eq!(&resolved[k], v);
            }
        }
    }
}
