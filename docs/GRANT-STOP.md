# Exact grant stop requests (unreleased after alpha.6)

The existing origin-node command now drives the backend stop path:

    eefn jobs stop JOB_ID
    eefn jobs get JOB_ID

The first command acknowledges the request, not that every effect has stopped.
Plain output explains this distinction and gives the inspection command. JSON
clients retain the same command/report path. The coordinator-owner jobs stop API
uses the same TaskEngine operation. No browser/controller, port knowledge, JSON
editing, LM, permission mutation or remote-management approval is needed to stop
the exact work already authorized by that pinned coordinator. Existing job-origin
ownership checks still apply; an origin cannot stop another origin's job.

## Durable intent and dispatch

TaskEngine saves stop_requested_at_ms in the existing execution record before
waking active backend drivers. A watch notification is only a wakeup, never
authority; drivers re-read the retained intent. No repeated status polling or UI
timer drives cancellation. Unissued work remains unissued. For already-issued
work, each exact target has one retained send-attempt checkpoint before network
I/O. Both grants of a peer pair are contacted concurrently so an unavailable
source does not delay its destination. Targets cannot change node, issuer,
session, grant digest or attempt. Intent/delivery evidence cannot be erased,
retimed, retargeted or replaced by ordinary journal edits.

Control messages carry only the bounded identity/digest descriptor. They never
resend original prompts, program arguments, model input or media. The digest
still binds the entire original grant, including those omitted private fields.

The separately advertised grant_stop schema v1 promises durable_intent_only.
The existing execution_grants shape stays unchanged. Older coordinators can
ignore the separate registration field; absent/unsupported node support is an
unconfirmed stop, never an ordinary-RPC, permission, restart or update fallback.
Actual stop delivery additionally requires the original pinned credential-TLS
session; knowing a grant ID, address, process name or PID is not authority.

## Admission races and node persistence

The node records exact stop identity in an independently versioned adjunct of
its existing grant journal. It does not create another job store or runner.
The stop must fit the node's existing receipt/page budget, including outstanding
result/progress reservations. Corrupt/incomplete/future adjunct metadata fails
closed without repair or deletion, including unversioned stop evidence before
any root journal initialization can write.

If the original grant was not yet consumed, stop atomically consumes that
authority as metadata-only unknown evidence before admission can win. This is
not fabricated execution acceptance, completion or proof that nothing happened
elsewhere. The root v2 receipt shape remains readable and prevents replay even
for readers that do not understand the adjunct. A changed grant ID/session cannot
reuse the stopped logical attempt. No output is invented for this case.

For already-consumed work, a scope-bound watch is signalled after persistence.
Claim/watch registration and stop signalling share the journal lock, preventing
a claim-then-subscribe race from losing the request. The common executor drops
its operation before final journal I/O and retains unknown effects with
grant_stop_requested_effects_unknown. Existing native/Python owned-tree cleanup,
peer socket lifetime and finite-pipeline cancellation apply. Repeated identical
node stops reuse the original timestamp and never execute work or alter a
completed result. Registry entries retire with their accepted grant.

## What a receipt proves

Per-grant stop receipts prove retained stop intent for an exact scope. They
always say execution_stopped:false and external_effects_stopped:false. A signal
does not prove the future was still running, a child exited or a speaker/device
buffer drained. Job tracking can become interrupted/unknown even when observed
native processes exited. Finished actions are not undone. Unknown grants never
resume, replay, migrate or acquire a replacement lease automatically.

Lost, corrupted, unsupported or changed-session replies remain outcome_unknown.
The saved send attempt is not automatically retransmitted, retargeted or rearmed
by restart. Inspect jobs and original grant receipt/result evidence. Missing or
deleted history is not proof that a stop/work request was never accepted, and a
different unreplicated coordinator database cannot supply global history.

This command currently targets actively tracked work. An already-interrupted
unknown job remains an inspection/recovery case, not permission to resume work
or stop a replacement session. Restart never silently rearms a saved delivery.

This is not a hard real-time or universal physical-stop guarantee. A synchronous
operation blocked inside its poll, blocked/full journal storage, outside services,
already-issued OS/network effects, continuous media and virtual-device buffers
still need separate lifecycle/resource acceptance. Late blocking stop callbacks
retain their control permit and may finish after their waiter disappears, but
cannot start work. Remote-management permission is neither granted nor bypassed:
this control reduces only the original grant's authority.

## Verification

Focused tests cover pre-admission durable consumption, claim/watch races,
idempotence, completed-output preservation, exact scope/receipt rejection,
quotas/corrupt evidence, no replay and native parent/child cleanup. Coordinator
tests cover immutable stop records, offline one-attempt behavior and refusal to
poll new dispatch after a retained stop. Public native and paired synthetic audio
tests pass on two authenticated logical nodes, alongside six existing public
regression suites. Both workspace/all-target feature modes pass 873 tests plus
one ignored each. Final headless apps/helpers build and match evaluated hashes;
fixture processes are terminal. Exact evidence is recorded in [status](STATUS.md).
Physical two-PC, broad stop-proof/resource, bootstrap,
installer/signing/Defender and stable v0.4.0 acceptance remain open.
