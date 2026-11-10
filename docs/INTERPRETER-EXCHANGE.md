# Core interpreter service exchange

This unreleased internal increment connects durable interpreter reservations to
the existing authenticated grant transport. Source `97fa922` passes all four
jobs in the [hosted run](https://github.com/eepyfishy/eef/actions/runs/42637734975)
and an independent source/log audit, including 1,711 Windows/headless Rust tests
and all 37 native suites.
This exchange checkpoint contains no ordinary user input or source promotion
wiring and adds no public inference command. The complete automatic broker
remains unfinished. The following
[source application increment](INTERPRETER-APPLICATION.md), accepted separately
at `d43f5b9`, consumes live completions through review and deterministic
compilation. It does not change the
acceptance scope of this exchange checkpoint.
The first hosted source failed compilation at an ignored timing fixture's
journal constructor. The next revision compiled but Windows exposed a saved
failure arriving before its RPC reply. Both failed runs are retained. The
accepted revision recognizes this exact failure checkpoint without recreating
source ownership, a successful completion or another inference continuation.

The caller must establish authenticated input ownership and reconstruct an exact
task from the retained source. A current opaque origin/provider selection and
policy pin are required for new work. Exact retained history is inspected before
peer, model or capacity lookup and never recreates a live continuation.

For new work, the exchange joins the shared active-job lifecycle and reserves
the exact provider through the existing capability scheduler before charging
an allowance or claiming source ownership. It then uses the
[allowance ledger](INTERPRETER-ALLOWANCES.md), exact pre-delivery fence and
`invoke_grant_observed` transport. The gateway's existing workers persist
acceptance and completion; fresh lease admission, node-local permission and
model checks remain mandatory. There is no ordinary-RPC inference fallback,
provider substitution, model download or second worker system.

While an exchange is active, existing stop and gateway events plus lightweight
configuration/source change notifications wake its observer. Notifications are
only hints: the backend rechecks current journal, authentication, disclosure,
source, model, permission revision and policy under their existing fences.
The only time-based wake-up is the original finite execution deadline; this
adds no idle polling loop. Policy changes, cancellation, changed settings or a
retired peer trigger the existing exact stop operation with a durable one-send
intent. Unknown replies do not imply shared computation stopped.

A successful reply must confirm the exact issued grant and match its durable
observation. It returns an opaque in-process completion whose original job and
capacity guards remain owned. Retained history cannot reconstruct this object.
Dropping an unconsumed completion retires initial ownership as unknown rather
than automatically applying or replaying a proposal. Failed or
malformed results cannot create a review, plan approval or tool execution.

Synthetic tests connect two logical private nodes, one model-free and one with a
mock Ollama service. They cover all three stage definitions and exact messages,
grant/result attribution, unchanged source review, no replay, lost continuations,
source/policy/settings/peer cancellation, invalid output and scheduler admission
before allowance consumption. This is not a physical model-quality test or
end-to-end public-command acceptance.

Remaining work includes consuming live completions through all three existing
source transitions with atomic policy/source fences, normal model-free input and
explicit commands, retained-result recovery, user-facing diagnostics, physical
two-PC/model acceptance and the other stable v0.4 release gates.
