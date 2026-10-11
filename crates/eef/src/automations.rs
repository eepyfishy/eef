//! Source-bound definitions in the existing single-coordinator job database.
//! No automatic approval, timers, grants, queues or execution runtime.
use super::*;
use eefn::automation::{AutomationCommand, AutomationReport, Definition, RemovalReceipt};

fn validate_definition(definition: &Definition) -> Result<()> {
    definition.validate()?;
    if let Some(source) = &definition.source {
        let plan: Plan = serde_json::from_value(source.clone())?;
        validate_retained_review(&plan)?;
        if plan.planning.is_none()
            || plan.status != PlanStatus::AwaitingApproval
            || plan.approval.is_some()
            || plan.execution.is_some()
            || serde_json::to_value(plan)? != *source
        {
            bail!("Invalid retained automation source")
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{event::EventBus, task::TaskEngine};
    use serde_json::{Value, json};
    fn define(plan: &Plan) -> AutomationCommand {
        AutomationCommand::Define {
            operation_id: uuid::Uuid::new_v4().to_string(),
            from_job: plan.plan_id.clone(),
            revision: plan.revision,
            plan_sha256: plan.planning.as_ref().unwrap().plan_sha256.clone(),
            name: "Status observation".into(),
            every_ms: 1000,
            max_runs: 3,
            valid_for_ms: 10000,
        }
    }
    fn fixture(store: Arc<JobStore>) -> (Arc<TaskEngine>, Plan, AutomationCommand) {
        let engine = TaskEngine::with_store(EventBus::new(20), store);
        let plan = crate::request_approval::tests::compiled(&engine);
        let command = define(&plan);
        (engine, plan, command)
    }
    fn source_owner(plan: &Plan) -> &str {
        plan.goal.request_context.as_ref().unwrap().origin_node()
    }
    fn record(value: &Value) -> Definition {
        serde_json::from_value(value["definition"].clone()).unwrap()
    }

    #[test]
    fn exact_disabled_source_is_durable_and_receipt_reuse_never_reinterprets_it() {
        let store = JobStore::in_memory().unwrap();
        let (_engine, plan, command) = fixture(store.clone());
        let saved = store
            .automation_command(command.clone(), Some(source_owner(&plan)))
            .unwrap();
        assert!(eefn::automation::confirmed(
            &saved,
            &command,
            Some(source_owner(&plan))
        ));
        let definition = record(&saved);
        assert_eq!(
            definition.source,
            Some(serde_json::to_value(&plan).unwrap())
        );
        assert_eq!(definition.state, "disabled");
        assert!(!saved["timers_started"].as_bool().unwrap());
        let usage = store.usage().unwrap();
        assert_eq!(usage["count"], 1);
        assert_eq!(usage["automation_count"], 1);
        assert!(usage["automation_bytes"].as_u64().unwrap() > 0);
        // Concurrent source lifecycle changes do not rewrite historical definitions.
        store
            .change(&plan.plan_id, "Explicit source cancellation", |p| {
                p.status = PlanStatus::Cancelled;
                for t in &mut p.tasks {
                    t.status = TaskStatus::Cancelled
                }
                Ok(())
            })
            .unwrap();
        let repeated = store
            .automation_command(command.clone(), Some(source_owner(&plan)))
            .unwrap();
        assert_eq!(repeated["reused"], true);
        assert_eq!(repeated["definition"], saved["definition"]);
        assert_eq!(store.list().unwrap().len(), 1);
        assert_eq!(
            store.get(&plan.plan_id).unwrap().status,
            PlanStatus::Cancelled
        );
    }
    #[test]
    fn other_origins_cannot_create_inspect_remove_or_take_over_definitions() {
        let store = JobStore::in_memory().unwrap();
        let (_engine, plan, command) = fixture(store.clone());
        assert!(
            store
                .automation_command(command.clone(), Some("other-node"))
                .is_err()
        );
        let saved = store
            .automation_command(command.clone(), Some(source_owner(&plan)))
            .unwrap();
        let id = record(&saved).id;
        assert!(
            store
                .automation_command(command, Some("other-node"))
                .is_err()
        );
        assert!(
            store
                .automation_command(
                    AutomationCommand::Show { id: id.clone() },
                    Some("other-node")
                )
                .is_err()
        );
        assert!(
            store
                .automation_command(
                    AutomationCommand::Remove {
                        id,
                        revision: 1,
                        operation_id: uuid::Uuid::new_v4().to_string()
                    },
                    Some("other-node")
                )
                .is_err()
        );
        let list = AutomationCommand::List {
            limit: 1,
            after: None,
        };
        let page = store
            .automation_command(list.clone(), Some("other-node"))
            .unwrap();
        assert!(eefn::automation::confirmed(
            &page,
            &list,
            Some("other-node")
        ));
        assert_eq!(page["items"], json!([]));
        assert_eq!(
            store.automation_command(list, None).unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn removal_is_compact_deduplicated_and_old_creation_cannot_resurrect_it() {
        let store = JobStore::in_memory().unwrap();
        let (_engine, _plan, command) = fixture(store.clone());
        let saved = store.automation_command(command.clone(), None).unwrap();
        let before = store.usage().unwrap();
        let remove = AutomationCommand::Remove {
            id: record(&saved).id,
            revision: 1,
            operation_id: uuid::Uuid::new_v4().to_string(),
        };
        let removed = store.automation_command(remove.clone(), None).unwrap();
        assert!(eefn::automation::confirmed(&removed, &remove, None));
        assert_eq!(removed["definition"]["source"], Value::Null);
        assert_eq!(removed["definition"]["revision"], 2);
        assert!(
            store.usage().unwrap()["automation_bytes"].as_u64()
                < before["automation_bytes"].as_u64()
        );
        assert_eq!(
            store.automation_command(remove, None).unwrap()["reused"],
            true
        );
        let reused = store.automation_command(command.clone(), None).unwrap();
        assert!(eefn::automation::confirmed(&reused, &command, None));
        assert_eq!(reused["definition"]["state"], "removed");
        let mut conflicting = command;
        if let AutomationCommand::Define { name, .. } = &mut conflicting {
            *name = "Different".into()
        }
        assert!(store.automation_command(conflicting, None).is_err());
        let conflict = AutomationCommand::Remove {
            id: record(&saved).id,
            revision: 1,
            operation_id: uuid::Uuid::new_v4().to_string(),
        };
        assert!(store.automation_command(conflict, None).is_err());
        assert_eq!(store.usage().unwrap()["automation_count"], 1);
    }
    #[test]
    fn source_revision_and_digest_changes_fail_without_mutation() {
        let store = JobStore::in_memory().unwrap();
        let (_engine, plan, command) = fixture(store.clone());
        let before = store.usage().unwrap();
        for hash in [false, true] {
            let mut wrong = command.clone();
            if let AutomationCommand::Define {
                revision,
                plan_sha256,
                ..
            } = &mut wrong
            {
                if hash {
                    *plan_sha256 = "0".repeat(64)
                } else {
                    *revision += 1
                }
            }
            assert!(store.automation_command(wrong, None).is_err());
            assert_eq!(store.usage().unwrap(), before);
        }
        store
            .change(&plan.plan_id, "Source cancelled", |p| {
                p.status = PlanStatus::Cancelled;
                for t in &mut p.tasks {
                    t.status = TaskStatus::Cancelled
                }
                Ok(())
            })
            .unwrap();
        assert!(store.automation_command(command, None).is_err());
    }
    #[test]
    fn job_and_definition_count_limits_are_shared_in_both_directions() {
        let store =
            JobStore::initialize(Connection::open_in_memory().unwrap(), None, 2, 1024 * 1024)
                .unwrap();
        let (_engine, plan, command) = fixture(store.clone());
        store.automation_command(command, None).unwrap();
        assert!(store.automation_command(define(&plan), None).is_err());
        // Use an ordinary valid pending plan rather than altering a compiled digest.
        let extra = crate::task::TaskPlanner
            .plan(crate::task::Goal {
                request_context: None,
                description: "System status".into(),
                template: "system_info".into(),
                params: json!({}),
                constraints: json!({}),
            })
            .unwrap();
        assert!(
            store
                .insert(extra)
                .unwrap_err()
                .to_string()
                .contains("history is full")
        );
    }
    #[test]
    fn restart_retains_disabled_definition_and_lowered_quota_allows_compaction() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        Connection::open(&path).unwrap();
        let store = JobStore::open(&path, 20, 1024 * 1024).unwrap();
        let (engine, plan, command) = fixture(store.clone());
        let saved = store.automation_command(command.clone(), None).unwrap();
        drop(engine);
        drop(store);
        let store = JobStore::open(&path, 1, 1024).unwrap();
        assert_eq!(
            store.get(&plan.plan_id).unwrap().status,
            PlanStatus::AwaitingApproval
        );
        assert_eq!(store.usage().unwrap()["over_limit"], true);
        let show = AutomationCommand::Show {
            id: record(&saved).id.clone(),
        };
        assert_eq!(
            store.automation_command(show, None).unwrap()["definition"],
            saved["definition"]
        );
        let remove = AutomationCommand::Remove {
            id: record(&saved).id,
            revision: 1,
            operation_id: uuid::Uuid::new_v4().to_string(),
        };
        assert!(store.automation_command(remove, None).is_ok());
        drop(store);
        let reopened = JobStore::open(&path, 1, 1024).unwrap();
        assert_eq!(
            reopened.automation_command(command, None).unwrap()["definition"]["state"],
            "removed"
        );
    }
    #[test]
    fn future_or_corrupt_snapshots_are_retained_and_refuse_restart() {
        for corruption in ["future", "source", "digest", "unknown"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("jobs.db");
            Connection::open(&path).unwrap();
            let store = JobStore::open(&path, 20, 1024 * 1024).unwrap();
            let (engine, _plan, command) = fixture(store.clone());
            let saved = store.automation_command(command, None).unwrap();
            let mut value = saved["definition"].clone();
            match corruption {
                "future" => value["schema_version"] = json!(2),
                "source" => value["source"]["tasks"][0]["action"] = json!("different"),
                "digest" => value["definition_sha256"] = json!("0".repeat(64)),
                _ => value["future_authority"] = json!(true),
            }
            let raw = serde_json::to_string(&value).unwrap();
            store
                .connection
                .lock()
                .unwrap()
                .execute(
                    "UPDATE automation_definitions SET record_json=?2 WHERE definition_id=?1",
                    params![value["id"].as_str().unwrap(), raw],
                )
                .unwrap();
            assert!(
                store
                    .automation_command(
                        AutomationCommand::List {
                            limit: 25,
                            after: None
                        },
                        None
                    )
                    .is_err()
            );
            drop(engine);
            drop(store);
            assert!(JobStore::open(&path, 20, 1024 * 1024).is_err());
            let db = Connection::open(&path).unwrap();
            let retained: String = db
                .query_row("SELECT record_json FROM automation_definitions", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(retained, raw);
        }
    }
    #[test]
    fn frozen_source_must_pass_compiler_validation_even_with_recomputed_record_hashes() {
        let store = JobStore::in_memory().unwrap();
        let (_engine, _plan, command) = fixture(store.clone());
        let mut definition = record(&store.automation_command(command, None).unwrap());
        definition.source.as_mut().unwrap()["tasks"][0]["action"] = json!("different");
        definition.source_sha256 =
            eefn::grants::definition_digest(definition.source.as_ref().unwrap()).unwrap();
        definition.definition_sha256 = definition.digest().unwrap();
        assert!(definition.validate().is_ok());
        assert!(validate_definition(&definition).is_err());
    }

    #[test]
    fn shared_bytes_refuse_new_definitions_and_jobs_while_removal_releases_payload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        Connection::open(&path).unwrap();
        let store = JobStore::open(&path, 20, 1024 * 1024).unwrap();
        let (engine, plan, command) = fixture(store.clone());
        let saved = store.automation_command(command, None).unwrap();
        let usage = store.usage().unwrap();
        let used = usage["payload_bytes"].as_u64().unwrap()
            + usage["reserved_bytes"].as_u64().unwrap()
            + usage["automation_bytes"].as_u64().unwrap();
        drop(engine);
        drop(store);
        let store = JobStore::open(&path, 20, (used - 1) as usize).unwrap();
        let before = store.usage().unwrap();
        assert!(
            store
                .automation_command(define(&plan), None)
                .unwrap_err()
                .to_string()
                .contains("storage is full")
        );
        let pending = crate::task::TaskPlanner
            .plan(crate::task::Goal {
                request_context: None,
                description: "Status".into(),
                template: "system_info".into(),
                params: json!({}),
                constraints: json!({}),
            })
            .unwrap();
        assert!(
            store
                .insert(pending)
                .unwrap_err()
                .to_string()
                .contains("storage is full")
        );
        assert_eq!(store.usage().unwrap(), before);
        let remove = AutomationCommand::Remove {
            id: record(&saved).id,
            revision: 1,
            operation_id: uuid::Uuid::new_v4().to_string(),
        };
        store.automation_command(remove, None).unwrap();
        assert_eq!(store.usage().unwrap()["over_limit"], false);
    }
}
pub(super) fn read_definition(connection: &Connection, id: &str) -> Result<Definition> {
    let raw: String = connection
        .query_row(
            "SELECT record_json FROM automation_definitions WHERE definition_id=?1",
            [id],
            |r| r.get(0),
        )
        .context("Automation definition unavailable")?;
    let record: Definition =
        serde_json::from_str(&raw).context("Damaged automation record; retained")?;
    validate_definition(&record)?;
    if record.id != id {
        bail!("Automation journal identity mismatch")
    }
    Ok(record)
}
pub(super) fn count(connection: &Connection) -> Result<usize> {
    let n: i64 = connection.query_row(
        "SELECT (SELECT count(*) FROM durable_jobs)+(SELECT count(*) FROM automation_definitions)+(SELECT count(*) FROM automation_authorizations)+(SELECT count(*) FROM automation_occurrences)+(SELECT count(*) FROM automation_occurrence_archives)",
        [],
        |r| r.get(0),
    )?;
    Ok(usize::try_from(n)?)
}
pub(super) fn definition_bytes(connection: &Connection) -> Result<usize> {
    let n: i64 = connection.query_row(
        "SELECT (SELECT COALESCE(SUM(length(CAST(record_json AS BLOB))),0) FROM automation_definitions)+(SELECT COALESCE(SUM(length(CAST(record_json AS BLOB))+reserved_bytes),0) FROM automation_authorizations)+(SELECT COALESCE(SUM(length(CAST(record_json AS BLOB))),0) FROM automation_occurrences)+(SELECT COALESCE(SUM(length(CAST(record_json AS BLOB))),0) FROM automation_occurrence_archives)",
        [],
        |r| r.get(0),
    )?;
    Ok(usize::try_from(n)?)
}
impl JobStore {
    pub(crate) fn validate_automations(&self) -> Result<()> {
        let connection = self.connection.lock().expect("job journal");
        let mut statement =
            connection.prepare("SELECT definition_id FROM automation_definitions")?;
        for id in statement.query_map([], |r| r.get::<_, String>(0))? {
            read_definition(&connection, &id?)?;
        }
        automation_authorizations::validate_all(&connection)?;
        automation_occurrences::validate_all(&connection)?;
        Ok(())
    }
    fn save_definition(
        &self,
        connection: &Connection,
        definition: &Definition,
        new: bool,
    ) -> Result<()> {
        validate_definition(definition)?;
        let encoded = serde_json::to_string(definition)?;
        let previous:Option<i64>=connection.query_row("SELECT length(CAST(record_json AS BLOB)) FROM automation_definitions WHERE definition_id=?1",[&definition.id],|r|r.get(0)).optional()?;
        if new && count(connection)? >= self.max_count {
            bail!("Job/automation history is full; increase jobs.max_count or remove finished jobs")
        }
        if !previous.is_some_and(|old| old >= 0 && encoded.len() as u64 <= old as u64) {
            let jobs:i64=connection.query_row("SELECT COALESCE(SUM(length(CAST(plan_json AS BLOB))+reserved_bytes),0) FROM durable_jobs",[],|r|r.get(0))?;
            let other:i64=connection.query_row("SELECT COALESCE(SUM(length(CAST(record_json AS BLOB))),0) FROM automation_definitions WHERE definition_id<>?1",[&definition.id],|r|r.get(0))?;
            let authorization_bytes = automation_authorizations::bytes(connection)?
                .checked_add(automation_occurrences::bytes(connection)?)
                .context("Storage overflow")?;
            let used = usize::try_from(jobs)?
                .checked_add(usize::try_from(other)?)
                .and_then(|bytes| bytes.checked_add(authorization_bytes))
                .context("Storage size overflow")?;
            if encoded.len() > 8 * 1024 * 1024
                || encoded.len() > self.max_bytes.saturating_sub(used)
            {
                bail!("Job/automation storage is full; definition was not saved")
            }
        }
        if new {
            connection.execute(
                "INSERT INTO automation_definitions(definition_id,record_json) VALUES (?1,?2)",
                params![definition.id, encoded],
            )?;
        } else {
            connection.execute(
                "UPDATE automation_definitions SET record_json=?2 WHERE definition_id=?1",
                params![definition.id, encoded],
            )?;
        }
        Ok(())
    }
    pub fn automation_command(
        &self,
        command: AutomationCommand,
        origin: Option<&str>,
    ) -> Result<serde_json::Value> {
        command.validate()?;
        if let Some(node) = origin {
            eefn::network::validate_remote_node_id(node)?;
        }
        let connection = self.connection.lock().expect("job journal");
        let mut report = AutomationReport {
            schema_version: 1,
            report_type: "automation_definition".into(),
            command: command.clone(),
            definition: None,
            items: vec![],
            next_after: None,
            reused: false,
            timers_started: false,
            execution_authorized: false,
            dispatched: false,
            automatically_retried: false,
        };
        let authorize = |definition: &Definition| -> Result<()> {
            if origin.is_some_and(|node| node != definition.owner_context.origin_node()) {
                bail!("Automation definition unavailable")
            }
            Ok(())
        };
        match &command {
            AutomationCommand::List { limit, after } => {
                // Validate all retained records, including filtered entries. Never
                // hide damaged/future records by presenting a successful subset.
                let mut statement = connection.prepare(
                    "SELECT definition_id FROM automation_definitions ORDER BY definition_id",
                )?;
                for id in statement.query_map([], |r| r.get::<_, String>(0))? {
                    let record = read_definition(&connection, &id?)?;
                    if origin.is_some_and(|node| node != record.owner_context.origin_node())
                        || after.as_ref().is_some_and(|a| record.id <= *a)
                    {
                        continue;
                    }
                    if report.items.len() == *limit as usize {
                        report.next_after = report
                            .items
                            .last()
                            .and_then(|v| v["id"].as_str())
                            .map(str::to_owned);
                        continue;
                    }
                    report.items.push(record.summary());
                }
            }
            AutomationCommand::Show { id } => {
                let record = read_definition(&connection, id)?;
                authorize(&record)?;
                report.definition = Some(record);
            }
            AutomationCommand::Define {
                operation_id,
                from_job,
                revision,
                plan_sha256,
                ..
            } => {
                let retained: bool = connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM automation_definitions WHERE definition_id=?1)",
                    [operation_id],
                    |r| r.get(0),
                )?;
                if retained {
                    let record = read_definition(&connection, operation_id)?;
                    authorize(&record)?;
                    if record.creation != command {
                        bail!(
                            "Automation operation receipt conflicts; inspect the retained definition"
                        )
                    }
                    // Removed receipts remain removed. Lost replies never license
                    // recreation, reinterpretation, a timer or execution.
                    report.reused = true;
                    report.definition = Some(record);
                } else {
                    let plan = read(&connection, from_job)?;
                    let context = plan
                        .goal
                        .request_context
                        .clone()
                        .context("Automation source requires immutable origin")?;
                    if origin.is_some_and(|node| node != context.origin_node()) {
                        bail!("Automation source unavailable")
                    }
                    if plan.revision != *revision
                        || plan.status != PlanStatus::AwaitingApproval
                        || plan.approval.is_some()
                        || plan.execution.is_some()
                        || plan
                            .planning
                            .as_ref()
                            .is_none_or(|p| p.plan_sha256 != *plan_sha256)
                    {
                        bail!(
                            "Automation source changed, approved or executed; inspect before defining"
                        )
                    }
                    let source = serde_json::to_value(plan)?;
                    let mut definition = Definition {
                        schema_version: 1,
                        id: operation_id.clone(),
                        owner_context: context,
                        created_at_ms: eefn::protocol::now_ms(),
                        creation: command.clone(),
                        source_sha256: eefn::grants::definition_digest(&source)?,
                        definition_sha256: String::new(),
                        revision: 1,
                        state: "disabled".into(),
                        source: Some(source),
                        removal: None,
                    };
                    definition.definition_sha256 = definition.digest()?;
                    self.save_definition(&connection, &definition, true)?;
                    report.definition = Some(definition);
                }
            }
            AutomationCommand::Remove {
                id,
                revision,
                operation_id,
            } => {
                let mut record = read_definition(&connection, id)?;
                authorize(&record)?;
                if let Some(receipt) = &record.removal {
                    if receipt.operation_id != *operation_id || receipt.source_revision != *revision
                    {
                        bail!("Removal receipt changed; inspect retained definition")
                    }
                    report.reused = true;
                } else {
                    if record.revision != *revision
                        || record.state != "disabled"
                        || operation_id == id
                    {
                        bail!("Definition state/revision changed")
                    }
                    record.source = None;
                    record.revision = 2;
                    record.state = "removed".into();
                    record.removal = Some(RemovalReceipt {
                        operation_id: operation_id.clone(),
                        source_revision: *revision,
                    });
                    self.save_definition(&connection, &record, false)?;
                }
                report.definition = Some(record);
            }
        }
        Ok(serde_json::to_value(report)?)
    }
}
