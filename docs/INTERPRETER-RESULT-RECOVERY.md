# Explicit retained interpreter result recovery

This is unreleased backend and command work awaiting complete hosted acceptance.
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

Both commands use the running node's discovered local endpoint, shared input
admission and runtime fence, then the existing authenticated submission channel.
The coordinator checks actual gateway session, node runtime and source ownership.
The local HTTP adapter remains usable headlessly and enforces its existing
same-origin policy with an 8 KiB request limit. There is no new worker, model,
dashboard controller or permission shortcut. Plain output prints validated
references/status and never turns model text into a suggested command.

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
These additions require fresh hosted validation.

Remaining work includes complete command acceptance, restart and physical
multi-PC tests, and investigation of the retained camera failure.
Real-model/onboarding, continuous workloads, strong
stop, MSI/signing/Defender and stable-release gates remain open.
