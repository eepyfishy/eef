# Owned GGUF job-commit ceilings (unreleased)

```text
eefn --json models memory-limit --model MODEL_ID --mib 1024
eef --json node models --node NODE_ID memory-limit --model MODEL_ID --mib 1024
eefn --json models memory-limit --model MODEL_ID --default
```

This optional Windows-only ceiling applies to committed virtual memory in one
owned GGUF runtime's unnamed job, including normal child processes in that job.
It is **not total node RAM, working-set size, mapped-file cache, VRAM, CPU,
bandwidth or aggregate multi-model admission**. It neither controls the shared
Ollama service nor creates an application sandbox. Microsoft documents
[job memory limits](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information)
separately from [job lifetime and membership](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects).
Allocation denial is not a promise that the process is automatically killed.
External effects/services and processes outside the job are not covered.

## Owner commands and saved/applied state

An exact existing local-GGUF model ID is required; duplicate/missing selections
fail without a partial update. Bounds are 64-1048576 MiB (1 MiB = 1024 squared
bytes). Default means no **eef-configured** job-commit ceiling, not exemption
from other OS/parent-job limits. The wire must explicitly provide integer `mib`
or null for reset; omitting the field is rejected. Invalid values/types do not
change saved configuration. Reselecting a GGUF with no new ceiling preserves the
existing ceiling; only the explicit reset removes it.

Commands reuse current local-owner or node-approved remote management, exact
node identity and the existing config write lock. They do not add a durable
configuration receipt ledger, runtime/CAS fence or exactly-once mutation claim.
A lost/substituted reply is unconfirmed; inspect `models show` before deciding
what to do. No automatic retry, fallback, provider switch or restart is sent.

Saving/resetting does not load, infer or change the running job. Apply on an
explicit node restart/next start; saved selections and registered runtime
selections are reported separately. `job_memory_limit_mib` is a bounded optional
configuration field, not a memory-use estimate or remote attestation. Old missing
fields normalize to no ceiling and are omitted from serialization/advertisements.
Peer parsing rejects non-GGUF ceilings, invalid types and bounds; eef's inventory
preserves the node-reported field without treating it as physical usage evidence.

## Enforcement and failure behavior

The existing native lifetime guard creates an unnamed non-inheritable Windows
job, adds `JOB_OBJECT_LIMIT_JOB_MEMORY` to its kill-on-close policy and sets the
byte ceiling. The exact limits are read back before process creation. The child
starts suspended, joins that job and resumes only after successful containment.
There is no breakaway, elevation or uncontained/unlimited fallback. Failed setup
prevents model startup. Normal lifetime cancellation/owned-root cleanup remains.

On other platforms, an explicit ceiling command is refused before saving, and
loading an imported configuration with a requested ceiling is refused before
spawning. Default/root-child behavior is unchanged; no non-Windows job/tree or
memory enforcement guarantee is added. A too-small valid ceiling can make the
model fail allocation or startup. The node's controls remain available, the
model reports an error and is not replaced by another backend/model. The owner
can inspect, reset/save another ceiling and restart explicitly. Model weights
are never deleted or copied by these commands.

## Verification and remaining scope

Native Windows tests compare the same 256 MiB committed allocation in a normal
descendant: it succeeds with no eef ceiling and is denied under a 128 MiB job
ceiling. The configured job limit is read back. Invalid ceilings are refused
before the root starts. This is real local kernel enforcement, not model quality,
physical-network acceptance or a total-host/GPU quota.

The explicit `EEF_TEST_MODEL_MEMORY_LIMIT=1` option in `tools/test-real-model.mjs`
reuses a verified artifact. `real-model-plQoua` passes with the optional text
candidate: a 64 MiB ceiling produces startup error and no surviving owned model
process; unapproved remote changes are refused; reset changes only saved state;
an explicit restart restores real CPU inference and the existing selected
load/unload, idle/on-demand and no-replay checks. No workload starts during the
failed load, no unlimited fallback is used and the artifact remains intact.
This is one-PC acceptance, not proof for every model/platform or memory release.

Full aggregate memory/VRAM/CPU admission, measured startup cost, dynamic quota
changes, broader per-model policy and physical/release acceptance remain open.
No installed node, release artifact or automatic model default is changed.

Final-source default-feature and dashboard-free workspace/all-target suites each
pass **655 tests** (eef 237 plus one ignored profiler, owner CLI 16, installer 6,
node library 383, node CLI 13). Fresh default binaries/examples, formatting/diff,
JS syntax and backend/browser source guard pass. `model-metadata-pNJ0V2` passes
approved/refused remote ceilings, exact target inspection, lost/substituted reply
refusal without replay, old-node refusal and explicit reset without restart.
The public native process-lifetime regression `process-lifetime-9KKnsl` passes.
Final-binary `real-model-z3cqpf` passes the same real ceiling/error/reset/restart
and CPU lifecycle acceptance, with no downloaded/copied weights or retained test
processes. These results do not complete broader quotas or stable release gates.
