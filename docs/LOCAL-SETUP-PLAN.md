# Same-PC private setup planning (unreleased)

Run `eefn setup --local` against a running development node. Plain text explains
the observation; `--json` returns a strict `node_local_setup_plan` report. No node
ID, port, secret argument, browser or model is required. The CLI discovers its
actual node owner API and submits a target/runtime/correlation-bound command to
`POST /api/commands/setup/local` (4 KiB maximum). UI clients can use that same
backend command; the browser does not own discovery or planning.

The normal `eefn setup` check remains unchanged in purpose. Both adapters use a
bounded read-only wait for local runtime initialization before inspection. The
new local plan additionally makes one bounded, non-proxied, non-redirected HTTP
GET to the discovered loopback coordinator's credential status. This is an
explicit owner-API inspection, not an unauthenticated network enrollment scan
or node protocol probe.

## Secret-free discovery

EEF publishes `owner-eef.json` alongside the existing per-user local discovery
file, using `EEF_DISCOVERY_DIR` when configured or the existing per-user default.
Its exact schema contains only `schema_version`, loopback `owner_address`,
loopback `node_address`, and a canonical coordinator `runtime_id`. It contains no
PSK, node credentials, invitation, certificate private key, permission or model.
Legacy `local-eef.json` behavior is separate and unchanged; private setup never
reads or falls back to its shared key. Discovery remains available when legacy
authentication is disabled. Publication failure is advisory, not a reason to
shut down an otherwise functioning coordinator owner API.

Only loopback-accessible IP-literal coordinator API/gateway bindings can publish
this same-PC advertisement; unspecified bindings are mapped to the matching
IPv4/IPv6 loopback address and actual bound ports are used. The file is capped at
4 KiB and rejects extra fields, remote/wildcard addresses, zero ports and invalid
runtime IDs. Corrupt metadata is refused rather than guessed or repaired.

This is one per-user advertisement, not a multi-coordinator registry, election,
priority decision or network-wide discovery service. The last local publisher
wins. The file can outlive an app: the observed credential report must match its
runtime ID. Unavailable or stale observations expose no live coordinator/trust
fields. A matching runtime is a local observation, not cryptographic peer proof;
files and local APIs share the existing same-user trust boundary. No automatic
trust-on-first-use or certificate installation follows from discovery.

## Truthful observations

States distinguish not found, unavailable, stale, unprovisioned, revoked,
credential-without-TLS-trust, and provisioned. A fresh report includes the public
saved certificate fingerprint when present and the coordinator's saved/applied
authentication restart requirement. Credential summaries are strictly validated,
without exposing credential values or returning arbitrary coordinator prose.
Saved revocation is reported even while old applied sessions remain accepted.

`success: true` means inspection completed, not successful pairing, protocol
connection, registration, inference or execution. `changed`, `pairing_submitted`,
`restart_requested` and `execution_authorized` remain false. The node runtime is
checked again after the coordinator inspection; retirement during that read
rejects the plan. Observations can become stale immediately afterward and cannot
be used as authority to mutate trust or run work.

This is the port-free discovery/inspection stage of first-run setup. The subsequent
[explicit local enrollment command](LOCAL-ENROLLMENT.md) adds private provisioning,
restart consent/apply and file-free invitation handoff. Completed registration
verification and full first-run/model setup remain separate requirements. Plans do not
provision/rotate credentials, undo revocations, replace node trust, change paused
preferences, enable features, download models, restart apps or submit workloads.
Default interpreter/bootstrap, physical two-PC and installer/release acceptance
remain open.

## Verification checkpoint

Final default and `--no-default-features` workspace/all-target suites each pass
**687 tests with one ignored**: EEF library 241, EEF CLI 17, installer 6, node
library 409, node CLI 14. Fresh default binaries/examples build; formatting,
diff, JavaScript syntax and backend/browser dependency source checks pass.

Headless two-logical-node credential fixture
`.validation/node-credentials-Mnv7Nj/results.json` passes against those binaries.
It checks port-free local planning before provisioning without either config
changing, secret-free discovery with legacy authentication disabled, advertised
runtime freshness, stale/corrupt metadata refusal, node/runtime/cross-site
boundaries and saved revocation before restart. It also retains existing private
invitation recovery, file-free pairing, pinned TLS and reviewed finite non-model
execution regressions. Metadata fault injection is restored only by the fixture;
the application does not repair an invalid advertisement.

Existing setup fixture `.validation/setup-check-tea8aE/results.json` also passes:
plain/JSON inspection, saved/applied distinctions, paused operation, exact
identity checks, bounded bodies, queue observations and no mutation/retry/fallback.
Neither fixture downloads/infers a model, modifies installed/remote nodes or
proves physical two-PC or complete first-run/release acceptance. Test processes
exit after the checks. No release version, installer, update feed or asset is
changed by this source increment.
