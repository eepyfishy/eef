# Asynchronous workload lease inspection

The original internal workload issuer now reads its existing coordinator journal
through the bounded native reader before preparing a lease reply. The caller
supplies its original probe deadline; the earlier of that deadline and the
original issuer's monotonic deadline bounds admission and observation. No source,
issuer window or renewal authority enters the reader.

Local clock correlation, reporting health and retirement are checked before the
read and after the returned snapshot. The snapshot must retain the exact running
definition in its issued or accepted state. The reply is computed after that wait,
with no further await, using the original window's remaining budget. The old
sequence checks, separately authenticated identity inputs, consumed-ACK requirement
and session/lease ceilings remain in force. There is no pre-read reply to become
stale during a journal wait and no fresh observation allowance after the read.

Polling this future establishes a cancellation guard on the borrowed issuer.
Dropping the pending future closes that original issuer before queuing its one
reserved uncertainty checkpoint, even if the caller retains the token. A deadline,
unavailable read capacity, damaged snapshot or reporting failure also retires it.
An unpolled future has started no operation. A malformed, wrong-peer or repeated
probe on an otherwise healthy issuer cannot consume a sequence, change durable
history or manufacture an acknowledgement; it does not retire that healthy issuer.

Read capacity remains charged to actual native work after cancellation. Historical
uncertainty persistence owns its separate admission-reserved reporting slot until
it finishes. A successfully delivered read releases its journal ownership before
waking its caller. Neither queued reads nor delayed retirement checkpoints can
restore an issuer or replay a source. Retirement is not proof of physical stop;
nodes retain their independent lease expiry and stop enforcement.

Four new shared regressions exercise real temporary journals with synthetic
issuers: cancellation while retaining the token, both caller and original issuer
deadlines, remaining budget after a delayed read, and invalid peer/probe sequence
handling. Existing lease, completion and receipt-inspection cases now await the
same asynchronous method, with their durable-state assertions retained. They launch
no source, model or node and alter no OS clock.

Fresh hosted compilation and complete acceptance are required. Initial issuance,
ACK persistence, normal completion and other coordinator writes still require
broader asynchronous integration. The public authenticated continuous-workload
execution route, long-session source behavior, physical two-PC tests and stable
release acceptance remain incomplete.
