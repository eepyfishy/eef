# Independent node credentials (unreleased)

This development increment introduces opt-in exact-node credentials alongside
the legacy network key. Independent credentials now require owner-pinned TLS 1.3.
It does not complete execution grants, durable application replay fencing,
direct peer authorization or coordinator clustering. Published alpha.6
installers do not contain these commands.

## Command-based migration

Both apps must be running updated builds with their local command/API services.
No browser or model is required. Find the existing stable node ID with
`eefn network show --json`. Do not change it to match an invitation.

On the coordinator, explicitly provision that exact ID and its reachable EEF
endpoint (not the node's advertised address):

```powershell
eef credentials provision --node EXISTING_NODE_ID --address REACHABLE_EEF_ENDPOINT --json > private-pairing.json
eef credentials show --json
eef restart --json
```

Verify the new coordinator runtime ID and applied policy with diagnostics and
`credentials show`. Transfer the private file securely to the node owner, then
apply it through the running node:

```powershell
eefn connection pair --code-file private-pairing.json --json
eefn connection show --json
```

Pairing saves the selected endpoint/key and public coordinator certificate,
then requests a node **app** restart. `connection show` reports the saved
certificate SHA-256 fingerprint; a connected credential session reports
`transport: pinned_tls13`. These are transport facts, not model/job readiness.
It preserves the stable ID, permissions, connection-enabled preference, models
and other settings. It turns off automatic shared-key local pairing. Restart
acknowledgement is not completed connection or model readiness; inspect status
and use `eef diagnostics --node EXISTING_NODE_ID --samples 1 --json` afterwards.
Private files accept bounded UTF-8 (with/without BOM) and BOM-marked UTF-16,
including PowerShell redirected JSON output or a raw base64 invitation.

The file is a reusable **secret**, not encryption or a one-time/expiring token.
Do not post it in issues, public repos or diagnostics, or put the code in shell
arguments. Keep only protected copies. Lost provisioning replies are not retried:
inspect `credentials show` and explicitly rotate if a new private code is needed.

## File-free invitation input (unreleased after alpha.6)

New builds support `eefn connection pair-stdin --json`. Supply a private raw
base64 invitation or the provisioning command's JSON report through a pipe;
never put the code in command arguments or shell history. The CLI accepts bounded
UTF-8/BOM or BOM-marked UTF-16 just like the existing file adapter. It refuses an
interactive terminal and times out after 15 seconds if a pipe never finishes.
Input is limited to 128 KiB; the extracted code is limited to 4,096 bytes.
No intermediate invitation file is created. The credential still persists in
the node's existing configuration/backup, as with file pairing; this is not a
secret-erasure or protected-storage guarantee.

Clients can submit a private body directly to the local owner API:

```text
POST /api/commands/connection/pair
{
  "schema_version": 1,
  "expected_node_id": "EXISTING_NODE_ID",
  "expected_runtime_id": "CURRENT_NODE_RUNTIME_UUID",
  "request_id": "NEW_CORRELATION_UUID",
  "code": "PRIVATE_BASE64_INVITATION"
}
```

The body is capped at 8 KiB, denies extra fields and uses the existing local
owner/cross-origin boundary. The shared service validates the exact invitation
node, schema, generated credential, endpoint and pinned coordinator certificate,
then merges under the existing configuration lock. It checks the expected node
runtime before saving. CLI and future UI are adapters; the core never reads
its own stdin, needs a browser, or obtains a model for pairing. Requests' debug
representations and public reports omit the code/credential; do not log raw
HTTP bodies or deliberately export private configurations.

Successful `node_credential_pairing` reports bind node, correlation, previous
runtime and certificate fingerprint. `saved: true` and `restart_requested: true`
confirm configuration saving and a node app restart request, **not completed
restart, coordinator registration, workload readiness or execution approval**.
`completed` and `execution_authorized` remain false. Feature permissions,
models, stable identity/name, unrelated settings and the connection-enabled
preference are preserved. A paused node stays paused until explicit resume.
The requested node restart also applies other already-saved pending settings,
as with existing connection controls; pairing is not a selective reload.

Lost/substituted/error replies return `local_pairing_reply_unconfirmed` with
`outcome_unknown: true`; `saved`/`restart_requested` are null, not rollback claims.
The CLI never echoes arbitrary server error text, retries pairing or falls back
to legacy/file/generic configuration saves. HTTP 404 is `unsupported_command`
without an automatic alternative. Inspect `connection show` and diagnostics
before deciding whether to pair again. `request_id` is ephemeral correlation,
**not a durable deduplication receipt**; reissuing pairing can request another
restart. No automatic rotation, coordinator restart or permission grant occurs.
Successful replies must have the exact bounded report fields and fixed public
note; unexpected fields or substituted prose are unconfirmed instead of being
printed. This also prevents an invalid reply from echoing the submitted secret.

This removes private-file handling for automation and client adapters, not the
remaining non-coder onboarding requirement. Same-PC `pair-local` remains the
explicit legacy shared-key path and cannot silently replace an independent
credential. Friendly private provisioning and first-run orchestration remain
open, together with bootstrap/model quality and physical release acceptance.

After all required nodes migrate, optionally disable legacy sign-in:

```powershell
eef credentials legacy --allow false --json
eef restart --json
```

This also makes the old shared-key `eef invite` unavailable. Older nodes require
an explicit compatible upgrade; there is no credential downgrade/fallback.
Local owner controls remain usable even if every outgoing node connection fails.

Credential-only startup no longer requires a dummy legacy `node.psk` or
`EEF_NODE_PSK`: with `allow_legacy: false`, the gateway constructs no legacy key.
Independent credentials, certificate validation and explicit pairing still apply;
no identity/credential is silently created to bypass missing setup. The local
shared-key pairing advertisement is cleared while legacy access is disabled,
even if an old key remains saved or in the process environment. Existing saved
keys are not automatically erased. Only an owner setting may re-enable legacy,
and that command rejects atomically without a valid non-placeholder effective
legacy secret (at least 12 characters, distinct from node credentials). Default
legacy-enabled installations retain their existing secret requirement. These
changes do not add credential transport to legacy ESP firmware or complete peer
authorization/release acceptance.

### Migration from the earlier unreleased credential build

Private invitations and public selectors now use schema **2**. Schema 1's raw
credential transport is refused, even with the correct secret. Already provisioned
development nodes need an explicit `--rotate` invitation and pairing again; do not
change IDs, reset permissions, enable legacy fallback or bypass certificate checks.
An older credential policy without a TLS identity keeps the local owner service
available but cannot accept credential connections. Explicit provisioning creates
the identity, and coordinator restart applies it. Existing shared-key legacy nodes
are unchanged until the owner migrates them.

## Revocation and rotation

```powershell
eef credentials revoke --node EXISTING_NODE_ID --json
eef restart --json
eef credentials provision --node EXISTING_NODE_ID --address REACHABLE_EEF_ENDPOINT --rotate --json > private-pairing.json
```

Revocation retains an exact-ID tombstone, blocking shared-key impersonation as
well as its previous credential. Re-provisioning requires `--rotate`, not an
implicit overwrite. Apply a rotation with coordinator restart followed by node
pairing as above. **Saved changes are not immediate revocation**: old sessions
and applied keys stay usable until EEF restart. A restart does not prove external
side effects stopped. No job or remote mutation is automatically replayed.

## Contract and trust boundary

- `node.authentication` stores `allow_legacy` and at most 256 exact identity
  entries. Each entry is an independent generated 256-bit hexadecimal credential,
  or null for revocation. Duplicate credentials, network-key reuse, wildcards,
  unknown fields and malformed policy fail closed. Missing policy preserves
  legacy compatibility; it does not claim untrusted nodes are isolated.
- Signed/selected remote IDs are exact byte strings: leading/trailing whitespace,
  control characters, the `*` wildcard and EEF's internal `local` executor alias
  are rejected before credential lookup, authentication receipts or registration.
  No trimming, case folding, automatic rename or identity replacement occurs.
  Display names and coordinator advertisement IDs use separate namespaces;
  calling a node "local" in its display name is not forbidden. Existing malformed
  or reserved manually assigned IDs require explicit owner correction/re-pairing,
  not silent migration. Ordinary generated `device-*` IDs remain unchanged.
- `node.authentication.tls` holds the persistent public DER certificate and
  private PKCS#8 key, base64 encoded. First explicit provisioning creates a random
  coordinator identity; provisioning another node, rotating a node secret and
  ordinary app restarts preserve it. Keep the config/backups private. Invitations
  include only its public certificate, never the coordinator private key.
- A bounded **plaintext node-ID selector** chooses the associated key before a
  mandatory TLS 1.3 handshake, encrypted probe and node authentication. It is public metadata,
  not identity proof. The decrypted authentication must match the selected ID.
  Unknown/revoked/wrong-key selectors cannot fall back or reserve a live identity.
- Provisioned and revoked IDs cannot authenticate with the network key, even
  while legacy access remains enabled for other IDs. Independent pairing does
  not confer any device-feature permission or trust additional coordinators.
- [Rustls](https://docs.rs/rustls/0.23.43/rustls/) and
  [Tokio Rustls](https://docs.rs/tokio-rustls/0.26.4/tokio_rustls/) provide standard
  TLS 1.3, using the existing ring backend; [rcgen](https://docs.rs/rcgen/0.14.10/rcgen/)
  generates the persistent self-signed identity. There is no custom TLS verifier:
  the node's root store contains only the owner-paired certificate, and the
  verified leaf must additionally match its exact bytes. System public roots,
  trust-on-first-use, silent certificate replacement, TLS 1.2, early data,
  resumption and session tickets are disabled for this path. The fixed protocol
  server-name and ALPN identify EEF's protocol, not a deployment-specific host.
  Handshake/probe deadlines and bounded existing frames still apply. Certificate
  verification failure means inspect pairing/clock; never bypass verification.
- Existing AES-GCM/HKDF/HMAC frames run **inside** TLS and exact-node HMAC proof
  remains mandatory; this is not mutual-certificate authentication. TLS protects
  session transport records against replay and provides ephemeral key exchange.
  Legacy framing alone does not gain those guarantees. Transport security does
  not authorize an application operation, prevent an authorized issuer from
  resending a logical command, persist grant/attempt replay state, constrain a
  stale leader or stop a remote side effect. Those grant/ownership gates remain.
- Credential status/configuration responses redact node keys and the coordinator
  private key. The certificate and fingerprint are public. Only an explicit
  provisioning operation returns a private code. Generic coordinator settings
  saves preserve the latest credential policy under its existing config lock;
  dedicated owner commands manage it. Remote node settings cannot change its
  identity/key/authentication mode/certificate, and automatic local pairing never replaces
  independently paired trust.
- Coordinator command mutations bind the current runtime ID. Node pairing binds
  the running stable node ID. These commands are local-owner operations, protected
  by the existing loopback/cross-site request guard. Config files/backups and
  invitation files still require per-user filesystem protection.

Validation uses unit/protocol tests and `tools/test-node-credentials.mjs`, with
two logical nodes on one PC. Physical two-PC migration/rollback, reviewed session
authorization, grants and peer paths remain separate acceptance gates. TLS tests
exercise exact-pin rejection, same/cross-session record replay, TLS 1.2/ALPN denial,
large fragmented streams, coalesced selector/ClientHello read-ahead and shutdown
during incomplete handshakes. This TLS certificate is **not Windows code signing**
and does not resolve installer Defender detections.
