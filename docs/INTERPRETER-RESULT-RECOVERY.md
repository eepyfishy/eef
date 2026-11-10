# Explicit retained interpreter result recovery

This is unreleased backend work awaiting hosted validation and public adapters.
There is no user-facing recovery command yet. It extends the existing journal
and review/compiler services without adding dependencies or background workers.

The first candidate `de3036f` failed hosted compilation: this module accessed the
runtime's private discovery field. A read-only runtime accessor now supplies the
same applied freshness value without exposing the field. The tests and source
gates are unchanged; a fresh complete hosted run is required.

The next candidate `5fb1298` compiled production code but failed compiling the
new tests because their `PlanStatus` import was missing. That import is now
explicit; no test assertions or production behavior change for this correction.

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

Remaining work includes read-only attempt discovery and explicit origin-scoped
command/API adapters, command validation, runtime retirement, restart and
physical multi-PC tests. Real-model/onboarding, continuous workloads, strong
stop, MSI/signing/Defender and stable-release gates remain open.
