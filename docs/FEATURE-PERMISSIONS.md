# Command-first feature permissions (unreleased)

```text
eefn permissions show
eefn permissions allow filesystem-read
eefn permissions allow filesystem-write
eefn permissions allow process-execution
eefn permissions deny camera
eefn permissions allow remote-management

eef node permissions --node NODE_ID show
eef node permissions --node NODE_ID allow filesystem-read
eef node permissions --node NODE_ID deny remote-management
```

Plain output explains saved/applied modes. Add `--json` for the same typed
backend reports. Commands use the already-running node and actual local API
marker; they do not start a second node, need a model/browser, edit a caller's
JSON input file, or ask users for ports/digests. `allow` is explicit owner consent
for the whole feature, not an inspection or automatic permission grant.

## Supported switches and scope

| CLI feature | Existing preference |
| --- | --- |
| `filesystem-read`, `filesystem-write` | Read/write flags and matching feature-wide grants |
| `process-execution` | Process flag and `process.exec` feature-wide grant |
| `applications` | Application flag and `application.control` feature-wide grant |
| `http-requests` | HTTP flag and `http.request` feature-wide grant |
| `wake-on-lan` | Wake flag and `network.wol` feature-wide grant |
| `speech-to-text` | Existing STT enabled flag; requires configured owner endpoint/model |
| `microphone`, `audio-output`, `text-to-speech` | Existing media flags |
| `camera`, `screen-capture`, `input-control` | Existing media flags |
| `peer-transfers` | Existing peer transport switch and local byte/rate/session limits; no work approval |
| `remote-updates` | Existing owner update permission; does not choose an update/feed |
| `remote-management` | Existing current owner approval for management from eef |

There are no per-folder, file, executable, application, host or target-computer
questions for feature-wide access. Existing byte/time/result limits, deterministic
validation, account permissions and required reviewed workload approval remain.
HTTP whole-feature access includes supported methods and private-network requests
under the existing executor semantics; it does not follow arbitrary redirects.
These preferences do not install a missing executor/service, authenticate a peer,
prove media/model health, authorize ambient recording, run work or grant Windows
administrator privileges. Bluetooth/Wi-Fi settings/stream relays are not invented
as working switches here.

Known microphone, audio-output, TTS, camera, screen and input-control capability
names also require their media switch when supplied by an owner-installed
Python plugin. Merely loading such a plugin does not advertise or enable its
disabled feature. Finite peer audio requires both endpoint media switches in
addition to the separate opt-in peer permission and reviewed execution grants;
see [scope and current setup limits](PEER-TRANSPORT.md#finite-capture-to-playback-unreleased-advanced-configuration).

Running programs and controlling apps/input can affect files independently of the
file switches. Do not treat feature preferences as a process sandbox. Speech
recognition still needs a configured service/model, and audio-file access still
requires separate file-read permission. Media permissions require the existing
available Python/native adapter; saving a flag is not proof that it was loaded.

## Saved/applied behavior, preservation and validation

Inspection does not widen legacy scoped permissions. `allow` explicitly sets the
enabled flag and adds only that feature's full-access token where required.
`deny` clears only that flag/token. Old root/app/program/host lists, limits,
provider settings, unknown future JSON and unrelated configuration survive.
Existing active scoped access remains validated. Dormant or bypassed root/program/
app scopes are preserved without requiring their old paths to exist; reactivating
scoped permissions validates them again. Invalid **active** legacy configuration
can still require explicit correction; commands do not silently repair it or
discard its scopes.

Except current remote-management approval, preferences apply to the executor on
explicit node restart. The report lists saved and applied `disabled`, `scoped`
or `feature_wide` modes, not installed capability readiness. Current management
approval is read from saved config on every remote mutation, so revocation is
immediate and cannot be re-enabled through a stale remote approval snapshot.
Other already-pending settings can also apply on restart. No restart is sent by
permission commands.

Reviewed execution rechecks relevant saved versus applied policy. Saved changes
therefore refuse new/pending affected operations even before restart; broadening
does not immediately broaden the old executor. In-flight operations can have
already produced effects. Revocation is not a general acknowledged Stop, undo or
guarantee against external services/process effects.

Both adapters inspect, then submit an exact current runtime and saved permission
digest. The service holds the existing preview/engine binding through the edit,
then the current config and live-state guards; retirement/replacement cannot
substitute a different engine. The edit merges under the existing config lock and
uses the existing save validation/backup. Concurrent permission changes reject
the old digest without saving. Digests cover the raw saved permission object and
current management flag; they are not permission grants or a monotonic/durable
idempotency ledger. A content restore can recreate a digest. No mutation is
automatically replayed, even if it would be a semantic no-op.

## Interfaces and failure handling

Local clients use `POST /api/commands/permissions` with schema version 1, exact
node/runtime, and `command: {operation: "show"}` or
`command: {operation: "set", feature: "filesystem_read", enabled: true,
expected_digest: "INSPECTED_DIGEST"}`. Requests are capped at 4 KiB and use the
existing local-owner/cross-origin boundary. CLI `allow`/`deny` supply the inspected
digest automatically; a rejection requires a new explicit decision.

eef clients use `POST /api/commands/node/permissions`. Show uses the existing
authenticated node's modern status to select its exact runtime, then the same
service. Set requires that inspected runtime/digest. Reserved `permission_status`
uses bounded control capacity and rejects Set payloads; mutations use ordinary
request capacity. Read-only remote inspection does not require management approval;
remote writes recheck it under the write lock, including writes to the approval
switch itself. There is no fallback to old `get`/generic-save endpoints.

Reports expose fixed feature IDs/modes, node/runtime IDs, digest and side-effect
flags, not raw scopes, names, addresses, secrets or arbitrary server prose.
Digests/IDs remain correlation data; no automatic upload occurs. Adapters validate
exact targets/commands, feature ordering/known modes, changed digest, management
state and fixed notes. Lost/substituted/unsupported write replies remain
unconfirmed: inspect `permissions show` before deciding whether to act again.
Sending/acknowledgement is not rollback, completion of a restart or execution.
The adapter's `sending_attempted` field tracks the mutating request only;
inspection still sends bounded read-only diagnostics/status requests.

## Verification (2026-10-14)

- Fresh default and dashboard-disabled workspace/all-target suites each passed
  **674 tests**, with one ignored. Permission tests cover all switch mappings,
  scoped/whole-feature preservation, missing dormant paths versus active scoped
  validation, stale digest/runtime/identity rejection, retired binding, current
  remote approval/revocation, strict reports and control-lane mutation rejection.
  A prepared executor test uses the new denial service and verifies Unknown with
  no file effect, before an app restart.
- Fresh binaries/examples, formatting, diff whitespace, changed JavaScript
  syntax and backend/browser source-boundary checks passed.
- Final `permission-commands-B1lkI2` passed local/coordinator plain-text and JSON
  commands without a model or UI, with no per-object allowlists. It preserves
  byte limits/provider/custom settings, separates saved/applied modes, requires
  current remote approval, rejects stale/cross-origin/oversized requests, and
  inspects dropped/substituted/unsupported writes without retry/fallback.
  A separately approved finite pipeline writes a real isolated fixture file.
- In the same run, revoking file-write permission during an owned native
  pipeline stage produced `local_policy_changed`/Unknown. A native observer held
  creation-verified root/child handles and observed both exited after **254 ms**,
  before the 10,000 ms grant deadline. The later write is absent. This is scoped
  containment/pending-stage evidence, not general external-effect Stop/undo.
- Final `setup-check-o5zHuA` passed the existing headless setup regression.
  Fixture processes exited; installed apps/nodes were not changed.

An earlier final-binary run, `permission-commands-Q88ctK`, submitted a concrete
request after seeing the TLS transport label but before connected readiness and
returned `concrete_not_sent`. Read-only SQLite inspection confirms zero durable
jobs. Its failed evidence is retained. The corrected fixture waits for connected
state, pinned transport and configured receipt support **before new input**;
it does not replay the failed request or relax any backend gate. This is a
fixture startup wait correction, not a claim that all connection/input readiness
races or physical timing are solved. The existing client also publishes its
connected label before installing the local submission mailbox; that source-order
window is addressed by the subsequent [input-channel readiness increment](INPUT-CHANNEL-READINESS.md),
which still does not claim atomic cross-layer or coordinator acceptance readiness.

All integration evidence is isolated localhost/Windows testing, not physical
two-PC or stable release acceptance.

## Remaining release scope

This exposes existing owner permissions as shared commands and an approved
coordinator adapter. UI visual work, installed nodes, released installers and
automatic update policy are unchanged. Non-coder first-run orchestration,
configured STT/backend setup commands, stronger acknowledged Stop, media/peer
workflows, aggregate resource limits, physical two-PC and stable release acceptance
remain open.
