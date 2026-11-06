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
pending. The preceding admission failure is under investigation and is not claimed
fixed by this interface. Physical audio, long sessions, two-PC acceptance,
continuous relays and the remaining stable release requirements remain open.
