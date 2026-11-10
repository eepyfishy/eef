# Live interpreter results and saved requests

This is an internal development increment on top of the accepted
[private service exchange](INTERPRETER-EXCHANGE.md). Source `d43f5b9` passes all
four jobs in the [hosted run](https://github.com/eepyfishy/eef/actions/runs/42722639395)
and its independent source/log audit: 1,716 Windows/headless Rust tests, one
ignored across 17 unfiltered groups, all 37 native suites and existing extra
gates. No published installer, command, update feed or
normal-input behavior changes in this increment.

The backend can consume an in-process successful completion once, using the
existing transitions for all three interpreter stages:

- Initial input becomes a durable review with its original text and immutable
  origin context. The remote model and source claim remain attributable.
- Clarification appends a source-bound proposal, preserving previous questions,
  answers and the first proposal.
- Method preparation either saves a clarification question or uses the ordinary
  deterministic compiler to save a concrete plan awaiting separate approval.

The completion cannot be cloned or deserialized. Retained allowance history,
including a successful result after its live continuation was lost, cannot
reconstruct it. This is not an explicit historical-result recovery mechanism.

Before saving, the backend rebuilds the typed untrusted candidate from the
confirmed semantic proposal. It pins both current gateway registrations while
holding the saved/applied authentication fence, engine shutdown gate and journal
gate through the bounded synchronous source write. The journal checks the exact
successful saved report, source and claim, current policy, runtime, participants,
model, permission revision and original deadline. A reconnect, changed source,
revocation, changed authentication or expired continuation refuses the write.
No lock spans model inference or asynchronous compilation.

The same journal update retains existing quotas, immutable-source checks,
planning validation and automation lineage. An edit failure does not partially
save its source mutation. Dropping an unconsumed completion uses the existing
ownership retirement; it never repeats inference or invents execution authority.

The ordinary draft-revision, method-question and compiler paths share their
transition helpers with this path. The coordinator does not load a model, add a
worker or poll while idle. There are no new dependencies or wire messages.

Focused hosted regressions cover all stages, question and concrete method
outcomes, preserved history and context, separate approval, historical reuse,
late cancellation/revocation/settings/disconnect/deadline changes, altered
completion data, shutdown, failed source edits, and registration lock lifetime.
These use private TLS node clients and a synthetic model service. They do not
prove real-model quality, physical two-PC operation or installer acceptance.

Public authenticated broker commands, automatic model-free normal input,
explicit uncertain-result recovery and stop usability remain required before
claiming the network interpreter is complete. The full stable-v0.4 roadmap,
physical tests and release gates remain open.
