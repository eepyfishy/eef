//! Reusable owner-approval checkpoints under the existing journal lock/quotas.
use super::*;
use eefn::automation_policy::{Authorization, PolicyCommand, Revocation};
const REVOCATION_RESERVE: usize = 1024;
fn reserve(record: &Authorization) -> usize {
    if record.revocation.is_none() {
        REVOCATION_RESERVE
    } else {
        0
    }
}
pub(super) fn bytes(connection: &Connection) -> Result<usize> {
    let n:i64=connection.query_row("SELECT COALESCE(SUM(length(CAST(record_json AS BLOB))+reserved_bytes),0) FROM automation_authorizations",[],|r|r.get(0))?;
    Ok(usize::try_from(n)?)
}
pub(super) fn read(connection: &Connection, id: &str) -> Result<Option<Authorization>> {
    let row:Option<(String,i64)>=connection.query_row("SELECT record_json,reserved_bytes FROM automation_authorizations WHERE authorization_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    let Some((raw, reserved)) = row else {
        return Ok(None);
    };
    let record: Authorization = serde_json::from_str(&raw)
        .context("Damaged or future authorization; retained without activation")?;
    let PolicyCommand::Approve { definition, .. } = &record.creation else {
        bail!("Missing policy creation");
    };
    let definition = automations::read_definition(connection, definition)?;
    validate_read_record(&record, id, reserved, &definition)?;
    Ok(Some(record))
}
fn validate_read_record(
    record: &Authorization,
    id: &str,
    reserved: i64,
    definition: &eefn::automation::Definition,
) -> Result<()> {
    crate::automation_policy::validate_authorization(record, definition)?;
    if record.id != id || reserved < 0 || usize::try_from(reserved)? != reserve(record) {
        bail!("Authorization identity/revocation reserve mismatch");
    }
    Ok(())
}
pub(super) fn records(connection: &Connection) -> Result<Vec<Authorization>> {
    let mut statement = connection.prepare(
        "SELECT authorization_id FROM automation_authorizations ORDER BY authorization_id",
    )?;
    let mut result = Vec::new();
    for id in statement.query_map([], |r| r.get::<_, String>(0))? {
        result.push(read(connection, &id?)?.context("Missing retained authorization")?);
    }
    validate_generations(&result)?;
    Ok(result)
}
fn validate_generations(records: &[Authorization]) -> Result<()> {
    let mut generations = std::collections::BTreeMap::<String, Vec<u64>>::new();
    for record in records {
        let PolicyCommand::Approve { definition, .. } = &record.creation else {
            unreachable!();
        };
        generations
            .entry(definition.clone())
            .or_default()
            .push(record.generation);
    }
    for sequence in generations.values_mut() {
        sequence.sort_unstable();
        if sequence
            .iter()
            .enumerate()
            .any(|(index, g)| *g != index as u64 + 1)
        {
            bail!("Authorization generation history is damaged; retain without activation");
        }
    }
    Ok(())
}
type Definitions = std::collections::BTreeMap<String, eefn::automation::Definition>;
/// One locked read's dependency index, never a cache across calls or writes.
/// Existing point readers and every record/generation predicate are retained.
pub(super) fn records_and_definitions(
    connection: &Connection,
) -> Result<(Vec<Authorization>, Definitions)> {
    records_loading_definitions(connection, automations::read_definition)
}
fn records_loading_definitions(
    connection: &Connection,
    mut load: impl FnMut(&Connection, &str) -> Result<eefn::automation::Definition>,
) -> Result<(Vec<Authorization>, Definitions)> {
    let mut definitions = Definitions::new();
    let mut records = Vec::new();
    let mut statement = connection.prepare(
        "SELECT authorization_id,record_json,reserved_bytes FROM automation_authorizations ORDER BY authorization_id",
    )?;
    for row in statement.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })? {
        let (id, raw, reserved) = row?;
        let record: Authorization = serde_json::from_str(&raw)
            .context("Damaged or future authorization; retained without activation")?;
        let PolicyCommand::Approve { definition, .. } = &record.creation else {
            bail!("Missing policy creation");
        };
        let source = match definitions.entry(definition.clone()) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(load(connection, definition)?)
            }
        };
        validate_read_record(&record, &id, reserved, source)?;
        records.push(record);
    }
    validate_generations(&records)?;
    Ok((records, definitions))
}
pub(super) fn validate_all(connection: &Connection) -> Result<()> {
    records(connection)?;
    Ok(())
}
impl JobStore {
    pub fn automation_authorization(&self, id: &str) -> Result<Option<Authorization>> {
        eefn::job_commands::validate_operation_id(id)?;
        read(&self.connection.lock().expect("job journal"), id)
    }
    fn save_authorization(
        &self,
        connection: &Connection,
        record: &Authorization,
        new: bool,
    ) -> Result<()> {
        let PolicyCommand::Approve { definition, .. } = &record.creation else {
            bail!("Missing policy command");
        };
        crate::automation_policy::validate_authorization(
            record,
            &automations::read_definition(connection, definition)?,
        )?;
        let encoded = serde_json::to_string(record)?;
        let cost = encoded
            .len()
            .checked_add(reserve(record))
            .context("Authorization size overflow")?;
        if new && automations::count(connection)? >= self.max_count {
            bail!("Job/definition/authorization history is full");
        }
        let previous:Option<i64>=connection.query_row("SELECT length(CAST(record_json AS BLOB))+reserved_bytes FROM automation_authorizations WHERE authorization_id=?1",[&record.id],|r|r.get(0)).optional()?;
        if !previous.is_some_and(|old| old >= 0 && cost as u64 <= old as u64) {
            let jobs:i64=connection.query_row("SELECT COALESCE(SUM(length(CAST(plan_json AS BLOB))+reserved_bytes),0) FROM durable_jobs",[],|r|r.get(0))?;
            let definitions:i64=connection.query_row("SELECT COALESCE(SUM(length(CAST(record_json AS BLOB))),0) FROM automation_definitions",[],|r|r.get(0))?;
            let other:i64=connection.query_row("SELECT COALESCE(SUM(length(CAST(record_json AS BLOB))+reserved_bytes),0) FROM automation_authorizations WHERE authorization_id<>?1",[&record.id],|r|r.get(0))?;
            let used = usize::try_from(jobs)?
                .checked_add(usize::try_from(definitions)?)
                .and_then(|n| n.checked_add(usize::try_from(other).ok()?))
                .context("Authorization size overflow")?;
            let intent_bytes = model_verification_intents::bytes(connection)?;
            let used = used
                .checked_add(automation_occurrences::bytes(connection)?)
                .and_then(|n| n.checked_add(intent_bytes))
                .context("Authorization size overflow")?;
            if cost > 8 * 1024 * 1024 || cost > self.max_bytes.saturating_sub(used) {
                bail!("Job/definition/authorization storage is full; no policy saved");
            }
        }
        if new {
            connection.execute("INSERT INTO automation_authorizations(authorization_id,record_json,reserved_bytes) VALUES (?1,?2,?3)",params![record.id,encoded,i64::try_from(reserve(record))?])?;
        } else {
            connection.execute("UPDATE automation_authorizations SET record_json=?2,reserved_bytes=?3 WHERE authorization_id=?1",params![record.id,encoded,i64::try_from(reserve(record))?])?;
        }
        Ok(())
    }
    pub(crate) fn automation_policy_checkpoint(
        &self,
        command: PolicyCommand,
        candidate: Option<Authorization>,
        runtime: &str,
        now: u64,
    ) -> Result<serde_json::Value> {
        command.validate()?;
        eefn::job_commands::validate_operation_id(runtime)?;
        let connection = self.connection.lock().expect("job journal");
        let retained = records(&connection)?;
        let mut result = None;
        let mut items = Vec::new();
        let mut next = None;
        let mut reused = false;
        match &command {
            PolicyCommand::Show { id } => {
                result = Some(read(&connection, id)?.context("Authorization unavailable")?);
            }
            PolicyCommand::List {
                definition,
                limit,
                after,
            } => {
                for record in &retained {
                    let PolicyCommand::Approve { definition: d, .. } = &record.creation else {
                        unreachable!();
                    };
                    if definition.as_ref().is_some_and(|id| id != d)
                        || after.as_ref().is_some_and(|id| record.id <= *id)
                    {
                        continue;
                    }
                    if items.len() == *limit as usize {
                        next = items
                            .last()
                            .and_then(|v: &serde_json::Value| v["id"].as_str())
                            .map(str::to_owned);
                        continue;
                    }
                    items.push(record.summary());
                }
            }
            PolicyCommand::Approve {
                definition,
                operation_id,
                generation,
                ..
            } => {
                if let Some(old) = retained.iter().find(|r| r.id == *operation_id) {
                    if old.creation != command {
                        bail!("Approval receipt conflicts with retained policy");
                    }
                    result = Some(old.clone());
                    reused = true;
                } else {
                    let record = candidate.context(
                        "Approval requires independently checked current issuer/providers",
                    )?;
                    let source = automations::read_definition(&connection, definition)?;
                    let current=retained.iter().filter(|r|matches!(&r.creation,PolicyCommand::Approve {definition:d,..} if d==definition)).map(|r|r.generation).max().unwrap_or(0);
                    if source.state != "disabled"
                        || source.revision != 1
                        || source.source.is_none()
                        || current != *generation
                        || record.creation != command
                        || record.runtime_id != runtime
                        || record.generation != current + 1
                        || now < record.approved_at_ms
                        || now >= record.expires_at_ms
                    {
                        bail!(
                            "Definition/generation/runtime/validity changed before policy checkpoint"
                        );
                    }
                    self.save_authorization(&connection, &record, true)?;
                    result = Some(record);
                }
            }
            PolicyCommand::Revoke {
                id,
                revision,
                operation_id,
            } => {
                let mut record = read(&connection, id)?.context("Authorization unavailable")?;
                if let Some(old) = &record.revocation {
                    if old.operation_id != *operation_id || old.source_revision != *revision {
                        bail!("Revocation receipt conflicts; inspect retained policy");
                    }
                    reused = true;
                } else {
                    if record.revision != *revision
                        || record.state != "approved"
                        || operation_id == id
                        || now == 0
                    {
                        bail!("Policy state/revision changed before revocation");
                    }
                    record.revision = 2;
                    record.state = "revoked".into();
                    record.revocation = Some(Revocation {
                        operation_id: operation_id.clone(),
                        source_revision: *revision,
                        revoked_at_ms: now,
                        runtime_id: runtime.into(),
                    });
                    self.save_authorization(&connection, &record, false)?;
                }
                result = Some(record);
            }
        }
        Ok(crate::automation_policy::report(
            runtime, &command, result, items, next, reused,
        ))
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use eefn::automation_policy::PolicyCommand;
    use serde_json::{Value, json};
    pub(crate) fn fixture(
        store: Arc<JobStore>,
    ) -> (
        Arc<crate::task::TaskEngine>,
        Plan,
        eefn::automation::Definition,
        Authorization,
    ) {
        fixture_with_validity(store, 10000)
    }
    pub(crate) fn fixture_with_validity(
        store: Arc<JobStore>,
        valid_for_ms: u64,
    ) -> (
        Arc<crate::task::TaskEngine>,
        Plan,
        eefn::automation::Definition,
        Authorization,
    ) {
        let engine =
            crate::task::TaskEngine::with_store(crate::event::EventBus::new(20), store.clone());
        let plan = crate::request_approval::tests::compiled(&engine);
        let saved = store
            .automation_command(
                eefn::automation::AutomationCommand::Define {
                    operation_id: uuid::Uuid::new_v4().to_string(),
                    from_job: plan.plan_id.clone(),
                    revision: plan.revision,
                    plan_sha256: plan.planning.as_ref().unwrap().plan_sha256.clone(),
                    name: "Reusable status".into(),
                    every_ms: 1000,
                    max_runs: 3,
                    valid_for_ms,
                },
                None,
            )
            .unwrap();
        let definition: eefn::automation::Definition =
            serde_json::from_value(saved["definition"].clone()).unwrap();
        let command = PolicyCommand::Approve {
            definition: definition.id.clone(),
            revision: 1,
            definition_sha256: definition.definition_sha256.clone(),
            generation: 0,
            operation_id: uuid::Uuid::new_v4().to_string(),
            valid_for_ms,
            max_runs: 3,
            max_execution_ms_per_task: 1000,
            max_result_bytes_per_task: 1024,
            max_reserved_execution_ms: 3000,
            max_reserved_result_bytes: 3072,
        };
        let scope = crate::automation_policy::fixed_scope(&plan).unwrap();
        let mut record = Authorization {
            schema_version: 1,
            id: command.receipt().unwrap().into(),
            creation: command,
            generation: 1,
            runtime_id: uuid::Uuid::new_v4().to_string(),
            issuer_id: "a".repeat(64),
            approved_at_ms: definition.created_at_ms + 1,
            expires_at_ms: definition.created_at_ms + valid_for_ms + 1,
            scope_sha256: eefn::grants::definition_digest(&json!(scope)).unwrap(),
            scope,
            authorization_sha256: String::new(),
            revision: 1,
            state: "approved".into(),
            revocation: None,
        };
        record.authorization_sha256 = record.digest().unwrap();
        crate::automation_policy::validate_authorization(&record, &definition).unwrap();
        (engine, plan, definition, record)
    }
    fn save(store: &JobStore, r: &Authorization) -> Value {
        store
            .automation_policy_checkpoint(
                r.creation.clone(),
                Some(r.clone()),
                &r.runtime_id,
                r.approved_at_ms,
            )
            .unwrap()
    }
    #[test]
    fn indexed_read_loads_each_definition_once_without_writes_and_matches_point_readers() {
        let store = JobStore::in_memory().unwrap();
        let (_, _, first_definition, first) = fixture(store.clone());
        save(&store, &first);
        let mut next = first.clone();
        next.id = uuid::Uuid::new_v4().to_string();
        if let PolicyCommand::Approve {
            operation_id,
            generation,
            ..
        } = &mut next.creation
        {
            *operation_id = next.id.clone();
            *generation = 1;
        }
        next.generation = 2;
        next.authorization_sha256 = next.digest().unwrap();
        save(&store, &next);
        let (_, _, second_definition, second) = fixture(store.clone());
        save(&store, &second);
        let connection = store.connection.lock().unwrap();
        let before: i64 = connection
            .query_row("SELECT total_changes()", [], |r| r.get(0))
            .unwrap();
        let mut loaded = Vec::new();
        let (indexed, definitions) = records_loading_definitions(&connection, |connection, id| {
            loaded.push(id.to_owned());
            automations::read_definition(connection, id)
        })
        .unwrap();
        assert_eq!(indexed, records(&connection).unwrap());
        assert_eq!(indexed.len(), 3);
        assert_eq!(definitions.len(), 2);
        loaded.sort();
        let mut expected = vec![first_definition.id.clone(), second_definition.id.clone()];
        expected.sort();
        assert_eq!(
            loaded, expected,
            "one actual definition validation per distinct dependency"
        );
        for (id, definition) in definitions {
            assert_eq!(
                serde_json::to_value(definition).unwrap(),
                serde_json::to_value(automations::read_definition(&connection, &id).unwrap())
                    .unwrap()
            );
        }
        let after: i64 = connection
            .query_row("SELECT total_changes()", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            after, before,
            "inspection must not write or grant authority"
        );
    }
    #[test]
    fn indexed_read_revalidates_changed_definition_on_the_next_call() {
        let store = JobStore::in_memory().unwrap();
        let (_, _, definition, first) = fixture(store.clone());
        save(&store, &first);
        let connection = store.connection.lock().unwrap();
        assert!(records_and_definitions(&connection).is_ok());
        connection
            .execute(
                "UPDATE automation_definitions SET record_json='{}' WHERE definition_id=?1",
                [&definition.id],
            )
            .unwrap();
        assert!(records(&connection).is_err());
        assert!(
            records_and_definitions(&connection).is_err(),
            "no cached dependency survives a call"
        );
    }
    #[test]
    fn indexed_read_preserves_identity_scope_reserve_and_generation_refusal() {
        let store = JobStore::in_memory().unwrap();
        let (_, _, _, first) = fixture(store.clone());
        save(&store, &first);
        let connection = store.connection.lock().unwrap();
        let original: (String, i64) = connection.query_row("SELECT record_json,reserved_bytes FROM automation_authorizations WHERE authorization_id=?1", [&first.id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert!(records_and_definitions(&connection).is_ok());
        for field in ["identity", "scope", "reserve", "generation", "future"] {
            let mut bad = first.clone();
            let mut reserved = original.1;
            match field {
                "identity" => {
                    bad.id = uuid::Uuid::new_v4().to_string();
                    if let PolicyCommand::Approve { operation_id, .. } = &mut bad.creation {
                        *operation_id = bad.id.clone();
                    }
                }
                "scope" => {
                    bad.scope.operations[0].executor_node = "other".into();
                }
                "reserve" => {
                    reserved = -1;
                }
                "generation" => {
                    bad.generation = 2;
                    if let PolicyCommand::Approve { generation, .. } = &mut bad.creation {
                        *generation = 1;
                    }
                }
                "future" => {}
                _ => unreachable!(),
            }
            bad.scope_sha256 = eefn::grants::definition_digest(&json!(bad.scope)).unwrap();
            bad.authorization_sha256 = bad.digest().unwrap();
            let raw = if field == "future" {
                "{\"schema_version\":999}".to_owned()
            } else {
                serde_json::to_string(&bad).unwrap()
            };
            connection.execute("UPDATE automation_authorizations SET record_json=?2,reserved_bytes=?3 WHERE authorization_id=?1", params![first.id, raw, reserved]).unwrap();
            let point = records(&connection).unwrap_err();
            let indexed = records_and_definitions(&connection).unwrap_err();
            assert_eq!(indexed.to_string(), point.to_string(), "{field}");
            connection.execute("UPDATE automation_authorizations SET record_json=?2,reserved_bytes=?3 WHERE authorization_id=?1", params![first.id, original.0, original.1]).unwrap();
            assert!(records_and_definitions(&connection).is_ok());
        }
    }
    #[test]
    fn durable_approval_does_not_mutate_source_and_exact_reuse_cannot_renew() {
        let store = JobStore::in_memory().unwrap();
        let (_engine, source, definition, record) = fixture(store.clone());
        let before = json!(store.get(&source.plan_id).unwrap());
        let saved = save(&store, &record);
        assert!(eefn::automation_policy::confirmed(
            &saved,
            &record.creation,
            &record.runtime_id
        ));
        assert_eq!(saved["occurrence_admission_available"], true);
        assert_eq!(store.usage().unwrap()["authorization_count"], 1);
        assert_eq!(store.usage().unwrap()["automation_count"], 1);
        assert_eq!(store.usage().unwrap()["count"], 1);
        let new_runtime = uuid::Uuid::new_v4().to_string();
        let reused = store
            .automation_policy_checkpoint(
                record.creation.clone(),
                None,
                &new_runtime,
                record.expires_at_ms + 1,
            )
            .unwrap();
        assert_eq!(reused["reused"], true);
        assert_eq!(reused["authorization"], saved["authorization"]);
        assert!(!record.current_at(
            &new_runtime,
            &record.issuer_id,
            1,
            &definition,
            record.approved_at_ms
        ));
        assert_eq!(json!(store.get(&source.plan_id).unwrap()), before);
        assert_eq!(store.list().unwrap().len(), 1);
        let mut conflict = record.creation.clone();
        if let PolicyCommand::Approve { max_runs, .. } = &mut conflict {
            *max_runs = 2;
        }
        assert!(
            store
                .automation_policy_checkpoint(conflict, None, &new_runtime, record.approved_at_ms)
                .is_err()
        );
    }
    #[test]
    fn generations_are_compare_and_swap_and_old_generation_is_not_current() {
        let store = JobStore::in_memory().unwrap();
        let (_engine, _source, definition, first) = fixture(store.clone());
        save(&store, &first);
        let mut next = first.clone();
        next.id = uuid::Uuid::new_v4().to_string();
        if let PolicyCommand::Approve { operation_id, .. } = &mut next.creation {
            *operation_id = next.id.clone();
        }
        next.authorization_sha256 = next.digest().unwrap();
        assert!(
            store
                .automation_policy_checkpoint(
                    next.creation.clone(),
                    Some(next.clone()),
                    &next.runtime_id,
                    next.approved_at_ms
                )
                .is_err()
        );
        if let PolicyCommand::Approve { generation, .. } = &mut next.creation {
            *generation = 1;
        }
        next.generation = 2;
        next.authorization_sha256 = next.digest().unwrap();
        save(&store, &next);
        assert!(!first.current_at(
            &first.runtime_id,
            &first.issuer_id,
            2,
            &definition,
            first.approved_at_ms
        ));
        assert!(next.current_at(
            &next.runtime_id,
            &next.issuer_id,
            2,
            &definition,
            next.approved_at_ms
        ));
        let all = store
            .automation_policy_checkpoint(
                PolicyCommand::List {
                    definition: Some(definition.id),
                    limit: 1,
                    after: None,
                },
                None,
                &first.runtime_id,
                first.approved_at_ms,
            )
            .unwrap();
        assert_eq!(all["items"].as_array().unwrap().len(), 1);
        assert!(!all["next_after"].is_null());
        assert!(!serde_json::to_string(&all).unwrap().contains("operations"));
    }
    #[test]
    fn source_change_over_budget_and_scope_substitution_refuse_without_save() {
        let store = JobStore::in_memory().unwrap();
        let (_engine, _source, definition, base) = fixture(store.clone());
        for field in ["digest", "run_budget", "duration_budget", "scope"] {
            let mut bad = base.clone();
            match field {
                "digest" => {
                    if let PolicyCommand::Approve {
                        definition_sha256, ..
                    } = &mut bad.creation
                    {
                        *definition_sha256 = "b".repeat(64);
                    }
                }
                "run_budget" => {
                    if let PolicyCommand::Approve { max_runs, .. } = &mut bad.creation {
                        *max_runs = 4;
                    }
                }
                "duration_budget" => {
                    if let PolicyCommand::Approve {
                        max_reserved_execution_ms,
                        ..
                    } = &mut bad.creation
                    {
                        *max_reserved_execution_ms = 999;
                    }
                }
                "scope" => {
                    bad.scope.operations[0].executor_node = "other".into();
                    bad.scope.operations[0].constraints["node_id"] = json!("other");
                }
                _ => unreachable!(),
            }
            bad.scope_sha256 = eefn::grants::definition_digest(&json!(bad.scope)).unwrap();
            bad.authorization_sha256 = bad.digest().unwrap();
            assert!(
                store
                    .automation_policy_checkpoint(
                        bad.creation.clone(),
                        Some(bad.clone()),
                        &bad.runtime_id,
                        bad.approved_at_ms
                    )
                    .is_err(),
                "{field}"
            );
        }
        store
            .automation_command(
                eefn::automation::AutomationCommand::Remove {
                    id: definition.id,
                    revision: 1,
                    operation_id: uuid::Uuid::new_v4().to_string(),
                },
                None,
            )
            .unwrap();
        assert!(
            store
                .automation_policy_checkpoint(
                    base.creation.clone(),
                    Some(base.clone()),
                    &base.runtime_id,
                    base.approved_at_ms
                )
                .is_err()
        );
        assert_eq!(store.usage().unwrap()["authorization_count"], 0);
    }
    #[test]
    fn compiler_validation_survives_source_removal_and_recomputed_hashes() {
        let store = JobStore::in_memory().unwrap();
        let (_engine, _source, definition, base) = fixture(store.clone());
        save(&store, &base);
        store
            .automation_command(
                eefn::automation::AutomationCommand::Remove {
                    id: definition.id.clone(),
                    revision: 1,
                    operation_id: uuid::Uuid::new_v4().to_string(),
                },
                None,
            )
            .unwrap();
        let removed =
            automations::read_definition(&store.connection.lock().unwrap(), &definition.id)
                .unwrap();
        assert!(crate::automation_policy::validate_authorization(&base, &removed).is_ok());
        let mut bad = base.clone();
        bad.scope.operations[0].params = json!({"unexpected":"parameter"});
        bad.scope_sha256 = eefn::grants::definition_digest(&json!(bad.scope)).unwrap();
        bad.authorization_sha256 = bad.digest().unwrap();
        assert!(bad.validate().is_ok());
        assert!(crate::automation_policy::validate_authorization(&bad, &removed).is_err());
        assert!(!base.current_at(
            &base.runtime_id,
            &base.issuer_id,
            1,
            &removed,
            base.approved_at_ms
        ));
    }
    #[test]
    fn restart_retains_approval_and_damaged_future_records_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        Connection::open(&path).unwrap();
        let store = JobStore::open(&path, 20, 1024 * 1024).unwrap();
        let (engine, _source, _definition, record) = fixture(store.clone());
        save(&store, &record);
        drop(engine);
        drop(store);
        let store = JobStore::open(&path, 20, 1024 * 1024).unwrap();
        assert_eq!(
            store.automation_authorization(&record.id).unwrap(),
            Some(record.clone())
        );
        drop(store);
        let db = Connection::open(&path).unwrap();
        let mut raw = json!(record);
        raw["schema_version"] = json!(99);
        let damaged = serde_json::to_string(&raw).unwrap();
        db.execute(
            "UPDATE automation_authorizations SET record_json=?1",
            [&damaged],
        )
        .unwrap();
        drop(db);
        assert!(JobStore::open(&path, 20, 1024 * 1024).is_err());
        let db = Connection::open(&path).unwrap();
        let retained: String = db
            .query_row(
                "SELECT record_json FROM automation_authorizations",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(retained, damaged);
    }
    #[test]
    fn missing_generation_fails_closed_without_pruning_remaining_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        Connection::open(&path).unwrap();
        let store = JobStore::open(&path, 20, 1024 * 1024).unwrap();
        let (engine, _source, _definition, first) = fixture(store.clone());
        save(&store, &first);
        let mut next = first.clone();
        next.id = uuid::Uuid::new_v4().to_string();
        next.generation = 2;
        if let PolicyCommand::Approve {
            operation_id,
            generation,
            ..
        } = &mut next.creation
        {
            *operation_id = next.id.clone();
            *generation = 1;
        }
        next.authorization_sha256 = next.digest().unwrap();
        save(&store, &next);
        // Deliberate isolated test corruption; production never deletes policies.
        store
            .connection
            .lock()
            .unwrap()
            .execute(
                "DELETE FROM automation_authorizations WHERE authorization_id=?1",
                [&first.id],
            )
            .unwrap();
        assert!(
            store
                .automation_policy_checkpoint(
                    PolicyCommand::List {
                        definition: None,
                        limit: 25,
                        after: None
                    },
                    None,
                    &first.runtime_id,
                    first.approved_at_ms
                )
                .is_err()
        );
        drop(engine);
        drop(store);
        assert!(JobStore::open(&path, 20, 1024 * 1024).is_err());
        let db = Connection::open(&path).unwrap();
        let raw: String = db
            .query_row(
                "SELECT record_json FROM automation_authorizations",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raw, serde_json::to_string(&next).unwrap());
    }
    #[test]
    fn shared_byte_limits_include_authorizations_for_all_three_record_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        Connection::open(&path).unwrap();
        let store = JobStore::open(&path, 20, 1024 * 1024).unwrap();
        let (engine, source, definition, record) = fixture(store.clone());
        let usage = store.usage().unwrap();
        let before = usage["payload_bytes"].as_u64().unwrap()
            + usage["reserved_bytes"].as_u64().unwrap()
            + usage["automation_bytes"].as_u64().unwrap();
        let cost = serde_json::to_string(&record).unwrap().len() + REVOCATION_RESERVE;
        drop(engine);
        drop(store);
        let store = JobStore::open(&path, 20, before as usize + cost - 1).unwrap();
        let unchanged = store.usage().unwrap();
        assert!(
            store
                .automation_policy_checkpoint(
                    record.creation.clone(),
                    Some(record.clone()),
                    &record.runtime_id,
                    record.approved_at_ms
                )
                .unwrap_err()
                .to_string()
                .contains("storage is full")
        );
        assert_eq!(store.usage().unwrap(), unchanged);
        drop(store);
        let store = JobStore::open(&path, 20, before as usize + cost).unwrap();
        save(&store, &record);
        assert_eq!(store.usage().unwrap()["available_bytes"], 0);
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
        let mut next_definition = definition.creation.clone();
        if let eefn::automation::AutomationCommand::Define { operation_id, .. } =
            &mut next_definition
        {
            *operation_id = uuid::Uuid::new_v4().to_string();
        }
        assert!(
            store
                .automation_command(next_definition, None)
                .unwrap_err()
                .to_string()
                .contains("storage is full")
        );
        let mut next = record.clone();
        next.id = uuid::Uuid::new_v4().to_string();
        next.generation = 2;
        if let PolicyCommand::Approve {
            operation_id,
            generation,
            ..
        } = &mut next.creation
        {
            *operation_id = next.id.clone();
            *generation = 1;
        }
        next.authorization_sha256 = next.digest().unwrap();
        assert!(
            store
                .automation_policy_checkpoint(
                    next.creation.clone(),
                    Some(next.clone()),
                    &next.runtime_id,
                    next.approved_at_ms
                )
                .unwrap_err()
                .to_string()
                .contains("storage is full")
        );
        assert_eq!(
            json!(store.get(&source.plan_id).unwrap().planning),
            json!(source.planning)
        );
        assert_eq!(store.usage().unwrap()["count"], 1);
        assert_eq!(store.usage().unwrap()["automation_count"], 1);
        assert_eq!(store.usage().unwrap()["authorization_count"], 1);
    }
    #[test]
    fn shared_quota_refusal_and_lowered_quota_revocation_use_reserved_headroom() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        Connection::open(&path).unwrap();
        let store = JobStore::open(&path, 3, 1024 * 1024).unwrap();
        let (engine, _source, _definition, record) = fixture(store.clone());
        save(&store, &record);
        assert_eq!(store.usage().unwrap()["authorization_reserved_bytes"], 1024);
        let mut second = record.clone();
        second.id = uuid::Uuid::new_v4().to_string();
        second.generation = 2;
        if let PolicyCommand::Approve {
            operation_id,
            generation,
            ..
        } = &mut second.creation
        {
            *operation_id = second.id.clone();
            *generation = 1;
        }
        second.authorization_sha256 = second.digest().unwrap();
        assert!(
            store
                .automation_policy_checkpoint(
                    second.creation.clone(),
                    Some(second.clone()),
                    &second.runtime_id,
                    second.approved_at_ms
                )
                .is_err()
        );
        drop(engine);
        drop(store);
        let store = JobStore::open(&path, 1, 1024).unwrap();
        assert_eq!(store.usage().unwrap()["over_limit"], true);
        let revoke = PolicyCommand::Revoke {
            id: record.id.clone(),
            revision: 1,
            operation_id: uuid::Uuid::new_v4().to_string(),
        };
        let revoked = store
            .automation_policy_checkpoint(
                revoke.clone(),
                None,
                &record.runtime_id,
                record.approved_at_ms - 1,
            )
            .unwrap();
        assert_eq!(revoked["authorization"]["state"], "revoked");
        assert_eq!(store.usage().unwrap()["authorization_reserved_bytes"], 0);
        assert_eq!(
            store
                .automation_policy_checkpoint(
                    revoke,
                    None,
                    &record.runtime_id,
                    record.expires_at_ms + 1
                )
                .unwrap()["reused"],
            true
        );
        assert_eq!(
            store
                .automation_policy_checkpoint(
                    record.creation.clone(),
                    None,
                    &record.runtime_id,
                    record.approved_at_ms
                )
                .unwrap()["authorization"]["state"],
            "revoked"
        );
    }
}
