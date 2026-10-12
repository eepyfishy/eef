//! Owner reservation contracts. A reservation is not dispatch or a timer.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, clap::Subcommand)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum OccurrenceCommand {
    /// Reserve one independently compiled matching job, without dispatching it.
    Reserve {
        #[arg(long)]
        policy: String,
        #[arg(long)]
        policy_sha256: String,
        #[arg(long)]
        job: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        plan_sha256: String,
        #[arg(long)]
        operation_id: String,
    },
    Show {
        id: String,
    },
    /// Retain proven terminal evidence and remove the live job atomically.
    Archive {
        #[arg(long)]
        reservation: String,
        #[arg(long)]
        reservation_sha256: String,
        #[arg(long)]
        job: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        operation_id: String,
    },
    ArchiveShow {
        id: String,
    },
    List {
        #[arg(long)]
        policy: Option<String>,
        #[arg(long, default_value_t = 25)]
        limit: u16,
        #[arg(long)]
        after: Option<String>,
    },
}
pub fn uuid(value: &str) -> Result<()> {
    crate::job_commands::validate_operation_id(value)?;
    if uuid::Uuid::parse_str(value)?.is_nil() {
        bail!("Nil authority/receipt identity");
    }
    Ok(())
}
impl OccurrenceCommand {
    pub fn receipt(&self) -> Option<&str> {
        if let Self::Reserve { operation_id, .. } | Self::Archive { operation_id, .. } = self {
            Some(operation_id)
        } else {
            None
        }
    }
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Reserve {
                policy,
                policy_sha256,
                job,
                revision,
                plan_sha256,
                operation_id,
            } => {
                uuid(policy)?;
                uuid(operation_id)?;
                crate::request_drafts::validate_id(job)?;
                if policy == operation_id
                    || *revision == 0
                    || *revision == u64::MAX
                    || !crate::request_result::valid_digest(policy_sha256)
                    || !crate::request_result::valid_digest(plan_sha256)
                {
                    bail!("Invalid exact occurrence reservation");
                }
            }
            Self::Archive {
                reservation,
                reservation_sha256,
                job,
                revision,
                operation_id,
            } => {
                uuid(reservation)?;
                uuid(operation_id)?;
                crate::request_drafts::validate_id(job)?;
                if reservation == operation_id
                    || *revision == 0
                    || *revision == u64::MAX
                    || !crate::request_result::valid_digest(reservation_sha256)
                {
                    bail!("Invalid exact occurrence archive");
                }
            }
            Self::Show { id } | Self::ArchiveShow { id } => uuid(id)?,
            Self::List {
                policy,
                limit,
                after,
            } => {
                if !(1..=100).contains(limit) {
                    bail!("Occurrence page limit must be 1-100");
                }
                for id in policy.iter().chain(after) {
                    uuid(id)?;
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delegation {
    pub schema_version: u32,
    pub reservation_id: String,
    pub policy_id: String,
    pub policy_sha256: String,
    pub definition_id: String,
    pub generation: u64,
    pub runtime_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<TriggerLink>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerLink {
    pub schedule_id: String,
    pub trigger_id: String,
}
impl Delegation {
    pub fn validate(&self) -> Result<()> {
        if let Some(link) = &self.trigger {
            uuid(&link.schedule_id)?;
            uuid(&link.trigger_id)?;
            if link.schedule_id == link.trigger_id
                || link.schedule_id == self.policy_id
                || link.trigger_id == self.reservation_id
            {
                bail!("Invalid durable trigger reference");
            }
        }
        for id in [
            &self.reservation_id,
            &self.policy_id,
            &self.definition_id,
            &self.runtime_id,
        ] {
            uuid(id)?;
        }
        if self.schema_version != 1
            || self.generation == 0
            || self.reservation_id == self.policy_id
            || !crate::request_result::valid_digest(&self.policy_sha256)
        {
            bail!("Invalid delegated approval reference");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reservation {
    pub schema_version: u32,
    pub id: String,
    pub command: OccurrenceCommand,
    pub delegation: Delegation,
    pub ordinal: u16,
    pub reserved_at_ms: u64,
    pub reserved_execution_ms: u64,
    pub reserved_result_bytes: u64,
    /// Actual newly approved job checkpoint, not copied source inference history.
    pub approved_job: Value,
    pub approved_job_sha256: String,
    pub reservation_sha256: String,
}
impl Reservation {
    pub fn digest(&self) -> Result<String> {
        let mut value = json!(self);
        value.as_object_mut().unwrap().remove("reservation_sha256");
        crate::grants::definition_digest(&value)
    }
    pub fn validate(&self) -> Result<()> {
        self.command.validate()?;
        self.delegation.validate()?;
        let OccurrenceCommand::Reserve {
            policy,
            policy_sha256,
            job,
            revision,
            plan_sha256,
            operation_id,
        } = &self.command
        else {
            bail!("Reservation lacks exact creation command");
        };
        let approval = &self.approved_job["approval"];
        if self.schema_version != 1
            || self.id != *operation_id
            || self.id != self.delegation.reservation_id
            || self.delegation.policy_id != *policy
            || self.delegation.policy_sha256 != *policy_sha256
            || !(1..=128).contains(&self.ordinal)
            || self.reserved_at_ms == 0
            || !(1..=crate::automation_policy::MAX_RESERVED_EXECUTION_MS)
                .contains(&self.reserved_execution_ms)
            || !(1024..=crate::automation_policy::MAX_RESERVED_RESULT_BYTES)
                .contains(&self.reserved_result_bytes)
            || serde_json::to_vec(self)?.len() > 2 * 1024 * 1024
            || self.approved_job["plan_id"] != *job
            || self.approved_job["revision"] != revision + 1
            || self.approved_job["status"] != "approved"
            || !self.approved_job["execution"].is_null()
            || self.approved_job["planning"]["plan_sha256"] != *plan_sha256
            || approval["operation_id"] != self.id
            || approval["source_revision"] != *revision
            || approval["approved_at_ms"] != self.reserved_at_ms
            || approval["delegation"] != json!(self.delegation)
            || crate::grants::definition_digest(&self.approved_job)? != self.approved_job_sha256
            || self.digest()? != self.reservation_sha256
        {
            bail!("Damaged/future occurrence reservation; retained without replay");
        }
        Ok(())
    }
    pub fn summary(&self) -> Value {
        let OccurrenceCommand::Reserve { job, .. } = &self.command else {
            unreachable!()
        };
        json!({"id":self.id,"policy_id":self.delegation.policy_id,"definition_id":self.delegation.definition_id,
            "job_id":job,"generation":self.delegation.generation,"ordinal":self.ordinal,
            "reserved_at_ms":self.reserved_at_ms,"reserved_execution_ms":self.reserved_execution_ms,
            "reserved_result_bytes":self.reserved_result_bytes,"reservation_sha256":self.reservation_sha256})
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OccurrenceRequest {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub command: OccurrenceCommand,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OccurrenceReport {
    pub schema_version: u32,
    pub report_type: String,
    pub runtime_id: String,
    pub command: OccurrenceCommand,
    pub reservation: Option<Reservation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<Archive>,
    pub items: Vec<Value>,
    pub next_after: Option<String>,
    pub reused: bool,
    pub execution_requested: bool,
    pub timers_started: bool,
    pub automatically_retried: bool,
}
pub fn confirmed(value: &Value, command: &OccurrenceCommand, runtime: &str) -> bool {
    let Ok(r) = serde_json::from_value::<OccurrenceReport>(value.clone()) else {
        return false;
    };
    if command.validate().is_err()
        || uuid(runtime).is_err()
        || r.schema_version != 1
        || r.report_type != "automation_occurrence"
        || r.runtime_id != runtime
        || r.command != *command
        || r.execution_requested
        || r.timers_started
        || r.automatically_retried
    {
        return false;
    }
    match command {
        OccurrenceCommand::Reserve { .. } | OccurrenceCommand::Show { .. } => {
            let Some(record) = r.reservation else {
                return false;
            };
            record.validate().is_ok()
                && r.archive.is_none()
                && r.items.is_empty()
                && r.next_after.is_none()
                && match command {
                    OccurrenceCommand::Reserve { .. } => record.command == *command,
                    OccurrenceCommand::Show { id } => record.id == *id && !r.reused,
                    _ => false,
                }
        }
        OccurrenceCommand::Archive { .. } | OccurrenceCommand::ArchiveShow { .. } => {
            let Some(archive) = r.archive else {
                return false;
            };
            archive.validate().is_ok()
                && r.reservation.is_none()
                && r.items.is_empty()
                && r.next_after.is_none()
                && match command {
                    OccurrenceCommand::Archive { .. } => archive.command == *command,
                    OccurrenceCommand::ArchiveShow { id } => archive.id == *id && !r.reused,
                    _ => false,
                }
        }
        OccurrenceCommand::List {
            policy,
            limit,
            after,
        } => {
            if r.reservation.is_some()
                || r.archive.is_some()
                || r.reused
                || r.items.len() > *limit as usize
            {
                return false;
            }
            let mut previous = after.as_deref();
            for item in &r.items {
                let Ok(s) = serde_json::from_value::<Summary>(item.clone()) else {
                    return false;
                };
                if uuid(&s.id).is_err()
                    || uuid(&s.policy_id).is_err()
                    || uuid(&s.definition_id).is_err()
                    || crate::request_drafts::validate_id(&s.job_id).is_err()
                    || s.generation == 0
                    || !(1..=128).contains(&s.ordinal)
                    || s.reserved_at_ms == 0
                    || !(1..=crate::automation_policy::MAX_RESERVED_EXECUTION_MS)
                        .contains(&s.reserved_execution_ms)
                    || !(1024..=crate::automation_policy::MAX_RESERVED_RESULT_BYTES)
                        .contains(&s.reserved_result_bytes)
                    || !crate::request_result::valid_digest(&s.reservation_sha256)
                    || policy.as_ref().is_some_and(|p| p != &s.policy_id)
                    || previous.is_some_and(|id| s.id.as_str() <= id)
                {
                    return false;
                }
                previous = item["id"].as_str();
            }
            r.next_after
                .as_deref()
                .is_none_or(|id| r.items.len() == *limit as usize && Some(id) == previous)
        }
    }
}
/// Private retained evidence, not a resumable job or a budget refund.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub schema_version: u32,
    pub id: String,
    pub command: OccurrenceCommand,
    pub runtime_id: String,
    pub archived_at_ms: u64,
    pub terminal_job: Value,
    pub terminal_job_sha256: String,
    pub archive_sha256: String,
}
impl Archive {
    pub fn digest(&self) -> Result<String> {
        let mut value = serde_json::to_value(self)?;
        value.as_object_mut().unwrap().remove("archive_sha256");
        crate::grants::definition_digest(&value)
    }
    pub fn validate(&self) -> Result<()> {
        self.command.validate()?;
        uuid(&self.runtime_id)?;
        if serde_json::to_vec(self)?.len() > 2 * 1024 * 1024 {
            bail!(
                "Terminal archive exceeds 2 MiB; keep the live job until evidence compaction is available"
            );
        }
        let OccurrenceCommand::Archive {
            job,
            revision,
            operation_id,
            ..
        } = &self.command
        else {
            bail!("Archive lacks its exact removal command");
        };
        if self.schema_version != 1
            || self.id != *operation_id
            || self.archived_at_ms == 0
            || self.terminal_job["plan_id"] != *job
            || self.terminal_job["revision"].as_u64() != Some(*revision)
            || !matches!(
                self.terminal_job["status"].as_str(),
                Some("completed" | "cancelled")
            )
            || self.terminal_job_sha256 != crate::grants::definition_digest(&self.terminal_job)?
            || self.archive_sha256 != self.digest()?
        {
            bail!("Damaged/future terminal archive; retain without replay or refund");
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Summary {
    id: String,
    policy_id: String,
    definition_id: String,
    job_id: String,
    generation: u64,
    ordinal: u16,
    reserved_at_ms: u64,
    reserved_execution_ms: u64,
    reserved_result_bytes: u64,
    reservation_sha256: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_trigger_link_preserves_old_shape_and_rejects_invalid_authority_ids() {
        let mut d = Delegation {
            schema_version: 1,
            reservation_id: uuid::Uuid::new_v4().to_string(),
            policy_id: uuid::Uuid::new_v4().to_string(),
            policy_sha256: "a".repeat(64),
            definition_id: uuid::Uuid::new_v4().to_string(),
            generation: 1,
            runtime_id: uuid::Uuid::new_v4().to_string(),
            trigger: None,
        };
        let old = json!(d);
        assert!(old.get("trigger").is_none());
        assert_eq!(serde_json::from_value::<Delegation>(old).unwrap(), d);
        d.trigger = Some(TriggerLink {
            schedule_id: uuid::Uuid::new_v4().to_string(),
            trigger_id: uuid::Uuid::new_v4().to_string(),
        });
        d.validate().unwrap();
        for id in [
            uuid::Uuid::nil().to_string(),
            d.trigger.as_ref().unwrap().schedule_id.clone(),
            d.reservation_id.clone(),
        ] {
            let mut bad = d.clone();
            bad.trigger.as_mut().unwrap().trigger_id = id;
            assert!(bad.validate().is_err());
        }
        let mut bad = json!(d);
        bad["trigger"]["resume"] = json!(true);
        assert!(serde_json::from_value::<Delegation>(bad).is_err());
    }
    fn archive_fixture() -> Archive {
        let job = "a".repeat(32);
        let operation_id = uuid::Uuid::new_v4().to_string();
        let command = OccurrenceCommand::Archive {
            reservation: uuid::Uuid::new_v4().to_string(),
            reservation_sha256: "b".repeat(64),
            job: job.clone(),
            revision: 4,
            operation_id: operation_id.clone(),
        };
        let terminal_job = json!({"plan_id":job,"revision":4,"status":"cancelled"});
        let mut archive = Archive {
            schema_version: 1,
            id: operation_id,
            command,
            runtime_id: uuid::Uuid::new_v4().to_string(),
            archived_at_ms: 123,
            terminal_job_sha256: crate::grants::definition_digest(&terminal_job).unwrap(),
            terminal_job,
            archive_sha256: String::new(),
        };
        archive.archive_sha256 = archive.digest().unwrap();
        archive
    }
    #[test]
    fn strict_archive_requires_exact_receipt_revision_and_bounded_private_evidence() {
        let archive = archive_fixture();
        archive.validate().unwrap();
        let original = json!(archive.command);
        for (key, value) in [
            ("revision", json!(0)),
            ("reservation_sha256", json!("bad")),
            ("operation_id", json!(uuid::Uuid::nil().to_string())),
        ] {
            let mut bad = original.clone();
            bad[key] = value;
            assert!(
                serde_json::from_value::<OccurrenceCommand>(bad)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        for field in ["execute", "refund", "replace", "retry", "renew", "force"] {
            let mut bad = original.clone();
            bad[field] = json!(true);
            assert!(serde_json::from_value::<OccurrenceCommand>(bad).is_err());
        }
        for field in [
            "schema_version",
            "terminal_job_sha256",
            "archive_sha256",
            "id",
            "archived_at_ms",
        ] {
            let mut bad = json!(archive);
            bad[field] = match field {
                "schema_version" => json!(2),
                "archived_at_ms" => json!(0),
                _ => json!("bad"),
            };
            assert!(
                serde_json::from_value::<Archive>(bad)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        let mut large = archive;
        large.terminal_job["payload"] = json!("x".repeat(2 * 1024 * 1024));
        large.terminal_job_sha256 = crate::grants::definition_digest(&large.terminal_job).unwrap();
        large.archive_sha256 = large.digest().unwrap();
        assert!(large.validate().is_err());
    }
    #[test]
    fn archive_reply_is_correlated_and_never_confused_with_dispatch_or_inspection() {
        let archive = archive_fixture();
        let report = json!(OccurrenceReport {
            schema_version: 1,
            report_type: "automation_occurrence".into(),
            runtime_id: archive.runtime_id.clone(),
            command: archive.command.clone(),
            reservation: None,
            archive: Some(archive.clone()),
            items: vec![],
            next_after: None,
            reused: false,
            execution_requested: false,
            timers_started: false,
            automatically_retried: false
        });
        assert!(confirmed(&report, &archive.command, &archive.runtime_id));
        for field in [
            "execution_requested",
            "timers_started",
            "automatically_retried",
        ] {
            let mut bad = report.clone();
            bad[field] = json!(true);
            assert!(!confirmed(&bad, &archive.command, &archive.runtime_id));
        }
        let mut bad = report.clone();
        bad["archive"]["terminal_job"]["revision"] = json!(5);
        assert!(!confirmed(&bad, &archive.command, &archive.runtime_id));
        let show = OccurrenceCommand::ArchiveShow {
            id: archive.id.clone(),
        };
        let mut shown = report;
        shown["command"] = json!(show);
        assert!(confirmed(&shown, &show, &archive.runtime_id));
        shown["reused"] = json!(true);
        assert!(!confirmed(&shown, &show, &archive.runtime_id));
    }
    #[test]
    fn strict_reservation_options_and_delegation_reject_widening_and_nil_receipts() {
        let command = json!({"operation":"reserve","policy":uuid::Uuid::new_v4().to_string(),
            "policy_sha256":"a".repeat(64),"job":uuid::Uuid::new_v4().simple().to_string(),"revision":2,
            "plan_sha256":"b".repeat(64),"operation_id":uuid::Uuid::new_v4().to_string()});
        assert!(
            serde_json::from_value::<OccurrenceCommand>(command.clone())
                .unwrap()
                .validate()
                .is_ok()
        );
        for (field, value) in [
            ("revision", json!(0)),
            ("policy_sha256", json!("bad")),
            ("operation_id", json!(uuid::Uuid::nil().to_string())),
        ] {
            let mut bad = command.clone();
            bad[field] = value;
            assert!(
                serde_json::from_value::<OccurrenceCommand>(bad)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        let mut extra = command.clone();
        extra["execute"] = json!(true);
        assert!(serde_json::from_value::<OccurrenceCommand>(extra).is_err());
        let d = Delegation {
            schema_version: 1,
            reservation_id: command["operation_id"].as_str().unwrap().into(),
            policy_id: command["policy"].as_str().unwrap().into(),
            policy_sha256: "a".repeat(64),
            definition_id: uuid::Uuid::new_v4().to_string(),
            generation: 1,
            runtime_id: uuid::Uuid::new_v4().to_string(),
            trigger: None,
        };
        assert!(d.validate().is_ok());
        let mut bad = d;
        bad.generation = 0;
        assert!(bad.validate().is_err());
    }
    #[test]
    fn correlated_list_reports_are_private_scope_free_bounded_and_nonexecuting() {
        let runtime = uuid::Uuid::new_v4().to_string();
        let policy = uuid::Uuid::new_v4().to_string();
        let command = OccurrenceCommand::List {
            policy: Some(policy.clone()),
            limit: 1,
            after: None,
        };
        let item = json!({"id":uuid::Uuid::new_v4().to_string(),"policy_id":policy,"definition_id":uuid::Uuid::new_v4().to_string(),
            "job_id":uuid::Uuid::new_v4().simple().to_string(),"generation":1,"ordinal":1,"reserved_at_ms":1000,
            "reserved_execution_ms":1000,"reserved_result_bytes":1024,"reservation_sha256":"a".repeat(64)});
        let report = json!(OccurrenceReport {
            schema_version: 1,
            report_type: "automation_occurrence".into(),
            runtime_id: runtime.clone(),
            command: command.clone(),
            reservation: None,
            archive: None,
            items: vec![item],
            next_after: None,
            reused: false,
            execution_requested: false,
            timers_started: false,
            automatically_retried: false
        });
        assert!(confirmed(&report, &command, &runtime));
        for (pointer, value) in [
            ("/runtime_id", json!(uuid::Uuid::new_v4().to_string())),
            ("/execution_requested", json!(true)),
            (
                "/items/0/policy_id",
                json!(uuid::Uuid::new_v4().to_string()),
            ),
            ("/items/0/ordinal", json!(0)),
        ] {
            let mut bad = report.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(!confirmed(&bad, &command, &runtime));
        }
        let mut leaked = report.clone();
        leaked["items"][0]["approved_job"] = json!({"private":"data"});
        assert!(!confirmed(&leaked, &command, &runtime));
    }
}
