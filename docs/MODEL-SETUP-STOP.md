# Attempt-specific model setup stop (unreleased)

The subsequent combined [reply-ownership checkpoint](REPLY-OWNERSHIP.md) passes
both full Rust feature modes (1,073 tests plus one ignored each) and all
twenty-eight native suites, including this feature. The earlier failures below
remain retained at their own checkpoint; they are not current acceptance.

The shared backend supports stopping one retained setup attempt without changing
the owner's saved auto/prompt preference, model binding or policy epoch:

```
eefn model-bootstrap-policy show
eefn model-bootstrap-policy stop --attempt ATTEMPT_UUID
eef node model-bootstrap-policy --node NODE_ID stop --attempt ATTEMPT_UUID
```

Use the exact attempt ID reported by `show`. Local commands target the running
node; EEF forwards the same typed command through the existing authenticated
channel. Both adapters inspect once and bind the mutation to the inspected node,
runtime and saved policy. There is no automatic retry, legacy fallback, model
inference, permission grant or application restart. Remote stop requires the
target node's current local remote-management approval, just like other setup
mutations. Read-only setup/show remains available without that write consent.

## Durable boundary

Only the current policy/runtime's approved, inspecting, downloading or verifying
attempt can be stopped. The backend writes `stopped / owner_stopped` before
requesting cancellation of that attempt's exact active transfer. Transfer
admission and configuration commit share configuration ownership with stop and
revalidate retained attempt identity and phase. A late provider response or
verification checkpoint cannot revive that authority or overwrite the stop with
failure/unknown. Failure cleanup uses that same ownership and does not retry an
owner stop's cancellation, even if its receipt journal becomes available later.
No selection or automatic application can proceed from the stopped attempt.

If commit ownership has already won, stop refuses rather than claiming rollback.
Configured, committing, restart, failed, unknown, historical and already-stopped
attempts require inspection; they are not silently changed. Unavailable/corrupt
evidence is not initialized or repaired. Repeated stop is refused without another
cancellation dispatch. The same policy epoch stays consumed across runtime
restart and no-op policy saves. A later deliberate policy epoch is a new owner
decision, not a retry or evidence that old external effects disappeared.

The command's optional `setup_stop` receipt separates transfer cancellation
`requested`, `not_requested` and `unknown`. Physical-stop verification and
configuration rollback remain false. A different manual transfer is never
targeted. Cancellation can fail after the durable stop; this does not restore
setup authority. A shared Ollama service may still finish its operation, and
downloaded cache/provider artifacts are not removed. This command does not stop
a model workload, unload a shared provider, cancel a queued activation or grant
automatic recovery. General download controls and reviewed workload stop retain
their own separate authority and evidence.

## Compatibility and validation

Older strict clients/nodes may reject the new stop operation, stop receipt or
owner-stop reason. That is an unsupported observation/command, never permission
to send a generic cancel, disable policy or retry via another route.

Both final workspace/all-target feature modes pass 1,067 tests plus one existing
ignored profiler each, including nine added regressions. Fresh headless apps and
helpers build against the same source. Combined native acceptance remains open.
Tests cover pre-inspection stop,
late metadata/admission, transfer completion, selection/restart fencing,
unrelated-transfer preservation, changed scopes, malformed/corrupt observations
and current remote consent. A cancellation fault test temporarily withdraws a
real receipt owner, retains an unknown cancellation outcome, then restores the
receipt owner and checks failure cleanup does not dispatch cancellation again.
An earlier full headless run passed 1,066 tests plus one ignored before that
fault regression. A later interim headless run completed before cleanup ordering
was corrected, but its validation guard refused changed source; it is not final
acceptance. Its log and old hashes remain under `.validation/setup-stop-*-before-cleanup*`.
The corrected public fixture `model-setup-08ucWg` passes local and pinned-TLS EEF
commands, policy/config preservation, consent denial, late-provider fencing and
repeated-stop refusal, using synthetic provider metadata rather than model weights.

The combined native run passes its first fifteen suites, then fails in the
unchanged model-metadata harness (`model-metadata-WtvTRG`). Its initial error
observation is incomplete; it is not counted as a passing run. A separate fresh
diagnostic (`model-metadata-rad5vQ`) captures `node_busy / not_started` for the
second sequential clear-interpreter mutation, with no acknowledgement or unknown
outcome claim. It is retained, not replayed in its original runtime.
The ordinary request path holds admission through encrypted response writing;
the same pattern exists at published `f4e4c66`. This is a capacity-ownership lead,
not a proven cause or evidence that the new checkpoint is regression-free.
Do not add delays/retries, widen capacity or weaken the fixture to obtain a pass.
All nine previously unrun suites then pass in a separate sequential check,
including process lifetime, local pipelines, permissions and credentials. There
are twenty-four passing suite results, not a passing twenty-five-suite run.
Neither metadata failure is erased. All 294 source/resource and nine native
artifact hashes remain unchanged; no matching fixture processes remain observed.
The read-only partial evidence audit records `combinedNativeAccepted: false` in
`.validation/setup-stop-partial-evidence.json`.

Next investigation: establish a deterministic request-admission/reply-flush
lifetime regression before choosing a correction. Preserve bounded execution
and bounded pending replies, framing retirement and no-replay; merely releasing
all ownership early could replace capacity refusals with an unbounded reply
queue. The current ownership pattern is a lead, not an accepted correction.

Evidence is `.validation/setup-stop-{headless,default}-final.log`,
`.validation/setup-stop-native-failed-metadata.log` and the private diagnostic
log. Source/native inventories remain separate from semantic model quality and
physical two-PC evidence. No source checkpoint has been published, and no
installed node, release artifact, version, default-model decision, update feed,
Defender setting or MSI tooling changed.
