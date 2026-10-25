# Bounded Python extension exchange (unreleased)

Python remains an opt-in extension runtime, not a second controller or workload
authority. Node transport, planning and grants remain Rust. Camera, microphone,
screen and other Python-backed operations retain their existing local permissions
and consumed grant/pipeline lifecycle. This does not enable peer media producers.

## Configuration and applied state

The running node exposes typed budget commands without JSON editing:

    eefn python show
    eefn python configure --memory-mib 256 --timeout-seconds 30
    eefn python configure --max-stdout-bytes 8388608
    eefn python configure --default-memory
    eef node python --node NODE_ID show
    eef node python --node NODE_ID configure --memory-mib 256

Only specified fields change. The explicit default-memory flag removes the
optional job-commit ceiling without resetting time or pipe budgets. Repeated
matching saves are no-ops. Saved and applied values are reported separately;
restart remains a separate owner action and applies all pending settings, not
only these budgets. Show does not probe Python, import a plugin or access a device.
Remote writes require current local management consent, rechecked under the
existing configuration lock; read-only status cannot be used as a write action.

The node service, local-owner API and eef adapter share the same typed command.
Exact runtime identity and a digest of normalized saved limits fence mutations.
An old digest is not authority to replay a save after settings changed. Runtime
retirement is held out of the synchronous save. Unsupported, dropped or altered
replies cause no retry, fallback generic save, restart, permission widening or
plugin execution. Inspect python show after uncertainty before deciding on a
new explicit change. This is configuration inspection, not a durable operation
ledger or proof of hardware health. Old alpha.6 installers lack these commands.

Local-owner POST /api/commands/python and coordinator-owner POST
/api/commands/node/python use these typed runtime/digest-fenced requests.
Configure carries the complete desired limits object, not a partial patch;
clients must preserve the other inspected limits, as the CLI does. Stored legacy
configuration still defaults missing budget fields compatibly.

Optional node configuration `python_bridge` defaults compatibly:

```json
{
  "python_bridge": {
    "timeout_seconds": 30,
    "max_request_bytes": 37748736,
    "max_stdout_bytes": 37748736,
    "max_stderr_bytes": 65536
  }
}
```

The shared node configuration service validates these settings and retains normal
remote approval. They cannot change the interpreter, install a plugin or enable
a permission. Limits apply on node runtime restart. Status exposes `saved`,
`saved_valid`, `applied`, `enabled` and `restart_required`; no applied interpreter
is shown when disabled/unavailable. Decoded but out-of-range limits disable Python
startup, not the node's Rust transport/commands. Malformed/wrong-typed files still
follow the normal configuration parsing rules. The shared save service rejects invalid
limits before changing configuration or making a proposal.

Allowed ranges: 1–300 seconds, 1 KiB–64 MiB each for request/stdout, and
1 KiB–1 MiB for stderr. Default 36-MiB request/stdout budgets accommodate the
existing adapters' maximum 25-MiB binary media after base64/JSON encoding; they
are ceilings, not reserved allocations. Inspection replies additionally cap at
64 KiB. Unknown policy fields are rejected. There is no automatic retry or
runtime fallback if a limit rejects an extension.

The optional max_job_memory_mib setting adds a Windows-owned Job aggregate commit
ceiling of 64–1,048,576 MiB, covering the interpreter and its associated child
processes. Omitted/null means no explicit ceiling and is omitted when serialized,
preserving the legacy default shape. The same native containment code already
used for owned model servers sets and verifies the ceiling before the new process
resumes; this also covers the interpreter availability check. An unsupported
platform or failed native assignment never falls back to an unlimited process.
On non-Windows nodes an explicit ceiling prevents Python startup, not the Rust
node core; the typed save command refuses unsupported non-null ceilings.

This is committed virtual memory for each owned bridge process tree, not measured
resident RAM, VRAM, CPU, device buffers, external services or a node-wide budget.
Concurrent bridge calls have separate trees/ceilings; their budgets do not form
an aggregate quota. Allocation failure may be handled by Python or fail a call;
it does not undo effects, prove a grant unconsumed or authorize replay. Defaults
remain unchanged. A Job Object is not a security sandbox.

The request/stdout/stderr settings remain **encoded pipe/data bounds**. Parent grants tighten stdout
to their result budget, and pipeline stages to the existing retained-data budget,
with at most 4 KiB extra for the host envelope, always within the local ceiling.
The existing final/stage result validator still applies. A large camera/WAV
result may therefore fail even when the global bridge ceiling is higher.

## Lifetime and failure semantics

The shared consumed-grant loop now drops its executing future before final
blocking journal I/O. Deadline/permission failure therefore requests Python's
existing owned-tree cleanup before attempting the final outcome save, even if
that save subsequently waits on a journal lock. This does not add an immediate
remotely acknowledged jobs-stop command or prove external effects stopped.
See [blocked-journal/native evidence and limits](PROCESS-LIFETIME.md#executor-retirement-before-final-grant-checkpoint-subsequent-increment).

Request JSON is serialized through a bounded writer before spawning the host;
oversized input cannot execute that invocation's import/handler. The original
deadline begins before serialization and is checked again before spawn. After
bounded serialization, plugin-file metadata is awaited under that deadline.
Dropping its waiter may leave a read-only OS worker finishing later; that worker
cannot launch the host. After
spawn it covers stdin writing/closing, concurrent stdout/stderr reads, root wait
and Windows owned-tree retirement. A full stdin pipe or inherited output pipe
cannot wait outside this deadline. Readers belong to the calling future, not
detached tasks. A limit failure rejects the full reply; truncated prefixes are
never parsed as success.
The host uses explicit UTF-8 mode and compact, non-ASCII-escaped JSON so Unicode
requests/results do not depend on the Windows locale or inflate their data
budget merely because the user's language is not ASCII.

Windows reuses the existing unnamed job: start suspended, contain before resume,
no uncontained/elevated/breakaway fallback. Root exit retires remaining owned
descendants and observes that job empty within the same deadline before success.
Timeout/error/cancellation drops owned containment and requests root termination;
error handling may spend at most one additional second attempting to reap the
root. Cancellation of the caller may preempt that cleanup. Other platforms retain
root-child-only cleanup, not a process-tree or sandbox guarantee.

An accepted workload failing here remains uncertain under its existing receipt
rules; it is not reclassified as unstarted or automatically replayed. Process
termination/pipe closure is not proof that device/network/filesystem effects were
undone, an external resource was released, or an application stopped physically.
Plugins intended to create lasting applications must use a separate approved
application-lifetime contract, not escape this finite bridge.

Diagnostics identify request/stdout/stderr limit, deadline, failed process,
invalid reply or invalid metadata without embedding raw stdout/stderr, capture
bytes, error/trace text or malformed JSON. This intentionally replaces raw plugin
error forwarding. Consumed reports retain a static `python_*` error code with
`Unknown`, never an invented unstarted/successful outcome. Inspect local dependency/plugin configuration when an extension
fails; do not infer success, missing hardware or malware from the generic error.

## Verification scope

The command-first ceiling checkpoint passes 855 workspace/all-target tests plus
one ignored each, with and without dashboard features. The public command
fixture uses native eef/eefn and a synthetic plugin, checking explicit owner
consent/revocation, unchanged imports/runtime during configuration, separate
restart application, exact saved-policy fences, older endpoint refusal and
single-send dropped/corrupted replies. A separate real-interpreter fixture
compares 192-MiB allocations with no ceiling versus a 128-MiB ceiling in both
the Python host and its descendant: uncapped succeeds; capped is refused.
This is not total-node memory, hardware or physical two-PC acceptance. Exact
checkpoint evidence and failed harness runs are retained in [status](STATUS.md).

Rust pipe regressions cover exact/overflow boundaries, strict limits, bounded
request serialization, diagnostic privacy, blocked stdin, simultaneous output
draining, deadline/EOF waits and stderr flooding. The separate
`python-bridge-fixture` example and `tools/test-python-bridge.mjs` exercise a
caller-supplied trusted interpreter with isolated synthetic plugins. They are
not installed commands or execution authority.

`tools/test-local-pipelines.mjs` additionally exercises real Python through
production reviewed/approved/consumed pipeline grants when `EEF_TEST_PYTHON`
explicitly names an existing interpreter. It does not download Python or import
real device libraries. Ordinary runs without that variable do not claim Python
acceptance. Synthetic pipeline success, noisy/stalled/oversized failure and fresh
subsequent work are distinct from real camera/microphone/playback, two-PC media,
aggregate resource quotas, strong physical stop and full release acceptance.

### Verified checkpoint: 2026-10-19

Both final locked/offline all-target feature modes pass **823 tests plus one
ignored each**, including seven new pipe/encoding/privacy tests and one policy
approval/saved-applied case. No-dashboard binaries/examples build. The existing
cached CPython 3.14.5 runs `python-bridge-nsRyTw` successfully: bounded and Unicode
round-trips, stdout/stderr overflow, invalid/error output redaction, oversized
request refusal, deadline, cancellation and owned descendants with/without
inherited output pipes. No matching fixture process remains.

`local-pipelines-tq2Urg`, explicitly supplied that interpreter, passes the normal
public request/planning/owner approval/consumption/result path. Synthetic capture
success is followed by stdout/stderr/deadline/stage-budget failures, exact static
causes, one uncertain attempt without replay, then successful new work. A separate
ordinary capture grant confirms its smaller result ceiling also bounds the pipe.
Existing public model-download, credential and durable-request regressions pass
in `model-downloads-fCfgrN`, `node-credentials-sv2zpt`, `request-drafts-a26Vua`.
Browser initialization/copy/status smoke and recursive backend/browser guard pass.

Final headless SHA-256:

```text
eef.exe  ff37fbe1c0b02ddd65feffb8a99f84db950e19d770b387a00898ee2560515878
eefn.exe 7cf4f43dfaa010ba9cdb21a0561e014265d0b6b5ebe6e03f81adab7d01ec948e
python-bridge-fixture.exe f8ed7666b5386f61a2ec883748ff382183a39ea1b5cb48e62569dae6edff523c
```

This is real interpreter/pipe/production-grant evidence on one PC with synthetic
plugins, not hardware, physical two-PC, peer-media, bootstrap/default-LM, installer
or release acceptance. No model, installed/remote node, update policy or release
changed. Native peer startup remains disabled pending producer/sink integration.
