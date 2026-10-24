# Bounded node session output (unreleased after alpha.6)

A later [execution/reply ownership increment](REPLY-OWNERSHIP.md) addresses a
reproduced completed-reply capacity mechanism with separately bounded reply
ownership. Its combined validation is pending. The dated initial evidence below
does not establish acceptance of that subsequent increment.

The existing node-to-coordinator encrypted connection now has one shared output
guard. This does not add a transport, retry a command, change authentication or
replace the reviewed execution boundary.

## Contract

Every post-probe node frame uses a five-second cooperative deadline, including
waiting for the writer lock, encoding, writing and flushing. This covers sign-in,
registration, heartbeat, model registration refresh, input submission, ordinary
control replies, grant lease/ACK/final replies and receipt/result inspection.
The existing reachability probe remains bounded on its separately owned stream.

Failure, timeout, or cancellation after acquiring the writer retires that session
before releasing the lock. Waiting senders recheck retirement after acquiring
the lock; they cannot append another frame to a possibly incomplete encrypted
line. A queue-wait timeout also retires the connection conservatively. The signal
retains failure even if its watcher has disappeared. Cancellation while merely
waiting for the lock has written no frame.

The node supervisor reconnects using the existing probe/authentication and
registration flow. This is connection recovery, not operation recovery: it does
not resend outstanding submissions, execute grants again, retry configuration
changes, or retransmit old responses. Local command/API service belongs to the
node runtime and remains independently usable.

Outgoing encrypted lines, including the newline, must obey the receiver's existing
8 MiB bound. Oversized messages fail before any of their bytes are written. Encoding
still allocates plaintext/ciphertext before this check; this is not an allocation
or total RAM budget. Existing framing, crypto and wire version are unchanged.

## Uncertainty and limits

A successful frame write is not recipient acceptance. A failed reply can follow
an actual saved configuration change or completed effect. Retirement cannot prove
rollback, not-started, native process termination, backend cancellation or that
all effects have stopped. Reviewed operations retain their existing journal and
explicit receipt/result recovery contracts. No automatic replay or fallback is
authorized by a broken connection.

The deadline is cooperative, not a hard real-time guarantee: synchronous encoding,
configuration I/O and native/backend activity are not preempted by it. Request
capacity and reserved controls were unchanged at the initial checkpoint. It did not diagnose or fix the
intermittent `node_busy`/`not_started` reply recorded in the CPU settings checkpoint.

## Evidence (2026-10-11)

The encrypted real-TCP unit
`client::tests::unread_peer_retires_session_without_busy_rejection_or_replaying_work`
first fails against the previous ordinary-reply path: an unread peer keeps the
session alive past the test's twelve-second deadline. It issues 64 metadata reads
below work capacity 128, without busy rejection, inference or configuration
mutation. After the fix it retires in about 5.4 seconds and preserves the exact
saved file. Four controlled writer tests cover completed frames, partial output,
queue-wait timeout, I/O error with no watchers, cancellation and refusing later
frames; a protocol test covers the outgoing encoded-size bound.

`node tools/test-node-output.mjs` runs the actual API-only node executable against
a test-only legacy encrypted loopback peer. It owner-authorizes one saved CPU
setting, stops draining output and sends bounded read-only inspection requests.
The first successful fixture (`node-output-2kR8eg`) observes reconnection in
6,332 ms, the same node runtime ID, no busy rejection, usable local CLI commands,
the unchanged saved setting, and only probe/auth/register/heartbeat traffic on
the new session. Its inactive selections use a tiny test-only non-model file;
there is no model loading, inference, download or contact with personal Ollama.
It does not establish the outcome of an unobserved remote reply.

An earlier fixture (`node-output-YmpDBE`) fails before the intended save because
its inactive model selections have invalid file/port configuration. It is not
counted as a pass. The corrected fixture supplies existing paths and distinct
saved ports; no production validation, timeout or capacity rule is relaxed.

These are isolated loopback checks. Pinned TLS is covered separately by existing
grant/credential regressions, not by the output fault fixture. Physical two-PC
output-fault acceptance, full termination/resource guarantees, durable automation
and stable release acceptance remain open. Installed nodes, UI assets, published
installers and update feeds are unchanged.
