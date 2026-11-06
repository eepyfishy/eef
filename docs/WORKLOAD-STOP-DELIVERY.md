# Durable workload stop delivery

Workload stop uses the existing jobs-stop intent and original engine ownership.
An active issued workload can retain `stopping` and its first stop timestamp;
revocation closes the in-memory lease-delivery gate before persistence. Dropping
the original issuer then records `interrupted`/`unknown`, preserving its exact
definition and consumed ACK. This is uncertainty about execution, not evidence
that a remote source or external effect stopped.

The coordinator driver has a typed stop operation for the original workload
definition. It first reserves one send attempt in the existing job journal, then
makes at most one network attempt on that exact issuer/session/node scope. It
never manufactures a schema-one execution grant. Existing credential/TLS,
advertised journal/stop support and original-session routing checks apply.
Reconnect, an uncertain reply or a repeated call cannot retarget or resend.

The attempt and optional node intent receipt use the existing bounded native
writer and quota. Native persistence owns only bounded metadata, its original
capacity slot and the actual journal lifetime. Cancelling an observer does not
release that capacity while storage still runs. Cancellation/deadline is checked
again after acquiring storage. A commit already underway can outlive observation;
its saved send reservation remains conservative and cannot authorize a retry.

Workload history adds an optional `stop` field. Absent fields retain schema-two
issuance and schema-three completion compatibility; a retained stop delivery uses
execution-record schema four. The scope, first send timestamp and confirmed
receipt cannot be erased or replaced. A node receipt confirms durable stop intent
only: `execution_stopped`, `external_effects_stopped`, `execution_authorized` and
`automatically_retried` must remain false. The command-facing job view exposes
workload state/stop records and reports unknown workload effects accurately.

Eight new regressions cover issued/accepted stop retirement, one exact send
reservation, compatible old history, forged and immutable receipts, cancellation
during actual journal contention, offline uncertainty, reopening without resume,
and typed routing/receipt validation through the server's session machinery. The
transport case uses an in-memory authenticated connection fixture, not a physical
node or a fresh credential/TLS handshake. No source runs in these new tests.

The owner start/stop driver is not yet connected to public workload execution.
Capability advertising, a complete credential/TLS source execution test, final
main acceptance and physical two-PC/release validation remain required. Earlier
main capture deadline failure remains unresolved. These backend changes do not
update installed apps, nodes, versions, feeds, permissions or releases.
