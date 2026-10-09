# Independent node credentials (unreleased)

This development increment introduces opt-in exact-node credentials alongside
the legacy network key. It does not complete execution grants, session replay
fencing, direct peer authorization or coordinator clustering. Published alpha.6
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

Pairing saves the selected endpoint/key and requests a node **app** restart.
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

After all required nodes migrate, optionally disable legacy sign-in:

```powershell
eef credentials legacy --allow false --json
eef restart --json
```

This also makes the old shared-key `eef invite` unavailable. Older nodes require
an explicit compatible upgrade; there is no credential downgrade/fallback.
Local owner controls remain usable even if every outgoing node connection fails.

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
- A bounded **plaintext node-ID selector** chooses the associated key before the
  normal encrypted reachability probe and authentication. It is public metadata,
  not identity proof. The decrypted authentication must match the selected ID.
  Unknown/revoked/wrong-key selectors cannot fall back or reserve a live identity.
- Provisioned and revoked IDs cannot authenticate with the network key, even
  while legacy access remains enabled for other IDs. Independent pairing does
  not confer any device-feature permission or trust additional coordinators.
- The existing AES-GCM/HKDF/HMAC implementation and bounded framing are reused;
  no new cryptographic primitive/construction or transport dependency is added.
  The existing timestamp/nonce check is not session-level message replay fencing,
  forward secrecy or proof of a durable execution grant. Captured frames and
  coordinator restarts require a separately reviewed session-security increment
  before scoped grants/peer workloads are declared safe.
- Credential status/configuration responses redact keys. Only an explicit
  provisioning operation returns a private code. Generic coordinator settings
  saves preserve the latest credential policy under its existing config lock;
  dedicated owner commands manage it. Remote node settings cannot change its
  identity/key/authentication mode, and automatic local pairing never replaces
  independently paired trust.
- Coordinator command mutations bind the current runtime ID. Node pairing binds
  the running stable node ID. These commands are local-owner operations, protected
  by the existing loopback/cross-site request guard. Config files/backups and
  invitation files still require per-user filesystem protection.

Validation uses unit/protocol tests and `tools/test-node-credentials.mjs`, with
two logical nodes on one PC. Physical two-PC migration/rollback, reviewed session
security, grants and peer paths remain separate acceptance gates.
