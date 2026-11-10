# Plain interpreter command guidance

This unreleased output follow-up awaits hosted validation. It changes the
renderer over an already confirmed command receipt; it does not request data,
run a model, edit the journal, approve work or implement result reconciliation.

Plain `eefn requests interpret-network` output identifies the interpreter node
and current saved request status. A successful review with an unanswered question
shows that exact saved question and an answer command with its current revision.
The answer command uses the node's applied workflow policy. Answered questions
are not presented as unanswered, and an existing completed/approved history is
not described as newly awaiting approval.

An actual plan awaiting approval gets a read-only `requests plan` command.
Uncertain outcomes get inspection/stop guidance without an answer or continuation
command. A final service attempt whose result was not applied is distinguished
from one still unconfirmed; neither is automatically retried or replaced. These
messages do not prove that model computation stopped or recover a lost result.

Question lines remain prefixed or indented as question content, and terminal control
characters are removed from that display. The renderer does not invent a question,
dump the input text, or print model-provided content as a trusted next command.
IDs and revisions in generated guidance come from the validated saved view.
Detailed JSON continues to carry its existing private source/review data.

The existing authenticated node-pair regressions now render actual initial,
method-question, concrete-plan and interrupted replies. They require the saved
question/revision, read-only plan guidance, interpreter identity and no suggested
continuation during uncertainty. No test count, model provider, command schema,
permission, execution rule, dependency or background worker changes.
