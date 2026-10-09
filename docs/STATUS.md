# Architecture conformance status

## Unreleased: independent exact-node credential foundation (2026-10-10)

Local-owner `eef credentials` commands now provision, inspect, rotate and revoke
an exact stable node identity and explicitly enable/disable legacy sign-in.
`eefn connection pair --code-file` applies a bounded private invitation through
the running node service, preserving ID, permissions, models and connection-enabled
preference while requesting an app restart. It accepts UTF-8/BOM and BOM-marked
UTF-16 redirected command output. No JSON configuration editing or browser is
needed for this workflow. The invitation is a reusable secret, not a one-time token.

The gateway selects one independently generated key using a bounded public
node-ID selector. Decrypted identity must match it. Wrong/unknown/revoked keys
cannot fall back, reserve identities or poison another identity's replay cache.
Provisioned/revoked IDs also cannot authenticate with the shared network key.
Revoked identities retain tombstones; replacing a credential requires explicit
rotation. Legacy compatibility remains the default for other identities until
the owner disables it. Saved changes are not immediate revocation: coordinator
restart is required to retire old sessions/apply keys. Local automatic pairing
cannot silently replace independently configured trust.

Status/config views redact credentials. Generic coordinator configuration saves
preserve the latest credential policy; remote node settings cannot rewrite its
trust or ID. Current-runtime command checks, existing local API request guards,
independent-key/size validation and atomic config edits apply. Authentication
timestamps reject extreme values without overflow. The existing replay cache is
bounded and duplicate attempts no longer refresh receipt lifetime. Bounded legacy
timestamp nonces from v0.3.2 remain compatible.

This reuses existing cryptography/framing. It does **not** establish session
message-replay fencing, forward secrecy, execution grants, acknowledged remote
cancellation, separately authorized peer data paths or cluster safety. See
[credential migration and limitations](NODE-CREDENTIALS.md). Physical two-PC
migration/rollback and reviewed issuer-session security remain required gates.

Validation: 233 workspace/all-target Rust tests pass with and without dashboard
features. Protocol tests cover independent keys, cross-identity and shared-key
impersonation denial, revoked/unknown IDs, explicit legacy denial, bidirectional
encrypted registration/control, shutdown cleanup, replay bounds and old nonces.
Private-file/config tests preserve owner policy and reject malformed/oversized
invitations. Command-based two-logical-node fixtures passed in default and
headless builds, including disabled-legacy reconnection, deferred revocation,
explicit rotation, protected config saves, private-code redaction and PowerShell
file encodings. These are isolated one-PC fixtures, not physical acceptance.
Existing node-command, scoped discovery, request-review and job-shutdown fixtures
also passed; no model download, installed-node migration, permission change,
release-feed change, installer/signing operation or publication was performed.

Latest headless command evidence: `node-credentials-SJw2Zc` and
`node-commands-Cjv77Z`. Earlier regression runs: `peer-discovery-zGryU9`,
`request-review-qyExIK` and `job-shutdown-FGZnrw`. Fixture paths are beneath
the ignored `.validation/` folder and contain test-only private credentials;
never publish those directories as diagnostics.

## Unreleased: reserved-capacity remote restart workflow (2026-10-10)

The bounded control pool initially protected raw status/restart requests, but
EEF's restart command first requested full configuration and diagnostics through
work capacity. Updated nodes now include a versioned control snapshot in existing
status replies: approval state, stable node ID, current runtime ID and connection
state. EEF uses reserved status for preflight and completion polling, avoiding
full-configuration retrieval over the network and work-slot dependence. The node
still rechecks owner approval when applying restart. Control slots are bounded;
confirmed rejection reports not-started, without retry or an uncertain execution
claim. Saturating all controls still prevents immediate restart.

Older same-ID status replies without the new schema retain read-only get/diagnostic
compatibility. Versioned malformed/mismatched replies, capacity rejections and
transport failures do not fall back or send restart. No permissions are enabled
by inspection, and interrupted inference is never automatically resubmitted.

Validation: 225 workspace/all-target Rust tests passed in both feature modes.
The rebuilt headless model/control fixture (`model-metadata-KXp27q`) holds the
only work slot, then verifies both denied and approved full restart commands.
Approval denial sends no restart; approval confirms the same stable node with a
new runtime ID, without replaying interrupted inference. Unit tests cover bounded
status projection, strict schema/identity/runtime validation, legacy inspection,
and confirmed preflight/restart capacity rejection without retry. Formatting,
diff, JavaScript syntax and browser-boundary checks passed. This uses isolated
one-PC processes and fake inference, not physical two-PC validation. No installed
node, model, permission, release-feed, installer or signing change.

## Unreleased: event-loss recovery and observer survival (2026-10-10)

A deterministic overload test evicted registration/disconnect events from the
gateway's 256-event channel and reproduced stale/missing scheduler entries.
The gateway now retains only each live session's latest sanitized registration
and metrics, with no request history or adapter bindings. EEF reconciles providers,
models and node observations at listener start and after lag. State notifications
consult live snapshots so old disconnects cannot delete a newer session.
Normal heartbeats clone only metrics, without revalidating all advertised models.
Scheduler-owned counts/limits and local adapters survive refresh. Metrics-only
disconnected nodes are also removed. Recovery updates observations directly;
it does not replay lifecycle triggers or lost submissions into automation.

World and assistant observers previously exited permanently on broadcast lag.
They now keep consuming later events and expose dropped-event counters. World
observations conservatively retain `state_may_be_stale` after a gap; automatic
world-condition rules suspend while that flag is set. Explicit commands and
separate event/keyword rules remain available. This flag lasts for the world
runtime's lifetime; reconnect or node reconciliation cannot prove that missing
application/sensor/task observations were restored. No automatic work replay,
execution lease or full world-state reconstruction is claimed.

Validation: 222 workspace/all-target Rust tests passed with and without dashboard
support. Gateway-overload and world-observer regressions failed before their
fixes; the final gateway test verifies startup/lag reconciliation, live provider
and model recovery, metrics-only cleanup, local adapter/counter preservation,
observer repair without synthetic connection triggers, and no lost request replay.
Assistant tests verify resumed listening and suspension of stale world rules.
Sanitized snapshot tests cover latest state, independent copies and secret/binding
redaction. Formatting, diff, JavaScript syntax and browser-boundary checks passed.

Rebuilt API-only process fixtures passed request review (`request-review-Z8RKQG`),
local preview (`request-preview-TLS7o4`), node commands/diagnostics
(`node-commands-lFybtB`), shutdown/recovery (`job-shutdown-W21ICe`), scoped
two-logical-node discovery (`peer-discovery-G9JUZN`) and model/job/limits controls
(`model-metadata-SbnZB8`). These are isolated one-PC fixtures with fake inference,
not a physical two-PC acceptance run. No installed node, model, permission,
release-feed, installer or signing change.

One combined capacity-one model fixture (`model-metadata-Ukr9mW`) received an
unconfirmed multi-stage model-setting reply after reconnect. Capacity rejection
was previously classified as uncertain at this boundary. EEF now preserves a
confirmed `NodeBusy`/`not_started:true` result as `node_busy`, with no automatic
mutation retry; missing confirmation remains uncertain. Tests cover rejection
both before and after preflight. The successful fixture verifies real saturated
preflight rejection without saving configuration, then restores the normal work
budget for unrelated model-control checks. A capacity slot remains occupied
through reply handling; receiving one reply does not guarantee immediate spare
capacity for another command.

## Unreleased: explicit node-to-coordinator request review (2026-10-10)

`eefn requests review` extends bounded local interpreter preview through the
existing authenticated node submission path. The gateway overwrites origin and
capability context using accepted registration snapshots; EEF revalidates the
strict schema, bounds and hint membership. Both service and CLI confirm response
identity, original text and review-only flags. EEF returns review/clarification
state without invoking its brain, planning, dispatching or writing a job.

Review is ephemeral, not a durable receipt or approval. It has no automatic
retry, default-model selection or chat fallback. The one-inference preview
budget is shared; restart invalidates waits. The configured external model may
continue computation after local cancellation. Local preview remains usable
with EEF paused. Interactive clarification and independent execution grants
remain planned; a model's well-formed proposal is not proof of understanding.

Validation: strict-schema Rust tests and isolated headless request review
(`request-review-MriG3f`) and preview (`request-preview-kg5qj1`) fixtures passed.
Cases cover exact original text, gateway-stamped origin/area, clarification,
invalid authority fields, shared inference capacity, restart invalidation,
offline EEF, lost/corrupted replies and missing older endpoints without fallback.
No jobs were created. Final combined verification is recorded above when complete.
These use fake inference on one PC, not physical two-PC validation. No installed
node, model, permission or release change.

## Unreleased: bounded node request admission (2026-10-09)

Incoming node requests previously spawned an unbounded number of execution
futures. A shared node-client budget now defaults to 16 work requests and two
separate control requests, configurable through validated `request_limits`.
Control capacity is limited to ping and node status/restart; restart approval and
all other normal permission checks remain in the executor. Excess work receives
`NodeBusy`/`not_started:true` without spawning an execution future. No backlog or
automatic node replay is added. Permits last through execution/reply handling and
are released on completion or cancellation, including connection teardown.

Diagnostics expose sanitized applied-limit snapshots and a busy-rejection counter.
Older configs receive defaults. Invalid owner/approved-remote saves must not replace
valid saved configuration. These are network request-future limits, not limits on
CPU/RAM, local API actions, background downloads or effects surviving cancellation.
The roadmap's full workload runtime and independent execution grants remain planned.

`eefn connection limits` inspects or saves work/control values through the shared
node command service, without JSON editing, a browser, a second runtime or an
automatic restart. Atomic partial updates preserve unrelated configuration and
no-ops do not rewrite it. Reports distinguish saved from applied limits.

Validation: 212 workspace/all-target Rust tests passed in both feature modes.
Rebuilt headless processes passed the model/job/limits command fixture
(`model-metadata-Z2dWdz`): invalid saves leave configuration unchanged, partial
updates preserve other fields, saved/applied values differ until restart, and
setting limits does not restart the runtime. Saturated work rejects without
starting inference while ping/status and permission-checked restart remain
responsive. These are isolated local fixtures with fake inference, not physical
two-PC validation. No installed-node, model, permission or release change.

## Unreleased: saturated reply queues fail promptly (2026-10-09)

A regression reproduced `send_to_node` waiting indefinitely behind a full
outbound queue. Besides job replies, the shared coordinator event listener uses
this path to reject excess submissions; one blocked node could therefore stall
other registration/heartbeat/disconnect processing. Reply admission now uses the
existing bounded queue without waiting, reports queue-full separately from an
offline node, and never schedules rejected replies for later delivery. Ordinary
remote invocation retains its existing bounded send/reply deadline.

Queue admission is not a delivery acknowledgement. A missing job reply still
requires inspection; already-accepted work is not rolled back or replayed.
Validation: 207 workspace/all-target Rust tests passed with and without dashboard
support. The queue-saturation regression failed before the fix; it now verifies
prompt rejection, no late delivery, recovery after capacity frees, and separate
offline handling. The full 12-test gateway group passed 20 consecutive standalone
runs (240 test executions). Additional admission tests cover corrupt retained
history and 20 rounds of receipt creation racing shutdown, without orphan jobs.

Rebuilt headless processes passed node commands (`node-commands-PtQaP9`), job
shutdown/recovery (`job-shutdown-iAr9JH`), model/job controls including receipt
reuse (`model-metadata-C7NolE`) and two-logical-node scoped discovery
(`peer-discovery-iKmjK8`). These use isolated local fixtures and fake inference,
not a physical two-PC run. Formatting, JavaScript syntax, diff and backend/browser
dependency checks passed. No installed-node, model, permission or release-feed
change; no installer/signature or stable-0.4.0 readiness claim.

## Unreleased: gateway session shutdown ownership (2026-10-06)

New protocol regressions reproduced Stop returning with an authenticated session's
identity lease still reserved, and a probed connection still waiting for auth.
The listener previously spawned detached handlers and each handler spawned a
detached socket writer. Clearing the connection map did not retire those tasks.

The gateway now owns a reaped session task set. Stop signals its listener, drains
handlers including unfinished authentication, releases their socket halves and
identity leases, publishes disconnects for remaining live records and fails
pending calls before allowing a new generation. Read and write futures share the
handler lifetime. An interrupted Stop retains its handle/cleanup state so another
Stop can finish it; Start rejects that incomplete shutdown instead of overlapping
generations. This stops local transport, not already-dispatched remote effects.

Validation: 204 Rust tests passed in both feature modes at the gateway checkpoint;
later admission race/corruption tests raised this to 206. Protocol regressions
cover same-ID reconnection, pending replies, incomplete authentication and
cancelled Stop cleanup. The two lifecycle regressions failed before the fix.
Rebuilt headless CLI (`node-commands-0BNwrq`), shutdown/recovery
(`job-shutdown-P0g8tj`), model/job controls (`model-metadata-WBv0aW`) and two-logical-
node discovery (`peer-discovery-Wjc9Bq`) passed. All are isolated local fixtures,
not physical two-PC validation. No installed node or release artifact changed.

## Unreleased: retained-history creation deduplication (2026-10-06)

Node-origin `jobs.create` now atomically checks the authenticated origin/receipt
against retained journal records before insertion. Identical normalized input
returns the original job snapshot and `admission.reused:true`, without another
runner, history mutation or implicit resume. Changed input and ambiguous legacy
receipts fail closed. Original area context is retained across node moves; a
different origin cannot collide with or read another node's receipt.

The existing journal and admission gate are reused, with no second job framework,
schema migration, permanent tombstone collection or automatic retry. Protection
survives coordinator restart only while history remains. Removing that history,
switching databases or using older coordinators ends the guarantee. Owner-local
creation and non-creation controls retain their existing semantics. This is not
an execution grant, a remote stop acknowledgement or exactly-once execution.

Validation: 201 Rust tests passed in both dashboard/default and headless feature
configurations at this checkpoint. Tests cover 32 simultaneous identical creates,
one executor call, changed-input rejection, distinct owners, immutable origin area,
reopen with uncertain work and no implicit dispatch, ambiguous legacy receipts,
quota exhaustion, history removal and shutdown. Isolated headless protocol tests
passed (`model-metadata-N0nlSE`), including concurrent repeated receipts while
inference is held and reuse after completion/cancellation. The backend is a fake
Ollama HTTP fixture, not real inference or a physical two-PC test. Shutdown/recovery
also passed (`job-shutdown-dpq8CY`). Installed nodes and release feeds are unchanged.

## Unreleased: connection-stage diagnostics (2026-10-06)

The coordinator's existing CLI/API diagnostic report now distinguishes
authenticated connections, gateway-accepted registrations and registrations
still pending. The three counts use one live connection snapshot, never the
historical route map. Remote provider and healthy-provider counts are taken
separately from the scheduler registry, excluding local adapters. No node names,
addresses, paths, capabilities, permissions or model details are added to reports.

The report explicitly warns that these separate snapshots are not an execution
readiness guarantee. Registration precedes scheduler processing; a registered
node can advertise no capabilities/models; health does not reserve capacity or
check permissions. This makes the reconnect race found on September 27 visible
without changing job admission, adding retries or requiring dashboard code.

Validation: 197 Rust workspace/all-target tests passed both with and without
dashboard support. New regressions cover the encrypted authentication/register/
disconnect lifecycle, invalid and repeated registration, empty capability
advertisements, historical-route exclusion, gateway stop and remote-vs-local
provider counts (including unhealthy and at-capacity providers).

Rebuilt headless binaries passed command/API integration with no nodes and with
a model-free registered node (`node-commands-tPLxCI`), including redaction checks.
The shutdown/recovery regression also passed (`job-shutdown-2Qi0hd`). Formatting,
JavaScript syntax, diff and backend/browser dependency checks passed. These are
isolated local-process checks, not physical two-PC validation. This is unreleased
source work, with no installed-node, permission or update-feed change.

## Unreleased: Stop and runner-retirement coordination (2026-09-27)

A concurrent regression reproduced a Stop request leaving a job in `stopping`
after its last runner exited. Stop previously sampled runner presence separately
from saving its state, while runner cleanup checkpointed interruption separately
from removing that runner. Either gap could leave nobody to settle the request.

Stop's presence check/checkpoint and runner cleanup/removal now use the existing
admission gate, consistently locking runner state before the journal. No remote
call or await is performed under this gate. Stop before execution starts still
prevents dispatch; already-claimed work may finish, with its results retained and
dependent steps cancelled. An uncertain runner exit remains `interrupted` for
review, not a promise of confirmed remote cancellation. Repeating Stop on finished
history remains an error without rewriting the saved result.

This builds on the unreleased shutdown-admission fix below. It adds no execution
lease, remote cancellation, schema/wire change or media feature. No installed
node, release, permission, model or update feed is changed.

Validation: 194 Rust workspace/all-target tests passed with and without dashboard
support. The new race regression failed before the fix and then passed 20
consecutive standalone runs (512 raced retirements per run). Additional tests
verify Stop before a resumed runner's first poll, retention of a finished claimed
step, cancellation of its dependents, and unchanged history after a rejected
second Stop. Formatting and backend/browser boundary checks passed.

The first local-process shutdown run (`job-shutdown-zBtmmz`) exposed a separate
fixture readiness gap: its new job failed with no HTTP provider available after
socket reconnection. Authentication is counted before registration reaches the
scheduler. The harness now waits for the exact healthy HTTP provider before
submitting new work, records readiness, and saves terminal failures immediately
instead of waiting for a success-only timeout. It never retries a failed job.
Three corrected runs passed (`job-shutdown-aszVTz`, `job-shutdown-zFa6UY`,
`job-shutdown-6p7EqW`); each observed socket reconnection before provider readiness.

Rebuilt headless node commands (`node-commands-Dw037G`) and model/job controls
(`model-metadata-B2xIky`) passed. Model tests use a fake Ollama backend, not real
inference. JavaScript syntax, diff and documentation-link checks passed. No new
installer, physical two-PC deployment or remote-stop guarantee is claimed.

## Unreleased: shutdown-safe job admission (2026-09-26)

Backend development resumed after the documentation-only pause. A regression
against the unchanged alpha.6 engine reproduced a rejected enqueue leaving a
new pending job in the journal: persistence happened before the shutdown check.
New-job persistence, resume checkpointing and runner registration now share a
single admission/closing gate. Shutdown closes that gate before notifying and
draining runners, including jobs not yet polled. Failed admission creates no
runner guard, and missing-executor rejection saves no new job on either path.

This is in-process coordination, not durable command deduplication, remote
execution leases, remote cancellation or exactly-once effects. No schema/wire
change, model download, installed-node change or release is included. The prior
presence/media design remains planned; no UI feature work was resumed.

Validation: 191 Rust workspace/all-target tests passed with and without dashboard
support. Six new cases cover rejected enqueue/run admission, missing executors,
unchanged rejected-resume checkpoints, shutdown before first poll, concurrent
submission/shutdown, and failed quota admission followed by a successful retry.
The concurrent test also passed 30 consecutive standalone runs. The original
rejected-enqueue regression failed against the unchanged engine before the fix.

Rebuilt headless binaries passed local-process shutdown/recovery
(`job-shutdown-vXIWhf`), node commands (`node-commands-frrGFw`), and model/job
protocol regression (`model-metadata-MzTiFh`). The latter uses a fake Ollama
backend, not real model inference. Formatting, diff, documentation-link and
backend/browser boundary checks passed. No physical two-PC deployment or new
installer validation is claimed for this unreleased increment.

## 2026-09-22 documentation-only direction: presence and media

Recorded the owner's [presence and supervised media requirements](PRESENCE-AND-MEDIA.md):
fixed virtual audio input/output, user-preferred microphone/TTS mixing and output
routing, optional assistant participation, reusable lightweight presence for
welcoming/locating/handoff, and EEF-approved peer or coordinator-relayed sessions.
The design includes concurrent resource use, bounded bandwidth, local sensing,
authorization expiry and explicit privacy/automation preferences.

All remain planned. Development was paused at that checkpoint; no runtime code,
installed node, configuration, permission, release, model or update feed changes
accompanied that documentation. Alpha.6's actual validation remains recorded below.

## v0.4.0a6 release checkpoint

Alpha.6 packages the shutdown-order and command-response fixes documented below.
Those checkpoints describe their original pre-release validation, not alpha.6
availability. See [release notes](RELEASE-0.4.0a6.md) for final artifact and live
deployment evidence. Automatic-update feeds remain paused; no self-development
agent, permission expansion or model installation is introduced.

The alpha.6 optimized regressions and both installer validation suites passed.
The local coordinator is upgraded with configuration, database and startup choice
preserved. The physical two-PC in-flight interruption test now passes against an
existing alpha.5 node: interrupted checkpoint, unsafe-resume refusal, no observed
replay, and no late-response state change. The historical pre-release notes below
are retained as the validation sequence, not the current deployment state.

The release is published as a prerelease with two hash-verified installer assets.
The physical node was then updated from alpha.5 to alpha.6 through one exact-
artifact CLI request and a separate approved restart. Its staged binary passed
an on-node Defender scan, stable identity/configuration/permissions were preserved,
and all five version-matched probes passed. Both live automatic feeds remain
paused. This validates a successful explicit update, not crash-safe update
receipts, rollback or model workloads.
The physical interruption and durable-job persistence regressions also passed
again with both roles on alpha.6; test-owned history was saved and then removed.

## Post-alpha.5 interrupted-job shutdown ordering

Live two-PC tests on installed alpha.5 verified successful and failed remote
read jobs, bounded safe retries, subsequent healthy work, and persistence of
full stored job results across coordinator restart without redispatch.

A separate in-flight process test exposed a shutdown race: the gateway closed
before the job engine checkpointed interruption, so its synthetic "node gateway
stopped" response could mark an uncertain remote action as failed. The action
was not retried, but its state was misleading. Shutdown now interrupts and
drains job runners before stopping the gateway. This does not cancel remote
effects or establish exactly-once execution.

`tools/test-job-shutdown.mjs` reproduces the problem with the published alpha.5
binaries using an isolated held HTTP request, without shell commands or personal
settings. The rebuilt headless source passes interruption, unsafe-resume refusal,
no replay, late-response isolation and subsequent healthy-work checks. The
installed alpha.5 applications and published installers are unchanged; the fix
requires a future build. Private live-test records remain outside the repository.

Validation: 185 Rust tests passed with and without dashboard support. The new
process regression failed against the published alpha.5 binaries and passed
four consecutive times against rebuilt headless binaries. Existing node-command
and explicit-update suites, formatting and backend/browser boundary checks passed.
Live test histories were removed only after their evidence was saved; the real
network remains on alpha.5 with no active test jobs or changed permissions.

## Post-alpha.5 command compatibility diagnostics

The physical update test found an empty HTTP 404 when a new restart CLI contacts
an older coordinator without that endpoint. The CLI now explains the possible
coordinator-version mismatch instead of exposing a JSON EOF error. Malformed
responses include HTTP status and unconfirmed-outcome guidance. No fallback,
retry, permission change or new command endpoint is introduced. This source
change is not in the already-published alpha.5 installers.

Validation: 185 Rust tests passed with and without dashboard support, plus
formatting and backend/browser boundary checks. A rebuilt headless CLI issued a
read-only model-list request to the real older coordinator and reported the
actionable HTTP 404 error with exit status 1, without fallback or retry.

## v0.4.0a5 release checkpoint

Alpha.5 includes the explicit-update correction below and a paused-feed response
that contains no artifact. Both public feeds are withdrawn while all releases
are prereleases. New clients report `feed_paused`; older clients fail closed.
See [release notes](RELEASE-0.4.0a5.md) for build and deployment evidence.
The following explicit-update checkpoint describes source validation before
alpha.5 packaging; its historical "unreleased" label is no longer its availability.

## Unreleased explicit remote update correction

- `eef node update` selects an exact version/HTTPS artifact/hash/size, with
  per-request prerelease consent. The distinct authenticated node action honors
  remote-update approval, checks current owner revocation again before activation,
  preserves stable-only automatic feeds and does not need process/filesystem grants.
- A shared installation lock prevents concurrent explicit/feed installs; newer
  pending selections are preserved. Bundle product/version and artifact integrity
  are verified. `eef node update-status` reads the actual installed selector.
  Restart remains a separate approved command. No command or automatic fallback
  can silently opt old nodes into prereleases.
- The fix is not inside published alpha.4. No published installer, installed
  node, stable manifest or teacher-submission repository is changed by this source
  increment. See [command and recovery limits](COMMANDS.md#explicit-remote-updates).
- Validation checkpoint 2026-09-20: all 183 Rust tests passed with and without
  dashboard support. Tests cover strict one-shot consent, URL/hash/size validation,
  downgrade/pending selection refusal, installation locking, corrupt selectors,
  artifact integrity, product/version matching, activation-time revocation and
  unchanged configuration. Stable-feed tests still reject prereleases even when
  a manifest includes opt-in fields. Payload fixtures are never executed.
- `explicit-updates-fx10vh` passed real local command/transport denial and
  permission revocation with headless binaries; mock coordinator replies cover
  exact confirmation, lost replies, older nodes and no replay/fallback. No real
  release download or installation was performed by this process suite.
  `node-commands-YnBFtD`, formatting, JavaScript syntax and backend/browser
  dependency checks passed. An earlier dashboard-enabled command run also passed
  (`explicit-updates-B2o0VA`).

## v0.4.0a4 release checkpoint

The following development checkpoints through owner-selected request preview
are included in v0.4.0a4. Their historical "unreleased" labels describe their
original validation dates, not alpha.4 availability. See the
[release notes](RELEASE-0.4.0a4.md) for packaging and deployment evidence.
No default interpreter is approved and preview never dispatches work.

## Unreleased owner-selected request preview

- `eefn requests preview --text TEXT --json` now uses exactly one applied model
  with `llm.infer` and the explicit `request_interpreter` role. It uses the shared
  strict proposal contract, retains original input, requires reported completion,
  and always reports no dispatch/no execution authority. No job, permission,
  default model, download or coordinator submission is created.
- The service has one-preview capacity, a 20-second deadline and a 512-token
  output limit. A weak runtime binding is invalidated on node restart so old
  results cannot become current proposals. Missing/ambiguous roles, invalid or
  partial results and unavailable backends fail without fallback. CLI discovery,
  target/correlation checks, origin guard and no-replay behavior work without UI
  or an EEF connection. External providers may continue compute after disconnect.
- Validation checkpoint 2026-09-20: 178 Rust tests passed with and without
  dashboard support. `request-preview-nFGNN7` passed paused-node CLI preview,
  original text retention, invalid/incomplete rejection, role checks, exact target,
  origin guard, busy admission, timeout/capacity release, restart invalidation,
  lost-reply/no-retry and old-endpoint/no-fallback checks. `model-startup-8ieqHk`
  additionally passed llama.cpp preview and model lifecycle regressions.
  `model-metadata-Qv24TM` and `model-downloads-Fy8QLw` passed existing model/job,
  response-boundary, approval and transfer regressions. All process backends were
  synthetic, not a real interpreter quality or physical two-PC acceptance claim.
- Automatic chat interpretation, interactive clarification/approval, EEF context
  validation and execution grants remain planned. No release, installed node or
  teacher-submission snapshot changed. See [usage](COMMANDS.md#request-preview-development)
  and [contract limits](INTERPRETATION.md).

## Unreleased bounded inference response handling

- Ollama and llama.cpp now share bounded chat normalization: 4 MiB backend JSON
  limit for declared and chunked bodies, rejection of missing/non-text completions
  and provider errors, plus additive `finish_reason` / `completion_complete`
  metadata. Missing or unknown completion evidence stays unknown. A valid result
  never grants tool execution and requests are not automatically retried.
- Validation on 2026-09-19: 176 Rust tests passed with and without dashboard
  support. `model-startup-uO7sIk` checked completed/partial/missing llama.cpp fixture
  responses over the real node connection along with startup/crash regressions.
  `model-metadata-sn1HSi` checked complete/partial/unknown Ollama fixture responses,
  missing content, declared/chunked overflow, stalled-body timeout with no replay,
  and existing model/job/approval behavior. `model-downloads-tW5Y1e` passed transfer
  regressions. Source boundary, formatting and diff checks passed.
- Fake backend acceptance only for this increment. No interpreter was enabled,
  no additional model weights were fetched, and no installed node or release was
  changed. See [response contract](NODE_MODELS.md#inference-response-bounds-development).

## Unreleased interpretation contract foundation

- Added a shared typed Rust proposal parser/prompt. Original input is preserved;
  version/type/byte/collection limits, mandatory nullable fields, duplicates,
  unknown fields, capability context and clarification consistency are checked.
  Every accepted proposal remains untrusted and requires review or clarification;
  it cannot provide execution authority, origin identity or target/grant fields.
- This is not integrated into chat, command submission, EEF planning or live
  interpretation. No model role/default or automatic actions were enabled. The
  test-only stdin example is not packaged; the CPU harness's `contract` profile
  uses that native parser rather than duplicating validation in JavaScript.
- Validation on 2026-09-19: 173 Rust tests passed with and without dashboard
  support. Process suites passed in `model-startup-BRHv2I`,
  `model-downloads-YMYajE` and `model-metadata-ITt7MW`. Real CPU contract evaluation
  in `model-benchmark-gmKt3L` completed: 6/12 outputs were structurally accepted,
  0/12 matched both expected intent and clarification. All accepted reports
  retained original text and `execution_authorized: false`. The candidate remains
  unapproved; schema validation is not semantic accuracy or a safety proof.
- See [contract and remaining integration](INTERPRETATION.md) and
  [candidate evaluation](MODEL-CANDIDATE-EVALUATION.md). No additional weights
  were downloaded for this increment; installed nodes and teacher snapshot stayed
  unchanged.

## Unreleased lightweight-model CPU evaluation

- Added an opt-in bounded CPU benchmark that verifies an existing catalog
  artifact and runs fixed synthetic classification inputs without executing
  actions or changing node configuration. It records runtime identity, startup,
  Windows peak working set, latency and strict output-schema/expected-intent counts.
- On 2026-09-19 the pinned 491,400,032-byte candidate was downloaded once into
  ignored `.validation/model-artifacts/` and its SHA-256 verified. Real CPU runs
  completed in `model-benchmark-MYTDGw` and `model-benchmark-5uqQZo`: baseline
  4/12 expected classifications, detailed prompt 9/12. No default was approved.
  See [method, evidence, upstream license references and limits](MODEL-CANDIDATE-EVALUATION.md).
- This is one-PC runtime feasibility, not physical two-PC acceptance or a
  security evaluation. Installed nodes, stable update feeds and teacher snapshot
  remain unchanged. The reusable development model cache uses about 469 MiB.

## Unreleased post-startup model process monitoring

- A runtime-scoped monitor checks the owned GGUF group once per second after
  readiness. An observed exit, missing owned process or process-status error
  withdraws group availability and stops remaining owned model children. Node
  commands remain available; saved selections are retained and retry requires
  explicit restart. No work is replayed and no automatic respawn is performed.
- Existing authenticated registration refresh propagates the error without an
  intentional reconnect. Local model/capability status also updates with the
  connection paused or unreachable. Runtime exits are not startup issue codes.
  Detection and propagation are not instantaneous; ongoing HTTP health, hangs,
  successful inference, per-slot recovery and resource usage remain unmeasured.
- Validation on 2026-09-19: 167 Rust workspace/all-target tests passed with and
  without dashboard support. `.validation/model-startup-c2Vj4K` killed a ready
  fixture child and verified unavailable/error metadata, surviving-sibling
  cleanup, command availability, unchanged connection/runtime identity, explicit
  restart recovery and paused-connection failure reporting. Existing loading
  and restart-cleanup checks also passed. `.validation/model-downloads-ltpT2Z`
  and `.validation/model-metadata-QbhXFk` passed transfer/receipt/approval and
  model/job regressions. The backend/browser boundary check and formatting passed.
- These are isolated fake-backend process tests, not real inference or physical
  two-PC acceptance. No model weights, release, installed node, stable update
  policy or teacher-submission snapshot was changed.

## Unreleased model install preflight

- `eefn models install-plan --model ID` inspects the provider, source/license
  metadata, versioned GGUF artifact contract, local disk snapshot, cache candidate
  and active-download state without fetching weights or changing node state.
- Installation uses the same artifact validator before admitting a transfer.
  Invalid/ambiguous catalog entries and known disk failures leave prior receipts
  untouched. Cache reuse checks exact size as well as digest. Existing remote
  approval/configuration admission checks and transfer-time limits remain in force.
- Explicit llama.cpp selection no longer contacts Ollama to decide its backend.
  Ollama plans do not mislabel GGUF size/hash as provider metadata. License review,
  runtime compatibility and default-model/bootstrap policy remain unverified or
  planned; no model artifact, installed node or release was changed.
- Validation on 2026-09-13: 165 Rust tests passed with and without dashboard support,
  including unsafe/incomplete artifact metadata, insufficient/unknown space,
  cache size/digest semantics, duplicate catalog IDs and preservation of prior
  receipts after failed admission. Isolated `model-downloads-LaQNkt` verified
  read-only CLI preflight, provider-specific unknowns, active-transfer blocking,
  no Ollama request for explicit GGUF, and rejection after a catalog change.
  `model-metadata-7fbTQb` and `model-startup-0rIH0b` passed the existing model/job,
  approval, receipt and startup regressions. Evidence is under `.validation/`;
  fake backends only, no real inference or physical multi-node acceptance.

## Unreleased command-based model installation

- `eefn models installed`, `inspect`, `install`, `download-status` and
  `cancel-download` use a typed local command service backed by the existing model
  installer. They need a running node but no browser, EEF connection or model.
- CLI install attempts carry a pre-send UUID. Lost replies remain unknown and
  are never automatically replayed. Exact-ID status/cancellation cannot target
  a newer transfer; the currently retained ID cannot be admitted twice.
  Only the latest in-memory transfer is retained, not a durable receipt ledger.
- Selection commands remain compatible. Installation does not select/load a
  model, enable permissions or restart the node. No default model, bootstrap
  policy, UI changes, release or physical-node deployment is included here.
- Validation on 2026-09-13: 161 Rust tests passed with and without dashboard support.
  Isolated `.validation/model-downloads-lkTc15` passed installed-model inspection,
  explicit admission, progress after CLI exit, exact-ID cancellation, one lost
  reply recovered by receipt, refusal of duplicate/stale IDs and no legacy API
  fallback. `.validation/model-metadata-Cq58tM` and `model-startup-vW9fZA` passed
  the existing model/job/approval and nonblocking-startup regressions. Fake local
  backends only, no real model downloads/inference or physical two-PC claim.
  Validation disabled debug symbols and incremental compilation to limit disk
  growth; no installed application, default update feed or global Git identity
  was changed.

## Unreleased nonblocking local model startup

- GGUF group startup no longer holds up node registration, heartbeats, jobs or
  restart commands. A scoped loader is cancelled with the runtime and cleans up
  partially started children. Startup failure retains owner selections.
- Loading/error/unloaded groups remain visible with unavailable metadata; both
  capability advertisement and direct inference refuse non-ready groups.
  Readiness refreshes the existing authenticated registration without reconnect.
  EEF preserves unchanged capability in-flight counts, limits and measured load
  atomically while replacing the advertised capability snapshot.
- No default model, interpreter, inference weights, peer transport, UI changes,
  installer changes or physical-node deployment are included in this increment.
  Group readiness is not ongoing health monitoring or per-slot lifecycle control.
- Validation on 2026-09-13: 157 Rust workspace tests passed, including the
  dashboard-free build and duplicate-capability refresh/accounting regression.
  The initial default-feature pass also passed all 157 tests. Final API-only
  process fixtures passed in `.validation/model-startup-Rt9wAe` and
  `.validation/model-metadata-2tkVvQ`: registration/commands while health is gated,
  restart cleanup of loading and ready children, readiness refresh without a
  reconnect, paused-connection loading/readiness, invalid/missing optional runtime
  isolation, job/receipt recovery and approval checks. Fake backends only, not
  physical two-PC inference. Source boundary, formatting and diff checks passed.

## v0.4.0a3 release preparation

This opt-in alpha packages the backend checkpoints below. Stable update feeds
remain unchanged. Both roles now reject prerelease feed updates at check and
installation time; a changed manifest cannot bypass the installation guard.
See [release scope and limits](RELEASE-0.4.0a3.md).

## Unreleased optional runtime startup isolation

- Failed llama.cpp startup now cleans up the local model group and continues
  node registration without advertising that group. Missing Python or failed
  plugin inspection withholds affected capabilities rather than disabling the
  core connection. Owner settings remain unchanged; no silent model substitution.
- Status and diagnostics expose bounded `startup_issues` codes, without raw
  exception text or paths. This is a startup snapshot, not continuous health.
  Nonblocking model loading and individual slot recovery remain future work.
- All 153 Rust tests passed both with and without dashboard features on
  2026-09-13. Isolated `.validation/model-metadata-rIcvW6` verified connected job
  commands and system ping despite missing model/Python runtimes, plus approved
  remote restart and recovery. It also passed receipt-loss and authorization
  revocation checks. No physical model/hardware test or deployment is implied.
- Command/discovery, first-run and browser-copy regressions passed:
  `.validation/node-commands-oRA9UM`, `.validation/peer-discovery-UfZBLg`,
  `.validation/first-run-Zbjo2h`. No model download or installed-node change.

## Unreleased job receipt recovery

- The node CLI generates an operation receipt before sending. Updated EEF stores
  it in the authenticated job origin context while keeping transport reply IDs
  independent. `eefn jobs find --operation-id UUID` searches only that origin's
  retained creation history, filtering before the bounded result page.
- Lookup does not replay work or deduplicate submissions; an empty match is not
  proof that an uncertain request was never accepted. It is not a control-command
  receipt ledger. See [command recovery semantics](COMMANDS.md).
- All 152 Rust tests passed on 2026-09-13, including persistence/reopen, filtering
  beyond the first 100 jobs, origin scoping and independently correlated replies.
  Isolated `.validation/model-metadata-bnagjD` deliberately dropped the local
  creation reply, verified exactly one submission and recovered the saved job
  using the CLI receipt. It also passed the earlier model/auth/job/restart checks.
- A read-only installed-coordinator check still showed two connected nodes,
  two registered models and no pending restart. Those v0.4.0a2 installations were
  not updated; new-code execution used a fake backend, not physical inference.

## Unreleased remote authorization race fixes

- Legacy remote configuration writes now read, authorize and save under the
  same lock as local commands. They preserve concurrent owner changes and
  cannot restore revoked management approval. Denied writes remain proposals.
- Remote model installs recheck approval at download admission after backend
  inspection, and reject changed model configuration. Approval revocation does
  not retroactively cancel already admitted work; explicit cancellation remains
  available. No new model source or automatic download was added.
- All 150 Rust tests passed on 2026-09-13. The isolated process regression
  `.validation/model-metadata-E9N5Nq` revoked permission during fake backend
  discovery and verified that no pull started, then checked denied remote saves.
  No model weights, installed apps, physical node settings or releases changed.

## Unreleased backend/browser separation checkpoint

- Browser assets are optional in both crates; both executables support `--no-ui`.
  Node HTTP routes moved to `api.rs`, with core status/configuration, connection,
  proposal, startup and update operations owned by `NodeService`. Browser files
  contain presentation routes/native picker only. A source dependency guard
  checks that runtime modules do not import the browser/HTTP adapters.
- Added `eefn connection show/pause/resume/pair-local`. Pausing the outgoing EEF
  connection does not disable the local command API. Connection config edits
  serialize with model edits, preserving unrelated saved settings.
- Saved `dashboard.ui_enabled` / `web.ui_enabled` preferences hide just the
  browser, without changing legacy listener enablement. The node adapter can
  change its browser gate without rebinding its command listener. CLI `--no-ui`
  remains an override; no-dashboard builds cannot enable browser assets.
- 148 default-feature Rust tests passed on 2026-09-12, including browser-gate
  and configuration validation. Saved-preference integration passed in
  `.validation/model-metadata-S3J9GL`: both roles' commands worked with UI hidden,
  node UI enable/disable preserved the command endpoint, and connection controls,
  models, jobs and restarts passed against an isolated fake HTTP backend.
- The earlier no-dashboard build passed 146 Rust tests and process integration
  (`.validation/model-metadata-2i2Vce`); all 148 tests also passed without dashboard
  features on 2026-09-13 after the saved-preference follow-up. This is not real inference or physical
  two-PC acceptance. No installed app, model, release feed or signing status was
  changed. See [boundary and remaining scope](BACKEND-BOUNDARY.md).
- Follow-up command/discovery, first-run/job and browser-copy regressions passed
  on 2026-09-13 (`.validation/node-commands-6qd4MR`,
  `.validation/peer-discovery-yOxtQ3`, `.validation/first-run-RPsvjj`).

## Unreleased node-origin job commands

- Added `eefn jobs list/get/output/pause/resume/stop/remove/generate-text` and
  guarded typed `/api/commands/jobs`. The runtime service uses the running node's
  existing submission connection and actual API discovery; no second node,
  browser controller, local LM or remote-management permission is required.
- EEF preserves authenticated origin ownership and existing durable lifecycle
  rules. Creation acknowledges a job, not completed work; pause/stop report
  intermediate states. Commands distinguish known-not-sent from rejected or
  uncertain mutations and never automatically resubmit after a lost reply.
- Explicit output inspection returns bounded text-model content only, while
  ordinary status remains redacted. It uses the same origin check as controls;
  it does not export file contents, media, parameters or another origin's results.
- 144 Rust tests passed on 2026-09-12. Isolated integration evidence:
  `.validation/model-metadata-YCa5L1` verified origin scoping, text output,
  pause/resume without repeated execution, stop and finished-history removal,
  alongside remote model selection and restart tests. The backend was a fake
  local HTTP service, not real inference or a physical two-PC test.
- Command and discovery regressions passed (`.validation/node-commands-9NIgvh`,
  `.validation/peer-discovery-ltgV03`), as did first-run/job persistence and
  browser-copy regressions (`.validation/first-run-cZdAMC`). No installed app, release artifact or
  model was changed or downloaded. UI development remains paused.

## Unreleased coordinator-to-node model commands

- EEF now exposes `node models` using the same typed model-selection operations
  and CLI adapter as EEFN. Mutations require node-local management approval,
  rechecked under the config lock. Inspection remains read-only. No implicit
  downloads, loads, restarts or legacy full-config save fallback.
- Requests are sent once; uncertain replies are explicitly unconfirmed. Remote
  GGUF/projector paths are validated on the target node, not the coordinator.
- 140 Rust tests passed on 2026-09-12. Isolated integration evidence:
  `.validation/model-metadata-FyzHyP`; old-node refusal/compatibility:
  `.validation/model-metadata-r6gwng`. Command, discovery, first-run/job and
  browser-copy regressions passed (`.validation/node-commands-gHb7T0`,
  `.validation/peer-discovery-lez654`, `.validation/first-run-6iYR9q`).
- A read-only check of the installed coordinator still reported two connected
  nodes and no pending restart. These installed v0.4.0a2 apps were not changed;
  the new feature tests used isolated processes and a fake backend, not real
  inference. No release or automatic telemetry upload was performed.

## Unreleased explicit model-role routing checkpoint

- Added bounded, read-only `eef models route` and its guarded API. It previews
  connected candidates by capability/role/node/backend/model without inference,
  permission changes, readiness claims or reservations.
- Existing routing enforces explicit `model_role` and backend constraints even
  during tier fallback. Existing durable jobs support `generate_text` and
  `analyze_image`; automatic interpretation/planning remains pending.
- 135 Rust tests passed on 2026-09-12. Isolated integration evidence:
  `.validation/model-metadata-y8lWNI`. A fake backend verified exact role dispatch,
  missing-role job failure without fallback, and no inference during preview.
  This is not real inference or a physical two-PC test. Installed apps and
  published releases remain unchanged.

## Unreleased local restart and API discovery checkpoint

- Added `eefn restart` with local-owner node/runtime preconditions, one send,
  bounded completion observation and explicit uncertain outcomes. It shares the
  node core restart operation with existing browser/approved remote controls.
  Completion is a new initialized runtime, not a workload/model/reconnection claim.
- A bounded per-config local API marker records the actual bound endpoint.
  Network/model/input commands and restart polling follow it instead of pending
  port settings. It is identity-checked and loopback-only, with old-node fallback
  only when the marker is absent. No remote listener was added.
- 133 Rust tests passed on 2026-09-12. Isolated process testing verified restart
  and API port migration (`.validation/model-metadata-YyzqX8`); command and
  discovery tests passed (`.validation/node-commands-g9t1Vl`,
  `.validation/peer-discovery-QXwoD4`), as did first-run/job/browser regressions
  (`.validation/first-run-ezjgWB`). No real model download or release deployment.

## Unreleased node model-selection commands

- Added shared node `models show/select-ollama/select-gguf/hints/provider/remove`
  commands and the guarded local owner API. Offline commands use the instance lock;
  running commands use the existing node service and configuration lock. No JSON
  editing, model download or automatic restart is required to save a selection.
- Extended existing backend selections with optional capability restrictions and
  role labels, projected into metadata and enforced by updated node executors.
  Unsupported capabilities are rejected; an empty capability list disables that
  model's inference operations. Role-based scheduling is still pending.
- Saved selections and registered models remain distinct. No-op edits avoid writes;
  corrupt/missing config, wrong targets and invalid hints are rejected. Removal
  affects selection only, not model files. GGUF ports can be allocated internally.
- 130 Rust tests passed on 2026-09-12. Evidence:
  `.validation/model-metadata-3wV2vO`, `.validation/node-commands-YJoF6P`,
  `.validation/first-run-lVU6xZ`, `.validation/peer-discovery-sDhh29`;
  browser copying/focus checks also passed. Tests use isolated configs and a fake
  backend, not real inference. Published apps/feeds and the real network were not
  changed. See [commands and downgrade caveat](COMMANDS.md).

## Unreleased versioned model metadata checkpoint

- Added a shared, bounded `model_metadata` registration schema: multiple
  capabilities/input/output modalities, optional roles, lifecycle, availability
  and resource estimates. New Ollama/GGUF advertisements preserve legacy fields.
  Unknown values remain unknown; model names never infer capabilities.
- Registration validates the entire model snapshot before replacing it, including
  empty snapshots. Other nodes and load metrics are preserved. Duplicate identities
  and malformed/unsupported metadata are rejected, not silently downgraded.
- Inventory and scoped peer discovery carry validated metadata. Existing text/vision
  routing filters declared capabilities, input/output support and unavailable/error/
  loading state. Backend-specific failures no longer poison another backend's model;
  updated nodes enforce an explicitly requested backend without silent fallback.
- General owner configuration for non-text/vision models, role-based planning,
  default LM, resource-aware admission, lifecycle controllers and leases remain
  pending. See [contract and compatibility](MODEL-METADATA.md). No UI changes.
- 124 Rust tests passed. Isolated fake-backend protocol tests passed with both
  development binaries (`.validation/model-metadata-wnlYlL`) and the published older
  node (`.validation/model-metadata-G5SVAH`), using separate temporary configs.
  They verify registration, CLI/inventory, stable identity, model-list clearing,
  and new-node backend enforcement. These are not real inference or two-PC tests.
- Command integration (`.validation/node-commands-uOMOCv`) and scoped discovery
  (`.validation/peer-discovery-rxQzSI`), first-run/job persistence
  (`.validation/first-run-OQ15vZ`), and dashboard copying/focus regressions passed.
  Validation recorded on 2026-09-12. Published
  installers/feeds and both real installed apps remain unchanged; no model was
  downloaded. No signing or Microsoft clearance is claimed for this checkpoint.
  Both real nodes remained connected; installed executable hashes were unchanged.

## Unreleased model-inventory and approved restart checkpoint

- After the owner enabled local remote management, the physical second-PC node
  acknowledged restart and reconnected under the same stable ID with a new runtime
  ID. Evidence: `.validation/live-discovery-QGI8x0/results.json`. This closes the
  positive remote-restart gate left pending in the earlier checkpoint below.
- Added `eef models list` and guarded GET `/api/commands/models`: bounded pages of
  registered models, node/capability filters, distinct node/backend/model identity,
  and registration freshness. Unknown roles, lifecycle and resource estimates are
  null; paths, addresses and secrets are omitted. No backend scan/load/download.
- This is a read-only legacy normalization slice. General multi-capability model
  configuration and registration, role selection, and scheduling changes remain
  pending; published v0.4.0a2 nodes remain compatible. No visual UI work.
- 113 Rust tests passed, including model normalization, redaction, empty inventory,
  identity conflicts, paging and response limits. Command, peer-discovery,
  first-run and browser copying/focus regression checks passed.

Final inventory checkpoint evidence (2026-09-12 local time):
`.validation/node-commands-SzT9ir`, `.validation/peer-discovery-vsS9dj`,
`.validation/first-run-VTll5F`, and
`.validation/live-discovery-3S9iBX/results.json`. The live run used two physical
PCs and published v0.4.0a2 nodes. Inventory reported two advertised models on the
local node and an empty model list on the connected second-PC node. Discovery
grant/revoke, two coordinator restarts, and approved remote node restart passed;
all ten remote pings passed at 2.82-6.14 ms. No inference or model download ran.
The development coordinator SHA-256 was
`18a9a4189cc0711a5a60296448325e427ff398b9bc245baac500f96e9548aefc`.
Its local Defender scan passed with engine `1.1.26080.3`, definitions
`1.459.158.0`; this is not Microsoft clearance or a signing claim.
The original discovery policy and published coordinator executable were restored
(hash verified); both nodes reconnected with no pending coordinator restart.
No release assets, feeds or remote node binaries were changed.

## Unreleased command/restart follow-up

- Added coordinator-owner `discovery show/grant/revoke` and `restart` commands,
  backed by shared core services and guarded local APIs. Exact, directional
  disclosure only; saved/applied policy and restart completion remain distinct.
- Serialized saved-config edits preserve unrelated pending settings, reject
  corrupt/missing configuration and keep no-op grants from rewriting files.
- Added `eef node restart --node ID`, using the existing node-owner management
  approval. Bounded completion checks require the same stable node ID and a new
  runtime ID. Lost replies/timeouts are uncertain, not automatic restart retries.
- Bounded outbound request queue waits and cleaned up cancelled response waits.
  These changes are in the coordinator's shared transport; no new wire protocol.
- 109 Rust tests passed. Physical testing confirmed real two-node
  registration, grant/revoke behavior, reconnect after two coordinator restarts,
  and 10/10 remote pings. The remote node's restart permission was off: denial
  passed; positive physical remote restart was confirmed in the subsequent checkpoint above.
- Published v0.4.0a2 installers/feeds are unchanged. No UI redesign, permission
  widening, model downloads or signing claim. See [commands](COMMANDS.md).

Final follow-up evidence (2026-09-11): `.validation/peer-discovery-21turW`,
`.validation/node-commands-BUeKRY`, `.validation/first-run-LrJf2O`, and
`.validation/live-discovery-wIG53K/results.json`. The live run used two physical
PCs with published v0.4.0a2 nodes and a locally built development coordinator
(SHA-256 `8fadfbfd78a64be8e3f6d8658ed4b7a8aa9ac909abf4217919cb51abdd379eda`).
Its local Defender scan passed with engine `1.1.26080.3`, definitions
`1.459.158.0`; no Microsoft clearance is implied. The final ten remote ping
samples passed at 3.00-5.78 ms. Reconnect/grant/revoke and denied remote restart
passed; no actual second-PC node restart was sent without local approval.
After restoration, both nodes reconnected, no pending restart remained, the
original self-only discovery policy was applied, and the published executable's
hash matched its backup. No release assets or test network secrets were published.

## v0.4.0a2 diagnostic alpha

Workspace version `0.4.0-alpha.2` packages the command/service and scoped-discovery
increments below, plus minimal local diagnostics, authenticated ping/latency
probes and explicit-address private pairing codes. See
[release scope](RELEASE-0.4.0a2.md) and [two-PC testing](TWO_PC_TEST.md).
98 Rust tests and final packaged command/discovery/first-run/browser and installer
validation passed; hashes and scan details are in the release notes. Physical
connectivity was subsequently confirmed; full workload acceptance remains pending.
MSI, default LM, independent peer credentials and leases are not
claimed by this test release. Diagnostic reports are local, not automatic uploads.

## Active direction (2026-09-11)

[Command-first core roadmap](ROADMAP-CORE.md) supersedes the original milestone
order. UI polish is paused. Shared command services, identity/address separation,
coordinator advertisements, general model lists, a default lightweight node LM,
structured interpretation, authorized workloads and local/peer execution are
the next increments. Most are still planned extensions, not completed features.
The published v0.4.0a implementation below remains the baseline; its installers
still do not supply/download a model.

Implementation packaged in v0.4.0a2: shared `NodeService` state/model operations, network
show/set commands, API-only `--no-ui`, validated independent node/coordinator
advertisements and EEF node-view propagation. 91 Rust tests and command/browser
integration passed; see [commands and limitations](COMMANDS.md). Peer-address
distribution is now implemented as exact owner-scoped pull discovery with
monotonic freshness and bounded sanitized pages (`network peers`). Discovery
does not authorize direct communication. Leases, general model metadata,
default LM and MSI remain pending. The discovery follow-up passed 97 Rust tests
and a two-logical-node command integration test; physical acceptance is pending.

## v0.4.0a alpha scope

The earlier workspace was `0.4.0-alpha.1`, presented as **0.4.0a**. The owner approved
shipping this reduced alpha scope and deferring the remaining features.
See [RELEASE-0.4.0a.md](RELEASE-0.4.0a.md) and [ROADMAP-0.4.0.md](ROADMAP-0.4.0.md)
for the historical prerequisites and seven milestones. They are not all
implemented; development now covers the reliability prerequisite and an
origin/area/resource-routing, model management, and durable one-shot job
implementations with automated acceptance checks.

- Node-first browser and CLI input over the running node's authenticated
  connection; bounded pending requests, no submission replay after disconnect.
- Encrypted EEF reachability probe before authentication/registration, without
  creating a node record. Older EEF's encrypted auth denial is accepted only as
  service reachability; a fresh socket still requires normal authentication.
- Persistent Stop/Resume connection controls. Stop waiting for a reply is
  explicitly distinct from cancelling work already running on other nodes.
- Mutating/unknown tasks no longer retry automatically unless their caller
  explicitly sets `constraints.retry_safe`; read-only retry behavior remains.
- Feature-wide permission switches, retaining default denial and old scoped
  configurations. Remote update installation requires its own permission and
  cannot replace the locally selected update feed.
- Compact navy/blue dashboard, node terminology, stable DOM updates that preserve
  selection and editing, and decorative icons excluded from accessible names.
- Bounded inbound frame reads, random authentication nonces, atomic live identity
  reservation, and response matching against the authenticated executor.
- Update extraction checks unpacked size against free space, stages new files,
  and refuses to delete an already installed version. No fixed 5 GB reserve.
- Optional signing build hooks and certificate-aware payload framing. No public
  certificate has been configured and no signed release has been produced.
- Authenticated submission-origin snapshots retained through goals, tasks,
  inference transport, events, replies and bounded SQLite conversation history.
  Compatible metadata registration and additive history migration.
- Hierarchical node/resource areas, node-qualified stable resource IDs, owner
  adapter bindings and locality-aware routing after suitability/freshness checks.
  Explicit unavailable resources fail without falling back to another resource.
- Node location/resource forms, also available through EEF remote configuration.
  Resource creation is independent of permission grants and hardware activation.
- Remote-save now explicitly includes validated metadata; remote status returns
  applied areas/resources and last-request diagnostics. Tests cover both proposal
  approval and trusted remote management without changing identity or grants.
- Model selection inspects Ollama's reported capabilities on demand. Unknown
  capabilities require an explicit owner choice; embedding-only models are not
  offered as text generation. Selected vision models can also serve text requests.
- Model-service metadata is bounded to 4 MiB; progress lines to 64 KiB and 4096
  layers. Progress retains layer counts during verification. Cancellation has its
  own terminal state and optional job-ID matching; it is not an installation error.
- GGUF downloads recheck remaining disk space during transfer, verify existing
  files before reuse, and preserve damaged files/indexes instead of overwriting
  them. The UI distinguishes node-drive free space from unknown Ollama storage.
- Update manifests are capped at 1 MiB and artifacts at 512 MiB, with optional
  exact `size_bytes`, hexadecimal checksums, and streamed bounds even without
  Content-Length. Artifacts are still buffered in memory within that limit.

- SQLite-backed one-shot job definitions, attempts, results, origin and executor
  assignments. Checkpoints precede dispatch and dependent steps. A local database
  ownership lock prevents two coordinators opening the same job journal.
- Interrupted jobs do not execute on startup. Explicit resume retains completed
  steps and refuses uncertain non-idempotent actions. Pause/Stop finish the current
  claimed batch; they do not acknowledge remote cancellation or undo effects.
- Jobs history, step details and Pause/Resume/Stop controls in the node app over
  its existing authenticated connection. Nodes see/control only their own origin
  jobs; the loopback EEF owner API can manage all jobs. Finished history removal
  is explicit. UI responses omit raw parameters, file contents and media results.
- Shutdown joins node submissions and interrupts active runners before reopening
  the journal. Cancellation drops scheduler capacity guards. Runtime instance IDs
  let integration checks distinguish the old instance from a completed restart.
- State-dependent recovery headroom within the logical job budget keeps safe
  interruption/stop/removal available even after lowering the quota. Older
  development journals migrate additively, including partial migration recovery.
  Dashboard forms configure history count and MiB budget; Jobs shows effective
  network usage. Physical disk exhaustion and corruption still fail closed.
- EEF configuration forms load saved preferences, not stale running settings,
  while status keeps reporting applied settings until restart. Subsequent saves
  preserve a redacted saved network key rather than reverting to the running key.

Known remaining boundaries: real two-PC acceptance, remote-task cancellation
acknowledgements, verified Ollama-server storage reporting,
physical resource acceptance, recurring jobs and controlled migration,
media relay/correlation, Wi-Fi and
Bluetooth controls, mesh/ESP-NOW, distributed optimization, and coordinator
consensus/replication/fencing are not completed by this slice. Defender reports
against v0.3.2 remain unresolved; passing development tests do not clear them.

This file distinguishes working behavior from planned architecture. It prevents
the open-source project from presenting an unsafe approximation as complete.

## UX/UI changes in 0.3.2

- Graphical per-user installer with folder picker, startup/launch choices,
  extraction progress, and a completion screen. No administrator bypass.
- Home, Devices, Models, Permissions, Network, Settings, and Advanced pages;
  form-based common settings, explicit save/apply/restart, and mobile layout.
- Persistent generated device ID, detected hostname, single-instance launch,
  and same-account local EEF pairing in either app launch order.
- Live connection/retry/error states and applied permissions; Windows hardware
  metadata is detected without opening the camera or recording audio.
- Owner-requested model downloads, progress/cancel, hash-verified GGUF catalog,
  and Ollama-first selection. No model is included or downloaded on installation.
- Device configuration through EEF's authenticated node transport. Owners
  approve proposals locally unless they explicitly allow remote management;
  remote requests cannot grant themselves that permission or replace device ID.
- Restart controls for both roles; optional automatic restart after saving
  device settings. An installed update is handed off when Restart is used.
- Local dashboards reject cross-site browser requests and non-loopback Host
  headers. They are local controls, not remotely exposed management servers.
- The old fixed 5 GB floor is removed. Payload space checks remain.

This is not completion of the entire proposed UX/architecture roadmap.
Automatic LAN discovery, per-camera/microphone selection, richer voice/provider
forms, persistent offline-device history, and a larger curated model catalog
remain future work. Offline history currently lasts for the EEF session.
Detection is not proof a media permission or driver works; live recording,
vision inference, Wake-on-LAN, and two-PC failover require device-specific tests.

## Maintenance changes in 0.3.1

- Hash-locked Windows Python runtime dependencies and retained vendor checksums.
- Component and installer Defender scan gates, dependency provenance, and
  per-file bundle inventories; see [ANTIVIRUS.md](ANTIVIRUS.md) for the
  original detection and the limits of the new passing scans.
- Fewer bundled llama.cpp executables, explicit installer launch consent,
  quiet-upgrade startup preservation, and clearer Windows version metadata.
- Isolated installation/upgrade regression coverage for both installers.

This maintenance release does not implement the proposed 0.4 architecture
roadmap or establish that the 0.3.0 detection was a false positive.

## Implemented in 0.3.0

- Rust coordinator and PC node executables in one workspace.
- Normal `node -> EEF -> selected node(s) -> EEF -> originating node` requests.
- Authenticated, encrypted node transport with configurable endpoints and
  priority-ordered reconnect/failover across the configured endpoint list.
- Live capability/model registration and load, latency, capacity, hardware,
  permission, modality, and constraint-aware routing.
- Dependency-aware task plans, concurrent ready subtasks, timeouts, retries,
  cancellation state, structured results, and node reassignment on retry.
- Structured identity, mutable, working, event, conversation, and task state.
- Default-deny node policies for scoped filesystem access, exact allowlisted
  process execution, application control, host/method-limited HTTP, configured
  STT providers, Wake-on-LAN, microphone, audio, TTS, camera, screen, and input
  control.
- Filesystem roots are canonicalized and junction/symlink escapes are rejected;
  public HTTP destinations are DNS-checked and pinned with redirects disabled.
- Owner-selected Ollama and llama.cpp models; no coordinator model and no model
  in the node bundle.
- Optional isolated Python plugin sidecars using the bundled CPython runtime.
- Configurable `off`, `prompt`, and `auto` verified updates for both programs,
  with version handoff and rollback.
- Local EEF and EEFN dashboards for full configuration, capability permission
  switches, and optional startup.
- Two self-extracting Windows installers with per-user defaults, config
  preservation, generated coordinator secret, and a 5 GB storage floor.
- Standalone EEFN operation with zero coordinators; EEF only becomes
  operational when at least one node connects.

## Not yet complete

- Automatic coordinator discovery, quorum election, replicated coordinator
  state, and partition-safe split-brain prevention. Configured node priorities
  already provide deterministic failover when the preferred EEF disconnects.
- A separately authenticated EEFN peer mesh that remains online when every EEF
  coordinator is unavailable.
- Transport plugins beyond TCP. The protocol is not coupled to a VPN vendor,
  but the pluggable transport interface is still future work.
- Recurring jobs, data dependencies for ongoing media workflows, remote execution
  fencing/deduplication, and controlled migration. One-shot interrupted graphs
  are retained locally and require explicit safe resume.
- Signed release manifests. Updates currently require HTTPS plus an exact
  artifact SHA-256.

Reliable coordinator election cannot guarantee both availability and a single
leader during arbitrary network partitions without quorum. The intended next
milestone is a fixed-membership, majority-lease coordinator cluster with a
durable term/vote log, followed by state replication. The mesh is a separate
node role and must not silently promote ordinary nodes into coordinators.
