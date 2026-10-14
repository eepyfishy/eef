# Node commands

v0.4.0a6 supports these commands without a browser. Deterministic commands need no LM. Run
from the installation directory, or supply `--config` to select a node config.
Older v0.4.0a installers do not contain them.

## Plain-language setup inspection (unreleased after alpha.6)

`eefn setup` explains the running node's connection, private receipt support,
saved/applied interpreter choices and next steps without JSON or port numbers.
`eefn --json setup` exposes the same shared backend report for clients. Inspection
does not change permissions/settings, download/load a model, probe inference,
restart an app or authorize work. Configured is not healthy; connected is not
approved. See [observations, compatibility and remaining setup](SETUP-CHECK.md).

## Explicit native command authoring (unreleased after alpha.6)

`eefn requests submit-process --text PURPOSE --program PROGRAM --arg=VALUE`
authors a native command without a JSON file or model. Repeat literal arguments;
optional working directory, timeout and exact executor choices are preserved in
the existing concrete request. It stops at compilation, normally
`awaiting_approval`; separate owner approval/execution and node permissions remain.
See [explicit input/receipt semantics](EXPLICIT-REQUESTS.md).

## Selected model residency (unreleased after alpha.6)

`eefn models memory-limit --model MODEL_ID --mib MIB` (or `--default`) and the
equivalent `eef node models --node NODE_ID memory-limit ...` save a Windows-only
owned-GGUF job-commit ceiling. Saved/applied separation, explicit restart and
current owner approval remain mandatory. It is not a RAM/working-set/VRAM budget
or control of shared Ollama. See [limits and failure semantics](MODEL-MEMORY-LIMITS.md).

`eefn model-lifecycle-policy show|set` and
`eef node model-lifecycle-policy --node NODE_ID show|set` configure eager/manual/on-demand
startup, optional idle unload and operation bounds in the same backend service.
Use the exact saved-policy digest and, when running, runtime ID. Saving does not
restart or infer; explicit restart applies changed policy, while a saved mismatch
pauses new idle unloads immediately. Local locked offline setup is also supported.
With `--startup on-demand`, eligible selected GGUF models stay cold until actual
authorized inference. Status, compilation and approval never load them. Loading
consumes the original request deadline and transfers readiness directly to an
active-use pin; failure/cancellation never silently retries or switches provider.
See [choices, bounds and idle timing](MODEL-LIFECYCLE-POLICY.md).

Shared Ollama inference has a separate applied request preference:
`set --ollama-keep-alive-seconds SECONDS` (-1 indefinite, 0 after response,
1-86400 seconds) or `set --ollama-keep-alive-default` (inherit server policy).
New code defaults to inheritance rather than forcing indefinite retention.
Saved changes require explicit restart. This does not stop/control the shared
service, prove residency or enable owned lifecycle commands for Ollama.

`set --thinking-mode inherit|disabled|enabled` saves a node-wide inference hint
for Ollama/llama.cpp, also used by local interpretation after explicit restart.
It uses the same owner approval, runtime and saved-digest checks. Inherit remains
the default; this is not proof that a model supports or honored the request.
Caller/model output cannot override it. See [thinking preferences](MODEL-LIFECYCLE-POLICY.md#thinking-request-preference-unreleased).

Interpreter sampling is also configurable through the policy command: decimal
`--interpreter-temperature`, `--interpreter-top-p`, `--interpreter-min-p`,
`--interpreter-presence-penalty`, `--interpreter-repeat-penalty`, integer
`--interpreter-top-k` and `--interpreter-seed`, or `--interpreter-sampling-default`.
Defaults remain unchanged. These saved owner choices require explicit restart,
apply only to interpretation/preparation and never authorize execution. See
[bounds and wire/default semantics](MODEL-LIFECYCLE-POLICY.md#interpreter-sampling-preferences-unreleased).

`eefn model-lifecycle show|load|unload|cancel` and
`eef node model-lifecycle --node NODE_ID show|load|unload|cancel` share the running
node's owned-GGUF service. Load/unload require exact runtime, generation and
operation UUID; acceptance is queued, not completion. Active inference blocks
unload, cancellation targets only one load, and exact receipt reuse never replays.
Remote mutations require current node-owner management approval. No model files
are deleted, no model is selected/downloaded and no inference is authorized.

Lifecycle Show also returns scoped owned-job accounting when available: observed
job-commit ceiling, peak commit, active process count and cumulative CPU ticks.
Missing/unsupported/failed observations are explicit, never zero estimates.
See [states, units and limits](OWNED-JOB-ACCOUNTING.md); these are not RAM/VRAM
quotas, readiness evidence or permission to execute.
Ollama is not controlled by this increment. See [commands, bounds and remaining
lifecycle work](MODEL-LIFECYCLE.md).

## Finite local pipeline authoring (unreleased after alpha.6)

Finite local dataflow uses the existing `requests submit --text PURPOSE --file
pipeline.json` path with a schema-1 `PlanInput`, `template:"local_pipeline"` and
`params:{pipeline:DEFINITION}`. It compiles one source-bound reviewed task; separate
owner approval/execution is still required. Stage identities are fixed and data
remains local between steps. See [definition, commands and limits](LOCAL-PIPELINES.md).

The subsequent recurring-graph increment reuses `automations define`, separate
owner `automation-policies approve` and explicit `automation-schedules enable`.
Its frozen review includes all child capabilities; every bounded run gets fresh
job/grant identities under the same fixed graph and allowances. No timer starts
at definition/approval, and exhausted/restarted receipts cannot implicitly rearm.
This is finite recurrence, not continuous ambient capture or safe continuation.

## Automation definition authoring (unreleased after alpha.6)

The subsequent `eef automation-schedules enable|stop|show|list|resolve-refusal` connects exact
owner policies to bounded automatic fixed-input work through the existing core
compiler/grant driver. Enable permits future execution; exact receipt reuse
never re-arms authority. Stop/restart/unknown outcomes do not replay work. See
[commands, limits and native acceptance](AUTOMATION-SCHEDULES.md).

`resolve-refusal` closes only a retained backend pre-launch refusal using its
exact schedule revision, trigger, digest and operation receipt. It never infers
no work from a missing job, re-arms a timer, retries a grant or refunds a reserved
allowance. Any fresh activation is a separate owner command with current checks.

`eefn automations define|list|show|remove` shares the running node's authenticated
command service. It retains exact unexecuted compiled-source snapshots, bounded
timing/run declarations and compact removal receipts in the existing job database
and quotas. Definitions stay disabled: no timer, approval or workload is started.
Foreign origins, stale sources and conflicting receipts fail without mutation.
After an uncertain reply inspect the original definition ID; do not manufacture
a replacement receipt or assume rollback. See [commands and remaining execution
requirements](AUTOMATION-DEFINITIONS.md). Browser and model operation are not required.

`eef automations define|list|show|remove` exposes that same service to the local
coordinator owner. It uses the configured loopback API and current runtime fence,
never starts a second coordinator, and can inspect multiple origins without
rewriting their immutable context. Negative/lost/corrupt mutation replies remain
uncertain; retain the exact command/receipt and inspect. Source-bearing reports
are private. No timer, approval, execution, fallback or automatic retry is added.

## Reusable owner approval records (unreleased after alpha.6)

`eef automation-policies approve|list|show|revoke` adds explicit fixed-scope owner
approval checkpoints to that same journal. Approval requires the exact disabled
definition, last generation, receipt, expiry, run count, per-task grant bounds and
aggregate duration/result allowances. It does not approve the original job,
consume an occurrence budget, issue grants or start timers. Reports explicitly
say `occurrence_admission_available: true` for the subsequent explicit owner
reservation service; saving a policy itself never reserves or runs work.

Policies retain runtime/issuer identity and generation history. Restart does not
reactivate them; exact receipt reuse is historical inspection, not renewal.
Only the guarded coordinator-owner command can create/revoke these records;
node submission, node management and remote-update permissions cannot grant them.
Show/approval scope is private. See [exact commands, bounds, revocation, storage
and the remaining execution stage](AUTOMATION-APPROVALS.md).

`eef automation-occurrences reserve|show|list` now atomically consumes bounded
policy authority and approves an independently compiled exact matching job,
without dispatch. It retains original source history and immutable reservation
receipts, rejects overlap/uncertainty, and checks policy authority at ordinary
execution start and every issuance. Restart cannot silently reactivate an old
runtime's authority. Existing grants/receipts/recovery remain the only execution
path. No timers, hidden retries or budget refunds are supplied. See [commands,
privacy, lifecycle, storage and remaining work](AUTOMATION-OCCURRENCES.md).

`eef automation-occurrences archive|archive-show` adds exact-revision owner
archiving for proven completed or unissued-cancelled occurrences. It atomically
retains private terminal evidence while removing the live job; original budgets
and request-identity tombstones survive. Failed/unknown/partially issued work
cannot be archived. Lost replies require exact receipt inspection, not a new
command. This is bounded evidence retention, not byte reclamation or automatic
cleanup. See [archive commands and limits](AUTOMATION-OCCURRENCES.md#explicit-terminal-archiving-subsequent-unreleased-increment).

## Default workload boundary (unreleased after alpha.6)

`eefn --ask TEXT`, `eefn requests input --text TEXT [--operation-id UUID]`, and
node `/api/chat` share the running node's [input workflow](INPUT-WORKFLOW.md).
It saves origin-owned review and, if applied policy allows it, prepares an eligible
one-shot request awaiting approval. It never starts a second runtime, approves
or executes work. Reusing an input receipt only inspects retained history.
`requests workflow show|set --prepare-on-input true|false` exposes saved/applied
policy; explicitly restart after saving a change. Fresh configurations enable
preparation; older configurations missing the field stay review-only.

`jobs generate-text` now shares the explicit `request-text` review/compiler flow.
Raw job creation, legacy resume/dispatcher/model chat and ordinary workload RPC
are retired. Pairing/configuration/restart/authorized updates stay available.
Public core constructors enforce this boundary, with no runtime/config opt-out.
Use reviewed requests and exact grants instead; preserve receipts after uncertainty.
See [compatibility and verification limits](EXECUTION-BOUNDARY.md). The older
release-specific descriptions below must not be read as current dispatch behavior.

Input requires exactly one selected model with the explicit `request_interpreter`
role. Installing/selecting a model alone does not assign that role. Inspect
saved/applied selections with `eefn models show --json`, then explicitly choose
an existing text-capable selection with its actual backend/model ID:

```powershell
eefn models interpreter --backend BACKEND --model MODEL_ID --json
eefn restart --wait-seconds 60 --json
```

The unreleased `interpreter` command atomically moves only `request_interpreter`
to the chosen selection across both backends, preserving other roles and all
capability restrictions. `models clear-interpreter` removes only that role.
Neither command selects/downloads/loads models, switches provider, restarts,
changes permissions or approves work. Provider must activate the chosen backend.
Missing selections, disabled text inference and malformed/duplicate selections
are rejected unchanged. Repeated assignment/clear is a no-op. Older builds do
not support these operations; there is no multi-write hints/save fallback.
Manual hints can still produce multiple interpreter models, which fail as
ambiguous instead of choosing automatically. Unreleased `--ask`,
review and HTTP input failures expose `setup_hint` for these setup cases; node
HTTP input's human-readable `reply` includes that guidance. The existing browser
model picker does not configure roles. First-time non-coder setup, general durable
automation and reusable bounded approvals remain release requirements. Bounded
answer-triggered interpretation/preparation is available through the new workflow.
See [client acceptance](CLIENT-ACCEPTANCE.md).

## Answers through the running node (unreleased after alpha.6)

`eefn requests respond JOB_ID --revision REV --text ANSWER [--operation-id UUID]`
records an answer and follows the applied `resolve_on_answer` policy. It may
resolve/prepare an eligible request, never approve or execute it. Reusing its
receipt only inspects retained history, including partial workflows. New answers
retain exact source revision/digest; older missing evidence stays unknown.
`eefn requests workflow set --resolve-on-answer true|false --json` patches this
field independently; explicitly restart to apply it. Existing configurations
missing the field default to false; fresh configurations enable it.
Manual `answer`, `resolve` and `prepare` stay explicit. Node chat can target an
answer using strict `response_to: {id, expected_revision}`; it never guesses the
target. See [source, receipts, failure behavior and API](ANSWER-WORKFLOW.md).

Node status includes an optional live `last_workload` scope snapshot after durable
grant admission and a successful ACK write, immediately before execution. It
contains grant ID, capability, immutable context and exact resource/model/backend
IDs, never private parameters/output. `execution_may_have_started: true` is not
a completion or stop assertion. Control polling cannot erase it; rejected work
cannot replace it. This is a last-attempt snapshot, not history, latency measurement,
durable evidence, or proof of all currently running effects. Use existing grant
receipts/results for durable inspection. Legacy last-activity fields may still
change during ordinary control activity.

## Explicit interrupted-result reconciliation (unreleased after alpha.6)

`eefn requests recover-result JOB_ID --revision REV --grant-id GRANT_UUID
--operation-id RECOVERY_UUID --json` explicitly settles one interrupted reviewed
attempt from the exact executor's retained completed evidence. EEF's local owner
uses the same `eef requests recover-result ...` form. Inspect the current job
first and keep the entire receipt. No replay, fabricated wire ACK, implicit
remaining-step dispatch or remote stop is authorized. Exact same-actor receipt
reuse is read-only and needs no live executor. A missing/unconfirmed reply can
mean a committed decision: inspect before reusing that exact command. See
[reconciliation contract](RESULT-RECOVERY.md) for scope, storage and limits.

## Explicit retained executor output (unreleased after alpha.6)

`eefn requests result JOB_ID --grant-id GRANT_UUID --json` reads a bounded saved
final report from the exact executor through EEF. Only the immutable request
origin or local EEF owner (`eef requests result ...`) is authorized. Executor-local
`eefn grants result --id GRANT_UUID --json` also works offline without starting a
runtime or creating a journal. Routine receipt commands remain metadata-only.
Reads perform no inference, replay, lease renewal, job change or reconciliation.
Legacy/missing output is reported honestly. Schema-2 storage contains private
output: back up data, stop old runtimes and do not downgrade the journal. See
[full contract and quota semantics](RETAINED-RESULTS.md).

## Reply ownership across reconnect (unreleased after alpha.6)

Remote RPC waiters now belong to the exact authenticated gateway session as well
as the stable node ID. Registering a waiter and retiring its route are serialized;
cleanup cannot drain the old session before its waiter is recorded. Response
matching and disconnect failure check both IDs, so a different session cannot
confirm/fail the waiter simply by using the same persistent node ID.

The gateway also stamps a private routing generation on accepted submissions,
overwriting any caller field. EEF's completed/busy replies use that original
session and matching transport ID, never the latest connection for the node.
If the original session ended or its queue is full, the reply is not rerouted,
queued for reconnect or replayed. Accepted work may still have been saved or run;
use the existing receipt/history inspection rather than assuming cancellation.

This is internal reply routing; it changes no node wire requirement, persisted
node identity or job origin. Published legacy clients remain compatible with the
updated coordinator. The generation is not an execution grant, durable issuer
epoch, node-side attempt fence, executor acknowledgement or exactly-once promise.

## Coordinator scheduling limits (unreleased after alpha.6)

The local EEF owner can inspect/save non-model admission policy without a
dashboard, LM or permission changes. These commands require the running
coordinator; they never launch another instance, edit a file as fallback or
retry a mutation. Installed alpha.6 builds do not implement them.

```powershell
eef scheduling show --json
eef scheduling limits --freshness-seconds 25 --max-in-flight 8 --per-provider-concurrency 2 --json
eef scheduling node --node NODE_ID --max-in-flight 3 --json
eef scheduling remove-node --node NODE_ID --json
eef restart --json
eef scheduling show --json
```

Use an exact ID from diagnostics/network inspection, not a node name or wildcard.
Limits are saved atomically under the existing configuration lock and apply only
after an **explicit coordinator restart**. Inspection reports `saved_policy`,
`applied_policy`, `restart_required`, the current `runtime_id`, and a bounded
`admission` snapshot. An unchanged save does not rewrite the file. Generic
settings saves preserve the latest scheduling policy; typed commands own edits.

| Policy | Default | Supported values | Scope |
| --- | --- | --- | --- |
| `freshness_seconds` | 10 | 1-300 | Coordinator-received non-model provider heartbeat age |
| `max_in_flight` | 0 | 0-4096 | Aggregate coordinator-owned non-model dispatch futures |
| `per_provider_concurrency` | 0 | 0-4096 | Additional ceiling per current capability/action registration |
| `node_limits` | Empty | Up to 256 exact IDs, each 0-4096 | Aggregate owned futures across one node's non-model providers |

`limits` needs at least one value. Zero removes only that additional ceiling;
it does not widen an executor's existing limit, local feature permissions or
node request-admission policy. Applicable nonzero ceilings intersect. Global
and exact-node budgets retain outstanding futures when their registration is
removed/replaced; late release cannot decrement a replacement's counters.

`admission` includes `total_in_flight`, `retired_in_flight`, up to 256 exact-node
counts and 512 current providers, with explicit truncation flags. Counts are
observational, not execution acknowledgements. These limits do **not** constrain
physical CPU/RAM, the dedicated `llm.infer`/`vlm.analyze` route, or direct owner
diagnostics/recovery controls. Remote effects may continue after a local timeout;
releasing a future does not prove remote cancellation. Pipelines, execution
grants, executor acknowledgements and continuous resource budgets remain planned.

Local command routes are `GET/POST /api/commands/scheduling`. Mutations require
strict schema 1, the observed `expected_runtime_id` and a typed `command`.
Cross-site/remote browser callers, stale runtime IDs, extra fields, invalid limits
and wildcard IDs fail closed. CLI confirms report identity, safety flags, applied
policy and the requested saved delta. A lost or inconsistent mutation reply is
unconfirmed, never permission to replay; inspect `scheduling show` before retrying.
No plan, job or execution authority is created by saving scheduling limits.

## Explicit remote updates

Unreleased exact-node pairing and credential migration commands are documented
in [independent node credentials](NODE-CREDENTIALS.md). They are not in the
published alpha.6 installers and never grant feature permissions.
Credential-only startup with legacy access disabled now needs no dummy legacy
secret and advertises no shared-key local fallback. Re-enablement still requires
a valid effective legacy secret; see the migration contract above.

Added in v0.4.0a5.

Automatic and legacy feed updates stay stable-only for both roles. The new
command selects one exact node artifact without editing its saved feed/policy.

Feed checks use the manifest's internal version, not GitHub's release checkbox.
Marking an existing `0.3.2` GitHub release as a prerelease does not by itself
withdraw that version from an existing feed. Feed publication must be managed
separately when there is no eligible stable release.
Both public feeds are currently paused: `schema_version: 1`, `enabled: false`,
and a reason, with no artifact fields. Alpha.5 checks report `feed_paused: true`
and no update; applying a paused feed fails before download. Older clients show
a feed-check error because their required artifact fields are absent. This is
intentional fail-closed behavior, not permission to change feeds or disable checks.

```powershell
eef node update --node NODE_ID --version VERSION --url HTTPS_INSTALLER_URL --sha256 SHA256 --size-bytes BYTES --allow-prerelease --json
eef node update-status --node NODE_ID --json
eef node restart --node NODE_ID --wait-seconds 60 --json
```

Use the internal version and exact byte count/hash of the release's EEFN
installer. `--allow-prerelease` is required for that particular alpha/beta/RC;
it is not a subscription or saved preference. Stable artifacts do not need the
flag. No publisher, model or release URL is hardcoded. Choose trusted artifacts:
the hash binds the download to your selection, not to a verified publisher.

The node must have **Remote updates** enabled in its applied policy and current
saved configuration. Disabling it takes effect for explicit update admission
and is checked again under the configuration lock immediately before activation.
General filesystem/process access is not needed. Restart is separate and still
requires its own management approval; applying an artifact never restarts Windows.

The backend uses `node.update/apply_explicit` with a strict `schema_version: 1`
request containing `expected_node_id`, canonical UUID `request_id`, `version`,
`url`, `sha256`, `size_bytes`, and `allow_prerelease`. Only credential-free HTTPS
URLs/redirects are accepted. Downloads have a 512 MiB ceiling, exact size/hash
verification, and bundle product/version checks. New versions must advance the
running version; an already-pending selection is not silently replaced. Explicit
and feed installers share a nonblocking installation lock. Feed callers cannot
inherit the explicit opt-in, including from a manifest field.

The CLI sends once and checks target, request ID and installed-version confirmation.
Timeouts, lost replies, rejection and unexpected responses are reported conservatively
as unconfirmed; there is no replay, shell fallback or feed replacement. Requests
have no durable receipt/deduplication promise. Inspect `update-status` before
retrying: it reads the actual version selector, not a cached dashboard status.
A successful apply needs a separate restart and running-version verification.

Published alpha.4 and older nodes do not implement this action. They still need
an explicit installer upgrade to a build containing this fix. Installing alpha.4
alone does not provide these commands. Use the alpha.5 installer for that
bootstrap; publishing a release does not automatically upgrade older nodes.

## Durable request review and clarification (unreleased after alpha.6)

Local GGUF CPU worker settings are also now command-driven, without reselecting
files or editing JSON: `eefn models cpu-threads --model MODEL_ID --threads 4`.
Approved coordinator management uses the same action under `eef node models`.
Saving is separate from restart/application; `--auto` restores backend defaults,
not a measured budget. See [CPU settings and confirmation](MODEL-CPU-THREADS.md).

Save a request through the running node, then inspect/answer it without a
dashboard. These commands are not in the published alpha.6 installers.

```powershell
eefn requests open --text "Inspect that node" --json
eefn requests open-proposal --file proposal.json --json
eefn requests draft DRAFT_ID --json
eefn requests answer DRAFT_ID --revision 1 --text "The bedroom node" --json
eefn requests resolve DRAFT_ID --revision 2 --json
eefn jobs find --operation-id OPENING_REQUEST_UUID --json
eefn jobs stop DRAFT_ID --json
eefn jobs remove DRAFT_ID --json
```

`open` uses the existing explicitly selected interpreter. `open-proposal` and
`revise-proposal` take the same strict UTF-8 file as `review-proposal` below and
need no model. Missing/ambiguous interpreter selection fails before saving;
there is no automatic model install, chat fallback or command inference.
As a deterministic alternative to `resolve`, use
`eefn requests revise-proposal DRAFT_ID --revision SOURCE_REVISION --file resolved-proposal.json --json`
with the current inspected revision; do not reuse a pre-resolution revision.

EEF stores a **non-executable review record** in the existing durable job journal,
with one stable ID, status `review`, gateway-stamped original node/area/receipt,
exact original text, proposal revisions and clarification answers. No tasks,
runner, dispatch, approval or execution grant are created. The record stays
`review` after restart, not interrupted/resumed work. Existing job quotas and
recovery headroom apply; full/damaged storage rejects instead of replacing history.
Existing ordinary job records need no rewrite. Older coordinators do not understand
the new `review` status: do not downgrade a database containing these records to
an old build. Keep a backup before testing a development build; there is no lossy
automatic downgrade or conversion to executable jobs.

Only the authenticated original node can read or mutate the full draft through
node commands; the local coordinator owner can explicitly inspect it using
`eef requests inspect` below. Later
commands cannot change its original text, original origin/area or opening receipt.
An answer records the exact current question, proposal index and supplied text;
`answer_recorded` does **not** mean model uncertainty has been resolved. An
explicit proposal revision can change hints and clarification state, but still
requires review and never grants execution authority. EEF validates its capability
hints against fresh owner-disclosed network registrations, not file-authored permissions.

### Explicit clarification reinterpretation (unreleased after alpha.6)

After recording an answer, `requests resolve` reinterprets the saved original
request and recorded questions/answers using the node's explicitly selected
`request_interpreter` model. It removes the need to author a revised proposal
file, not the requirement for review or separate planning/approval/execution.

```text
eefn requests resolve DRAFT_ID --revision SOURCE_REVISION --operation-id UUID --json
```

The source must still be `review`; its current proposal must ask a clarification
and have at least one recorded answer. No answer, stale
revision, wrong original node, oversized context, full proposal history or a
missing/ambiguous interpreter cannot cause a fallback model, download or save.
The prompt preserves original text as its own message and includes the current
proposal plus all recorded questions/answers as untrusted data. The complete
prompt is capped at 64 KiB without truncating answers, with existing 512 output
tokens and 20-second inference/single-capacity limits. If uncertainty remains,
the new proposal can ask another question; `resolve` does not promise semantic
correctness or force `needs_clarification` to false.

EEF atomically checks the exact source revision, full review-record digest and
proposal/answer counts, then validates hints against fresh disclosed capability
names. It appends one proposal and source-evidence receipt, preserving original
identity/text, earlier proposals and all answers. The evidence retains reported
interpreter backend/model ID; it is **origin-reported**, not model attestation,
execution permission or proof of a correct answer. Concurrent answers, edits,
Stop, disclosure changes and runtime replacement cannot overwrite a newer state.
If a runtime changes after save submission, EEF may still commit that eligible
revision; the node reports uncertainty and never automatically sends it again.
Persisted source lineage is revalidated on read/restart; old builds without this
field must not be used to downgrade these records.

Successful schema-1 `request_resolution` replies set `proposal_saved: true` and
`coordinator_acknowledged: true` while execution/dispatched/automatic-retry flags
stay false. `saving_attempted` and `model_inference_requested` describe this
adapter attempt, not proof an external model actually ran or stopped. A lost
save/HTTP reply is uncertain: inspect `requests draft DRAFT_ID`, retaining the
reported `command.operation_id`. Repeating the **same operation ID and source
revision** inspects an already saved resolution without inference, renewal or
another save, even if the interpreter is busy/unavailable or the job has since
been compiled. The reply uses a separate fresh coordinator inspection receipt;
it does not claim the read was the original save. If no receipt is retained and
the exact source is still eligible, an explicitly repeated command can request
a new inference attempt; there is no automatic replay or exactly-once inference
claim. Removing history loses this recognition.

Local owner API: `POST /api/commands/requests/resolve` with exactly
`schema_version: 1`, `expected_node_id`, and `command: {id, expected_revision,
operation_id}`. It remains available with the dashboard disabled. The core
service, not browser state, owns the read/context/inference/atomic-save workflow.
Read/context waits are bounded at five seconds each, saving at thirty seconds;
node CLI has a sixty-five-second HTTP deadline, no redirects/proxy/retry, and
rejects changed identity, source receipt or safety flags. Missing commands return
unsupported without converting the request to chat or executable work.

There is no automatic answer-triggered resolution, automatic semantic compiler,
approval or execution yet; automation can invoke these explicit backend commands.

### Review concurrency and retention

Inspect the returned `revision` before each answer/revision; the command must
specify it. Concurrent edits have one committed winner; stale attempts do not
overwrite it. History permits at most 16 answers and 16 appended proposal revisions;
answers are at most 8192 UTF-8 bytes, and the complete review record at most 512 KiB.
Existing interpretation text/proposal bounds also apply. Increasing these bounds
is not an owner permission flag. Stop cancels a review immediately; it cannot be
paused/resumed into execution. Removal is explicit and only after cancellation.

Node responses use schema 1 / `report_type: request_draft`. Successful replies
confirm the exact origin/operation/ID/revision/requested delta with
`coordinator_acknowledged: true`, `execution_authorized: false`, `dispatched: false`
and `automatically_retried: false`. The result is `coordinator_request_draft`.
Lost/corrupt mutation replies are unconfirmed and may have saved data. Inspect
`jobs find` with the opening UUID, or `requests draft` with the known ID, before
deciding whether to retry; inspection cannot infer or resubmit a request.

Within the retained record, repeating the opening receipt with the same original
text/initial proposal, or an answer/revision receipt with the same content, returns
the retained record without another write. Receipt/content conflicts fail. A reused
receipt acknowledges its **past** change even if the draft was later revised or
cancelled; it does not apply the change again or undo cancellation. This is not a
permanent/global receipt ledger, exactly-once effects, or node execution fencing.
Removing history/database replacement removes this receipt protection.

Local loopback/Host/Origin-guarded routes are
`POST /api/commands/requests/open`, `/open-proposal`, and `/drafts`.
Opening requests have exactly `schema_version`, `expected_node_id`, canonical UUID
`request_id`, `text`, plus `proposal` for supplied input. Draft commands have the
same first three fields and `command`, with one of:

```json
{"operation":"get","id":"lowercase-compact-uuid"}
{"operation":"answer","id":"lowercase-compact-uuid","expected_revision":1,"text":"Bedroom"}
{"operation":"revise","id":"lowercase-compact-uuid","expected_revision":2,"original_text":"Exact original","proposal":{}}
```

The final example needs a complete valid proposal, not an empty object. Extra
authority/context fields, wrong targets, invalid IDs and cross-site callers fail.
Commands use the existing authenticated submission connection; no second node
runtime or browser is needed. `jobs list/get/find` expose bounded review state/counts,
not raw proposals/answers; explicit draft inspection includes private user content.
Do not publish those snapshots. Explicit concrete compilation is described below;
automatic semantic planning, owner approval, execution grants and executor
acknowledgements remain required stage 6 work.

`node tools/test-request-drafts.mjs` tests two logical, independently credentialed
nodes with pinned TLS 1.3, no browser or model: ownership, immutable origin/text,
answers/revisions, stale/duplicate/conflicting receipts, restart retention and
stop/removal. `test-request-review.mjs` additionally checks fake-inferred durable
review and receipt lookup after a lost save reply. Neither is physical two-PC
acceptance or real model reasoning.

## Concrete request plans awaiting approval (unreleased after alpha.6)

The local coordinator owner can compile a resolved one-shot review into the
existing journal without a dashboard, inference, permission change or dispatch:

```powershell
eef requests inspect DRAFT_ID --json
eef requests compile DRAFT_ID --revision 4 --file concrete-plan.json --json
```

Example input (replace `EXACT_NODE_ID` with an installed, connected node's ID):

```json
{"schema_version":1,"template":"system_info","params":{},"constraints":{"node_id":"EXACT_NODE_ID"}}
```

This is an explicit backend compiler interface, not the final non-coder setup
workflow or an automatic translation of model hints. It reuses existing task
templates. Every compiled capability must appear in the latest reviewed proposal;
widening that set requires a proposal revision first. Original text, origin/area,
opening receipt and complete review history remain unchanged. Inputs are strict
UTF-8 JSON (optional BOM), bounded to 64 KiB, with exactly the four shown fields.
Unused parameters, empty/unknown templates and unsupported constraints fail.
Continuous/unknown workloads and implicit camera-result-to-VLM wiring are not
implemented here and reject rather than produce a misleading executable plan.

Non-model tasks use current capability/action/parameter validation and existing
node/name/area/resource/GPU constraints. Compilation requires a fresh registered
remote node, never the coordinator's internal `local` adapter. Model tasks use
explicit prompt or messages, optional installed model ID/tier, and node/backend/
model-role constraints. Unsupported physical model constraints reject instead of
being ignored. The chosen cached model route must still match its current gateway
registration, including normalized role/readiness metadata. No model is installed,
loaded or inferred; no resource reservation is made. Availability at compilation
is not a promise that a later execution can use the same resource.

EEF freezes exact node, model/backend or physical resource bindings, bounded task
definitions and zero automatic retries, and stores `awaiting_approval` atomically
under the same job ID. It records a SHA-256 definition digest. This detects
definition inconsistencies; it is **not** a signature, execution grant, node-side
authorization or protection against someone controlling the database. Restart
retains the plan without activating a runner. Existing execution and pause/resume
entry points reject it. The originating node can inspect bounded status or stop
it; cancellation keeps the definition/digest but marks tasks cancelled. Removal
is explicit. Older builds cannot read this new state; back up development databases
and do not downgrade them automatically.

`inspect` returns private original text, task inputs and target bindings. Ordinary
origin-owned job/draft responses omit these concrete parameters/bindings; inspection
by another node remains denied. Do not attach full owner reports to public issues.
Compilation itself adds no execution authority. The subsequent explicit owner
approval command below still does not change legacy task/chat execution into a
grant-enforced workflow.

Owner API: `POST /api/commands/requests/planning` on the existing guarded loopback
API, with exactly `schema_version: 1`, current diagnostic `expected_runtime_id`
and `command`. Inspect uses `{"operation":"inspect","id":"DRAFT_ID"}`. Compile
adds `expected_revision`, a canonical UUID `operation_id`, and the exact `input`
object above. Extra authority fields, cross-site callers and runtime/revision
changes reject. Reports are `report_type: request_planning`, with
`execution_authorized`, `dispatched` and `automatically_retried` all false.
The CLI checks the concrete definition, runtime, receipt and requested input.

The CLI sends once. A lost or corrupt compile reply is unconfirmed: inspect that
ID before retrying, because the plan may already be saved. Within the retained
record, repeating the same compile UUID/revision/input acknowledges the stored
plan without a write, even after cancellation; conflicts reject and cannot replace
it. Reopening with the original node receipt returns the same retained ID even
after compilation. There is no implicit replan, replay, permanent receipt ledger,
executor acknowledgement or exactly-once effect promise.

`test-request-drafts.mjs` checks CLI compilation, pinned-TLS origin isolation,
restart, receipt/revision conflicts and non-executing cancellation.
`test-request-review.mjs` also checks exact installed model/backend/role binding
without invoking its fake inference provider. These remain local fixtures, not
physical two-PC or real model acceptance.

### Planning through the input node (unreleased after alpha.6)

Explicit selected-interpreter preparation can produce the concrete input or save
a new clarification without editing JSON:

```powershell
eefn requests prepare DRAFT_ID --revision SOURCE_REVISION --operation-id UUID --json
eefn requests plan DRAFT_ID --json
```

It requires a resolved one-shot review and an explicitly selected interpreter.
A method saves as `awaiting_approval`; a question saves as an answerable review
proposal. Use `requests answer`, then `requests resolve`, then a new preparation
operation against the new revision. Nothing is approved or executed by preparation.
Full original/proposal/question/answer history, exact source evidence and immutable
origin are retained. Invalid/stale output cannot replace a concurrent edit.

The guarded node API is `POST /api/commands/requests/prepare`, with exactly
`schema_version:1`, `expected_node_id`, and `command:{id,expected_revision,operation_id}`.
Successful `request_preparation` reports include `checkpoint_saved`; an exact
saved receipt returns `inspection_only:true`, with no inference or saving attempt,
even after later compilation or withdrawal of the interpreter role. Unconfirmed
mutations remain uncertain unless known not sent: inspect `requests plan` or reuse
the exact source revision and operation ID. No model/file/chat fallback, implicit
approval, new runtime or automatic retry. See [preparation contract](SEMANTIC-PREPARATION.md).

The running input node can now request the same EEF compiler and inspect its own
retained plan without a dashboard or coordinator port:

```powershell
eefn requests plan DRAFT_ID --json
eefn requests compile DRAFT_ID --revision 4 --template system_info --node EXACT_NODE_ID --operation-id UUID --json
eefn requests compile DRAFT_ID --revision 4 --file concrete-plan.json --operation-id UUID --json
```

Choose `--template` or `--file`, never both. Template mode supplies empty parameters
and optional exact `--node`, so basic system-info compilation needs no JSON file.
Parameter-bearing explicit templates use the bounded input file above; selected
interpreter preparation is available separately below. Omitting the
operation ID generates one, returned in the report. Reuse only that exact receipt,
revision and input after inspection, not a new UUID to replace a retained plan.

EEF stamps authenticated origin, checks ownership before any private view and
filters candidates **before ranking** to fresh origin/owner-disclosed peers under
its applied directional discovery policy. Default is self-only; a faster hidden
provider cannot be selected or exposed. Exact constraints remain exact; no hidden
executor fallback or local coordinator adapter. Existing capability/model ranking,
fresh registration checks, resource ownership, parameter validation, durable
revision/digest checkpoint and zero-retry definitions remain authoritative.
Discovery is not execution permission. Owner API compilation retains its broader
owner visibility; new node compilation does not gain those privileges.

New guarded node-local API: `POST /api/commands/requests/planning`, with exactly
`schema_version: 1`, `expected_node_id`, canonical UUID `request_id`, and `command`.
Inspect uses `{"operation":"inspect","id":"DRAFT_ID"}`. Compile uses the same
`id`, `expected_revision`, `operation_id` and `input` shape as owner compilation;
`operation_id` must equal `request_id`. No node `approve` operation exists.
Wrong origins/nodes, extra authority fields, cross-site callers, ambiguous input
options and conflicting revisions/receipts reject. This adapter starts no second
node, interpreter/model, runner or executor probe.

Node reports use `report_type: node_request_planning`; the acknowledged result is
a strict `coordinator_node_planning` snapshot, bound to origin/job/receipt/runtime.
It includes retained original text/review history, exact input, digest, task inputs,
dependencies and bindings, not grant authority, outputs, credentials, endpoints or
private resource adapter configuration. Treat it as private. Ordinary jobs/draft
views remain redacted; selected executors and other origins cannot inspect it.
Historical own snapshots and exact receipt inspection do not perform a new peer
lookup, including after withdrawal/restart; they never renew or dispatch anything.

Every report sets `execution_authorized`, `dispatched`, `automatically_retried`
false: compilation only saves `awaiting_approval`. Retained status may reflect a
separately requested later approval/execution, not authority from this command.
Unconfirmed compile replies—including generic coordinator errors that could occur
after a checkpoint—leave `outcome_unknown: true`; only a known not-sent mutation
can report no uncertain save. Inspect the retained plan before an explicit retry.
Read-only inspection never has a mutation outcome to reconcile. Approval remains
the separate coordinator-owner command below. Automatic semantic compilation,
delegated owner approval and the complete non-coder workflow remain later work.

## Bounded owner request approval (unreleased after alpha.6)

After inspecting the exact private plan, the local EEF owner can retain a
conditional approval without a browser:

```powershell
eef requests approve DRAFT_ID --revision 5 --digest PLAN_SHA256 --valid-for-ms 60000 --max-execution-ms 1000 --max-result-bytes 16384 --json
```

Replace the ID, revision and digest with the inspected values. All bounds are
explicit; nothing defaults to unlimited authority. Validity is 1 ms-24 hours and
must be at least the execution bound. Execution is 1-30000 ms and no wider than
the compiled deadline or selected node's advertised limit. Results are 1 KiB-1 MiB
and no wider than the selected node's limit. These are future RPC/result bounds,
not CPU/RAM accounting, process-tree termination or automatic retry permission.

The exact plan digest/revision, original context, inputs and frozen bindings must
still be valid. Each selected executor must be fresh, independently authenticated
over owner-pinned TLS, and advertise its durable grant gate. Selected resource
and model/backend/role routes are rechecked without rebinding, installing, loading
or invoking a model. Legacy/shared-secret nodes and unsupported grant contracts
reject, leaving the compiled plan unchanged. Availability is not a reservation;
dispatch must recheck it and the applied node permissions later.

EEF atomically retains `approved` plus a strict owner receipt: exact plan digest,
issuer certificate fingerprint, source revision, approval/expiry times, limits and
a canonical approval digest. **It issues no execution grant and starts no runner.**
The issuer is not a shared network secret and approval is not a signature or
protection against the local owner modifying their database. `execution_authorized`,
`dispatched` and `automatically_retried` remain false in these planning reports:
approval is conditional authorization for the future issuance step, not present
executor authority. Legacy execution paths, pause and resume cannot activate it.

The existing guarded `POST /api/commands/requests/planning` uses `command`:

```json
{"operation":"approve","id":"DRAFT_ID","expected_revision":5,"operation_id":"CANONICAL_UUID","plan_sha256":"PLAN_SHA256","valid_for_ms":60000,"limits":{"max_execution_ms":1000,"max_result_bytes":16384}}
```

The outer schema/runtime checks and private owner report rules above also apply.
Extra permission/execute fields and cross-site callers reject. The CLI sends once
and validates the retained receipt, digest, revision and exact requested bounds.
Use `--operation-id CANONICAL_UUID` to provide a receipt explicitly. If a reply is
lost, inspect first and keep its operation ID: repeating exactly the same original
revision/digest/bounds/UUID returns retained approval without a write or renewal,
even after expiry, disconnect or cancellation. A different approval or changed
limits cannot overwrite it. No automatic renewal or replan exists yet; cancel and
explicitly create/review new work rather than reviving a cancelled plan.

Restart preserves approval but never resumes work. Expiry is checked against
current wall time, not implemented as an automatic journal rewrite. The bounded
job view exposes `approved_no_grant`, expiry and `approval_active` (state/time
only, not node readiness or a grant); it omits approval internals and execution
inputs. The originating node can inspect its review or Stop, preserving the
receipt/digest while cancelling all unstarted tasks. Other nodes cannot read it.
Quota, concurrent revision and shutdown failures leave the prior plan unchanged;
damaged approval records are retained and reject, not repaired or activated.

The subsequent explicit execution command below adds durable issuance and
persisted executor ACK/completion. Automatic semantic planning, explicit
uncertainty reconciliation and physical two-PC acceptance remain required. This is not
the final non-coder workflow or a stable v0.4.0 release. Approval/replay protection
depends on retaining the existing job database; deleting history is not a reset
or proof that effects elsewhere never happened. Back up before using these new
development states; older executables must not automatically downgrade the data.

## Explicit approved request execution (unreleased after alpha.6)

The originating running node can request one exact approved plan without a
dashboard, inference for planning, port numbers or another node process:

```powershell
eefn requests execute REQUEST_ID --revision 3 --approval-digest APPROVAL_SHA256 --json
```

The approval digest/revision come from private owner approval/inspection, not the
model's proposal. The local EEF owner has the same `eef requests execute` command.
Both accept `--operation-id CANONICAL_UUID`; keep that receipt if the reply is
lost. API automation uses the guarded node-local `POST /api/commands/requests/execute`
with `schema_version: 1`, `expected_node_id` and exactly this command:

```json
{"id":"REQUEST_ID","expected_revision":3,"operation_id":"CANONICAL_UUID","approval_sha256":"APPROVAL_SHA256"}
```

The guarded EEF-local route uses `expected_runtime_id` instead of `expected_node_id`.
Nodes submit through their existing authenticated connection; they cannot submit
grants, overwrite the original context, or activate another origin's request.
The executor may be a different node but does not become the request's owner.
Receipt reports have `report_type: coordinator_request_execution`, durable ID,
original context, source revision/approval digest, current status and
`execution_requested: true`, `persisted: true`, `automatically_retried: false`.
This confirms the persisted request, **not completion, a reservation or remote stop**.

The existing job journal and admission/shutdown gate own this workflow. Each
unstarted task rechecks fresh fixed providers/resources and exact installed model/
backend/role; missing roles or changed issuer/bounds reject, never rebind. The
existing atomic scheduling reservation includes granted inference as a provider
future, but does not enforce physical RAM/VRAM or model-instance load budgets.
One task/attempt is issued at a time after successful predecessors, with no
implicit result-to-parameter/media wiring, migration, fallback or automatic retry.

EEF saves the complete exact grant and one executor assignment **before dispatch**.
The node consumes its durable receipt before acceptance/effects. A direct gateway
observer saves matching acceptance and completion in the same EEF job record
before confirming them to its runner; broadcast event loss cannot drop these
checkpoints. ACK persistence is not a second distributed permission-to-start or
an atomic transaction with remote side effects: work may already be running by
the time EEF saves the ACK. Journal failure cannot produce confirmed success.
Logical journal headroom is reserved before issuance for bounded result/recovery
records; physical disk/temporary journal overhead remains additional.

Normal origin job views expose bounded grant IDs/states/acknowledgement, not raw
inputs or outputs. Explicit `jobs output` reads text-model output saved once in
the grant report; task summaries do not duplicate large payloads. Private owner
`requests inspect` includes full grants and results and must not be published.
Task result duration is currently unmeasured (`duration_ms: 0`), not a latency
measurement. Inputs/limits/definitions, original text/context, planning and approval
digests remain frozen; extra authority fields and incompatible records reject.

Exact retained command retries return the existing receipt/status without another
runner, grant, inference, write, renewal or journal rewrite, including after
completion or interruption. A different operation ID cannot restart the same
request. Concurrent first submissions may be rejected during admission; inspect
the retained receipt before retrying. There is no permanent deduplication after
explicit finished-history removal/database replacement.

Lost acceptance/completion, disconnect, shutdown, deadline or failed journal
confirmation leave issued attempts `unknown`/`interrupted`. Restart never revives
them, even for reads/inference. Existing pause/resume paths reject reviewed grants.
Stop prevents further issuance and can cancel before any grant is issued; while
one is pending it is `stopping` until a matching result or uncertainty is recorded.
Already-completed effects are not undone. Uncertain issued work cannot be declared
stopped or safely replayed/migrated: inspect node receipts and effects instead.
The explicit `requests abandon` command can close idle local tracking while
retaining acknowledged uncertainty; it does not stop remote effects or reconcile
execution/output. Reconciliation and stronger OS/backend termination remain
pending; do not repair these states by editing JSON or erasing journals.

These grants still advertise `rpc_future_only`: already-launched applications,
blocking/native/backend work or sent network effects may outlive the future.
Updated roles use the fresh `issuer_lease_challenge_v1` advertised timing mode,
not comparisons between the PCs' wall clocks. New grants require updated EEF and
EEFN; no clock/legacy fallback is available. Challenges expire after two seconds,
including queue/network time, and cannot renew a grant. Missing/expired proof
denies execution; uncertainty after consumption still requires receipt inspection.
No new command, NTP configuration or automatic update is implied. See
[lease timing and limits](EXECUTION-GRANTS.md#limits-and-honest-termination).
Legacy task/chat/management paths are unchanged,
not retroactively grant-enforced; migrating remaining work paths is a release gate.
Automatic semantic compilation, continuous pipelines, physical budgets, peer/media
paths, cluster fencing and physical two-PC/release acceptance remain open.
Headless fixtures cover an origin node granting a write to another node,
unchanged owner markers after receipt reuse/restart, pending HTTP effects after
abrupt EEF termination without replay, exact model roles and saved text output.
Fake inference and two logical local nodes are not real model or two-PC acceptance.

## Node grant receipts (unreleased after alpha.6)

`eefn grants receipt --id GRANT_UUID --json` inspects this node's existing consumed
receipt without starting a runtime, creating/repairing a journal or replaying work.
It is also available through the guarded node-local
`POST /api/commands/grants/receipt` API. An accepted receipt may be running or
interrupted/uncertain; it is not authorization to repeat an operation.

The [node execution grant boundary](EXECUTION-GRANTS.md) now validates exact
owner-pinned issuer/session/operation scopes and durable node-side consumption.
Normal EEF reviewed plans **still await approval**. Owner approval, persisted EEF
issuance/ACK integration and fixed reviewed-plan dispatch are described above;
legacy RPCs are unchanged. No raw grant-issuance or receipt reset/replay command is exposed
by this receipt inspector, and no public installer/update feed includes this work yet.

## Executor receipt inspection through EEF (unreleased after alpha.6)

The original input node or local EEF owner can inspect the exact executor's
consumed receipt without visiting that executor, dispatching work or rewriting a job:

```powershell
eefn requests receipt JOB_ID --grant-id GRANT_UUID --json
eef requests receipt JOB_ID --grant-id GRANT_UUID --json
```

Obtain the grant UUID from the original node's `jobs get` metadata or private owner
`requests inspect`. This is distinct from `eefn grants receipt`, which reads the
local node's own journal and also works while it is stopped. The network command
requires the origin node/EEF and exact executor to be running and connected using
independent credentials and the same owner-pinned TLS issuer. A new authenticated
session can inspect an old session's receipt but cannot reactivate its lease.
Issuer replacement, unsupported nodes, missing/damaged receipts or busy controls
fail without legacy fallback or automatic retry. No caller chooses a different
executor, issuer, attempt, parameters or grant scope. An executor does not thereby
gain ownership of another node's original request.

Equivalent guarded APIs: `POST /api/commands/requests/receipt`.
EEF accepts exactly `schema_version: 1`, `expected_runtime_id` and
`command: {"id":"JOB_ID","grant_id":"GRANT_UUID"}`. The node adapter accepts
exactly `schema_version: 1`, `expected_node_id`, canonical `request_id` and the same
command. Extra authority fields and cross-site callers reject. Node forwarding
reuses the existing authenticated submission path; EEF checks retained origin and
issuance before reading the executor. The distinct wire query uses reserved
node control capacity, not workload admission. Its bounded read/response retains
that permit; a failed partial response retires the framed connection. Shared-secret
connections cannot use this query, and it is not an executable capability.

A confirmed `coordinator_request_receipt` report includes the original context,
coordinator revision/attempt-state snapshot and strict bounded node/issuer/grant/
scope-digest/job/task/attempt/state/timestamp metadata. The executor scope digest
must match the complete retained issuance. Local inspection now validates the same
metadata and journal owner/version. There are no parameters, original text, model
output or media in a consumed receipt. Report `success` means the **inspection**
was confirmed, not that the underlying operation succeeded.

All reports set `execution_requested: false`, `automatically_retried: false` and
`reconciled: false`. `accepted` may mean running or interrupted; `completed` means
a terminal node checkpoint, which does not recover lost output or its success
status. Even a completed receipt cannot prove all external effects stopped.
A missing receipt does not prove nothing happened. Inspection never changes the
coordinator revision/status, returns an unknown attempt to a runner, cancels remote
effects, issues another grant, prunes receipts or supplies a recovery authorization.
Explicit local tracking abandonment is available below. Lost-output recovery,
remote termination confirmation and execution/migration reconciliation remain required.

## Explicit tracking abandonment (unreleased after alpha.6)

After inspecting an uncertain granted attempt, the original node or local EEF
owner can explicitly close its coordinator tracking without claiming remote stop:

```powershell
eefn requests abandon JOB_ID --revision REV --grant-id GRANT_UUID --acknowledge-effects-may-continue --json
eef requests abandon JOB_ID --revision REV --grant-id GRANT_UUID --acknowledge-effects-may-continue --json
```

Both support `--operation-id UUID` for reuse of the **same exact receipt** after an
unconfirmed response. Never generate a new receipt to conceal uncertainty. The
job must be an idle reviewed execution in `interrupted`, with the exact current
revision and retained unknown grant. Live runners, closing admission, other
origins, stale revisions, unknown grants, reused action IDs and missing/false
acknowledgement reject without changing data. A busy denial is not permission to
force the journal. No model, executor connection, grant, fallback or automatic
retry is needed or invoked. Empty/unissued interruptions can still use normal
Stop; this command requires actual uncertain issuance.

The atomic `abandoned` checkpoint retains original input/context, approved plan,
assignments, unknown attempt, any ACK/report and result metadata. It records the
exact decision receipt, authenticated actor class (`origin_node` or
`coordinator_owner`) and timestamp; unissued tasks are cancelled locally. Unknown
tasks/effects are **not** changed into success or remote cancellation. Restart
cannot revive tracking/authority. Exact retries by the same actor inspect without
another write; different decisions/actors cannot replace the receipt. Legacy
run/resume/pause/Stop and fresh execution commands do not replay this work. An
original execution receipt may still be inspected through exact reuse, not reissued.

Guarded API: `POST /api/commands/requests/abandon`. EEF takes exactly
`schema_version: 1`, `expected_runtime_id`, `command`; the node adapter instead
takes `schema_version: 1`, `expected_node_id`, `command`. The command contains only
`id`, `expected_revision`, canonical `operation_id`, `grant_id` and
`acknowledge_effects_may_continue: true`. Node forwarding binds the decision's
receipt to authenticated submission context; a caller cannot supply its own actor.
Cross-site callers and extra authority fields reject.

`coordinator_request_abandonment` confirms a persisted **local tracking decision**,
not effect completion. It reports `tracking_abandoned: true`,
`remote_stop_confirmed: false`, `remote_effects_unknown: true`,
`execution_requested: false`, `automatically_retried: false`. Ordinary job views
expose closure/uncertainty metadata without the private receipt or parameters;
`can_abandon_tracking` is a state hint, not admission or authority. There is no
resume, recovered output, rollback, remote process termination or migration promise.

Keep history for investigation. `jobs remove` is a **separate explicit** action
after abandonment, never automatic cleanup. Removal loses coordinator receipt
recognition/history, not the node's consumed nonce ledger. Missing history or
receipt evidence still cannot prove no effects; neither removal nor abandonment
authorizes recreating/repeating uncertain work. Do not erase the node journal.
This is the abandon-tracking recovery branch, not full execution reconciliation.

## Supplied structured proposals (unreleased)

Automation may submit a proposal for **review only**, without interpreter
inference or an installed model:

```powershell
eefn requests review-proposal --file proposal.json --json
```

The UTF-8 file (optional BOM, at most 64 KiB) contains exactly `schema_version: 1`,
`original_text`, and `proposal` from [the existing interpretation contract](INTERPRETATION.md).
The node must already be running and connected. CLI validates file shape; the
runtime independently validates proposal shape, and EEF checks capability hints
against fresh owner-disclosed network registrations. The input node need not
implement the suggested feature; undisclosed/stale/disconnected peers contribute
no vocabulary. Neither a file nor a schema-valid reply supplies origin,
permission, approval, a job, a plan or an execution grant. Original text is retained.

This adapter uses the same bounded submission channel, current-runtime invalidation,
confirmed response checks and no-replay behavior as model-based review. It reports
`interpretation_source: supplied_proposal`; it does not claim the proposal came
from a model or was semantically approved. New CLI checks also compare the exact
supplied proposal with the confirmed result. Older nodes return unsupported;
there is no inference/chat/job fallback, model selection, download or durable receipt.
Input files may contain private user text; do not include them in public reports.

Model-based `requests review/open` now obtain the same network vocabulary before
inference, with a five-second read-only lookup deadline and no local fallback or
automatic replay. EEF recomputes membership after inference and on supplied
proposal revision. Offline `requests preview` stays local. Discovery remains
self-only until the coordinator owner grants directional peer disclosure and
restarts EEF; this is not execution permission. See
[cross-node interpretation context](INTERPRETATION.md#cross-node-interpretation-context-unreleased-after-alpha6).

Local owner endpoint: `POST /api/commands/requests/review-proposal`, with exactly
`schema_version`, `expected_node_id`, canonical UUID `request_id`, `text`, and
`proposal`. It remains available in no-dashboard builds. This is an additional
deterministic input adapter, not completion of interactive clarification or planning.

## Request preview (development)

Added in v0.4.0a4; requires an explicitly selected model. It is not available in
the older alpha.3 installers. The heading retains its development anchor for links.

```powershell
eefn models hints --backend ollama --model OWNER_SELECTED_MODEL --role request_interpreter
eefn restart --wait-seconds 30 --json
eefn requests preview --text "Is this node running?" --json
```

The model must already be selected and support `llm.infer`; use `--backend
llamacpp` for an existing GGUF selection. `hints` replaces the selected model's
role list; include any other intended roles too. Selection/role changes require
restart. Preview uses exactly one **applied** `request_interpreter` role, not an
arbitrary installed model or pending configuration. Missing/ambiguous roles are
errors, not permission to download or pick another model.

Preview calls the node's configured model backend and validates an untrusted
proposal. It works with the coordinator connection paused and browser UI disabled.
The text is not sent to EEF and no job, plan, permission or action is created.
`execution_authorized` and `dispatched` are always false. A schema-valid result
still requires review; it is not proof that the model understood the request.
See [proposal schema, bounds and limitations](INTERPRETATION.md).

One preview runs at a time, with a 20-second service deadline and 512 output
tokens. Partial/unknown backend completion is rejected. Restart invalidates
in-flight previews; a shared/external model server may continue computation after
the local request is cancelled. CLI reply timeout is 25 seconds. Requests are not
persisted or automatically retried, and this command never falls back to chat,
job submission or legacy endpoints.

Local owner endpoint: `POST /api/commands/requests/preview` with
`schema_version: 1`, `expected_node_id`, canonical UUID `request_id`, and `text`.
The JSON report has `report_type: request_preview`, target/runtime/correlation
identity, `success` and either `result` or `error_code`.

Common errors: `node_not_running`, `runtime_unavailable`, `runtime_changed`,
`interpreter_unavailable`, `interpreter_ambiguous`, `interpreter_busy`,
`interpreter_backend_failed`, `interpretation_timeout`, `interpretation_incomplete`,
`interpretation_invalid`, `invalid_preview_input`, `invalid_preview_request`,
`invalid_interpreter_context`, `unsupported_command`, `preview_unconfirmed`.
Existing deterministic commands remain model-independent.

### Coordinator review (unreleased after alpha.6)

```powershell
eefn requests review --text "Is this node running?" --json
```

This explicit command uses the same applied interpreter selection and bounded
inference as preview, then sends original text and the strict proposal through
the running node's authenticated EEF connection. EEF revalidates the schema and
capability hints using that session's accepted registration snapshot. It stamps
origin node/area and request identity itself; caller/model fields cannot override
them. Advertised capabilities are context, not permission to execute.

The report has `report_type: request_review`, `coordinator_acknowledged`, and a
`coordinator_request_review` result containing `request_context` and the validated
interpretation. It returns `review_required` or `clarification_required` with the
model's question. A successful review is not approval, semantic verification or
completed work. `execution_authorized`, `dispatched`, and result `persisted` are
false: there is no plan, job, execution grant, clarification session or durable
review receipt. Answering a question requires a new explicit request for now.

An active EEF connection is required to complete review; a paused/offline node
may still run local interpretation before reporting `coordinator_unavailable`.
Preview remains available without EEF. Restart invalidates an in-flight review;
EEF may already have reviewed it. No retry, chat fallback, model selection or
download is automatic. Submission waits at most 120 seconds after bounded
inference; the CLI deadline is 150 seconds. A lost/mismatched reply reports
`review_unconfirmed`, with `outcome_unknown: true`; only review may have happened,
never execution requested by this command.

Local owner endpoint: `POST /api/commands/requests/review` uses the same body as
preview. Additional errors: `coordinator_unavailable`, `review_rejected`,
`review_unconfirmed`. Older EEF versions may reject the new submission kind;
`unsupported_command` describes an older node's missing local endpoint.

## Added in v0.4.0a4: model download commands

```powershell
.\eefn.exe models installed --json
.\eefn.exe models inspect --model INSTALLED_OLLAMA_ID --json
.\eefn.exe models install-plan --model CATALOG_OR_OLLAMA_ID --json
.\eefn.exe models install --model CATALOG_OR_OLLAMA_ID --json
.\eefn.exe models download-status --json
.\eefn.exe models download-status --id DOWNLOAD_UUID --json
.\eefn.exe models cancel-download --id DOWNLOAD_UUID --json
```

These commands require the running alpha.4 node's local command API, not a
browser, EEF connection or loaded LM. They discover the actual endpoint from the
existing instance marker. A stopped node returns `node_not_running` without
starting a process or transfer. Published v0.4.0a3 lacks this command endpoint;
the CLI does not retry through legacy routes. Existing selection commands remain
unchanged. Remote coordinator download CLI commands are not added here.

`installed` exposes the existing installed-model browser, catalog, backend and
storage report. `inspect` reads installed Ollama capability metadata; it does not
load the model. `install` explicitly admits one transfer using the saved provider
and catalog. Auto provider selection prefers reachable Ollama. GGUF downloads
retain the existing HTTPS, declared size, digest verification and disk checks.
Ollama manages its own storage; its free space is unknown, not the node's disk.
Selecting, loading and assigning interpreter roles remain separate commands.
No new default model or automatic bootstrap has been selected or enabled.

An install returns a CLI-generated `operation_id`, `accepted:true` and
`completed:false`. The running service owns the transfer after the CLI exits.
Inspect `download.state` for `downloading`, `installed`, `cancelled` or `error`;
an accepted request is not installation success. Cancellation requires the exact
ID and reports `cancel_requested` separately from terminal status. It never
cancels whichever download happens to be active. Cancellation cannot undo a
completed file or stop other users of shared Ollama layers/transfers.

Receipts retain only the latest transfer **in memory**, not durable job history.
A newer admitted download replaces the old record; a process restart loses it.
Repeated admission of the currently retained ID is refused. This is not durable
deduplication or exactly-once installation. Ordinary node runtime restarts retain
the service and its transfer; a process upgrade/shutdown does not.

A lost or unrecognized mutation reply returns `outcome_unknown`, retains the ID,
and does not retry. Query `download-status --id` before deciding to try again.
`operation_not_found` means the ID is not retained; it does not prove the request
was never accepted. Other reports include `download_command_rejected`,
`inspection_failed` and `unsupported_command`. JSON stays on stdout, diagnostics
on stderr; unsuccessful commands exit nonzero.

The typed local endpoint is `POST /api/commands/model-downloads`, with
`schema_version:1`, `expected_node_id` and a `command` tagged by `operation`:
`installed`, `inspect`, `install`, `status` or `cancel`. Install carries `model`
and a canonical UUID `operation_id`; inspect carries `model`; status accepts a
nullable `operation_id`; cancel requires one. Unknown fields, wrong node identity
and unsupported schemas are rejected before transfer admission. The existing
local-origin guard protects it. No remote permissions are widened.

### Added in v0.4.0a4: install preflight

`install-plan` is a read-only command (`operation:"plan"` with `model` on the
same typed endpoint). It may query the configured Ollama service's model list,
but never fetches model artifacts, hashes cached weights, creates a directory,
changes transfer progress or modifies selections. Explicit llama.cpp selection
does not query Ollama. Automatic provider choice still prefers reachable Ollama.

Inspect `plan.can_request` and `plan.blocker`; a successfully inspected blocked
plan is still a successful read command. Reported blockers include an unreachable
Ollama service, an active download, insufficient/unknown local space and a cached
file whose size differs from the catalog. Malformed artifact fields or duplicate
matching catalog IDs reject the command. Installation repeats this inspection
against current configuration before admission, then retains existing locked
configuration/approval checks and transfer-time disk/digest verification.

For GGUF, `artifact` is a normalized version-1 descriptor containing HTTPS URL,
SHA-256 and positive byte count. The same validator is used by transfer execution.
Local disk availability is a snapshot, not a reservation; an existing cache file
is only a candidate until installation verifies both digest and size. A corrupt
cached file is preserved for inspection, never silently replaced.

For Ollama, the exact artifact, byte count and free storage are unknown/provider-
managed. A catalog's GGUF hash/size is not reused as Ollama metadata. A reachable
provider can accept an explicit request even though those checks remain unknown.
`can_request` is not a guarantee of installation, usable inference, or sufficient
space for the provider. License/source labels are catalog metadata, not a review;
`license_reviewed` and `runtime_compatibility_verified` remain false. This plan is
not an automatic bootstrap policy, default-model selection or execution grant.

## Added in v0.4.0a3: backend-only operation and connection controls

Both `eef --no-ui` and `eefn --no-ui` keep their command/API services running
without mounting browser HTML, JavaScript, CSS or legacy pages. Node API-only
mode also omits the interactive file-picker endpoint. These are runtime flags;
developers can additionally compile either role with `--no-default-features`
to omit bundled dashboard assets entirely. Default builds retain the dashboard.

```powershell
.\eefn.exe connection show --json
.\eefn.exe connection pause --json
.\eefn.exe connection resume --json
.\eefn.exe connection pair-local --json
```

Connection controls require a running development node and call the same backend
operations as the existing UI. Pause saves disabled outgoing EEF connections;
resume uses the existing endpoints and trust. Both request a node runtime restart
to apply the change, including other pending settings. This does not restart
Windows or shut down the local command API. Inspect the returned saved policy
separately from live connection state; acknowledgement is not reconnection.

`pair-local` explicitly clears selected EEF endpoints and enables automatic
same-user local pairing. It is an owner configuration change, not a network scan
or trust grant for arbitrary peers. It must not be used merely to inspect status.
No connection command implicitly downloads models or executes inference.

Unreleased private pairing also supports `eefn connection pair-stdin --json`
for a private piped invitation/report, without a temporary invitation file or
secret argument. Clients call `POST /api/commands/connection/pair` with strict
node/runtime/correlation identity and a private code body. The shared core
preserves feature permissions, identity, models and paused preference, saves
the certificate/credential, and requests (not verifies) a node restart. It does
not provision/rotate credentials, restart EEF, enable a paused connection or
approve work. Lost/substituted replies remain unknown, without fallback/retry;
inspect `connection show` and diagnostics. The correlation is not a durable
receipt. See [input, privacy, bounds and setup limits](NODE-CREDENTIALS.md#file-free-invitation-input-unreleased-after-alpha6).

Saved UI preferences are `dashboard.ui_enabled` on the node and `web.ui_enabled`
on EEF, both defaulting to true. False hides the browser while keeping commands
available. Apply saved preferences with the role's restart command. On the node,
the browser route gate changes without rebinding its command listener; that node
runtime restart still applies other pending settings. `--no-ui` overrides a saved
true value. Builds without dashboard assets cannot enable the browser.

Compatibility: existing node `dashboard.host/port/enabled` fields still configure
the local HTTP listener. Legacy `enabled:false` disables that listener, including
commands; use `ui_enabled:false` or `--no-ui` to hide only the browser. No old
explicitly disabled listener is silently enabled by the new preference.

## Added in v0.4.0a3: jobs through the running node

For new explicit text work, the unreleased grant-aware command is:

```powershell
eefn jobs request-text --prompt "Explain this topic" --role USER_SELECTED_ROLE --operation-id UUID --json
eefn requests submit --text "Show status" --template system_info --operation-id UUID --json
```

It saves/compiles caller inputs, requires no interpreter and performs no inference
until separate owner approval and origin execution. Inspect `requests plan` with
the returned job ID. Preserve the receipt and use `jobs find` after a lost reply
or partial compilation. Exact retries retain immutable input and never start a
second model call or silently replace choices. See [explicit request contract](EXPLICIT-REQUESTS.md).
The subsequent unreleased boundary moves `generate-text` to this same workflow;
it no longer directly executes legacy work. The syntax below remains available,
but a compiled request awaits separate approval and execution.

```powershell
.\eefn.exe jobs list --json
.\eefn.exe jobs find --operation-id RECEIPT_UUID --json
.\eefn.exe jobs generate-text --prompt "Explain the current task" --role request_interpreter --json
.\eefn.exe jobs get JOB_ID --json
.\eefn.exe jobs output JOB_ID --json
.\eefn.exe jobs pause JOB_ID --json
.\eefn.exe jobs resume JOB_ID --json
.\eefn.exe jobs stop JOB_ID --json
.\eefn.exe jobs remove JOB_ID --json
```

These deterministic commands need the development node; `generate-text` and
`output` also need the development coordinator. They use the running node's
actual local command endpoint and existing authenticated EEF connection, never
launching a second node or guessing an executor from its hostname. They work in
API-only mode. Node-local management approval is not required to control this
node's own jobs; execution still requires the executor's normal permissions.

EEF stamps the origin from the connection. List, status, controls and output are
restricted to jobs submitted by that stable origin node, not jobs merely executed
there or created by the coordinator owner. `generate-text` optionally accepts
`--target-node`, `--backend` and `--model`; `--role` is an exact advertised model
label, not authority. This now creates an existing durable explicit reviewed
text request, not an automatic natural-language plan. Prompt size is bounded to
32768 bytes.

Acknowledgement is not completion. Inspect `data.status`; pausing/stopping may
remain in progress until the current step finishes. Stop does not undo remote
effects. Legacy resume is now refused, including for retained ordinary jobs;
granted uncertain work cannot be resumed by that path. Remove deletes only
finished job history and cannot stop an active job.
List returns the existing bounded view (first 100 visible jobs plus total).

Unreleased coordinator reliability changes after alpha.6 serialize admission,
shutdown, Stop and runner retirement. A Stop racing runner cleanup cannot leave
`stopping` with no local runner to settle it. An uncertain exit can still produce
`interrupted`; inspect the saved state rather than assuming remote work stopped.
Stop cannot undo completed effects. These changes are not in published alpha.6
installers and do not introduce automatic retries or remote execution leases.

The unreleased assignment follow-up allows only one executor assignment per
durable task attempt, including concurrent/repeated calls. Model jobs select one
provider for that attempt; they no longer silently traverse a fallback chain
inside it. A policy-permitted retry needs a new recorded attempt. The assignment
includes the selected `node_id`, `model_id` and `backend`; old records without a
backend keep it unknown rather than guessing. Duplicate/stale/reassigned writes
fail without altering the checkpoint. An assignment failure does not mark an
unchosen model backend unavailable. Legacy non-job interactive chat retains its
old fallback behavior; it does not have durable-job/grant semantics.

This is coordinator-side journal integrity, not node-side replay fencing, an
execution grant or proof a previous remote attempt stopped. Existing retry-safe
policy remains; uncertain remote work still needs the roadmap's expiry and
acknowledged-stop boundaries before safe active-work migration can be claimed.

`output` explicitly returns text-model results only: at most 32 tasks and 32768
aggregate UTF-8 content bytes with truncation flags. Pending text is null. Normal
status still omits output; file contents, media and raw parameters are not exposed
by this command. Output can contain sensitive user-requested text; don't publish
it as diagnostics without reviewing it.

Failures distinguish `not_sent`, coordinator `job_rejected`, and unconfirmed
transport outcomes. Mutating errors remain conservatively uncertain unless known
not sent: an error does not prove a checkpoint was rolled back. Inspect list/status
before an explicit retry. Job commands are not automatically resubmitted after timeout or
reconnection. Receipt protection for creation is limited as described below;
other command operation IDs remain correlation only.

The unreleased coordinator reply path also fails promptly if a node's bounded
outbound queue is full, keeping shared event processing responsive. A reply may
therefore be unconfirmed even though its job was saved or executed. This is not
rollback or permission to replay: use receipt lookup and inspect retained status.

Creation recovery in the current development build: the CLI generates a receipt
`operation_id` before sending, including it in an unconfirmed local-reply report.
With an updated coordinator, creation returns `creation_correlated:true` and the
same ID is persisted as the job's `request_context.request_id`. Use `jobs find
--operation-id RECEIPT_UUID` to search this origin node's retained creation
history without executing anything. Matching happens before the 100-result limit;
`total` and `truncated` describe the matched set. Inspect each returned job status.

Lookup is not an execution receipt ledger: pause/resume/stop/remove commands do
not become new job creations. No match can mean in-flight work, removed history,
a different coordinator, or an older coordinator without correlation support;
it does **not** prove that creation was never accepted. Published alpha.6 and older
coordinators do not deduplicate receipt reuse and may return multiple matches.
Do not automatically retry an uncertain mutation. Transport IDs remain independently generated, so reused
correlation cannot mix up replies. Both roles must be updated for lookup; older
coordinators reject `jobs.find` rather than returning an unfiltered list.

Unreleased after alpha.6: node-origin `jobs.create` admission uses the authenticated
origin node plus its operation receipt. An identical normalized goal returns the
retained job with `data.admission.reused:true`, without dispatch, resume, new history
events or resetting results. New admission returns `reused:false`. Different
description/template/parameters/constraints under the same receipt fail closed;
so do ambiguous duplicate receipts left by older versions. The original origin
area stays unchanged even if the node has since moved. Different origin nodes
may independently use the same receipt. This applies to the existing node command
API's `operation_id`; the CLI still creates fresh receipts and never auto-replays.

The journal serializes lookup/insertion with existing shutdown admission, and
the receipt remains durable in the job's existing context across coordinator
restart. It is **retained-history deduplication**, not a permanent receipt ledger:
removing history ends protection, switching/restoring databases can lose it, and
older coordinators have no such guarantee. `admission.retention` explicitly says
`while_job_history_exists`. Existing interrupted work stays interrupted; controls
retain their existing semantics. No new execution lease, remote cancellation or
exactly-once side-effect guarantee is introduced. Loopback-owner creation without
node origin is unchanged. Lookup scans retained records under the journal lock;
it adds no unbounded tombstone collection or separate job framework.

## Added in v0.4.0a3: model route inspection

`eef models route --capability llm.infer --role request_interpreter --json`
previews matching connected model instances without inference. Optional `--node`,
`--backend`, `--model` and `--limit` (1-32, default 8) narrow the view. Supported
capabilities are `llm.infer` and `vlm.analyze`. A role must match an explicit owner
label; unknown roles do not match. This grants no execution or reservation and
does not prove model readiness. State may change before actual dispatch.

Durable jobs may use `generate_text` or `analyze_image` with
`constraints.model_role` and the existing node/model constraints. Routing does
not escape the requested role/backend when falling back across tiers. Image
payloads must be supplied by the caller; this does not capture camera images or
implement automatic interpretation/planning. Node permissions still apply.

## Added in v0.4.0a3: coordinator discovery controls and restart

These additions are in the development tree, **not the published v0.4.0a2
installers**. They work with existing v0.4.0a2 nodes; only EEF needs the new code.

```powershell
.\eef.exe discovery show --json
.\eef.exe discovery grant --requester NODE_A --target NODE_B --json
.\eef.exe discovery revoke --requester NODE_A --target NODE_B --json
.\eef.exe restart --json
.\eef.exe node restart --node NODE_B --json
.\eef.exe node restart --node NODE_B --wait-seconds 60 --json
```

Use exact stable IDs. A grant lets A inspect B, not B inspect A, and never grants
execution or direct peer access. Self visibility is implicit and cannot be
revoked. Duplicate grants and absent revocations are no-ops. Bounds and secret
redaction are unchanged. No node permissions or connection settings are modified.

The running coordinator owns these commands. `show` distinguishes `saved_policy`
and `applied_policy`; `restart_required` refers to discovery policy only. Saving
a grant or revocation does not activate it. `restart` acknowledges the request,
not completion: verify a new diagnostics `runtime_id` and node reconnection.
Existing jobs are not automatically replayed. Plan restarts around active work.

`node restart` restarts the **node app, not Windows**. The node owner must first
enable Settings > Management from EEF > "Allow EEF to apply settings and restart
this node." This existing permission also permits remote settings changes; the
command cannot enable it itself. Denial returns `approval_required` with exit 1
and sends no restart. Restarting can interrupt active work; use an idle node.

Unreleased after alpha.6: updated nodes add `control_schema_version:1`,
`remote_allowed`, `runtime_id` and `connection_state` to existing
`node.configure/status`. EEF uses reserved status capacity for restart approval,
runtime inspection and completion polling. Model-work saturation alone therefore
does not block this workflow. The node checks approval again when applying restart.
An explicitly rejected full control pool reports `node_busy`/`not_started`, without
retry. Legacy same-node status replies without the schema retain read-only
get/diagnostics compatibility; malformed versioned replies and missing transport
replies do not trigger fallback or restart.

The default wait is 30 seconds (0-60 supported). Results carry an operation ID,
stable node ID, prior/new runtime IDs, `restart_requested`, `acknowledged`,
`completed` and `outcome_unknown`. With a nonzero wait, success means a connected
node with the same stable ID and a new runtime ID was observed. With zero wait,
success means acknowledgement only, with `completed:false`. A timeout or lost
reply does not prove failure to restart: inspect diagnostics before any retry.
No restart is automatically resent, and these operation IDs are not durable
deduplication keys. Completion does not verify recovered jobs or model readiness.
POST `/api/commands/node/restart` accepts `{node_id,wait_seconds}` under the
existing local owner/Origin/Host guard. The command works with v0.4.0a2 nodes'
diagnostics and management protocol; no new remote listener is introduced.

GET `/api/commands/discovery` returns this policy view and `runtime_id`. POST
accepts `{schema_version:1,expected_runtime_id,command:{operation,requester,target}}`;
the CLI reads the runtime ID automatically. `show` has no requester/target fields.
Wrong schema/runtime IDs, unknown fields and untrusted origins are rejected.
No automatic retry after a failed mutation. Read state before retrying explicitly.

The core service serializes edits and reads the latest saved configuration before
changing one grant, preserving unrelated pending settings. This protects concurrent
commands within the running coordinator; it is not distributed configuration
locking or a merge guarantee for external file editors/full-config replacements.
Corrupt or missing saved config is rejected rather than recreated from defaults.

`tools/test-live-discovery.mjs` is an explicit live-network acceptance test. Set
`EEF_LIVE_TEST=1`, `EEF_LIVE_EEF_BINARY`, `EEF_LIVE_EEF_CONFIG`,
`EEF_LIVE_NODE_BINARY`, `EEF_LIVE_NODE_CONFIG` and `EEF_LIVE_REMOTE_NODE` to
owner-selected local paths and the other node's stable ID. Use an idle test
coordinator with no pending changes. It temporarily grants one-way visibility,
restarts EEF, tests discovery/pings, revokes the grant and restarts again. It
attempts grant cleanup on failure and records local evidence under `.validation`.
It does not deploy binaries, update remote nodes, run media/model workloads, upload data
or infer physical device count from IDs alone. Do not run against production jobs.
Set `EEF_LIVE_RESTART_NODE=1` additionally to request a remote node restart after
grant cleanup. With local management approval it verifies the new runtime ID;
without approval it verifies denial and sends no restart. This is opt-in because
it can interrupt work on the other node.
Set `EEF_LIVE_MODEL_INVENTORY=1` to also verify read-only model registration
inventory on both nodes. This requires the development coordinator command below;
it does not scan backends, download models or run inference.

## Added in v0.4.0a3: local node restart and actual API discovery

```powershell
.\eefn.exe restart --json
.\eefn.exe restart --wait-seconds 60 --json
```

This is a local owner command: remote-management approval is not required. It
restarts the node app, not Windows, and applies all pending node settings. It
refuses to start a stopped node. It sends one restart request, never an automatic
retry. The existing browser and approved remote restart paths call the same core
restart operation; their permission guards remain unchanged.

The default completion wait is 30 seconds, with 0-60 supported. Zero waits for
acknowledgement only. A positive wait requires the same stable node ID and a new
initialized runtime ID. Results distinguish requested, acknowledged, completed
and unknown outcome. Timeout/lost replies return failure with an uncertain outcome,
not proof that restart did not happen. Completion does not verify EEF reconnection,
workloads or model readiness. If the restart disables the local API, local
completion cannot be observed; inspect state before any explicit retry.

POST `/api/commands/restart` takes
`{schema_version:1,expected_node_id,expected_runtime_id}` and checks the current
node/runtime before queueing. Null runtime is allowed only when the current node
has not initialized one, allowing a repaired startup configuration to be retried.
Operation IDs in CLI output are correlation IDs, not durable deduplication keys.

New nodes record their bound local API beside the configuration as
`CONFIG_FILENAME.api.json`. Commands consult it only while the instance lock is
held by the running node, validate identity and loopback scope, and reject corrupt
or oversized records. Thus saved port/disable settings do not redirect commands
away from the still-running API. Restart completion polling follows the updated
record if the port changes. This is local discovery, not a new remote listener.
Missing records fall back to config for older nodes; stopped-node commands ignore
stale records. Failure to write discovery is logged without stopping the node.

## Added in v0.4.0a3: node model selection commands

Unreleased after alpha.6, both local `eefn models interpreter --backend BACKEND
--model MODEL_ID` and approved `eef node models --node NODE_ID interpreter
--backend BACKEND --model MODEL_ID` use one node-core atomic role assignment.
`clear-interpreter` is available through both adapters. These are not in alpha.6
installers. Node and coordinator adapters validate exact saved role/target reports;
unconfirmed replies do not cause a retry or legacy fallback. Saved roles are not
applied readiness or a model-quality attestation. See setup guidance above.

The development coordinator also exposes these operations for a connected node:

```powershell
.\eef.exe node models --node NODE_ID show --json
.\eef.exe node models --node NODE_ID select-ollama --model OWNER_MODEL_ID --modality text --role request_interpreter --json
.\eef.exe node models --node NODE_ID hints --backend ollama --model OWNER_MODEL_ID --clear-roles --json
.\eef.exe node models --node NODE_ID provider auto --json
.\eef.exe node models --node NODE_ID remove --backend ollama --model OWNER_MODEL_ID --json
```

Both EEF and the target node need the development version. `show` is read-only
over the existing authenticated coordinator connection. Changes require the
node owner's existing management approval. EEF first inspects support/approval;
the node checks current approval again under its configuration lock before
applying the change. Approval cannot be granted by these commands. Older nodes
fail with `model_commands_unavailable`, without a legacy full-config save fallback.

Local and remote model commands share their argument adapter and node core
operation. Remote `select-gguf --file`/`--projector` paths refer to existing files
on the **target node**, not on the coordinator. Removal never deletes model files.
No download, model load, provider switch or restart is implicit. Run the separate
approved `eef node restart --node NODE_ID` command when ready to apply all pending
settings. Saved selections and registered models remain distinct.

The EEF result wraps the node report in `data` and includes `mutation_requested`,
`acknowledged` and `outcome_unknown`. A mutation is sent once; a lost/ambiguous
response returns `model_change_unconfirmed`, not an automatic retry. Inspect
saved selections before retrying. This does not promise durable deduplication or
exactly-once writes. Approval denial is known not to apply the selection change.

These commands operate on the existing Ollama/GGUF configuration through the node
core service, without JSON editing or a browser:

```powershell
.\eefn.exe models show --json
.\eefn.exe models select-ollama --model OWNER_MODEL_ID --modality text --role request_interpreter --json
.\eefn.exe models select-gguf --model OWNER_MODEL_ID --file C:\Models\owner-model.gguf --json
.\eefn.exe models hints --backend ollama --model OWNER_MODEL_ID --capability llm.infer --role request_interpreter --json
.\eefn.exe models hints --backend ollama --model OWNER_MODEL_ID --clear-roles --json
.\eefn.exe models provider ollama --json
.\eefn.exe models remove --backend ollama --model OWNER_MODEL_ID --json
```

Selection commands do not download/load models or restart the node. They report
`saved_selections` separately from `registered_models`; registration is not proof
of loaded state. Apply pending changes with `eefn restart`. Offline commands
hold the instance lock and save for its next start. An existing saved identity is
required (normally created by setup; `network set` can initialize it).

Provider selection remains `auto`, `ollama`, or `llamacpp`; auto prefers an
available Ollama service. Selecting a model does not silently change provider.
Ollama selection names need not be installed yet; unavailable selections are not
advertised on connection. GGUF requires an existing file and can accept
`--projector`, `--gpu-layers`, `--context`, and an optional advanced `--port`.
Without a port, the command reuses the model's saved internal port or briefly
allocates a loopback port; availability is checked again when the backend starts.
It never launches a model server during selection.

Repeated `--capability` and `--role` flags form bounded lists. Capabilities may
restrict existing adapter support, not add unsupported executors. `--clear-capabilities`
on `hints` disables that selection's inference capabilities; it does not remove
the model or unload it immediately. Role labels do not grant permissions or select
a default interpreter yet. Empty roles explicitly clear labels. Re-selecting an
existing model without selection hints preserves its current hints.

The local owner API is POST `/api/commands/models`, taking
`{schema_version:1,expected_node_id,command}`. Operations are `show`,
`select_ollama`, `select_gguf`, `hints`, `remove`, and `provider`; see the shared
typed commands in `eefn/src/model_selection.rs`. Edits use the same config lock as
other node settings, preserve unrelated values, reject corrupt/missing files,
and avoid writing on no-ops. Removing a selection never deletes model files.
Output omits GGUF/projector paths, backend service URLs and credentials. A failed
or timed-out response is not permission to automatically retry a mutation.

Published older nodes do not enforce these new selection restrictions. Do not
downgrade a restricted node and assume that its restrictions remain enforced.

## Added in v0.4.0a3: registered model inventory

```powershell
.\eef.exe models list --json
.\eef.exe models list --node NODE_B --json
.\eef.exe models list --capability vlm.analyze --limit 8 --json
.\eef.exe models list --after NODE_A --json
```

GET `/api/commands/models` is the same read-only owner operation; optional query
fields are `node_id`, `capability`, `after` and `limit`. The node filter and cursor
are mutually exclusive. Pages contain 1-32 nodes (default 8), each with its models,
within 512 KiB. Follow `next_after`; pages are not a frozen network snapshot.
Disconnected/unregistered target nodes fail rather than appearing as empty models.
A connected, registered node with no selected/advertised models has `models:[]`.

Each model instance uses the tuple `(node_id, backend, model_id)`, so the same
model name on two backends/nodes remains distinct. Duplicate identities in a
registration are rejected, not silently merged. Registration freshness is shown
separately; stale metadata is not readiness or proof that inference will work.

Inventory uses [versioned model metadata](MODEL-METADATA.md) when supplied, or
normalizes legacy text/VLM hints when it is absent. `metadata_source` identifies
which path was used. Unrecognized legacy modality supplies no capability hints.
It does not inspect every installed file or backend, invent roles, measure
residency/memory, or verify advertised capabilities. Unreported
roles, lifecycle and resource estimates are `null`, not zero. Private paths,
network addresses, configuration and credentials are not projected.

The command loads/downloads no model and grants no permission. The subsequent
metadata increment makes existing text/vision routing respect advertised
capabilities/state; general model configuration, role bindings and new capability
executors remain later work. Published v0.4.0a2 installers do
not include this new coordinator command; their nodes remain compatible.

## Node network commands (v0.4.0a2)

```powershell
.\eefn.exe network show
.\eefn.exe network set --name "Laptop" --advertise-address 26.1.2.3
.\eefn.exe network set --coordinator-id eef-laptop --coordinator-address 26.1.2.3:51335 --coordinator-state standby
.\eefn.exe network show --json
.\eefn.exe network set --clear-address --clear-coordinator
.\eefn.exe --no-ui
```

## Diagnostics and private pairing

### Node request capacity (unreleased after alpha.6)

```powershell
.\eefn.exe connection limits --json
.\eefn.exe connection limits --work 4 --control 2 --json
.\eefn.exe restart --wait-seconds 15 --json
```

These commands contact the running node; they do not launch a second runtime.
`limits` with no options is read-only. Specified values merge with current saved
settings under the shared config lock, preserving identity, permissions and other
configuration. Identical values do not rewrite the file. The response separates
`saved_request_limits` from `applied_request_limits` and never requests restart;
use the explicit restart command when it is safe to interrupt current work.

Node configuration accepts `request_limits: {"work":16,"control":2}`. These are
the defaults for old configurations as well. `work` supports 1-128 simultaneous
network execution futures; `control` supports 1-8 separate ping/status/restart
futures. The reserved operations are `system.ping` and `node.configure` actions
`status`/`restart`. This changes admission only: restart still needs the normal
owner approval, and every admitted request still passes the engine's permissions.
Model installs/updates and other management actions use the work allowance.
The reserve is per wire request, not an unbounded workflow queue. Updated EEF/node
restart checks and polling use status capacity throughout; legacy get/diagnostics
compatibility and model controls use work capacity. A received reply may still
occupy its request slot briefly.

Owner configuration saves (or already-approved remote configuration saves) validate
the limits before writing. Apply changes by restarting the node. Status/diagnostics
report `request_limits` from the last connected runtime, or null when unavailable,
and cumulative `busy_rejections` for the service process. Saved configuration can
differ until restart. No visual UI or new startup permission is required.

Excess requests return `success:false`, `error_type:NodeBusy`, `not_started:true`;
they are not queued or automatically replayed by node admission. Existing
coordinator retry rules still apply to retry-safe work. Capacity is released when
the request future ends, including failures/cancellation. A failed busy-response
write ends the connection rather than continuing a potentially partial frame.
This bounds incoming network request futures, not CPU/RAM, downloads started by
other services, local API operations or remote effects that outlive cancellation.
It is not completion of local workload resource budgets or execution grants.

Remote model commands now surface confirmed capacity rejection as `node_busy`
with `not_started:true` and `outcome_unknown:false`. `mutation_requested` says
whether rejection happened at inspection or after a mutation was sent. No model
change started in either confirmed rejection case, and no automatic retry is
sent. Missing `not_started` confirmation or a lost reply remains uncertain.

### Reports and pairing

```powershell
.\eefn.exe network diagnose --json
.\eef.exe diagnostics --json
.\eef.exe diagnostics --node YOUR_NODE_ID --samples 5 --json
.\eef.exe invite --address YOUR_COORDINATOR_ADDRESS:51335 --json
```

EEF commands contact its running loopback API; node diagnostics also work while
stopped, with `running:false` and no live measurements. `--config` selects an
explicit installation. Reports are printed locally, never automatically uploaded.
They include stable node/runtime IDs, software version, basic state and numeric
counters, excluding machine names/addresses/paths/secrets/raw errors. Counters
last for the node-service process: `connection_checks` counts checking events,
`successful_connections` counts connections, and `failed_connection_attempts`
counts disconnected-error events, not unique networks or exhaustive packet loss.

Unreleased after alpha.6, coordinator diagnostics distinguish connection stages:
`connected_node_count` counts authenticated live connections;
`registered_node_count` counts those whose registration the gateway accepted;
`registration_pending_node_count` counts the remainder. These three counts share
one gateway snapshot and exclude historical/disconnected routes. Registration
can still be awaiting scheduler processing, and a registered node may advertise
no capabilities or models.

`remote_provider_count` and `healthy_remote_provider_count` count remote
capability/action provider entries in a separate scheduler-registry snapshot,
not nodes. Local adapters are excluded; health is the registry's current flag,
not a new probe, capacity reservation or permission check. `model_count` remains
a separate model-registry count. Across these snapshots, connection/registration
changes may still be propagating. The report's `readiness_note` makes this
limitation explicit: no aggregate count guarantees a particular job can run.
Check the required node/capability, model and permissions; dispatch can still fail.

Unreleased after alpha.6: `node_event_loss_count` counts skipped gateway events
since this EEF runtime started, and `node_state_reconciliation_count` counts
completed live-state reconciliations, including listener startup. Recovery restores
accepted registrations and metrics, not lost requests or completed effects.
Historical routes do not re-add offline nodes. State repair itself does not replay
connection/disconnect triggers into automation. No work is automatically resubmitted.

`assistant_event_loss_count` and `world_event_loss_count` count gaps in their
separate observer streams. Observers continue accepting later events. A world
gap sets `world_state_may_be_stale` and `world_rules_suspended`; the equivalent
world summary flag is `state_may_be_stale`. Automatic world-condition rules are
suppressed for that world runtime's remaining lifetime. Node reconnection cannot
reconstruct missed application/task/sensor state. Explicit commands and separate
event/keyword rules are unchanged; these counters are not delivery acknowledgements.

Explicit ping probes sample 1-10 `system.ping` calls with a two-second timeout
per sample and report round-trip milliseconds, versions and failures. They do
not test inference or perform device I/O. A failed sample makes CLI exit nonzero.
GET `/api/diagnostics` exists on both local APIs; EEF additionally accepts POST
`/api/diagnostics/probe` with `{node_id,samples}`. Reports contain pseudonymous IDs;
review before sharing. Raw configs/logs are not support reports.

The invite command accepts an explicit reachable host:port (IPv6 in brackets).
POST `/api/network/invite` also accepts `{address}`; omission keeps the hostname
fallback for existing clients. A pairing code contains the network secret and
is deliberately NOT a diagnostic report. Share it privately only with the node
owner; never publish it in logs, issues or release notes. Address selection does
not configure firewall/overlay software or test reachability.

Addresses are examples, not defaults. IPv6 endpoints use `[address]:port`.
Coordinator advertisements require a port; node advertisements may be a host
alone. Nothing opens a peer listener or configures an overlay. Active/standby
state is owner-reported metadata, not verified leadership or new authority.

`set` edits only supplied fields, preserving ID, connection endpoints, secret,
models, permissions and other config. `show` omits PSKs/private resource bindings.
Stopped-node commands hold the instance lock; an initial `set` creates ordinary
persistent identity/config without starting models, discovery or listeners.
`show` does not create identity. Offline edits apply at next start.

Running-node commands use its guarded loopback API, reading the configured port
automatically. They never register a second node or edit around its lock. Online
edits report `restart_required` and distinguish saved `network` from
`applied_network`. Restart applies advertisements without changing identity.
If the API is explicitly disabled or an old release lacks the endpoint, commands
fail rather than enabling it or bypassing the running process. Stop to edit offline.

`--no-ui` omits HTML/JS/CSS/legacy pages and ignores `--open-dashboard`, while
keeping local API guards. Existing `dashboard.enabled=false` still disables
the entire API. Other lifecycle command families are not implemented yet;
their existing APIs remain available without a browser.

`--json` prints one result on stdout; logs go to stderr. Success exits 0. Command
failures exit 1 with `schema_version:1`, `success:false`,
`error_code:"command_failed"` and an error description. Clap syntax errors retain
normal behavior (exit 2). Expected node ID is a wrong-instance check, not authentication.

## API and registration

`POST /api/commands/network` retains the existing loopback/Origin/Host guards:

```json
{
  "schema_version": 1,
  "expected_node_id": "your-persisted-node-id",
  "command": {
    "operation": "set",
    "changes": {"advertised_address": "26.1.2.3"}
  }
}
```

Inspect with `"command":{"operation":"show"}`. Unknown fields, conflicting
set/clear operations, malformed addresses and wrong targets are rejected. HTTP
errors retain the existing API shape/status, not the CLI wrapper.

Registration has additive `network:{advertised_address,coordinator}` metadata;
coordinator fields are `coordinator_id`, `address`, `state`. Missing metadata means
no advertisement; missing coordinator state means unknown. Gateway validation
keeps it separate from observed source IP/port. EEF world/status entries expose
it; disconnected entries stay marked offline. Node status reports applied values.
Existing owner-approved remote configuration may also propose `network` changes.

Metadata does not initiate connections, establish trust, authorize execution or
implement failover. Scoped discovery is now available as described below.

## Inspect the registered network

```powershell
.\eefn.exe network peers --json
.\eefn.exe network peers --node node-b --json
.\eefn.exe network peers --limit 1 --json
.\eefn.exe network peers --after node-a --json
```

Requires a running node connected to EEF; offline commands do not start a new
connection for discovery. Requests use the existing authenticated submission
channel. EEF uses server-stamped origin, not a caller-supplied requester ID.
The local API accepts `command:{operation:"peers",query:{limit:32}}`; optional
query fields are `node_id` and `after` (mutually exclusive).

By default a node can inspect **only itself**. EEF's owner can configure exact,
directional disclosure grants in EEF configuration, using the existing owner
config API or a config file. The development grant CLI is described above;
published v0.4.0a2 does not have it. Example:

```yaml
discovery:
  grants:
    node-a: [node-b]
  freshness_seconds: 30
```

This allows A to inspect B, not B to inspect A or A to inspect B's other peers.
No wildcards or implicit transitive grants. Use actual persistent node IDs.
Policy changes, including revocation, apply after **EEF restart**, not when
saved. Invalid policies fail validation; no fallback to unrestricted discovery.
Limits: 256 requesters, 256 targets each, 4096 total grants, 256 KiB policy and
freshness of 1-300 seconds. Policy is local to this coordinator, not replicated.

Results contain a bounded, sanitized registration projection: ID, display name,
advertisement/coordinator hints, capabilities and model ID/backend/modality.
They omit observed connection source IP/port, PSKs, private adapter bindings and
arbitrary model fields. Registration limits are 128 capabilities (128 bytes
each), 64 models (ID 256 bytes, backend/modality 64 bytes each), and 256-byte
display names. Oversized/malformed registrations are rejected, not silently
advertised as complete.

Only currently registered connections are considered. Monotonic heartbeat age
drives `online`/`stale` status; `online` means recent control-channel activity,
not verified reachability from another node. Stale entries retain ID/name but
lose addresses, coordinator hints, models and capabilities. Disconnected entries
disappear rather than returning historical addresses. Use `remaining_fresh_ms`
conservatively and re-query; discovery is not an authorization cache.

Pages have at most 32 entries and a 512 KiB response budget. Continue with
`next_after` until null; cursors are lexicographic IDs, not a snapshot or grant.
Each page rechecks applied policy/current connections. Concurrent registrations
may change results; start a new listing when a complete current view is needed.
Missing and denied individual lookups use the same error to avoid leaking
whether an unauthorized ID exists. `direct_access_authorized` is always false:
knowing a peer address does not authorize a connection, transfer or workload.

Trust boundary: this increment inherits v0.4.0a's shared-network-PSK identity
authentication. It does not provide independent cryptographic identities for
mutually untrusted key holders; a holder of that shared key can authenticate as
an otherwise unused node ID. Exact-ID disclosure policy is not a substitute for
that future security work. Do not use these grants as tenant isolation or expose
an unauthenticated peer listener. Separate peer/issuer credentials remain a
prerequisite for direct data paths and execution leases.

`node tools/test-peer-discovery.mjs` verifies two logical nodes on one PC with
no UI/models: self-only defaults, directional grants, pagination, forged query
rejection, stale redaction, restart-applied revocation and disconnect removal.
It does not establish physical multi-PC/overlay reachability or direct peer execution.

`node tools/test-node-commands.mjs` validates real local processes/public commands
with isolated config, no model and no browser: offline/headless operation, stable
IDs, address separation, coordinator metadata, saved/applied state, permission
preservation and invalid address/target/Origin rejection. It is not a physical
second-PC or peer-connectivity test.

`node tools/test-job-shutdown.mjs` holds a loopback HTTP request from an isolated
node, restarts its coordinator, and checks durable interruption, refusal to
resume uncertain non-retry-safe work, no replay, ignored late replies and healthy
subsequent work. It uses no personal configuration, external service or shell
command. Set `EEF_TEST_BINARY_DIR` to choose binaries (default `target/debug`).
The harness waits for the required registered capability, not just an authenticated
socket, before submitting new work. It records terminal failures without replaying
them; reconnection alone is not proof that the scheduler has a ready provider.
Published alpha.5 reproduces the shutdown-order failure; alpha.6 includes the
fix required for this regression to pass.
