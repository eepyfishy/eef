# Select an installed GGUF without a file path (unreleased)

The owner can select a node-installed text GGUF by catalog ID:

```powershell
eefn models installed --json
eefn models select-installed-gguf --model MODEL_ID --json
eefn models select-installed-gguf --model MODEL_ID --interpreter --json
```

No file path or port is required. Without `--interpreter`, this selects the model
without assigning/moving that role. With it, the existing atomic interpreter-role
operation moves only `request_interpreter` off other selections and onto this
one, retaining other roles and adapter restrictions. A role is not a permission
or execution grant. This command does **not** download or load weights, start a
second node, restart, delete files or approve a default model. The provider is
preserved unless `--interpreter --activate-backend` explicitly saves llama.cpp
as provider in that same write; see [atomic backend choice](INTERPRETER-SETUP.md#save-the-interpreter-and-its-backend-together).

The same target-node operation is available from eef with current node-owner
management approval:

```powershell
eef node models --node NODE_ID select-installed-gguf --model MODEL_ID --interpreter --json
```

eef does not interpret the ID as a path on its own device. Both roles need the
new development implementation; no legacy save/file-selection fallback is used.
The existing owner endpoint `POST /api/commands/models` takes:

```json
{
  "schema_version": 1,
  "expected_node_id": "NODE_ID",
  "command": {
    "operation": "select_installed_gguf",
    "model_id": "MODEL_ID",
    "interpreter": true,
    "request_id": "6e711dda-9c9c-4c30-8639-98ab28c513f6"
  }
}
```

Both CLIs generate a fresh canonical non-nil UUID `request_id` for each explicit
invocation. API clients should do the same. This correlates a rejection with the
current command; it is not a durable idempotency key, grant or automatic retry
instruction. Omitting it remains compatible with successful selection. A malformed
supplied ID is refused before preparation; missing correlation cannot establish
the confirmed preflight-failure contract below.

## Verification and save boundary

- Exactly one current catalog entry and installed-index entry must match the ID.
  This convenience operation supports declared text GGUFs only. Vision/projector
  and custom external files still use explicit `select-gguf` configuration.
- The node derives its content-addressed cache filename from the catalog digest,
  refuses symbolic-link escapes and mismatched index paths, and never follows an
  arbitrary index-provided path. Catalog/cache digest and size must agree. The
  complete file's size and SHA-256 are verified before a selection can be saved.
- Hash reads are asynchronous, with one active verification per node service and
  a 60-second hash-phase budget. Metadata preparation uses normal filesystem
  calls; this is not a hard real-time OS-I/O cancellation guarantee. Windows holds
  a read handle excluding writer/delete access through the save. Files remain
  owner-mutable after return; this is not runtime-load or remote-binary attestation.
- The short configuration save rechecks identity, current remote approval, model/
  catalog content snapshots and the selected installed record. A changed snapshot
  refuses the save; unrelated latest settings survive. This is content comparison,
  not a durable revision/ABA fence or a model-file lock after the command returns.
- A matching existing selection is reused rather than reserialized: tuning,
  restrictions, roles and extension fields survive. A different custom file or
  projector under the same ID is refused, not replaced. New internal ports use
  the existing automatic allocation; a selected port is not a held reservation.

Successful reports include `installed_artifact` with the ID, exact digest/bytes
and `verified_at_selection: true`. It records integrity observed by the node, not
license review, runtime compatibility, residency, model quality or execution
authority. Node/coordinator adapters validate the typed report; missing/forged
confirmation is not success. Output omits model paths and service URLs.

## Saved is not applied

Selection and role changes remain saved settings. Running models/interpreter
preferences are not replaced until an explicit restart; offline edits save for
the next app start. `auto` retains its preference for reachable Ollama. Selecting
a local GGUF does not override that choice; inspect the saved provider and decide
explicitly which backend to activate. The optional `--activate-backend` requires
`--interpreter` and saves that choice atomically, but does not itself restart or
load the model. Assigning a role to an inactive backend
does not prove interpreter readiness. `eefn setup` distinguishes saved/applied
interpreter choices. Restart applies **all** pending settings, not models alone.

A checksum failure leaves the file and settings intact. Fix/inspect the cache
before another explicit attempt. With current request correlation, a preparation
failure returns `installed_selection_preflight_rejected`, a static `cache_issue`
and `configuration_write_attempted: false`, `outcome_unknown: false`. Reasons
distinguish missing/ambiguous/invalid metadata, metadata/path/type/size/checksum
mismatches, unreadable cache, custom-selection conflicts, hash timeout and busy
verification. Raw filesystem/parser errors and private paths are not returned.
Unclassified preparation errors use static guidance, never text matching.

Only failures before entering the configuration-save path get that report. Both
adapters validate its exact node, echoed command/request ID, flags and static
fields. Stale, malformed or lost replies and later snapshot/save failures remain
uncertain: inspect `models show` before deciding what to do. A missing request ID
retains legacy conservative failure handling. There is no automatic retry,
persistent selection receipt ledger or exactly-once write claim. This is a
correlated report from the node, not independent attestation of its behavior.

## Verified scope

The explicit backend-choice follow-up is verified with 714 passing tests plus
one ignored in both feature modes, synthetic local/remote command regressions
and full-size cached CPU startup/unload. See
[current activation evidence](INTERPRETER-SETUP.md#acceptance-and-remaining-setup)
for the exact fixtures, measurements and remaining quality/release boundaries.

The preflight-rejection follow-up passes **710 workspace/all-target tests plus
one ignored in each feature mode**. `installed-selection-GJEMo2/results.json`
passes fresh executable local/offline selection, confirmed corruption/path
rejections, an old rejection substituted after a real successful save, lost-reply
inspection without replay and unsupported-command refusal. The stale rejection
is not accepted as proof of no write. `model-metadata-RC5rma/results.json` and its
`installed-selection-results.json` pass approved coordinator-mediated selection
and confirmed corrupt-cache rejection with unchanged configuration, alongside
the existing protocol regressions. These use synthetic cache bytes and mock
Ollama on this PC, not real inference or physical two-PC acceptance. Fresh
binaries/examples build; formatting, JavaScript syntax, source boundary and diff
checks pass. No release, installed application, physical node or update feed
was changed. The full-size real-artifact evidence below predates this follow-up;
it is not a new real-model or bootstrap quality result.

Focused tests cover corruption, duplicate/stale metadata, catalog/type/path
mismatch, preservation, explicit role assignment, one verification at a time,
changed model/catalog/index snapshots, current approval revocation and the Windows
read lease. The initial unit fixture lacked a required node name; correcting the
fixture also tightened negative tests to check their actual cache error causes.

`installed-selection-IE9MbV/results.json` passes actual node CLI/API and offline
edits using small synthetic cache bytes: no paths/ports, saved/applied separation,
preserved provider/permissions/tuning/extensions, corrupt/index refusal, lost-reply
inspection without replay and unsupported-command refusal without fallback.
Its early fixture failures used an uninitialized API port and assumed paused
Ollama models were network-advertised; the corrected check uses the existing
setup service's actual applied interpreter, without changing production behavior.

`model-metadata-5yFpK9/results.json` and `installed-selection-results.json` pass
coordinator-mediated selection on the target node, denial before approval,
verified metadata/explicit role checks, unchanged provider/runtime and existing
protocol regressions. These are isolated same-PC processes with mock Ollama and
synthetic cache files, not loaded GGUF weights or physical two-PC acceptance.
Final fresh-binary reruns also pass as `installed-selection-5YtTRv` and
`model-metadata-ZZrrAr` (including its installed-selection result).

`real-installed-selection-hHBj1p/results.json` additionally passes against the
existing verified 1,280,835,840-byte 2B development artifact. The offline owner
command fully verifies/selects it and explicitly assigns the role in **16,696 ms**,
using a unique new test config beside the controlled cache so no weight copy is
needed. The pre-existing installed index remains byte-identical. No node/model
runtime is launched, no inference occurs and no old configuration is edited.
This proves full-size cache selection on this PC, not model quality, activation,
resource quotas, another device's performance or automatic bootstrap approval.
Default bootstrap, real-model quality, complete onboarding, physical and release
acceptance remain unfinished.

Both locked/offline workspace/all-target suites pass **708 tests plus one ignored
each**, with default features and without dashboard features (eef 243, owner CLI
17, installer 6, node library 427, node CLI 15). Fresh binaries/examples build;
format, diff, JavaScript syntax and backend/browser dependency checks pass. No
model weights were downloaded/copied, no installed application was changed and
no release/update feed was modified. The node-name validation message now uses
"node" terminology; its bounds and policy are unchanged.
