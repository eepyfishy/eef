# Prepare microphone activity requests without JSON

This command is in development and is not included in published installers.
It prepares a bounded, metadata-only microphone activity request through the
already running node. It needs no language model or dashboard.

```powershell
eefn requests submit-microphone
eefn requests submit-microphone --source-node living-room --microphone mic --blocks 100
```

The first form uses this node and requires exactly one available configured
microphone resource. With more than one, inspect `eefn workloads sources` and use
`--microphone RESOURCE_ID`. A selector is a configured resource ID, not a device
path or private driver setting. The second form uses a source node disclosed to
the origin; address knowledge alone does not authorize its inspection.

The coordinator selects the resource through its existing credential-authenticated,
pinned TLS session. It saves the exact caller intent and the resolved resource and
adapter digests in the existing review/job journal, then compiles the plan. The
source node keeps its private device settings. Nothing is approved or executed by
this command, and it does not enable microphone permission or listening.

The default request observes 100 blocks of 1,600 frames at 16,000 Hz, mono:
approximately ten seconds of source audio, with only bounded activity metadata
reported. The maximum session budget is 60 seconds and each lease is at most ten
seconds; these are upper bounds, not permission or a promise of completion.
The shared camera/microphone defaults allow 100 events and 64 KiB of event bytes,
fitting a fresh node's conservative journal reservations and receipt byte limit.
Explicit larger `--max-events` or `--max-total-bytes` still needs compatible node
owner limits; authoring or approval does not raise those limits.
Parameters and limits remain configurable with `--sample-rate`, `--channels`,
`--block-frames`, `--blocks`, `--activation-level`, `--deactivation-level`,
`--max-session-ms`, `--max-lease-ms`, `--max-job-memory-mib`, `--min-interval-ms`,
`--max-events`, `--max-event-bytes`, `--max-total-bytes`, `--max-pending` and
`--priority`. Node policy can independently refuse these requested bounds.

Keep the operation ID and job ID returned by the command. Inspect the result with
`eefn requests plan JOB_ID`. The coordinator owner separately inspects and approves
the exact plan; execution then requires an explicit command and all current source,
permission, listening, policy, runtime and compatible-completion conditions.
Preparing a configured source can succeed while execution is unavailable.

After an uncertain reply, use `eefn jobs find --operation-id OPERATION_ID` before
deciding what to do. `--operation-id` can preserve an existing receipt, but reusing
it only inspects exact retained intent and bindings. It never chooses another
microphone, recompiles different input, renews approval, retries capture or resumes
old work. Changed input with the same operation ID is a conflict. A new request
requires a new explicit operation ID and separate review/approval.

The outcome follow-up distinguishes confirmed preflight refusal from an unconfirmed
reply. A refusal explains unavailable/disclosed source selection, missing or
ambiguous microphones, unapplied resource settings, conflicting input, or an
existing uncompiled review. It supplies a stable reason code and specific next
steps. Source availability and disclosure may be reported together: the response
does not reveal whether an undisclosed node exists.

Confirmed refusal means this attempt did not enter the review checkpoint. It does
not prove that earlier or concurrent requests do not exist, reserve an operation
ID, or prove that old effects stopped. A refusal without saved history is not a
durable job receipt. Any later explicit fresh preparation rechecks current source
settings; the backend never retries it automatically. Inspect the operation ID
before deciding what to do.

If review persistence or compilation has already been attempted, a failed reply
remains unconfirmed even when a saved review exists. Reusing exact typed input
with a saved uncompiled review returns its incomplete status without querying a
provider or continuing compilation. Further planning or cancellation must be
explicit. Completed compilation receipts still return their original choices.

To explicitly finish a saved concrete review after inspecting it, use its exact
review revision:

```text
eefn requests plan JOB_ID
eefn requests compile-saved JOB_ID --revision 1
```

`compile-saved` is a separate preparation command. The coordinator loads the
retained concrete input and preserves the original compilation receipt; no JSON
file, model inference or device selection is required. It rechecks origin ownership,
revision and current source bindings before saving. A stale revision, unavailable
source or changed binding refuses compilation. An already matching compiled receipt
returns its historical result without contacting the source or changing the job.
The original `submit-microphone --operation-id` receipt remains usable afterward.
Semantic reviews without exact concrete input still need their normal explicit
preparation. This command does not approve, execute, replay or enable permissions.
After an unconfirmed response, inspect the saved job before deciding what to do.
Keep the job ID and original review revision. Repeating the exact command reads
a matching compilation if it was already retained; the backend never retries it
automatically.
Older versions may refuse this new planning operation; no fallback is attempted.

The node API adapter is `POST /api/commands/requests/microphone`; its typed body is
`MicrophoneRequest`. CLI, API and future UI clients share the node service and
coordinator authoring backend. The dashboard controls no background lifecycle.
The new optional `workload_source` review field is omitted from historical records
without typed microphone intent, preserving their serialization and hashes. New
typed history must be read by a compatible version; it is never rewritten to make
an old reader accept it. Multiple typed source intents in one review are refused.

Shared source/receipt/CLI tests and actual credential/TLS origin tests are added;
the Windows combined source test now uses typed microphone submission before
separate owner approval and origin execution. Runtime/hosted verification remains
pending. The preceding admission failure was traced to its incomplete source
review capability set; that separate correction also awaits full verification.
This interface alone does not establish the fix. Physical audio, long sessions, two-PC acceptance,
continuous relays and the remaining stable release requirements remain open.

Windows run `42228933980` failed the new CLI's one-MiB parser guard, a disposable
venv setup deadline, and native registration. The parser follow-up keeps the same
flat command/flags with smaller boxed argument groups; it does not increase the
Windows stack requirement. The same guard now parses the microphone command too.
The venv timeout retains its 30-second failure boundary and adds numeric setup,
partial-file and exact-child cleanup observations only. It is not a proven timeout
fix. These changes need fresh full hosted acceptance; the earlier failed run stays
retained. No installed node or runtime policy is changed by these development tests.

Executable-level command acceptance is being added to the existing headless input
fixture. It uses a configured synthetic resource and separately paired credential/
TLS processes, with both dashboard pages disabled. It checks stopped-node handling,
preparation without an LM or microphone permission, exact receipt reuse, conflicting
input, missing/undisclosed sources, human-readable refusal and nonzero exit status.
A local reply proxy verifies that dropped or substituted replies remain uncertain
and send once; an older API returns not-sent without a fallback. Explicit history
inspection and receipt reuse must preserve the saved job, including after node
restart and interpreter withdrawal. Configuration and private device binding checks
remain separate from execution. These checks await hosted Windows verification;
they neither open a real microphone nor establish physical audio acceptance.
