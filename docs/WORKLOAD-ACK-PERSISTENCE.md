# Asynchronous workload consumption checkpoints

The existing internal workload issuer now awaits both its journal inspection and
consumed-ACK persistence without running SQLite on its async caller's thread.
The original probe/ACK deadline is capped by the original monotonic issuer deadline.
The acknowledgement is still required to match the exact consumed definition from
the pinned authenticated node session. No wire schema, permission or lease ceiling
changes, and no public workload launch route is added.

## Bounded ownership

The same `JobStore` owns a separate lazy native ACK queue using the existing bounded
journal-worker implementation. At most four ACK checkpoints are admitted, including
running work. The owner's job count and completion-headroom byte allowance can
reduce that capacity to zero. Capacity refusal never waits for a slot or queues a
retry. This is separate from the reporting slot reserved before workload issuance,
so an ACK write cannot consume the allowance needed to retire an issuer.

Each native operation owns its actual write slot and journal until it finishes.
The queue receives only the exact definition and accepted receipt; no original
issuer, renewal window, source, model or runtime handle enters native work. The
idle queue owns no journal. A completed callback releases its journal ownership
before waking the async caller. Runtime destruction does not join that native
write, and journal replacement still waits for the original store's actual
connection and file-lock ownership to end.

## Durable ACK before renewals

The original borrowed issuer retains its cancellation guard across the journal
read and ACK save. Dropping a polled future, losing write capacity, an unavailable
checkpoint or an elapsed deadline retires that token before queuing its one
uncertainty checkpoint. Keeping the token or removing a storage fault cannot
restore renewal authority.

Queued abandoned or expired writes refuse before editing. The worker checks again
after acquiring the journal and inside the guarded edit. The existing exact
running/issued definition and empty-ACK requirements remain. A SQLite commit that
has already started can finish after the caller stops waiting; such an accepted
receipt is historical evidence. Only the original caller can mark its original
window consumed, after durable success and another local liveness/deadline check.
If that caller cancelled, its issuer is retired and the retained uncertainty
checkpoint cannot replay a source or grant a new lease.

Identical already-saved acknowledgements are read-only. Invalid or conflicting
receipts cannot change the record or consume a lease sequence, and do not retire
an otherwise healthy issuer. A failed ACK save retires its issuer; successful
uncertainty persistence does not claim global database corruption. If that
uncertainty checkpoint also fails, the existing reporting-failure latch denies
new workload authority on the same handle.

## Verification and remaining work

Four shared regressions use actual temporary SQLite journals and synthetic issued
metadata. They hold an independent write transaction while reads remain available,
then exercise cancellation with a kept token and full write allowance, deadline
expiry before native completion, an SQL trigger rejecting ACK persistence, and
runtime destruction with retained journal ownership. They do not launch a source,
node, model or interpreter or change an OS policy. Existing lease, clock, receipt
and completion cases now await the same acknowledgement method.

Fresh complete hosted acceptance is required. Initial issuance and normal
completion persistence still need broader asynchronous integration. Public
authenticated continuous execution, long-session source behavior, physical
two-PC validation and stable release gates remain incomplete.

The first hosted candidate `79a505d0` did not compile its tests because the new
contention fixture declared `TaskEngine` instead of the constructor's existing
`Arc<TaskEngine>` return type. The corrected declaration retains the same fixture
ownership and all test assertions, with no production change. Failed run
`42041575687` is retained; it does not provide runtime acceptance for these cases.
