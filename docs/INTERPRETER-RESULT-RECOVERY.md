# Explicit retained interpreter result recovery

This unreleased backend and command work passes integrated hosted/source
acceptance at `f3f9ffb` in
[run 42857675899](https://github.com/eepyfishy/eef/actions/runs/42857675899):
all four jobs, 1,755 Windows/headless Rust tests, one ignored across 17 groups,
37 native suites and existing extra gates. The later plain-output follow-up
`33c9037` passes [run 42859748569](https://github.com/eepyfishy/eef/actions/runs/42859748569)
with the same counts. Active-inference inspection `bee2806` passes
[run 42862031369](https://github.com/eepyfishy/eef/actions/runs/42862031369)
with 1,757 Windows/headless Rust tests and the same other gates. Both have
independent source/log acceptance. Journal snapshot source `6569938` passes
[run 42932791140](https://github.com/eepyfishy/eef/actions/runs/42932791140):
all four jobs, 1,758 Windows/headless Rust tests, one ignored across 17 groups,
37 native suites and existing extra gates, with exact source/raw-log review.
Bounded-reader source `474500b` passes
[run 42933959948](https://github.com/eepyfishy/eef/actions/runs/42933959948):
all four jobs, 1,760 Windows/headless Rust tests and the same remaining gates,
with exact source/raw-log review. Retirement source `7f1d052` passes
[run 42935544215](https://github.com/eepyfishy/eef/actions/runs/42935544215):
all four jobs, 1,764 Windows/headless Rust tests and the same remaining gates,
with exact source/raw-log review. Physical and stable-release acceptance remain
separate.
It extends the existing journal
and review/compiler services without adding dependencies or background workers.

The first candidate `de3036f` failed hosted compilation: this module accessed the
runtime's private discovery field. A read-only runtime accessor now supplies the
same applied freshness value without exposing the field. The tests and source
gates are unchanged; a fresh complete hosted run is required.

The next candidate `5fb1298` compiled production code but failed compiling the
new tests because their `PlanStatus` import was missing. That import is now
explicit; no test assertions or production behavior change for this correction.

Candidate `990847c` passed all seven recovery regressions and three complete CI
jobs. Windows/headless run `42816124759` failed the existing native camera
workload completion case: it retained five events but returned an unconfirmed
source outcome. That failed run remains retained. Recovery command work does
not establish a cause or fix for this separate camera failure, and the candidate
has not passed full acceptance.

## Commands and inspection

The development node command/API service now exposes:

```text
eefn requests interpreter-results <request-id>
eefn requests recover-interpretation <request-id> --revision <current-revision> --grant-id <attempt-id>
```

Inspection reads the latest 48 origin-owned interpreter attempt headers and the
source revision/status. It does not start inference or edit the request, including
when no attempt exists. Headers contain attribution and completion/application
status, without prompts, grant bodies or credentials. A completed result is not
a promise that recovery is currently eligible. Use ordinary request/job inspection
for the resulting review, plan and stored recovery history.

Recovery is an explicit source-edit decision. The CLI generates and prints a new
operation ID unless `--operation-id` is supplied. Preserve the returned decision,
revision and attempt references after an uncertain response. Inspect before
retrying, and use the same `--operation-id` and exact original command to inspect
an already applied decision; do not generate replacement work automatically.
The separate `requests recover-result` command concerns executed-tool receipts,
not interpreter data.

Both commands use the running node's discovered local endpoint and runtime
fence, then the existing authenticated submission channel. Recovery mutations
share input admission. Read-only inspection remains available while that slot
is held by inference, subject to the existing eight-entry mailbox, eight pending
replies and bounded response wait; it creates no new queue or permission.
The coordinator checks actual gateway session, node runtime and source ownership.
The local HTTP adapter remains usable headlessly and enforces its existing
same-origin policy with an 8 KiB request limit. There is no new worker, model,
dashboard controller or permission shortcut. Plain output prints validated
references/status and never turns model text into a suggested command.

The plain-output follow-up distinguishes busy nodes, unavailable or changed
runtimes, unsent submissions and coordinator refusal using existing closed
error codes. Unconfirmed replies include only read-only inspection commands,
never a suggested replacement recovery decision. Empty attempt history is
explicit, and repeating an applied decision is described as a read of stored
history. Raw error text, model output and private settings are not printed by
these additions. Existing real node-pair/runtime tests assert that wording;
the plain-output checkpoint above passes complete hosted/source acceptance.

The inspection admission follow-up adds an authenticated private-node regression
with a model request held open after durable acceptance. It requires readable
in-flight headers, unchanged source and inference ledger, a busy refusal for
concurrent recovery mutation, and exactly one model call through completion.
A node-service case separately checks inspection while mutation admission is
held. The existing busy/retirement case now explicitly targets recovery mutation;
its refusal, runtime and late-reply assertions remain. The active-inference
inspection checkpoint above passes complete hosted/source acceptance.

The journal snapshot follow-up reads the source revision and its origin-owned
attempts under one existing journal lock. Separate reads could combine an older
request with an attempt created after an owner answer or subsequent planning.
Inspection still changes no source or allowance and creates no execution
authority. Store coverage includes that revision transition, foreign-origin
refusal and damaged evidence; the authenticated command regression also inspects
both recovered stages after compilation. This snapshot checkpoint passes full
hosted/source acceptance above; it does not establish acceptance of later code.

The bounded-reader follow-up serves this snapshot through the existing native
journal reader instead of waiting for SQLite on an async runtime thread. It shares
the existing four read slots and one lazy reader, with a five-second inspection
deadline spanning origin lookup and the final session check. A timeout or caller
disconnect does not release a slot or journal still held by an actual read.
Cancelled or expired reads recheck their waiter after acquiring the journal lock
and do not start a late snapshot. No synchronous fallback, repeated lookup,
replacement operation, new queue or permission is added. Read-only snapshots
cannot recover results or authorize work. Recovery mutations keep their existing
separate path and limits. This checkpoint and the later retirement follow-up
have separate full hosted/source acceptance above.

The subsequent [retirement follow-up](INTERPRETER-RETIREMENT.md) makes source
retirement persistence asynchronous after in-memory ownership ends. Inspection
can briefly retain the old claimed state while that checkpoint is queued. This
is not a live continuation or recovery authorization; explicit recovery still
requires all original source, revision, evidence and current-session checks.

A completed network interpretation can be retained even when its source update
did not finish. The old continuation remains retired. A fresh authenticated
origin request may explicitly apply the already saved semantic data; it cannot
restart inference, renew a grant, refund an allowance, or approve/execute tools.

The request contains only the source ID, exact current revision, new operation ID
and original attempt ID. The coordinator obtains all evidence from its journal.
It requires an exact final successful canonical result, no unresolved work or
stop intent, the original origin/context, and an idle unchanged unplanned review.
Initial input additionally requires the exact original claim in the unknown
state. Ordinary claim/promotion commands cannot take this recovery transition.

Current authentication and the originating node's live session/runtime are
checked through the save. Current disclosed semantic capabilities are validated
before preparing the update. Concrete methods use the ordinary deterministic
compiler and current available bindings, then await separate approval. The old
provider may be offline and its inference permission revoked: using retained
data is a new source-edit decision, not authorization to use that provider.

Source changes and a compact attribution receipt share one existing job update
and quota check. A failed write or full journal saves neither. The inference
attempt, its original attribution and all charged allowances remain unchanged.
The receipt preserves original operation/model/runtime references through the
bound evidence and separately records the new recovery operation and actor.
Each job retains at most 48 receipts; original attempt evidence is not copied
into each receipt. Ordinary edits cannot erase or replace this history. Reads
validate it against the original retained evidence and actual applied result.

Repeating the exact recovery decision is read-only, including after subsequent
planning. Conflicting operation reuse or a second decision for the same attempt
is rejected. Uncertain, stopped, failed or mismatched inference is not recoverable
by this path. This is separate from receipt inspection for executed tool work.

New regressions cover initial interpretation, clarification, method questions
and concrete plans; provider disconnection and inference-policy revocation;
read-only replay and later compilation; active, cancelled, stale and foreign
sources; ordinary-transition refusal; SQLite update failure; quota exhaustion;
and tampered or erased evidence. Tests use temporary journals, authenticated
private nodes and a mock model. Expiring a test continuation does not establish
physical wall-clock lease-expiry or restart acceptance.

New command regressions cover inspection/application through an authenticated
private node pair for every stage, unchanged source/allowance accounting on
inspection, no repeated inference, foreign/stale/session/revision refusals,
strict schemas and replies, shared admission and retirement with late-reply
rejection. CLI parsing and native command/API cases cover model-free inspection,
missing evidence, source preservation, runtime/node binding and HTTP refusal.
The integrated checkpoint above passes these command regressions.

The restart follow-up adds a real temporary-journal/coordinator restart and
credential/TLS origin reconnect. It checks that pending completed evidence and
an earlier recovery receipt survive unchanged, old runtime requests are refused,
and the new model-free origin can explicitly apply the pending result or read
its previous decision while the provider remains offline. The original claim's
runtime attribution and charged inference ledger stay unchanged. This is hosted
fixture coverage passed at the integrated checkpoint above; it does not
establish physical deployment acceptance.

The first restart candidate `191288a` failed test compilation in run
`42855147633`: the fixture redundantly unwrapped a `NodeClient` builder result
that was already a client. Removing that extra call changes no assertions or
production behavior; the failed source remains retained and fresh complete
hosted acceptance is required.

The next fixture candidate `56f5baf`, run `42855839190`, failed before reopening
the runtime: the fixture parsed coordinator settings as JSON even though the
normal save service writes YAML. It now obtains authentication and runtime
settings from the existing `Config` loader. Assertions and production code are
unchanged; this corrects test setup, not an observed product restart failure.

Remaining work includes physical multi-PC tests and investigation of the retained
camera failure.
Real-model/onboarding, continuous workloads, strong
stop, MSI/signing/Defender and stable-release gates remain open.
