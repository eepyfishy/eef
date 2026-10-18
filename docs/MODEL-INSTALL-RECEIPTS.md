# Durable model-install receipts (unreleased)

Installation admission and outcomes now survive full node-app replacement.
Commands still use the running node's existing model-transfer service, not a
dashboard controller, a new workload executor or automatic bootstrap.

```text
eefn models install --model CATALOG_OR_OLLAMA_ID --json
eefn models download-status --id OPERATION_UUID --json
eefn models cancel-download --id OPERATION_UUID --json
eefn models install-plan --model CATALOG_OR_OLLAMA_ID --json
```

Keep the returned operation ID after an uncertain reply. An accepted install
is not a completed download; `downloading` can mean admitted before transfer
starts. Historical `installed` means that operation finished, not that the
model remains installed, selected, resident or semantically suitable today.
Use installed-model inspection separately.

## Persistence and lifetime

The existing admission boundary rechecks owner authority and the exact current
model/catalog settings. It commits a bounded receipt before starting the
provider request. Cancellation records its request before changing the live
cancel flag; final outcome is saved before publishing terminal live state.
A failed final checkpoint reports live `unknown`, not installed success. No
receipt is evicted to admit another operation, and every retained ID rejects
readmission, even after later transfers, configuration changes or app restart.

Ordinary node-runtime restart keeps the same service and active transfer. Full
replacement takes an exclusive receipt-store lease after the old service releases
it, validates the complete retained history, and marks unfinished admissions
`interrupted`. This is not proof an HTTP request was sent or an external provider
stopped. Shared Ollama effects/layers may outlive the old app. No install command
or download is replayed, and no selection, inference, permission change or node
restart follows from recovery.

Receipts contain schema version, monotonic admission sequence, operation/requested-model/
backend identity, resolved-definition digest, acceptance/update timestamps,
known GGUF artifact size and cancellation-request flag. The definition digest
binds resolved catalog metadata and model settings; it is not the model file's
SHA-256 or proof of pinned Ollama tag contents. Raw URLs, paths, provider errors,
credentials, source descriptions, model output and byte progress are not stored.
Payload checksums detect accidental corruption, not a malicious local owner who
can rewrite both record and checksum.

Status reports `retention:"durable_receipts"` and `progress_retention:"live_only"`.
Current transfers keep their existing live progress. Historical/restored status
includes `durable_receipt`, `progress_available:false`, `completed:null`,
`automatically_retried:false` and `termination_confirmed:false`. Missing progress
is not presented as zero received bytes or a restored percentage. Receipt reads
do not write history. The latest receipt uses sequence, not wall-clock ordering.

## Storage and configuration

The version-1 SQLite store is beside the node config, with suffix
`.model-downloads.sqlite3`, plus a small exclusive-lease file. It is created only
at first installation admission; inspection of a node without a journal creates
neither file. Existing stores are validated/recovered at service initialization.
The general live-status mutex is not held through receipt fsync; byte-progress
updates do not perform journal writes. This is bounded metadata storage, not a
physical latency or process-memory guarantee under load.

Optional owner settings under `models.download_receipts` are `max_records`
(default 256, supported 1–4096) and `max_bytes` (default 1048576, supported
65536–67108864). Existing owner configuration services can supply them; normal
setup needs no override. Valid changes affect a new explicit install admission,
not an existing transfer or automatic restart. A lower limit never deletes old
history; it may block new admission. A higher limit permits a later explicit
request, without replay or a process restart. Install preflight reports this
separate `receipt_storage` scope and known quota/availability blockers; it does
not reserve space or report the node's free space as Ollama storage.

Unsupported/corrupt/misowned journals, an unavailable lease, malformed limits,
failed recovery or exhausted capacity block installation, not normal node
registration/control. The existing journal is not reset or repaired. After a
corrected policy/storage condition, an explicit install intent may re-open and
fully validate a disabled store; existing IDs still cannot replay. Status itself
does not attempt that mutation. Retain evidence instead of deleting history to
bypass an error or quota. Missing evidence never proves a request was not sent.

## Limits

No automatic partial-file reclamation, crash byte-range resume, durable progress
timeline, default artifact approval, automatic role binding, schema downgrade,
physical second-PC acceptance or stable-release acceptance is claimed. The
existing bounded in-process transfer recovery and full GGUF verification remain
unchanged. Completed model files are not deleted. Crash-orphaned partials remain
untrusted and are neither automatically resumed nor reclaimed.

The local command endpoint keeps request schema 1 and its existing operations.
Responses add receipt metadata and a new retention description. Older published
nodes retain their prior in-memory behavior; there is no legacy install fallback
or automatic upgrade. Do not downgrade/overlap runtimes expecting old binaries
to enforce the new receipt history.

## Verification (2026-10-18)

The nine new journal cases cover recovered interruption without fabricated byte
progress, live-owner exclusion, terminal history and sequence ordering despite
clock ties, retained cancellation requests, checksum/payload/size/version/owner/
ordinal corruption, failed admission/final/recovery writes, non-evicting quota,
explicit capacity increases, read-only absence/preflight, and later corruption
blocking new work. Existing cancellation/authority tests now admit actual bounded
receipts instead of injecting incomplete live-only records.

An additional service-level fault case reuses a verified synthetic cache file,
then rejects the final installed-receipt write with an isolated SQLite trigger.
The actual install worker reports live `unknown`; retained state remains only
admitted. A subsequent service owner preserves it as interrupted, without inferring
success from the intact file/index or deleting that file. Configuration/selections
remain unchanged. This is not inference or a real model download.

Fresh headless `model-downloads-QpAF1W` passes the command/API chain with mock
Ollama streams, not real weights or a physical second PC. Runtime restart retains
one active request; full replacement preserves an interrupted receipt and old
terminal history, with null byte progress and no new provider pull. Explicit old
ID reuse is refused. Historical inspection and terminal cancellation leave DB
bytes unchanged. Invalid receipt policy leaves status/config/model controls
usable and issues no pull; a corrected policy permits explicit full revalidation
while still refusing the old ID. Original exact cancellation, lost reply,
preflight, backend storage, no selection change and no legacy fallback cases pass.
No matching fixture EEFN process remains after cleanup.

The existing browser regression passes against real dashboard source and mocked
services, including copy/selection stability. Its additional cases verify missing
GGUF progress is not displayed as zero, historical completion is not presented as
current installed inventory, unknown/unavailable outcomes are not labelled
downloading, and actual live counts retain determinate/indeterminate progress.
This is a status-truthfulness fix only, not resumed UI development or a new
controller. No installed node, release, model download/default or update feed
changed; automatic bootstrap and full stable acceptance remain open.

Both final locked/offline workspace/all-target modes pass **768 tests plus one
ignored each**: EEF 247 plus one ignored opt-in profiler, owner CLI 17, installer
6, EEFN 480, node CLI 18. Fresh headless binaries/examples build; formatting,
diff, JavaScript syntax and the backend/browser source guard pass. Final-binary
`model-downloads-AtDkU5` repeats the complete public install/replacement/recovery
chain successfully. The earlier `QpAF1W` pass preceded the additional service
fault case and final binary refresh; it is not substituted for final-artifact
validation.

Final-binary `local-pipelines-LmJsgu` also passes finite/native execution,
durable pipeline progress, permission loss, recurrence and restart/no-replay
regressions. Both public fixture chains finished with no matching fixture
EEF/EEFN/native-helper processes remaining.

Evaluated final headless binary SHA-256:

```text
eef.exe  675bdf602ab701e29f0574749a01a09a68138e88441fcfbd899dbbc9d94cae6d
eefn.exe 2660d381da9ec58a8d1d9dc29349e7031bd845ae1978967689f4c9a4ddb97fcd
```
