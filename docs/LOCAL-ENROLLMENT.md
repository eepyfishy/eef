# Explicit same-PC private enrollment (unreleased)

With updated EEF and EEFN running under the same user on the same PC:

```text
eefn setup --local
eefn setup --enroll-local --restart-coordinator
eefn setup
```

The first command is read-only. Enrollment is an explicit owner mutation; the
extra restart flag authorizes a coordinator **app** restart when saved trust
needs applying. This interrupts coordinator work and applies **all** its pending
settings, not just this node's credential. Enrollment also requests a node app
restart to apply pairing, including other pending node settings. No Windows
restart, elevation, feature grant, model selection/download or workload follows.

No user-supplied node ID, endpoint/port, invitation argument, intermediate secret
file, dashboard, browser controller or model is needed. The CLI obtains a fresh
local plan, then calls `POST /api/commands/setup/enroll` on the node's actual local
owner API. The shared node service owns the workflow. Strict request bodies are
capped at 4 KiB and bind node, node runtime, correlation, expected coordinator
runtime/fingerprint and explicit restart consent. Cross-site requests are refused.

For an already provisioned and applied coordinator, `eefn setup --enroll-local`
can reuse the same credential without a coordinator restart. If new/pending
authentication requires restart, omitting consent refuses before provisioning.
No prompt silently assumes consent; a future UI can present these same choices.

## Workflow and boundaries

The backend permits one local enrollment per running node at a time. It snapshots
that node's connection settings privately, validates the separate secret-free
advertisement and fresh read-only coordinator state, and refuses a stale plan,
saved revocation, incomplete old TLS migration or an independently paired node
whose pinned coordinator certificate differs. It never rotates a credential,
restores a revocation, trusts a different coordinator automatically, enables a
paused connection, starts a second node runtime or falls back to legacy pairing.
Pairing replaces the outgoing endpoint list with the selected local coordinator
and disables automatic legacy local pairing; it is explicit enrollment to that
coordinator, not a multi-coordinator priority/failover configuration command.
If saved settings have lost an identity or TLS trust that is still applied in
the running coordinator, enrollment also refuses implicit regeneration. An
explicit owner repair/migration is required; deleting saved state must not turn
an applied revocation or existing key into a fresh identity.

The coordinator's owner-only `POST /api/commands/setup/enrollment` prepares or
reuses the exact node credential under the existing saved-config lock. Expected
runtime and saved certificate fingerprint must match. First provisioning can
generate persistent TLS identity only with explicit restart consent. The reply
contains a reusable **private** invitation, never printed by this enrollment
adapter or saved in a new invitation file. Existing configurations/backups still
persist secrets; this is not secret erasure or protected-storage attestation.

If restart is needed, `POST /api/commands/setup/restart` additionally requires a
digest of the exact prepared saved authentication policy. A changed policy or
runtime refuses that restart. `GET /api/commands/setup/enrollment` supplies only
runtime, saved/applied authentication digests and the public saved fingerprint.
After the restart acknowledgement, bounded read-only polling requires a new
coordinator runtime and the exact prepared authentication policy both saved and
applied before node pairing. Without a requested restart, the same runtime and
matching applied state are required. These hashes are CAS observations, not
credentials, monotonic revisions, durable receipts, election/quorum evidence or
protection from an ABA content rollback. No cluster safety is claimed.

Node pairing rechecks runtime, stable ID and the exact snapshotted connection
fields under the node config lock. A concurrent pause/trust/endpoint change is
not overwritten. Unrelated settings, including newer feature permissions, merge
from the latest file and remain unchanged by enrollment. Credentials remain
independent per node. A paused node stays paused until explicit `connection resume`.

## Partial outcomes, time bounds and restart observation

This is not an atomic transaction across the two apps. Coordinator preparation
can save trust before a later phase fails. A restart request is not completed
restart; authentication readback is not node registration or successful work.
Successful `node_local_enrollment` reports confirm saved node pairing and a node
restart request, and keep `registration_verified: false`. Inspect `eefn setup`
or `eefn connection show` afterward; a future client can poll those same backend
observations. Missing/default interpreter quality and first-run model bootstrap
remain separate gates even after private registration succeeds.

Reports distinguish attempted/confirmed preparation, known/unknown coordinator
changes and restart requests, applied authentication readback, and known/unknown
node save/restart. Unknown writes are null, not rollback claims. A lost node-owner
reply cannot confirm which phase was reached; inspect current state before
another explicit command. Neither backend nor adapter automatically retries any
prepare, restart or pairing write. Correlation is not durable deduplication;
another explicit enrollment can request another node restart even with the same
credential. Owner changes after a check can still invalidate later observations.

Coordinator HTTP calls use loopback metadata, no proxy/redirect, and a 5-second
request bound. Application readback polling is limited to 45 seconds, and the
node workflow has a 60-second async deadline. The CLI's owner-command HTTP deadline
is 75 seconds; its preceding read-only setup/runtime checks have separate bounds.
Configuration writes are synchronous existing operations, not a hard real-time
filesystem guarantee. Timeout/cancellation does not undo saved trust, stop an
already requested restart or prove that external side effects ceased.

This covers explicit same-PC private enrollment, not a complete graphical
installer/onboarding flow, remote enrollment, default model bootstrap, physical
two-PC acceptance, continuous media, peer/mesh transport, cluster leadership,
MSI/update/release acceptance or Defender/code-signing validation. It neither
updates installed apps nor publishes a release.

Owner HTTP controls should remain bound to loopback; public node connectivity
uses the separate authenticated node gateway. Mapping a wildcard owner listener
to loopback in discovery is not certification of network-exposed owner access or
OS-user isolation. Strong owner-API peer-boundary acceptance remains separate
release work; this checkpoint does not claim remote owner authentication.
The subsequent [owner socket-boundary increment](OWNER-API-BOUNDARY.md) requires
actual loopback TCP peers even on wildcard listeners. It does not add OS-user or
remote-proxy authentication.

## Verification checkpoint

Final workspace/all-target tests pass with default features and separately with
`--no-default-features`: **693 passed, one ignored in each configuration** (EEF
library 243, EEF CLI 17, installer 6, node library 412, node CLI 15). Fresh default
binaries/examples build, and formatting, diff, JavaScript syntax and backend/UI
dependency source checks pass. Early restart-unit fixture errors were corrected:
saved settings require a nonzero port, and the runtime must reload the saved
config instead of using the older startup snapshot. The final suites use those
corrected fixtures, including the missing-applied-trust regression guard.
Final formatter cleanup after the suite runs changed whitespace/layout only,
including that guard and its assertions; it introduced no logic changes.

`tools/test-local-enrollment.mjs` passes against the fresh executables, with final
evidence `.validation/local-enrollment-mM8Z9x/results.json`. Two logical nodes on
one PC start without private trust and enroll through the actual public commands,
without user-supplied endpoint/ID arguments or invitation files. It checks:

- Restart-consent refusal leaves both configs unchanged before preparation.
- Explicit enrollment restarts the coordinator, reads back exact applied trust,
  saves node pairing and preserves identity, feature permissions and models.
- A paused node stays paused, then registers only after explicit resume; both
  nodes register using independent credentials and a persistent TLS identity.
- Reusing an applied credential does not rotate it or restart the coordinator.
- Saved revocation is not restored; cross-site/extra-authority requests and a
  mismatched authentication restart digest are refused.
- A loopback fixture proxy drops the node-owner reply after actual success. The
  CLI reports unknown/null outcomes, does not echo credentials and issues exactly
  one enrollment request. Actual node restart/reconnection is observed separately,
  not inferred from the lost reply. Only fixture-injected endpoint metadata is
  restored by the test; application failures do not undo prior writes.

Earlier `.validation/local-enrollment-fXrW8F/results.json` also passes without the
subsequent dropped-reply case. Existing credential and setup regressions pass in
`.validation/node-credentials-gXOtOi/results.json` and
`.validation/setup-check-ryg7bj/results.json`. These are local, headless checks,
not physical two-PC, real-model, installer, signing or complete release evidence.
