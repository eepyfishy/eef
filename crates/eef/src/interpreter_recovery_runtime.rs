//! A fresh owner request applies retained data; no old continuation is recreated.
use super::*;
use eefn::interpreter_recovery::Command;

impl Mutation {
    fn apply_recovered(self, plan: &mut Plan, task: &Task, original: &Record) -> Result<bool> {
        match self {
            Self::Initial(proposal) => {
                let claim = crate::interpreter_recovery::initial_claim(original)?;
                let expected_revision = plan.revision;
                crate::request_interpretation::apply(
                    plan,
                    crate::request_interpretation::Transition::Recover {
                        claim,
                        expected_revision,
                        proposal,
                    },
                    Some(task.snapshot.request_context.origin_node()),
                    &task.snapshot.allowed_capabilities,
                )
            }
            other => other.apply_review(plan, task),
        }
    }
}

impl Runtime {
    /// The opaque current origin session comes from the authenticated gateway,
    /// never a request body. The provider may be offline; no work is sent to it.
    pub(crate) async fn recover_interpreter_result(
        &self,
        command: &Command,
        origin: &eefn::interpreter_service::Session,
    ) -> Result<(Plan, bool)> {
        command.validate()?;
        ensure!(origin.is_fresh(), "Interpreter recovery origin is stale");
        let source = self.engine.store.get(&command.id)?;
        ensure!(
            source
                .goal
                .request_context
                .as_ref()
                .is_some_and(|c| c.origin_node() == origin.node_id()),
            "Interpreter recovery source unavailable for this origin"
        );
        let original = self
            .engine
            .store
            .interpreter_allowance_records()?
            .0
            .into_iter()
            .find(|r| r.id() == command.grant_id)
            .context("Retained interpreter result unavailable")?;
        let (_, reused) =
            self.engine
                .interpreter_recovery_source(command, origin.node_id(), &original)?;
        let (task, mutation, capabilities) = if reused {
            (original.task()?, None, vec![])
        } else {
            let capabilities = self
                .disclosed_interpretation_capabilities(origin.node_id())
                .await?;
            // Recheck semantic scope against current disclosure, independently
            // of the original result's canonical old vocabulary and attribution.
            crate::interpreter_recovery::validate_capabilities(&original, &capabilities)?;
            let (task, mutation) = self.prepare_interpreter_mutation(&original).await?;
            (task, Some(mutation), capabilities)
        };
        self.node_server
            .with_interpreter_sessions(
                origin.node_id(),
                origin.node_id(),
                self.node_freshness_seconds(),
                |current, _| {
                    ensure!(
                        current.session_id() == origin.session_id()
                            && current.runtime_id() == origin.runtime_id(),
                        "Interpreter recovery origin session changed"
                    );
                    self.config.edit_for_restart(|saved| {
                        let policy = eefn::authentication::AuthenticationPolicy::parse(
                            saved.pointer("/node/authentication"),
                        )?;
                        let issuer = crate::interpreter_policy::current_issuer(
                            self.node_server.authentication_policy(),
                            &policy,
                        )?;
                        ensure!(
                            issuer.as_deref() == Some(current.issuer_id()),
                            "Interpreter recovery authentication changed"
                        );
                        let result = self.engine.commit_interpreter_recovery(
                            command,
                            current.node_id(),
                            &original,
                            &capabilities,
                            |plan| {
                                mutation
                                    .context("Recovery source decision changed")?
                                    .apply_recovered(plan, &task, &original)
                            },
                        )?;
                        Ok((result, false))
                    })
                },
            )
            .await
    }
}
