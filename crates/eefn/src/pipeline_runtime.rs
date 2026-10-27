//! Finite execution inside one AcceptedGrant future. No extra worker, child
//! grants, durable job identities, lease renewal or automatic replay.
use crate::conditional_pipeline::{ConditionalPipeline, Decision, StepOutcome, StepState};
use crate::{
    NodeEngine,
    grants::ExecutionGrant,
    pipeline::{FinitePipeline, encoded_bytes},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

enum Definition {
    Finite(FinitePipeline),
    Conditional(ConditionalPipeline),
}
impl Definition {
    fn from_grant(g: &ExecutionGrant) -> Result<Self> {
        if g.capability == crate::conditional_pipeline::CAPABILITY {
            Ok(Self::Conditional(ConditionalPipeline::from_grant(g)?))
        } else {
            Ok(Self::Finite(FinitePipeline::from_grant(g)?))
        }
    }
    fn graph(&self) -> &FinitePipeline {
        match self {
            Self::Finite(graph) => graph,
            Self::Conditional(definition) => &definition.graph,
        }
    }
    fn reference_counts(&self) -> Result<BTreeMap<String, usize>> {
        match self {
            Self::Finite(graph) => graph.reference_counts(),
            Self::Conditional(definition) => definition.reference_counts(),
        }
    }
    fn reference_sources(&self, index: usize) -> Result<Vec<&str>> {
        match self {
            Self::Finite(graph) => Ok(graph.steps[index]
                .inputs
                .iter()
                .map(|i| i.source_step.as_str())
                .collect()),
            Self::Conditional(definition) => definition.reference_sources(index),
        }
    }
    fn decision(
        &self,
        index: usize,
        values: &BTreeMap<String, Value>,
        skipped: &BTreeSet<String>,
    ) -> Result<Decision> {
        match self {
            Self::Finite(_) => Ok(Decision::Run {
                condition_matched: None,
            }),
            Self::Conditional(definition) => definition.decision(index, values, skipped),
        }
    }
    fn result(
        &self,
        completed: usize,
        failed: bool,
        output: Value,
        outcomes: Vec<StepOutcome>,
    ) -> Value {
        match self {
            Self::Finite(graph) => result(graph, completed, failed, output),
            Self::Conditional(_) => crate::conditional_pipeline::result(outcomes, output, !failed),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PipelineResult {
    schema_version: u32,
    report_type: String,
    success: bool,
    completed_steps: Vec<String>,
    failed_step: Option<String>,
    output: Value,
}
fn result(definition: &FinitePipeline, completed: usize, failed: bool, output: Value) -> Value {
    json!(PipelineResult {
        schema_version: 1,
        report_type: "local_pipeline_result".into(),
        success: !failed,
        completed_steps: definition.steps[..completed]
            .iter()
            .map(|s| s.id.clone())
            .collect(),
        failed_step: failed.then(|| definition.steps[completed].id.clone()),
        output
    })
}
/// Correlate compound completion shape with the exact ordered parent definition.
/// Endpoint evidence remains endpoint evidence, not independent physical proof.
pub(crate) fn confirmed_result(g: &ExecutionGrant, data: &Value) -> Result<bool> {
    if g.capability == crate::conditional_pipeline::CAPABILITY {
        return crate::conditional_pipeline::confirmed_result(g, data);
    }
    let definition = FinitePipeline::from_grant(g)?;
    let report: PipelineResult = serde_json::from_value(data.clone())?;
    if report.schema_version != 1
        || report.report_type != "local_pipeline_result"
        || report.completed_steps.len() > definition.steps.len()
        || report
            .completed_steps
            .iter()
            .zip(&definition.steps)
            .any(|(id, step)| id != &step.id)
    {
        bail!("Pipeline completion does not match the parent definition")
    }
    if report.success {
        if report.failed_step.is_some() || report.completed_steps.len() != definition.steps.len() {
            bail!("Incomplete pipeline cannot claim success")
        }
    } else {
        let step = definition
            .steps
            .get(report.completed_steps.len())
            .context("Pipeline failure step missing")?;
        if report.failed_step.as_deref() != Some(&step.id)
            || step.capability != "process.exec"
            || step.action != "run"
            || report.output.get("success").and_then(Value::as_bool) != Some(false)
        {
            bail!("Pipeline failed-step evidence is not a supported known unsuccessful exit")
        }
    }
    Ok(report.success)
}
pub(crate) async fn execute(
    engine: &NodeEngine,
    g: &ExecutionGrant,
    ledger: &crate::grants::GrantLedger,
    deadline: std::time::Instant,
) -> Result<Value> {
    g.validate()?;
    engine.check_grant_admission(g)?;
    let definition = Definition::from_grant(g)?;
    let graph = definition.graph();
    let mut references = definition.reference_counts()?;
    let mut values = BTreeMap::<String, Value>::new();
    let mut sizes = BTreeMap::<String, usize>::new();
    let mut retained = 0usize;
    let mut skipped = BTreeSet::<String>::new();
    let mut outcomes = Vec::new();
    for (index, step) in graph.steps.iter().enumerate() {
        engine.check_grant_policy(g)?;
        let condition_matched = match definition.decision(index, &values, &skipped)? {
            Decision::Run { condition_matched } => condition_matched,
            decision => {
                let (state, matched, dependency) = match decision {
                    Decision::SkipCondition => (StepState::SkippedCondition, Some(false), None),
                    Decision::SkipDependency {
                        source_step,
                        condition_matched,
                    } => (
                        StepState::SkippedDependency,
                        condition_matched,
                        Some(source_step),
                    ),
                    Decision::Run { .. } => unreachable!(),
                };
                skipped.insert(step.id.clone());
                outcomes.push(StepOutcome {
                    id: step.id.clone(),
                    state,
                    condition_matched: matched,
                    skipped_dependency: dependency,
                });
                consume_references(
                    &definition,
                    index,
                    &mut references,
                    &mut values,
                    &mut sizes,
                    &mut retained,
                    &skipped,
                )?;
                continue;
            }
        };
        let static_params = step.bound_params()?;
        engine.check_operation_policy(step.operation(&static_params), &g.limits, true)?;
        let mut params = step.resolve_with_limit(&values, graph.max_retained_bytes)?;
        if let Some(model) = &step.model_id {
            // Same trusted adapter alias as a single model grant, not a new
            // selection or model-provided identity.
            params["model"] = json!(model);
            params["backend"] = json!(step.backend);
        }
        // Resolved data may be larger than wire literals but cannot alter the
        // fixed operation/model/resource identities used for policy checks.
        let resolved_bytes = encoded_bytes(&params, graph.max_retained_bytes)?;
        consume_references(
            &definition,
            index,
            &mut references,
            &mut values,
            &mut sizes,
            &mut retained,
            &skipped,
        )?;
        observe(
            &definition,
            engine,
            ledger,
            g,
            index,
            crate::pipeline_progress::Phase::Starting,
            retained,
            resolved_bytes,
        )?;
        // Journal I/O cannot extend the parent's lease or start effects after
        // a deadline that elapsed while saving the pre-effect observation.
        if std::time::Instant::now() >= deadline {
            bail!("Pipeline parent deadline expired before stage")
        }
        let output = engine
            .execute_with_python_output_limit(
                &step.capability,
                &step.action,
                params,
                Some(graph.max_retained_bytes),
                Some(tokio::time::Instant::from_std(deadline)),
            )
            .await?;
        let bytes = encoded_bytes(&output, graph.max_retained_bytes)?;
        if step.capability == "process.exec"
            && step.action == "run"
            && output.get("success").and_then(Value::as_bool) == Some(false)
        {
            if matches!(definition, Definition::Conditional(_)) {
                outcomes.push(StepOutcome {
                    id: step.id.clone(),
                    state: StepState::Failed,
                    condition_matched,
                    skipped_dependency: None,
                });
            }
            observe(
                &definition,
                engine,
                ledger,
                g,
                index,
                crate::pipeline_progress::Phase::Failed,
                retained,
                0,
            )?;
            return Ok(definition.result(index, true, output, outcomes));
        }
        if matches!(definition, Definition::Conditional(_)) {
            outcomes.push(StepOutcome {
                id: step.id.clone(),
                state: StepState::Completed,
                condition_matched,
                skipped_dependency: None,
            });
        }
        if index + 1 == graph.steps.len() {
            observe(
                &definition,
                engine,
                ledger,
                g,
                index,
                crate::pipeline_progress::Phase::Completed,
                0,
                0,
            )?;
            return Ok(definition.result(index + 1, false, output, outcomes));
        }
        if references[&step.id] > 0 {
            retained = retained
                .checked_add(bytes)
                .filter(|n| *n <= graph.max_retained_bytes)
                .context("Aggregate retained pipeline output exceeds its budget")?;
            sizes.insert(step.id.clone(), bytes);
            values.insert(step.id.clone(), output);
        }
        observe(
            &definition,
            engine,
            ledger,
            g,
            index,
            crate::pipeline_progress::Phase::Completed,
            retained,
            0,
        )?;
    }
    if matches!(definition, Definition::Conditional(_)) && outcomes.len() == graph.steps.len() {
        return Ok(definition.result(graph.steps.len(), false, Value::Null, outcomes));
    }
    bail!("Pipeline ended without a final result")
}

fn consume_references(
    definition: &Definition,
    index: usize,
    references: &mut BTreeMap<String, usize>,
    values: &mut BTreeMap<String, Value>,
    sizes: &mut BTreeMap<String, usize>,
    retained: &mut usize,
    skipped: &BTreeSet<String>,
) -> Result<()> {
    for source in definition.reference_sources(index)? {
        let uses = references
            .get_mut(source)
            .context("Pipeline reference accounting missing")?;
        *uses = uses
            .checked_sub(1)
            .context("Pipeline reference accounting underflow")?;
        if *uses == 0 {
            values.remove(source);
            if let Some(bytes) = sizes.remove(source) {
                *retained = retained
                    .checked_sub(bytes)
                    .context("Pipeline retained byte accounting underflow")?;
            } else if !skipped.contains(source) {
                bail!("Pipeline retained size missing")
            }
        }
    }
    Ok(())
}

fn observe(
    definition: &Definition,
    engine: &NodeEngine,
    ledger: &crate::grants::GrantLedger,
    g: &ExecutionGrant,
    index: usize,
    phase: crate::pipeline_progress::Phase,
    retained: usize,
    resolved: usize,
) -> Result<()> {
    // Conditional outcomes live in the original final execution receipt. Never
    // label a skipped stage as completed in the legacy finite-progress schema.
    if matches!(definition, Definition::Finite(_)) {
        progress(engine, ledger, g, index, phase, retained, resolved)?;
    }
    Ok(())
}
fn progress(
    engine: &NodeEngine,
    ledger: &crate::grants::GrantLedger,
    g: &ExecutionGrant,
    index: usize,
    phase: crate::pipeline_progress::Phase,
    retained: usize,
    resolved: usize,
) -> Result<()> {
    let checkpoint = crate::pipeline_progress::PipelineProgress::checkpoint(
        g, index, phase, retained, resolved,
    )?;
    let checkpoint = ledger.record_pipeline_progress(g, checkpoint)?;
    engine.record_pipeline_progress(g, json!(checkpoint));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grants::{AcceptedGrant, GrantLedger, ReceiptState};
    fn grant(steps: Value, limit: usize) -> ExecutionGrant {
        let mut g = crate::grants::tests::grant();
        g.capability = "pipeline.run".into();
        g.params =
            json!({"pipeline":{"schema_version":1,"max_retained_bytes":limit,"steps":steps}});
        g
    }
    fn engine(dir: &std::path::Path, write: bool) -> NodeEngine {
        NodeEngine::new(write, vec![dir.into()], dir.into())
            .unwrap()
            .with_pipeline_runtime()
    }
    fn copy_steps(dir: &std::path::Path) -> Value {
        json!([
            {"id":"read","capability":"filesystem","action":"read","params":{"path":dir.join("source.txt")}},
            {"id":"write","capability":"filesystem","action":"write","params":{"path":dir.join("copy.txt")},
                "inputs":[{"parameter":"content","source_step":"read","pointer":"/content"}]}
        ])
    }
    fn conditional_grant(steps: Value, conditions: Value, limit: usize) -> ExecutionGrant {
        let mut g = grant(steps, limit);
        let graph = g.params["pipeline"].take();
        g.capability = crate::conditional_pipeline::CAPABILITY.into();
        g.params = json!({"pipeline":{"schema_version":1,"graph":graph,"conditions":conditions}});
        g.limits.max_result_bytes =
            limit + crate::conditional_pipeline::RESULT_METADATA_RESERVATION + 1024;
        g
    }
    fn ping_guard(expected: bool) -> Value {
        json!({"source_step":"sample","pointer":"/pong","operator":"equal","expected":expected})
    }
    #[tokio::test]
    async fn conditional_false_skips_effect_and_data_dependents_without_replay() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.txt");
        let later = dir.path().join("later.txt");
        let engine = engine(dir.path(), true);
        let g = conditional_grant(
            json!([
                {"id":"sample","capability":"system.ping","action":"run","params":{}},
                {"id":"write","capability":"filesystem","action":"write","params":{"path":first,"content":"PRIVATE-never-written"}},
                {"id":"dependent","capability":"filesystem","action":"write","params":{"path":later},"inputs":[{"parameter":"content","source_step":"write","pointer":"/path"}]}
            ]),
            json!({"write":ping_guard(false)}),
            1024,
        );
        let path = dir.path().join("receipts.db");
        let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        let report = AcceptedGrant::prepare(
            g.clone(),
            ledger.clone(),
            &engine,
            &g.issuer_id,
            &g.session_id,
            &g.executor_node,
        )
        .unwrap()
        .run(&engine)
        .await;
        assert_eq!(report.state, ReceiptState::Completed);
        assert!(report.success && report.persisted && report.confirms(&g));
        assert_eq!(report.data["report_type"], "conditional_pipeline_result");
        assert_eq!(report.data["outcomes"][0]["state"], "completed");
        assert_eq!(report.data["outcomes"][1]["state"], "skipped_condition");
        assert_eq!(report.data["outcomes"][2]["state"], "skipped_dependency");
        assert!(report.data["output"].is_null());
        assert!(!first.exists() && !later.exists());
        let retained = ledger.receipt(&g.issuer_id, &g.grant_id).unwrap();
        assert!(retained.get("pipeline_progress").is_none_or(Value::is_null));
        drop(ledger);
        let reopened = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        assert_eq!(
            reopened.receipt(&g.issuer_id, &g.grant_id).unwrap(),
            retained
        );
        assert!(
            AcceptedGrant::prepare(
                g.clone(),
                reopened,
                &engine,
                &g.issuer_id,
                &g.session_id,
                &g.executor_node
            )
            .is_err()
        );
        for path in [
            "/outcomes/1/condition_matched",
            "/outcomes/1/state",
            "/outcomes/2/skipped_dependency",
            "/outcomes/0/id",
            "/output",
        ] {
            let mut forged = report.clone();
            *forged.data.pointer_mut(path).unwrap() = match path {
                "/outcomes/1/condition_matched" => json!(true),
                "/outcomes/1/state" => json!("completed"),
                "/outcomes/2/skipped_dependency" => json!("sample"),
                "/outcomes/0/id" => json!("other"),
                _ => json!("PRIVATE-upstream-output"),
            };
            assert!(!forged.confirms(&g), "{path}");
        }
    }
    #[tokio::test]
    async fn conditional_true_runs_under_the_original_parent_and_missing_data_refuses() {
        for pointer in ["/pong", "/missing"] {
            let dir = tempfile::tempdir().unwrap();
            let target = dir.path().join("effect.txt");
            let engine = engine(dir.path(), true);
            let mut guard = ping_guard(true);
            guard["pointer"] = json!(pointer);
            let g = conditional_grant(
                json!([
                    {"id":"sample","capability":"system.ping","action":"run","params":{}},
                    {"id":"write","capability":"filesystem","action":"write","params":{"path":target,"content":"fixed authorized effect"}}
                ]),
                json!({"write":guard}),
                1024,
            );
            let ledger =
                GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner")
                    .unwrap();
            let report = AcceptedGrant::prepare(
                g.clone(),
                ledger.clone(),
                &engine,
                &g.issuer_id,
                &g.session_id,
                &g.executor_node,
            )
            .unwrap()
            .run(&engine)
            .await;
            assert!(report.persisted && report.confirms(&g));
            if pointer == "/pong" {
                assert_eq!(report.state, ReceiptState::Completed);
                assert!(report.success);
                assert_eq!(report.data["outcomes"][1]["condition_matched"], true);
                assert_eq!(
                    std::fs::read_to_string(&target).unwrap(),
                    "fixed authorized effect"
                );
            } else {
                assert_eq!(report.state, ReceiptState::Unknown);
                assert!(!report.success && !target.exists());
            }
            assert!(
                AcceptedGrant::prepare(
                    g.clone(),
                    ledger,
                    &engine,
                    &g.issuer_id,
                    &g.session_id,
                    &g.executor_node
                )
                .is_err()
            );
        }
    }
    #[tokio::test]
    async fn conditional_false_cannot_hide_missing_local_permission() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("denied.txt");
        let engine = engine(dir.path(), false);
        let g = conditional_grant(
            json!([
                {"id":"sample","capability":"system.ping","action":"run","params":{}},
                {"id":"write","capability":"filesystem","action":"write","params":{"path":target,"content":"denied even if skipped"}}
            ]),
            json!({"write":ping_guard(false)}),
            1024,
        );
        let ledger =
            GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner")
                .unwrap();
        assert!(
            AcceptedGrant::prepare(
                g.clone(),
                ledger.clone(),
                &engine,
                &g.issuer_id,
                &g.session_id,
                &g.executor_node
            )
            .is_err()
        );
        assert!(!target.exists());
        assert!(ledger.receipt(&g.issuer_id, &g.grant_id).is_err());
    }
    #[tokio::test]
    async fn conditional_guard_data_is_released_before_unrelated_large_stage() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("data.txt");
        let target = dir.path().join("copy.txt");
        let overhead = serde_json::to_vec(&json!({"path":source,"content":""}))
            .unwrap()
            .len();
        let payload = "x".repeat(1024 - overhead - 32);
        std::fs::write(&source, &payload).unwrap();
        let engine = engine(dir.path(), true);
        let g = conditional_grant(
            json!([
                {"id":"sample","capability":"system.ping","action":"run","params":{}},
                {"id":"skip","capability":"system.ping","action":"run","params":{}},
                {"id":"read","capability":"filesystem","action":"read","params":{"path":source}},
                {"id":"write","capability":"filesystem","action":"write","params":{"path":target},"inputs":[{"parameter":"content","source_step":"read","pointer":"/content"}]}
            ]),
            json!({"skip":ping_guard(false)}),
            1024,
        );
        let ledger =
            GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner")
                .unwrap();
        let report = AcceptedGrant::prepare(
            g.clone(),
            ledger,
            &engine,
            &g.issuer_id,
            &g.session_id,
            &g.executor_node,
        )
        .unwrap()
        .run(&engine)
        .await;
        assert!(report.success && report.confirms(&g));
        assert_eq!(std::fs::read_to_string(target).unwrap(), payload);
    }
    #[tokio::test]
    async fn conditional_native_failure_stops_even_unconditional_later_work() {
        let dir = tempfile::tempdir().unwrap();
        let executable = std::env::current_exe().unwrap();
        let target = dir.path().join("never.txt");
        let policy=serde_json::from_value(json!({"filesystem":{"read":true,"write":true,"roots":[dir.path()]},"process":{"enabled":true,"allowed_executables":[executable],"max_seconds":3,"max_output_bytes":1024}})).unwrap();
        let engine = engine(dir.path(), true).with_policy(policy).unwrap();
        let g = conditional_grant(
            json!([
                {"id":"sample","capability":"system.ping","action":"run","params":{}},
                {"id":"native","capability":"process.exec","action":"run","params":{"program":executable,"args":["--exact","pipeline_runtime::tests::unsuccessful_exit_helper","--skip","eef-pipeline-exit-17","--test-threads=1"],"timeout_seconds":2}},
                {"id":"write","capability":"filesystem","action":"write","params":{"path":target,"content":"never after known failure"}}
            ]),
            json!({"native":ping_guard(true)}),
            2048,
        );
        let ledger =
            GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner")
                .unwrap();
        let report = AcceptedGrant::prepare(
            g.clone(),
            ledger,
            &engine,
            &g.issuer_id,
            &g.session_id,
            &g.executor_node,
        )
        .unwrap()
        .run(&engine)
        .await;
        assert_eq!(report.state, ReceiptState::Completed);
        assert!(!report.success && report.confirms(&g));
        assert_eq!(
            report.error_code.as_deref(),
            Some("pipeline_stage_unsuccessful")
        );
        assert_eq!(report.data["outcomes"].as_array().unwrap().len(), 2);
        assert_eq!(report.data["outcomes"][1]["state"], "failed");
        assert_eq!(report.data["output"]["exit_code"], 17);
        assert!(!target.exists());
    }
    #[tokio::test]
    async fn all_stage_observations_do_not_replace_a_missing_final_parent_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipts.db");
        let effect = dir.path().join("effect.txt");
        let engine = engine(dir.path(), true);
        let g = grant(
            json!([
                {"id":"write","capability":"filesystem","action":"write","params":{"path":effect,"content":"finished stage"}}
            ]),
            2048,
        );
        let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        let accepted = AcceptedGrant::prepare(
            g.clone(),
            ledger.clone(),
            &engine,
            &g.issuer_id,
            &g.session_id,
            &g.executor_node,
        )
        .unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("CREATE TRIGGER fail_final BEFORE UPDATE ON execution_receipts WHEN NEW.state!='accepted' BEGIN SELECT RAISE(ABORT,'fixture final checkpoint failure'); END;").unwrap();
        let report = accepted.run(&engine).await;
        assert_eq!(report.state, ReceiptState::Unknown);
        assert!(!report.persisted);
        assert_eq!(
            report.error_code.as_deref(),
            Some("completion_checkpoint_failed")
        );
        assert_eq!(std::fs::read_to_string(&effect).unwrap(), "finished stage");
        let retained = ledger.receipt(&g.issuer_id, &g.grant_id).unwrap();
        assert_eq!(retained["state"], "accepted");
        assert_eq!(retained["pipeline_progress"]["phase"], "completed");
        assert_eq!(retained["pipeline_progress"]["completed_steps"], 1);
        assert_eq!(
            retained["pipeline_progress"]["termination_confirmed"],
            false
        );
        db.execute_batch("DROP TRIGGER fail_final").unwrap();
        drop(db);
        drop(ledger);
        let reopened = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        assert_eq!(
            reopened.receipt(&g.issuer_id, &g.grant_id).unwrap(),
            retained
        );
        assert!(
            AcceptedGrant::prepare(
                g.clone(),
                reopened,
                &engine,
                &g.issuer_id,
                &g.session_id,
                &g.executor_node
            )
            .is_err()
        );
    }
    #[tokio::test]
    async fn failed_progress_write_never_starts_or_replays_the_next_effect() {
        for fail_after_first_effect in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("receipts.db");
            let first = dir.path().join("first.txt");
            let later = dir.path().join("later.txt");
            let engine = engine(dir.path(), true);
            let g = grant(
                json!([
                    {"id":"first","capability":"filesystem","action":"write","params":{"path":first,"content":"private first effect"}},
                    {"id":"later","capability":"filesystem","action":"write","params":{"path":later,"content":"must not execute"}}
                ]),
                2048,
            );
            let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
            let accepted = AcceptedGrant::prepare(
                g.clone(),
                ledger.clone(),
                &engine,
                &g.issuer_id,
                &g.session_id,
                &g.executor_node,
            )
            .unwrap();
            let db = rusqlite::Connection::open(&path).unwrap();
            let when = if fail_after_first_effect {
                "WHEN NEW.payload LIKE '%\"phase\":\"completed\"%'"
            } else {
                ""
            };
            db.execute_batch(&format!("CREATE TRIGGER fail_progress BEFORE UPDATE ON execution_pipeline_progress {when} BEGIN SELECT RAISE(ABORT,'fixture checkpoint write failure'); END;")).unwrap();
            let report = accepted.run(&engine).await;
            assert_eq!(report.state, ReceiptState::Unknown);
            assert_eq!(first.exists(), fail_after_first_effect);
            assert!(!later.exists());
            let retained = ledger.receipt(&g.issuer_id, &g.grant_id).unwrap();
            assert_eq!(retained["state"], "unknown");
            assert_eq!(
                retained["pipeline_progress"]["sequence"],
                if fail_after_first_effect { 1 } else { 0 }
            );
            assert_eq!(
                retained["pipeline_progress"]["termination_confirmed"],
                false
            );
            assert!(
                !serde_json::to_string(&retained)
                    .unwrap()
                    .contains("private first effect")
            );
            db.execute_batch("DROP TRIGGER fail_progress").unwrap();
            drop(db);
            drop(ledger);
            let reopened = GrantLedger::open(&path, Default::default(), "owner").unwrap();
            assert_eq!(
                reopened.receipt(&g.issuer_id, &g.grant_id).unwrap(),
                retained
            );
            assert!(
                AcceptedGrant::prepare(
                    g.clone(),
                    reopened,
                    &engine,
                    &g.issuer_id,
                    &g.session_id,
                    &g.executor_node
                )
                .is_err()
            );
            assert!(!later.exists());
        }
    }
    #[tokio::test]
    async fn one_consumed_parent_executes_local_dataflow_and_never_replays() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("source.txt"), "owned synthetic input").unwrap();
        let engine = engine(dir.path(), true);
        let g = grant(copy_steps(dir.path()), 2048);
        let ledger =
            GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner")
                .unwrap();
        let accepted = AcceptedGrant::prepare(
            g.clone(),
            ledger.clone(),
            &engine,
            &g.issuer_id,
            &g.session_id,
            &g.executor_node,
        )
        .unwrap();
        assert!(
            !dir.path().join("copy.txt").exists(),
            "admission/ack starts no stages"
        );
        assert!(accepted.acknowledgement().confirms(&g));
        let report = accepted.run(&engine).await;
        assert_eq!(report.state, ReceiptState::Completed);
        assert!(report.success);
        assert!(report.confirms(&g));
        assert_eq!(report.data["completed_steps"], json!(["read", "write"]));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("copy.txt")).unwrap(),
            "owned synthetic input"
        );
        assert!(
            report.data.get("source").is_none(),
            "intermediate input is not coordinator output"
        );
        assert!(
            AcceptedGrant::prepare(
                g.clone(),
                ledger.clone(),
                &engine,
                &g.issuer_id,
                &g.session_id,
                &g.executor_node
            )
            .is_err()
        );
        let retained = ledger
            .result(
                &g.issuer_id,
                &g.grant_id,
                &crate::grants::definition_digest(&json!(g)).unwrap(),
            )
            .unwrap();
        assert_eq!(retained["report"], json!(report));
    }
    #[tokio::test]
    async fn every_child_permission_is_checked_before_consumption_or_effects() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("source.txt"), "fixture").unwrap();
        let engine = engine(dir.path(), false);
        let g = grant(copy_steps(dir.path()), 2048);
        let ledger =
            GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner")
                .unwrap();
        assert!(
            AcceptedGrant::prepare(
                g.clone(),
                ledger,
                &engine,
                &g.issuer_id,
                &g.session_id,
                &g.executor_node
            )
            .is_err()
        );
        assert!(!dir.path().join("copy.txt").exists());
    }
    #[tokio::test]
    async fn file_to_selected_model_to_file_is_local_and_uses_the_exact_backend() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let payload = loop {
                let mut buffer = [0; 4096];
                let n = socket.read(&mut buffer).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
                assert!(bytes.len() < 16384);
                if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..end]);
                    assert!(header.starts_with("POST /api/chat "));
                    let length = header
                        .lines()
                        .find_map(|line| {
                            line.to_lowercase()
                                .strip_prefix("content-length: ")
                                .map(|v| v.parse::<usize>().unwrap())
                        })
                        .unwrap();
                    if bytes.len() >= end + 4 + length {
                        break serde_json::from_slice::<Value>(&bytes[end + 4..end + 4 + length])
                            .unwrap();
                    }
                }
            };
            assert_eq!(payload["model"], "selected");
            assert_eq!(payload["messages"][0]["content"], "local synthetic prompt");
            let body=serde_json::to_string(&json!({"message":{"content":"local synthetic answer"},"done":true,"done_reason":"stop"})).unwrap();
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).as_bytes()).await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("source.txt"), "local synthetic prompt").unwrap();
        let engine = engine(dir.path(), true)
            .with_ollama(
                format!("http://{address}"),
                [
                    serde_json::from_value(json!({"model_id":"selected","modality":"text"}))
                        .unwrap(),
                ],
            )
            .unwrap();
        let g = grant(
            json!([
                {"id":"read","capability":"filesystem","action":"read","params":{"path":dir.path().join("source.txt")}},
                {"id":"infer","capability":"llm.infer","action":"run","model_id":"selected","backend":"ollama","params":{"max_tokens":16},
                    "inputs":[{"parameter":"prompt","source_step":"read","pointer":"/content"}]},
                {"id":"write","capability":"filesystem","action":"write","params":{"path":dir.path().join("answer.txt")},
                    "inputs":[{"parameter":"content","source_step":"infer","pointer":"/content"}]}
            ]),
            2048,
        );
        let ledger =
            GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner")
                .unwrap();
        let accepted = AcceptedGrant::prepare(
            g.clone(),
            ledger,
            &engine,
            &g.issuer_id,
            &g.session_id,
            &g.executor_node,
        )
        .unwrap();
        let report = accepted.run(&engine).await;
        assert_eq!(report.state, ReceiptState::Completed);
        assert!(report.success, "{report:?}");
        assert!(report.confirms(&g));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("answer.txt")).unwrap(),
            "local synthetic answer"
        );
        server.await.unwrap();
    }
    #[tokio::test]
    async fn missing_fields_and_over_budget_results_keep_parent_unknown_without_later_effects() {
        for oversized in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(
                dir.path().join("source.txt"),
                if oversized {
                    "x".repeat(1500)
                } else {
                    "fixture".into()
                },
            )
            .unwrap();
            let engine = engine(dir.path(), true);
            let mut steps = copy_steps(dir.path());
            if !oversized {
                steps[1]["inputs"][0]["pointer"] = json!("/missing")
            }
            let g = grant(steps, 1024);
            let ledger =
                GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner")
                    .unwrap();
            let accepted = AcceptedGrant::prepare(
                g.clone(),
                ledger.clone(),
                &engine,
                &g.issuer_id,
                &g.session_id,
                &g.executor_node,
            )
            .unwrap();
            let report = accepted.run(&engine).await;
            assert_eq!(report.state, ReceiptState::Unknown);
            assert!(!report.success);
            assert!(report.confirms(&g));
            assert!(!dir.path().join("copy.txt").exists());
            assert!(
                AcceptedGrant::prepare(
                    g.clone(),
                    ledger,
                    &engine,
                    &g.issuer_id,
                    &g.session_id,
                    &g.executor_node
                )
                .is_err()
            );
        }
    }
    #[tokio::test]
    async fn consumed_references_release_buffers_but_simultaneously_live_outputs_share_one_budget()
    {
        for simultaneous in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            for name in ["a.txt", "b.txt"] {
                std::fs::write(dir.path().join(name), "x".repeat(600)).unwrap();
            }
            let mut steps = json!([
                {"id":"read-a","capability":"filesystem","action":"read","params":{"path":dir.path().join("a.txt")}},
                {"id":"write-a","capability":"filesystem","action":"write","params":{"path":dir.path().join("copy-a.txt")},"inputs":[{"parameter":"content","source_step":"read-a","pointer":"/content"}]},
                {"id":"read-b","capability":"filesystem","action":"read","params":{"path":dir.path().join("b.txt")}},
                {"id":"write-b","capability":"filesystem","action":"write","params":{"path":dir.path().join("copy-b.txt")},"inputs":[{"parameter":"content","source_step":"read-b","pointer":"/content"}]}
            ]);
            if simultaneous {
                steps = json!([steps[0],steps[2],{"id":"combine","capability":"vlm.analyze","action":"run","model_id":"selected","backend":"ollama","params":{},
                    "inputs":[{"parameter":"prompt","source_step":"read-a","pointer":"/content"},{"parameter":"images","source_step":"read-b","pointer":"/content","wrap_array":true}]}]);
            }
            let engine = engine(dir.path(), true)
                .with_ollama(
                    "http://127.0.0.1:1".into(),
                    [
                        serde_json::from_value(json!({"model_id":"selected","modality":"vlm"}))
                            .unwrap(),
                    ],
                )
                .unwrap();
            let g = grant(steps, 1024);
            let ledger =
                GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner")
                    .unwrap();
            let accepted = AcceptedGrant::prepare(
                g.clone(),
                ledger,
                &engine,
                &g.issuer_id,
                &g.session_id,
                &g.executor_node,
            )
            .unwrap();
            let report = accepted.run(&engine).await;
            if simultaneous {
                assert_eq!(report.state, ReceiptState::Unknown);
                assert!(!dir.path().join("copy-a.txt").exists());
            } else {
                assert_eq!(report.state, ReceiptState::Completed);
                assert!(report.success);
                assert_eq!(
                    std::fs::read_to_string(dir.path().join("copy-b.txt"))
                        .unwrap()
                        .len(),
                    600
                );
            }
            assert!(report.confirms(&g));
        }
    }
    #[test]
    fn unsuccessful_exit_helper() {
        if std::env::args().any(|arg| arg == "eef-pipeline-exit-17") {
            std::process::exit(17)
        }
    }
    #[tokio::test]
    async fn known_native_unsuccessful_exit_stops_the_graph_without_false_success() {
        let dir = tempfile::tempdir().unwrap();
        let executable = std::env::current_exe().unwrap();
        let policy=serde_json::from_value(json!({"filesystem":{"read":true,"write":true,"roots":[dir.path()]},
            "process":{"enabled":true,"allowed_executables":[executable],"max_seconds":3,"max_output_bytes":1024}})).unwrap();
        let engine = engine(dir.path(), true).with_policy(policy).unwrap();
        let g = grant(
            json!([
                {"id":"native","capability":"process.exec","action":"run","params":{"program":executable,
                    "args":["--exact","pipeline_runtime::tests::unsuccessful_exit_helper","--skip","eef-pipeline-exit-17","--test-threads=1"],"timeout_seconds":2}},
                {"id":"must-not-write","capability":"filesystem","action":"write","params":{"path":dir.path().join("must-not-exist.txt"),"content":"not authorized after failure"}}
            ]),
            2048,
        );
        let ledger =
            GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner")
                .unwrap();
        let accepted = AcceptedGrant::prepare(
            g.clone(),
            ledger,
            &engine,
            &g.issuer_id,
            &g.session_id,
            &g.executor_node,
        )
        .unwrap();
        let report = accepted.run(&engine).await;
        assert_eq!(report.state, ReceiptState::Completed);
        assert!(!report.success);
        assert_eq!(
            report.error_code.as_deref(),
            Some("pipeline_stage_unsuccessful")
        );
        assert_eq!(report.data["failed_step"], "native");
        assert_eq!(report.data["output"]["exit_code"], 17);
        assert!(report.confirms(&g));
        assert!(!dir.path().join("must-not-exist.txt").exists());
        let mut false_success = report;
        false_success.success = true;
        false_success.error_code = None;
        assert!(!false_success.confirms(&g));
    }
    #[tokio::test]
    async fn waiting_model_stage_consumes_the_original_parent_deadline_without_later_work() {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let held = release.clone();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 4096];
            assert!(socket.read(&mut buffer).await.unwrap() > 0);
            held.notified().await;
        });
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("source.txt"), "fixture").unwrap();
        let engine = engine(dir.path(), true)
            .with_ollama(
                format!("http://{address}"),
                [
                    serde_json::from_value(json!({"model_id":"selected","modality":"text"}))
                        .unwrap(),
                ],
            )
            .unwrap();
        let mut g = grant(
            json!([
                {"id":"infer","capability":"llm.infer","action":"run","model_id":"selected","backend":"ollama","params":{"prompt":"wait without renewal"}},
                {"id":"write","capability":"filesystem","action":"write","params":{"path":dir.path().join("must-not-exist.txt"),"content":"not after deadline"}}
            ]),
            2048,
        );
        g.limits.max_execution_ms = 1000;
        let ledger =
            GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner")
                .unwrap();
        let accepted = AcceptedGrant::prepare(
            g.clone(),
            ledger.clone(),
            &engine,
            &g.issuer_id,
            &g.session_id,
            &g.executor_node,
        )
        .unwrap();
        let report = accepted.run(&engine).await;
        release.notify_one();
        server.await.unwrap();
        assert_eq!(report.state, ReceiptState::Unknown);
        assert!(report.confirms(&g));
        assert!(!dir.path().join("must-not-exist.txt").exists());
        assert!(
            AcceptedGrant::prepare(
                g.clone(),
                ledger,
                &engine,
                &g.issuer_id,
                &g.session_id,
                &g.executor_node
            )
            .is_err()
        );
    }
    #[test]
    fn wrong_parent_bindings_budget_and_false_success_are_rejected() {
        let steps =
            json!([{ "id":"inspect","capability":"system.info","action":"run","params":{}}]);
        let g = grant(steps, 1024);
        g.validate().unwrap();
        for key in ["resource_id", "model_id", "backend", "model_role"] {
            let mut raw = json!(g);
            raw[key] = json!("substituted");
            assert!(
                serde_json::from_value::<ExecutionGrant>(raw)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        let mut wrong = g.clone();
        wrong.params["pipeline"]["max_retained_bytes"] = json!(wrong.limits.max_result_bytes + 1);
        assert!(wrong.validate().is_err());
        let definition = FinitePipeline::from_grant(&g).unwrap();
        let mut data = result(&definition, 1, false, json!({}));
        assert!(confirmed_result(&g, &data).unwrap());
        data["completed_steps"] = json!([]);
        assert!(confirmed_result(&g, &data).is_err());
        data["completed_steps"] = json!(["other"]);
        assert!(confirmed_result(&g, &data).is_err());
    }
    #[tokio::test]
    async fn revoking_a_later_stage_permission_stops_a_waiting_stage_before_the_lease() {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let entered = std::sync::Arc::new(tokio::sync::Notify::new());
        let started = entered.clone();
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let held = release.clone();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 4096];
            assert!(socket.read(&mut buffer).await.unwrap() > 0);
            started.notify_one();
            held.notified().await;
        });
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("node.json");
        crate::setup::write_json(&config,&json!({"node_id":"owner","permissions":{"filesystem":{"read":true,"write":true,"roots":[dir.path()]}},
            "models":{"provider":"ollama","ollama":{"selected":[{"model_id":"selected","modality":"text"}]}}})).unwrap();
        let service = crate::NodeService::new(config, "owner".into());
        let engine = engine(dir.path(), true)
            .with_service(service.clone())
            .with_ollama(
                format!("http://{address}"),
                [
                    serde_json::from_value(json!({"model_id":"selected","modality":"text"}))
                        .unwrap(),
                ],
            )
            .unwrap();
        let g = grant(
            json!([
                {"id":"infer","capability":"llm.infer","action":"run","model_id":"selected","backend":"ollama","params":{"prompt":"waiting stage"}},
                {"id":"write","capability":"filesystem","action":"write","params":{"path":dir.path().join("not-after-revocation.txt"),"content":"must not write"}}
            ]),
            2048,
        );
        let ledger =
            GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner")
                .unwrap();
        let accepted = AcceptedGrant::prepare(
            g.clone(),
            ledger,
            &engine,
            &g.issuer_id,
            &g.session_id,
            &g.executor_node,
        )
        .unwrap();
        let (report, _) = tokio::join!(accepted.run(&engine), async {
            tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
                .await
                .unwrap();
            let mut config = service.read_config().unwrap();
            config["permissions"]["filesystem"]["write"] = json!(false);
            service.save_config(config).unwrap();
        });
        release.notify_one();
        server.await.unwrap();
        assert_eq!(report.state, ReceiptState::Unknown);
        assert_eq!(report.error_code.as_deref(), Some("local_policy_changed"));
        assert!(report.confirms(&g));
        assert!(!dir.path().join("not-after-revocation.txt").exists());
    }
    #[tokio::test]
    async fn generic_execute_and_ordinary_rpc_cannot_invoke_compound_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let engine = engine(dir.path(), true);
        assert!(engine.capabilities().contains(&"pipeline.run".into()));
        assert!(
            engine
                .execute("pipeline.run", "run", json!({}))
                .await
                .unwrap_err()
                .is::<crate::execution_boundary::ExecutionGrantRequired>()
        );
        assert!(crate::execution_boundary::require_control("pipeline.run").is_err());
    }
}
