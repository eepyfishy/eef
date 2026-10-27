//! Explicit conditional stages in one finite local graph. Conditions inspect
//! data only: they cannot choose operations, bindings, permissions or executors.
use crate::pipeline::{
    FinitePipeline, MAX_DEFINITION_BYTES, encoded_bytes, step_id, valid_pointer,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

pub const CAPABILITY: &str = "pipeline.when";
pub const TEMPLATE: &str = "conditional_pipeline";
/// Bounded outcome metadata must not consume the licensed local data budget.
pub const RESULT_METADATA_RESERVATION: usize = 4096;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionalPipeline {
    pub schema_version: u32,
    pub graph: FinitePipeline,
    pub conditions: BTreeMap<String, Condition>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    pub source_step: String,
    pub pointer: String,
    pub operator: Operator,
    pub expected: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    Equal,
    GreaterThan,
    LessThan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Run {
        condition_matched: Option<bool>,
    },
    SkipCondition,
    SkipDependency {
        source_step: String,
        condition_matched: Option<bool>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StepState {
    Completed,
    SkippedCondition,
    SkippedDependency,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StepOutcome {
    pub id: String,
    pub state: StepState,
    pub condition_matched: Option<bool>,
    pub skipped_dependency: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConditionalResult {
    schema_version: u32,
    report_type: String,
    success: bool,
    outcomes: Vec<StepOutcome>,
    output: Value,
}

pub(crate) fn result(outcomes: Vec<StepOutcome>, output: Value, success: bool) -> Value {
    serde_json::json!(ConditionalResult {
        schema_version: 1,
        report_type: "conditional_pipeline_result".into(),
        success,
        outcomes,
        output
    })
}

/// This validates the reported path, not the truth of an endpoint's private
/// observations or physical/external effects. Skips never authorize dependencies.
pub(crate) fn confirmed_result(
    grant: &crate::grants::ExecutionGrant,
    data: &Value,
) -> Result<bool> {
    let definition = ConditionalPipeline::from_grant(grant)?;
    let report: ConditionalResult = serde_json::from_value(data.clone())?;
    if report.schema_version != 1
        || report.report_type != "conditional_pipeline_result"
        || report.outcomes.is_empty()
        || report.outcomes.len() > definition.graph.steps.len()
        || (report.success && report.outcomes.len() != definition.graph.steps.len())
    {
        bail!("Conditional completion does not match its definition")
    }
    encoded_bytes(&report.output, definition.graph.max_retained_bytes)?;
    let mut skipped = BTreeSet::new();
    for (index, outcome) in report.outcomes.iter().enumerate() {
        let step = &definition.graph.steps[index];
        let condition = definition.conditions.get(&step.id);
        if outcome.id != step.id {
            bail!("Conditional outcome order changed")
        }
        let guard_skipped = condition.is_some_and(|c| skipped.contains(&c.source_step));
        let input_skipped = step
            .inputs
            .iter()
            .any(|input| skipped.contains(&input.source_step));
        match outcome.state {
            StepState::Completed | StepState::Failed => {
                if guard_skipped
                    || input_skipped
                    || outcome.skipped_dependency.is_some()
                    || outcome.condition_matched != condition.map(|_| true)
                {
                    bail!("Conditional operation cannot run with skipped or unconfirmed inputs")
                }
                if outcome.state == StepState::Failed
                    && (report.success
                        || index + 1 != report.outcomes.len()
                        || step.capability != "process.exec"
                        || step.action != "run"
                        || report.output.get("success") != Some(&Value::Bool(false)))
                {
                    bail!("Conditional known failure lacks exact native-exit evidence")
                }
            }
            StepState::SkippedCondition => {
                if condition.is_none()
                    || guard_skipped
                    || outcome.condition_matched != Some(false)
                    || outcome.skipped_dependency.is_some()
                {
                    bail!("Conditional skip lacks a completed guard source")
                }
                skipped.insert(step.id.clone());
            }
            StepState::SkippedDependency => {
                let source = outcome
                    .skipped_dependency
                    .as_deref()
                    .context("Skipped dependency missing")?;
                let allowed = if guard_skipped {
                    condition.is_some_and(|c| c.source_step == source)
                        && outcome.condition_matched.is_none()
                } else {
                    step.inputs
                        .iter()
                        .any(|i| i.source_step == source && skipped.contains(source))
                        && outcome.condition_matched == condition.map(|_| true)
                };
                if !allowed {
                    bail!("Conditional dependency skip is not supported by prior outcomes")
                }
                skipped.insert(step.id.clone());
            }
        }
    }
    let last = report.outcomes.last().unwrap();
    if !report.success && last.state != StepState::Failed {
        bail!("Conditional failure is not a known unsuccessful stage")
    }
    if matches!(
        last.state,
        StepState::SkippedCondition | StepState::SkippedDependency
    ) && !report.output.is_null()
    {
        bail!("A skipped final stage cannot publish another stage's output")
    }
    let last_step = &definition.graph.steps[report.outcomes.len() - 1];
    if report.success
        && last.state == StepState::Completed
        && last_step.capability == "process.exec"
        && last_step.action == "run"
        && report.output.get("success") == Some(&Value::Bool(false))
    {
        bail!("An unsuccessful final native stage cannot claim success")
    }
    Ok(report.success)
}

impl Condition {
    fn validate(&self) -> Result<()> {
        step_id(&self.source_step)?;
        if !valid_pointer(&self.pointer)
            || encoded_bytes(&self.expected, 512).is_err()
            || !(self.expected.is_boolean()
                || self.expected.is_number()
                || self.expected.is_string())
            || self.expected.as_str().is_some_and(|s| s.len() > 256)
            || (self.operator != Operator::Equal && !self.expected.is_number())
        {
            bail!("Invalid bounded scalar pipeline condition")
        }
        if self.expected.is_number() && self.expected.as_f64().is_none_or(|n| !n.is_finite()) {
            bail!("Pipeline condition requires a finite number")
        }
        Ok(())
    }

    fn matches(&self, source: &Value) -> Result<bool> {
        self.validate()?;
        let value = source
            .pointer(&self.pointer)
            .context("Conditional pipeline data is unavailable")?;
        let ordering = if value.is_number() && self.expected.is_number() {
            number_ordering(value, &self.expected)?
        } else if let (Some(left), Some(right)) = (value.as_bool(), self.expected.as_bool()) {
            left.cmp(&right)
        } else if let (Some(left), Some(right)) = (value.as_str(), self.expected.as_str()) {
            if left.len() > 256 {
                bail!("Conditional pipeline scalar exceeds its bound")
            }
            left.cmp(right)
        } else {
            // Missing, null, wrong-type or structured data is uncertainty, not
            // false, truthiness, permission to skip or permission to execute.
            bail!("Conditional pipeline data has the wrong scalar type")
        };
        Ok(match self.operator {
            Operator::Equal => ordering == Ordering::Equal,
            Operator::GreaterThan => ordering == Ordering::Greater,
            Operator::LessThan => ordering == Ordering::Less,
        })
    }
}

fn number_ordering(left: &Value, right: &Value) -> Result<Ordering> {
    if let (Some(a), Some(b)) = (left.as_i64(), right.as_i64()) {
        return Ok(a.cmp(&b));
    }
    if let (Some(a), Some(b)) = (left.as_u64(), right.as_u64()) {
        return Ok(a.cmp(&b));
    }
    if left.as_i64().is_some_and(|a| a < 0) && right.as_u64().is_some() {
        return Ok(Ordering::Less);
    }
    if right.as_i64().is_some_and(|b| b < 0) && left.as_u64().is_some() {
        return Ok(Ordering::Greater);
    }
    let a = left
        .as_f64()
        .filter(|n| n.is_finite())
        .context("Invalid numeric pipeline condition")?;
    let b = right
        .as_f64()
        .filter(|n| n.is_finite())
        .context("Invalid numeric pipeline condition")?;
    const EXACT_INTEGER_BOUND: f64 = 9_007_199_254_740_992.0;
    if ((left.is_i64() || left.is_u64()) && a.abs() > EXACT_INTEGER_BOUND)
        || ((right.is_i64() || right.is_u64()) && b.abs() > EXACT_INTEGER_BOUND)
    {
        bail!("Mixed numeric pipeline condition would lose integer precision")
    }
    a.partial_cmp(&b)
        .context("Unordered numeric pipeline condition")
}

impl ConditionalPipeline {
    pub fn validate(&self) -> Result<()> {
        self.validate_for_executor(None)
    }

    pub fn validate_for_executor(&self, executor: Option<&str>) -> Result<()> {
        self.graph.validate_for_executor(executor)?;
        if self.schema_version != 1
            || self.conditions.is_empty()
            || self.conditions.len() > self.graph.steps.len()
            || encoded_bytes(self, MAX_DEFINITION_BYTES).is_err()
        {
            bail!("Unsupported or unbounded conditional pipeline definition")
        }
        let indices = self
            .graph
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| (s.id.as_str(), i))
            .collect::<BTreeMap<_, _>>();
        for (target, condition) in &self.conditions {
            step_id(target)?;
            condition.validate()?;
            let target_index = indices
                .get(target.as_str())
                .context("Conditional pipeline target is unavailable")?;
            let source_index = indices
                .get(condition.source_step.as_str())
                .context("Conditional pipeline source is unavailable")?;
            if source_index >= target_index {
                bail!(
                    "Pipeline conditions require an earlier result, not a cycle or forward reference"
                )
            }
        }
        Ok(())
    }

    pub(crate) fn from_grant(grant: &crate::grants::ExecutionGrant) -> Result<Self> {
        if grant.capability != CAPABILITY
            || grant.action != "run"
            || grant.resource_id.is_some()
            || grant.model_id.is_some()
            || grant.backend.is_some()
            || grant.model_role.is_some()
            || grant
                .params
                .as_object()
                .is_none_or(|p| p.len() != 1 || !p.contains_key("pipeline"))
        {
            bail!(
                "Conditional pipeline requires one exact definition and no substituted root bindings"
            )
        }
        let definition: Self = serde_json::from_value(grant.params["pipeline"].clone())?;
        definition.validate_for_executor(Some(&grant.executor_node))?;
        if definition
            .graph
            .max_retained_bytes
            .checked_add(RESULT_METADATA_RESERVATION)
            .is_none_or(|n| n > grant.limits.max_result_bytes)
        {
            bail!("Conditional pipeline data and outcome metadata exceed the parent result budget")
        }
        Ok(definition)
    }

    /// Every guard read joins the existing last-use accounting. No per-cycle,
    /// hidden, unbounded or independent output cache is introduced.
    pub(crate) fn reference_counts(&self) -> Result<BTreeMap<String, usize>> {
        self.validate()?;
        let mut references = self.graph.reference_counts()?;
        for condition in self.conditions.values() {
            *references
                .get_mut(&condition.source_step)
                .context("Missing conditional reference accounting")? += 1;
        }
        Ok(references)
    }

    pub(crate) fn reference_sources(&self, index: usize) -> Result<Vec<&str>> {
        let step = self
            .graph
            .steps
            .get(index)
            .context("Conditional step index unavailable")?;
        let mut sources = step
            .inputs
            .iter()
            .map(|input| input.source_step.as_str())
            .collect::<Vec<_>>();
        if let Some(condition) = self.conditions.get(&step.id) {
            sources.push(&condition.source_step);
        }
        Ok(sources)
    }

    pub(crate) fn decision(
        &self,
        index: usize,
        values: &BTreeMap<String, Value>,
        skipped: &BTreeSet<String>,
    ) -> Result<Decision> {
        let step = self
            .graph
            .steps
            .get(index)
            .context("Conditional step index unavailable")?;
        let condition = self.conditions.get(&step.id);
        if let Some(condition) = condition {
            if skipped.contains(&condition.source_step) {
                return Ok(Decision::SkipDependency {
                    source_step: condition.source_step.clone(),
                    condition_matched: None,
                });
            }
            if !condition.matches(
                values
                    .get(&condition.source_step)
                    .context("Conditional source result unavailable")?,
            )? {
                return Ok(Decision::SkipCondition);
            }
        }
        for input in &step.inputs {
            if skipped.contains(&input.source_step) {
                return Ok(Decision::SkipDependency {
                    source_step: input.source_step.clone(),
                    condition_matched: condition.map(|_| true),
                });
            }
        }
        Ok(Decision::Run {
            condition_matched: condition.map(|_| true),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn definition() -> ConditionalPipeline {
        serde_json::from_value(json!({"schema_version":1,"graph":{"schema_version":1,"max_retained_bytes":1024,"steps":[
            {"id":"sample","capability":"system.ping","action":"run","params":{}},
            {"id":"infer","capability":"llm.infer","action":"run","model_id":"selected","backend":"ollama","params":{"prompt":"fixed"}},
            {"id":"sink","capability":"filesystem","action":"write","params":{"path":"fixed.txt"},"inputs":[{"parameter":"content","source_step":"infer","pointer":"/text"}]}
        ]},"conditions":{"infer":{"source_step":"sample","pointer":"/pong","operator":"equal","expected":true}}})).unwrap()
    }
    #[test]
    fn conditions_retain_guard_only_data_without_changing_graph_authority() {
        let d = definition();
        d.validate().unwrap();
        assert_eq!(
            d.reference_counts().unwrap(),
            BTreeMap::from([
                ("sample".into(), 1),
                ("infer".into(), 1),
                ("sink".into(), 0)
            ])
        );
        assert_eq!(d.reference_sources(1).unwrap(), vec!["sample"]);
        let values = BTreeMap::from([("sample".into(), json!({"pong":true}))]);
        assert_eq!(
            d.decision(0, &values, &BTreeSet::new()).unwrap(),
            Decision::Run {
                condition_matched: None
            }
        );
        assert_eq!(
            d.decision(1, &values, &BTreeSet::new()).unwrap(),
            Decision::Run {
                condition_matched: Some(true)
            }
        );
        assert_eq!(d.graph.steps[1].model_id.as_deref(), Some("selected"));
    }
    #[test]
    fn false_and_skipped_dependencies_are_explicit_not_missing_data_fallback() {
        let d = definition();
        let values = BTreeMap::from([("sample".into(), json!({"pong":false}))]);
        assert_eq!(
            d.decision(1, &values, &BTreeSet::new()).unwrap(),
            Decision::SkipCondition
        );
        assert_eq!(
            d.decision(2, &values, &BTreeSet::from(["infer".into()]))
                .unwrap(),
            Decision::SkipDependency {
                source_step: "infer".into(),
                condition_matched: None
            }
        );
        assert_eq!(
            d.decision(1, &BTreeMap::new(), &BTreeSet::from(["sample".into()]))
                .unwrap(),
            Decision::SkipDependency {
                source_step: "sample".into(),
                condition_matched: None
            }
        );
        for result in [
            json!({}),
            json!({"pong":null}),
            json!({"pong":"true"}),
            json!({"pong":{}}),
        ] {
            assert!(
                d.decision(
                    1,
                    &BTreeMap::from([("sample".into(), result)]),
                    &BTreeSet::new()
                )
                .is_err()
            );
        }
    }
    #[test]
    fn forward_cycles_unknown_targets_and_unbounded_values_refuse() {
        let good = serde_json::to_value(definition()).unwrap();
        for (path, value) in [
            ("/schema_version", json!(2)),
            ("/conditions/infer/source_step", json!("infer")),
            ("/conditions/infer/source_step", json!("sink")),
            ("/conditions/infer/source_step", json!("absent")),
            ("/conditions/infer/pointer", json!("/~2")),
            ("/conditions/infer/expected", json!(null)),
            ("/conditions/infer/expected", json!({})),
            ("/conditions/infer/expected", json!("x".repeat(257))),
            ("/conditions/infer/operator", json!("greater_than")),
        ] {
            let mut bad = good.clone();
            *bad.pointer_mut(path).unwrap() = value;
            assert!(
                serde_json::from_value::<ConditionalPipeline>(bad)
                    .unwrap()
                    .validate()
                    .is_err(),
                "{path}"
            );
        }
        let mut d = definition();
        d.conditions = BTreeMap::new();
        assert!(d.validate().is_err());
        d = definition();
        d.conditions
            .insert("absent".into(), d.conditions["infer"].clone());
        assert!(d.validate().is_err());
        let mut raw = good;
        raw["conditions"]["infer"]["execute"] = json!(true);
        assert!(serde_json::from_value::<ConditionalPipeline>(raw).is_err());
    }
    #[test]
    fn numeric_conditions_preserve_integer_precision_and_reject_mixed_loss() {
        let mut condition = Condition {
            source_step: "sample".into(),
            pointer: "".into(),
            operator: Operator::Equal,
            expected: json!(u64::MAX),
        };
        assert!(condition.matches(&json!(u64::MAX)).unwrap());
        assert!(!condition.matches(&json!(u64::MAX - 1)).unwrap());
        assert!(condition.matches(&json!(u64::MAX as f64)).is_err());
        condition.operator = Operator::GreaterThan;
        assert!(!condition.matches(&json!(-1)).unwrap());
        condition.expected = json!(4.5);
        assert!(condition.matches(&json!(5)).unwrap());
        assert!(!condition.matches(&json!(4)).unwrap());
        condition.operator = Operator::LessThan;
        assert!(condition.matches(&json!(4)).unwrap());
        assert!(condition.matches(&json!("4")).is_err());
    }
    #[test]
    fn guard_and_data_reads_count_separately_and_parent_budget_includes_metadata() {
        let mut d = definition();
        d.conditions.insert(
            "sink".into(),
            Condition {
                source_step: "infer".into(),
                pointer: "/ready".into(),
                operator: Operator::Equal,
                expected: json!(true),
            },
        );
        assert_eq!(d.reference_counts().unwrap()["infer"], 2);
        assert_eq!(d.reference_sources(2).unwrap(), vec!["infer", "infer"]);
        let mut grant = crate::grants::tests::grant();
        grant.capability = CAPABILITY.into();
        grant.params = json!({"pipeline":d});
        grant.limits.max_result_bytes = 1024 + RESULT_METADATA_RESERVATION;
        ConditionalPipeline::from_grant(&grant).unwrap();
        grant.limits.max_result_bytes -= 1;
        assert!(ConditionalPipeline::from_grant(&grant).is_err());
        grant.limits.max_result_bytes += 1;
        grant.model_id = Some("substituted".into());
        assert!(ConditionalPipeline::from_grant(&grant).is_err());
    }
    #[test]
    fn a_definition_does_not_enable_an_executor_or_extend_ordinary_rpc() {
        assert!(!crate::engine::CAPABILITIES.contains(&CAPABILITY));
        assert!(crate::execution_boundary::require_control(CAPABILITY).is_err());
        let mut grant = crate::grants::tests::grant();
        grant.capability = CAPABILITY.into();
        grant.params = json!({"pipeline":definition()});
        grant.validate().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let engine = crate::NodeEngine::new(false, vec![], directory.path().into()).unwrap();
        assert!(!engine.capabilities().contains(&CAPABILITY.into()));
        let ledger = crate::grants::GrantLedger::open(
            &directory.path().join("receipts.db"),
            Default::default(),
            &grant.executor_node,
        )
        .unwrap();
        assert!(
            crate::grants::AcceptedGrant::prepare(
                grant.clone(),
                ledger,
                &engine,
                &grant.issuer_id,
                &grant.session_id,
                &grant.executor_node
            )
            .is_err()
        );
    }
}
