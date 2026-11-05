# Nonblocking workload issuer retirement

The original coordinator workload issuer now closes its authority before queuing
its historical uncertainty checkpoint. Dropping the token, rejecting its local
liveness check, or failing its completion write no longer performs that fallback
SQLite write on the caller's thread. This extends the existing `JobStore`; it
does not add a second scheduler, database, public launch route or source factory.

## Reserve before issuance

Every new internal issuance reserves one opaque reporting slot before reading or
mutating the job. A full reporting allowance refuses issuance before a definition,
lease or offer can be returned. Each slot belongs to its exact store and cannot
be cloned, deserialized or substituted from another store.

The capacity is no larger than the owner's existing job count or the number of
completion reservations that fit the existing byte quota, with an internal ceiling
of 64. This bounds active issuers plus their queued/running final checkpoints.
It does not increase the existing job/storage quotas, source limits or node
concurrency. No public configuration or workload execution command is added.

## Retire before persistence

Retirement first latches the token retired and stops its original monotonic
window. Clock correction, a late ACK, an identical completion report, removing
a storage fault or dropping the token again cannot restore that issuer or cause
a second checkpoint attempt. The original session and short lease ceilings are
unchanged. Node leases still expire independently; coordinator retirement is not
proof that a microphone, driver, process or external effect stopped immediately.

Only the exact bounded definition, reporting slot and existing journal enter one
lazy native worker. There is no source future, live issuer, renewal handle, model,
device permission or runtime handle in the queue. The ordinary strict uncertainty
edit revalidates the retained definition and allowed state transitions inside the
existing journal transaction. It never overwrites completed history or invents
an ACK, output, completed report or replacement attempt.

Capacity and journal ownership remain with the actual operation after its caller
or runtime disappears. The idle receiver retains no store; runtime destruction
does not join a blocked SQLite checkpoint. The existing retirement signal still
waits for actual database/file-lock ownership to end before replacement. Successful
completion releases its reserved slot after the completion write, even if the
caller keeps the terminal token for read-only history.

## Unconfirmed or failed checkpoints

Queued is not saved. Read-only job inspection can temporarily retain its earlier
issued/accepted state while uncertainty persistence is pending; that history
cannot reconstruct a live issuer. Process restart uses the existing interrupted-
job handling and does not replay a pending checkpoint or source.

A failed worker admission, slot mismatch, callback panic or unavailable final
checkpoint latches denial of new workload authority on that store handle. Other
live issuers observe the same reporting failure during their existing liveness
checks and retire. A later successful ordinary write or removal of a test fault
does not clear this latch. Historical reads remain possible. This is conservative
reporting denial, not proof that every failure was physical disk corruption.
Opening a fresh validated store is separate recovery; it cannot recreate tokens
or revive interrupted work. No automatic retry, journal repair or budget refund
is introduced.

## Verification and remaining integration

Five new shared regressions use synthetic issuance with actual temporary journals:
held storage while current-thread timers and retired-token refusals remain
responsive; capacity reserved before issuance and held through persistence; a
real failed SQL checkpoint with no retry or evidence rewrite; runtime destruction
with continuing journal ownership; and successful completion releasing capacity
without restoring execution authority. Existing drop/clock/completion tests now
wait for the actual checkpoint before inspecting its durable result. Tests start
no node, source, interpreter, model or media device and change no OS clock.

Fresh hosted compilation and complete acceptance are required. This change moves
retirement and failed-completion uncertainty persistence off the caller. Initial
issuance, liveness reads, ACK writes, normal completion writes and other
`AttemptJournal` integration still require broader asynchronous work. The
authenticated public continuous-workload transport, long-session/source behavior,
physical two-PC validation and stable release gates remain incomplete.
