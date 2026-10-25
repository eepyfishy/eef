# Backend-owned model setup (unreleased after alpha.6)

This runner installs and configures an exact, explicitly owner-selected catalog
model without a dashboard. It does not supply an approved project default or
complete fresh-install onboarding or model-quality
acceptance. Existing inactive auto/prompt preferences stay inactive on upgrade.

## Commands and consent

    eefn model-bootstrap-policy configure --mode auto --model CATALOG_ID --enable-automation
    eefn model-bootstrap-policy show
    eefn models download-status --id ATTEMPT_ID
    eefn model-bootstrap-policy configure --disable-automation
    eef node model-bootstrap-policy --node NODE_ID configure --mode auto --model CATALOG_ID --enable-automation

The remote command still requires current local remote-management consent. Its
typed runtime/policy compare-and-swap is not bypassed by a generic settings save:
generic saves cannot enable/retarget an automatic policy or restore a stale
enabled snapshot after opt-out. Omitted policy and existing opt-outs are retained.
Generic owner saves may still disable automation and make unrelated changes.

Preserve/off/managed never start setup. Prompt with explicit automation enabled
waits for [one explicit approval](PROMPT-MODEL-APPROVAL.md) without changing mode.
Auto requires both the new opt-in and an unchanged
catalog binding. Project-default without an approved manifest remains blocked;
it never chooses the first catalog entry. Any existing saved or applied selection
is preserved rather than replaced or assigned a role automatically.

Policy commands wake a runtime-owned supervisor through the node service. Neither
HTTP/dashboard polling nor an open CLI session drives the operation. The policy
reply's download/selection/restart flags describe that preference write, not a
claim that an asynchronous operation has or has not completed. Inspect setup
status and the exact installer receipt instead.

## Provider and installer behavior

Auto provider preference reuses the existing installer decision: a reachable
configured Ollama service is preferred; explicit provider choices remain binding.
The exact catalog-to-Ollama mapping is retained. Already installed text-capable
metadata avoids another pull. Otherwise the runner uses one identified installer
request and its existing durable receipt, live byte progress and cancellation.
It does not introduce a second transfer implementation or copy model weights.
An inspection/admission cannot silently switch providers after its initial plan.

Ollama's reported text capability is not a pinned model manifest, GGUF checksum,
free-space proof, CPU compatibility, successful inference or semantic quality.
The catalog pin's GGUF size/hash never stands in for Ollama storage/artifact
identity. Provider-managed disk/transfer uncertainty remains explicit.

GGUF fallback uses existing before/during-transfer disk checks and exact size/hash
verification, including the existing-cache path. It then reuses installed-cache
selection verification, retains its read handle through the short commit, and
rechecks the current installed index/catalog. The new CPU slot requests zero GPU
layers. Bad cached bytes are not deleted, overwritten or repaired automatically.
These checks still do not prove a GGUF can load or interpret requests correctly.

## Persistent lifecycle and uncertainty

One bounded latest-attempt record is stored beside the node configuration in
CONFIG_BASENAME.bootstrap.sqlite3. The database has a supported schema, one row,
a 16 KiB record limit and 256 KiB file limit. Its full current record is validated
before writes; transactions fence lifecycle transitions and immutable identity.
Terminal records cannot be replayed or overwritten within the same policy epoch.
Older setup attempts can be replaced only by an explicit newer policy version;
the separate existing installer journal retains its own bounded receipts.

The runner saves inspecting intent before asynchronous provider inspection.
It rechecks runtime, policy version, exact catalog pin, absence of selections and
model-settings identity after inspection, at installer admission and immediately
before selection. Selection applies only the verified/provider-reported target
and its interpreter role, merging unrelated current owner settings under the
configuration lock. A committing checkpoint precedes the config write. Lost or
failed checkpoints/config writes remain unknown, not success or rollback.
If even the uncertainty checkpoint cannot be saved, status can show only the
last durable phase/time. That retained checkpoint is not proof a worker is still
running; inspect the exact installer receipt and current saved models too.

Interrupted nonterminal attempts become unknown on the next runtime, without
transfer or selection replay. Runtime shutdown/opt-out requests cancellation only
for that attempt's exact transfer ID; it cannot cancel a different manual transfer.
Cancellation and process death do not prove a shared Ollama server stopped or
that prior effects disappeared. Corrupt/future/unavailable setup evidence blocks
automation without initialization, repair, deletion or disabling core settings.
Read-only status does not create the sidecar. Core deterministic commands remain
usable without a model or a running dashboard.

After failure/unknown, inspect the retained attempt, installer receipt and current
models first. A deliberate disable followed by enable creates a new policy epoch;
it is a new owner decision, not evidence the previous effects stopped. Matching
no-op policy saves, ordinary process restarts and environment recovery do not
retry an uncertain or failed epoch.

## Configuration is not activation

Manual application is still the default. A separate opt-in now enables
[scoped automatic activation](MODEL-ACTIVATION.md), which refuses unrelated pending
settings and uses a captured startup configuration. Older setup opt-ins do not
authorize restarting. The statements below describe the default manual mode.

Configured means model/interpreter settings were saved. No model inference,
loading, application restart, unrelated settings application or workload
permission is requested by the runner. Pending restart is reported from the
node's existing live pending-settings hint; it is not readiness proof.
The setup report's readiness_verified stays false: it does not say a model is
unavailable, only that this setup flow has not verified it. Applied command-line
selections are protected even before asynchronous registration fills inventory.
Use the ordinary explicit node restart command when ready, then inspect model
lifecycle. Optional automatic application uses the separate scoped path above;
it is not a readiness or real-model quality claim.

    eefn restart
    eefn model-lifecycle show

## Verification (runner checkpoint, before scoped activation and prompt approval)

Thirteen added Rust regressions cover existing/missing Ollama metadata and one
installer receipt, exact async/admission/revocation fences, cached CPU selection
and bad-byte preservation, legacy/prompt/off/managed/default blocking, preserved
applied CLI choices, unsupported/corrupt/oversized evidence, terminal replay
refusal, policy opt-in/old digest compatibility and an 8 KiB supervisor-frame
ceiling. These fixtures do not contain real inference weights.

Both final locked/offline workspace/all-target modes pass 917 tests plus one
ignored profiler each: eef 268, owner CLI 21, installer 6, eefn 597 and node CLI
25. Logs are retained under .validation/setup-runner-workspace-default-final.log
and .validation/setup-runner-workspace-headless-final.log. Final headless
applications/examples build.

Public model-setup-nCcErg passes actual local and pinned-TLS remote commands,
remote consent, generic/stale activation refusal, live transfer progress, exact
revocation, abrupt termination of its own fixture node and retained unknown
without replay, existing Ollama reuse, verified synthetic CPU cache selection,
applied command-line selection protection, no automatic restart and reviewed
core work. It uses a synthetic provider and tiny cache, not physical two-PC or
real-model inference/quality acceptance. No actual model weights are downloaded.

Existing model-bootstrap-Uf7p70, unavailable-model-EE6q2s,
model-metadata-cZKHDL, installed-selection-79xaib, process-lifetime-n933mr,
local-pipelines-rAwfVN, python-policy-jFIcsw, python-bridge-X24ibr,
permission-commands-WkCPVC, node-commands-tW2mbH, node-credentials-xPsZiY and
model-downloads-TOLXRx also pass against this evaluated native build. The four
native artifact hashes are retained in .validation/setup-runner-final-native-hashes.json.
All four hashes match after acceptance. Source boundary, formatting, JavaScript
syntax and diff checks pass; evaluated fixture applications/helpers and llama-server
are terminal. No installed/remote node, visual UI, model default, version,
installer or automatic-update policy changed.

Approved default manifests/licensing/quality, complete first-run
dependency setup, fresh physical two-PC and installer/signing/Defender/stable
release gates remain open.
