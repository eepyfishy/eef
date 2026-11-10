//! A recovery receipt and source mutation share one existing job UPDATE.
use super::*;
use anyhow::ensure;
use eefn::interpreter_recovery::{Command, RecoveryActor};

impl JobStore {
    pub(crate) fn commit_interpreter_recovery(
        &self,
        command: &Command,
        actor: RecoveryActor,
        origin: Option<&str>,
        original: &Record,
        capabilities: &[String],
        unavailable: bool,
        edit: impl FnOnce(&mut Plan) -> Result<bool>,
    ) -> Result<(Plan, bool)> {
        let reused = std::cell::Cell::new(false);
        let plan = self.edit_with_precondition(
            &command.id,
            "Explicit retained interpreter result recovered; no inference or execution authorized",
            None,
            matches!(original.task()?.snapshot.source, Source::Initial { .. }),
            true,
            |connection, plan| {
                let records = snapshot(connection)?.0;
                let actual = records
                    .iter()
                    .find(|r| r.id() == command.grant_id)
                    .context("Interpreter recovery evidence unavailable")?;
                ensure!(
                    json!(actual) == json!(original),
                    "Interpreter evidence changed before recovery"
                );
                reused.set(crate::interpreter_recovery::eligible(
                    plan,
                    command,
                    actor,
                    origin,
                    actual,
                    unavailable,
                )?);
                Ok(())
            },
            |plan| {
                if reused.get() {
                    return Ok(false);
                }
                let mut recovery =
                    crate::interpreter_recovery::receipt(command, actor, original, capabilities)?;
                recovery.decided_at_ms = recovery.decided_at_ms.max(plan.updated_at_ms);
                recovery.validate()?;
                ensure!(edit(plan)?, "Recovery source mutation was not applied");
                plan.interpreter_recoveries.push(recovery);
                Ok(true)
            },
        )?;
        Ok((plan, reused.get()))
    }
}
