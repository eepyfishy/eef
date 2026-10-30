# Durable prepared-workload completion draft

The internal prepared microphone path now distinguishes confirmed finite source
completion from a generic operation result. It extends the existing grant journal
and final worker; it adds no second job database, dashboard controller, public
launch route, automatic replay or recovery lease. Policy remains opt-in and
startup never captures a source through this path.

## Required local evidence

Only the prepared path can pair the opaque owned-source outcome with its final
event-filter seal. The source outcome requires successful transport, validated
end/EOF, successful root exit and the exact owned-process exit witness. The
[lifetime handle coverage follow-up](PREPARED-MICROPHONE.md#lifetime-handle-coverage-follow-up)
requires complete unique held-handle coverage and signalled process handles;
empty job accounting alone cannot produce it. That correction still needs its
own hosted acceptance; the earlier failed tests remain failed.

The filter seal binds useful-event count, canonical event bytes and the last
required event, including a quiet source tail without fabricated events. It
requires every source observation through the returned summary, refuses failed
or mismatched streams and permanently retires after a sealing attempt. The
prepared runner alone combines it with the opaque source result; neither is a
serializable completion request.

The existing lease supervisor seals a local historical finish boundary after the
operation's final poll, its lease check and its active reporting check. Existing
stop and listening/permission generation supervision still surrounds the exact
source future. The boundary binds the entire contract digest and final validated
lease sequence. It cannot be deserialized, cloned, renewed or used to start work.
Retiring reporting in the final poll refuses the finish, even if the source
returned a value.

## Save and observation

The source future and reporting window retire before persistence. The final worker
receives bounded metadata, with the owned OS witness dropped before submission;
it receives no executable source, permission, live lease or renewal handle. The
same admission-reserved slot remains charged until the actual save ends, and the
caller waits at most three seconds. Generic consumed operations and all failed,
cancelled or unsealed sources retain the existing uncertainty path.

Under one immediate transaction the worker revalidates the exact consumed
definition, scope, adapter hash and finite source count/frame bounds. It audits
the full durable event prefix and compares the filter seal's count, byte total
and final event. Events may end before the final quiet observation; no summary
can replace a required missing transition. In-memory or durable stop intent
before completion commit retains uncertainty. An independent journal handle's
stop must be respected even without an in-memory notification.

The record describes a source that finished inside its captured approved window.
Historical persistence may finish after that window expires or after its waiter
is cancelled. This never extends source execution: all process and reporting
authority is already retired. A late committed record may be read as history;
it cannot acknowledge a timed-out waiter, renew, resume, replay or migrate work.
A stop racing after commit refuses the immediate success acknowledgement; readers
can inspect the already committed historical facts. No finite OS cleanup bound
or physical microphone/driver/external-effect stop guarantee is claimed.

## Existing journal and compatibility

The first successful completion atomically upgrades journal version four to five
and stores the bounded canonical completion in the existing `report` and
`report_sha256` columns. All retained ordinary/workload/stop evidence is validated
before the marker changes. Workload consumption without completion still upgrades
only to version four; ordinary-only journals remain version two. There are no new
tables. Older binaries refuse version five; do not overlap or downgrade runtimes.

The record fits the already reserved report headroom without reducing the approved
event budget or increasing owner storage limits. It binds the consumed digest,
adapter, source counts, event count/bytes/prefix hash, final lease sequence and
recording time. `owned_process_exit_observed` is true; `physical_effects_stopped`
remains false. Completed receipt schema three exposes this historical record;
accepted/unknown receipts retain schema two with no completion field. All forms
keep execution and restart authorization false, and ordinary output readers refuse
workload rows.

Reopen and read validate bounded SQL projections before materializing reports,
canonical encoding, checksums, metadata and the exact retained event prefix.
Missing, oversized, corrupted or downgraded evidence is retained without repair.
Transaction failure rolls back the version, report and completed state. Storage
failure denies new workload admissions on that handle; there is no automatic retry.

## Verification boundary

Eight added real-journal tests use explicitly synthetic source metadata to cover
reopen/legacy retention/replay refusal, a missing event, changed source/contract,
independent-handle stop, failed transaction rollback, blocked historical saves,
cancelled waiters and fourteen corruption/downgrade variants. Two filter and two
lease regressions cover final seals, quiet tails, final-poll retirement and the
difference between historical facts and live authority. The opt-in combined
native test now requires a completed record after the exact compiled Python source,
real held root/descendant exit and exact durable events. Its consent/cancellation
cases still require uncertainty. Hosted validation is pending; no local heavy
build, physical capture, installed update, model or release change was performed.

Authenticated public offer/probe/consumed-ACK/event/stop wiring, owner commands,
coordinator job/attempt integration, long-session transport and physical two-PC
acceptance remain required. This draft does not establish stable v0.4.0 readiness.
