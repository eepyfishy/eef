# Model bootstrap preferences (unreleased after alpha.6)

This is the policy/control foundation for automatic setup, **not an implemented
bootstrap runner or an approved default model**. Saving auto or prompt does not
install, select, load, infer with or restart anything in this build. Normal
first-run automatic setup remains unfinished. Existing manual installation and
interpreter-assignment commands continue to work independently.

## Commands and interfaces

Run these through an already-running node; no port numbers or JSON editing:

    eefn model-bootstrap-policy show
    eefn model-bootstrap-policy configure --mode off
    eefn model-bootstrap-policy configure --mode managed
    eefn model-bootstrap-policy configure --mode prompt --model CATALOG_ID
    eefn model-bootstrap-policy configure --mode auto --project-default
    eef node model-bootstrap-policy --node NODE_ID show
    eef node model-bootstrap-policy --node NODE_ID configure --mode off

Add --json for structured reports. A catalog model and --project-default conflict;
omitting both preserves the existing binding. Omitting --mode preserves the
existing mode. Empty configuration commands fail before contacting the service.
Plain output explains that automatic setup is unavailable. CLI and future UI
clients share the node-owned service; neither drives an automation loop.

The desired modes are preserve, off, managed, prompt and auto. A missing saved
policy is preserve, not implicit download consent. This stage does not alter
fresh-installer defaults or upgrade existing installations into automatic setup.
Off records an opt-out; managed records external model management. Neither
disables core commands or prevents a separate owner-requested manual install.
Prompt/auto record future workflow preferences only; they are not active setup
policies yet. No command cancels an existing manual download or applies unrelated
pending settings.

Local-owner APIs use POST /api/commands/model-bootstrap-policy. The EEF owner
adapter uses POST /api/commands/node/model-bootstrap-policy and existing
node.configure control framing. Remote inspection is read-only. Remote writes
require the current node owner's existing remote-management consent, checked
again under the configuration lock; they cannot enable that consent themselves.
Unsupported, lost or altered replies cause no legacy configuration fallback,
retry, download or restart. An offline CLI command never launches a node runtime.

## Owner catalog binding, not default endorsement

Selecting a catalog ID pins one unique, bounded text-GGUF entry from this node's
configured catalog. The node resolves it under its configuration lock. The pin
contains schema/version, model ID, exact full-definition digest, artifact
SHA-256/size, declared license and source/source-page URI digests. It reuses the
installer's HTTPS, artifact-size and hash validation. Reports do not disclose
source URLs, credentials or file paths. The full source remains in the configured
catalog rather than being copied into another catalog/store.
URI digests use the existing canonical-definition JSON-string hashing; the
artifact SHA-256 remains the checksum of actual model-file bytes.

Changed, missing, duplicate or invalid catalog definitions cannot silently rebind
the saved target. Inspection reports changed_or_unavailable. A new explicit
--model command can re-pin the owner's current definition. Even a harmless
description change changes the full-definition digest; re-pinning is deliberate.

The pin is **not** a complete trusted bootstrap manifest: declared license is not
reviewed licensing, and valid metadata is not CPU/runtime compatibility or
interpreter-quality approval. It describes the GGUF artifact; its size/hash must
never stand in for an Ollama manifest or provider-storage check. No bundled
project default is approved. --project-default clears the owner's catalog pin;
it does not select the first catalog entry, choose a model or authorize execution.

Saved selections and valid text-interpreter roles are observed separately. An
existing custom setup is reported, not replaced or automatically assigned a new
role. Interpreter_selected describes saved metadata, not applied availability,
successful inference or semantic quality. Automatic-setup/default-approval,
license-review, runtime-verification, download/selection/inference/execution,
restart and retry flags remain explicitly false.

## Atomicity, compatibility and uncertainty

Every typed save carries the exact current node runtime and previously inspected
policy, revision and digest. The digest binds both content and revision. A stale
view, concurrent writer or A-to-B-to-A rollback cannot revive an old compare-and-
swap. The node's central configuration writer owns revisions for generic saves
and restore paths too; callers cannot reset or forge the counter. Counter
exhaustion fails without wrapping. Matching no-ops do not rewrite the file.

Older clients omitting the policy preserve it, including opt-outs. An unsupported
or corrupt existing optional policy is preserved opaquely during unrelated owner
configuration/permission changes, with its original version/counter untouched.
It cannot disable core owner controls or be repaired/reset by an incidental
write. Bootstrap inspection/edits remain unconfirmed until a supported recovery
or migration is available. New unsupported policy values are rejected against a
healthy saved policy. This does not repair other pre-existing configuration
errors or promise every arbitrary malformed node configuration can run.

Configuration commands are not a permanent execution/idempotency ledger. A lost
save reply can leave a changed policy. Inspect show before deciding on a new
explicit command; the adapters retain the exact pre-send request and do not
replay it or interpret uncertainty as rollback. Policy saves leave model
configuration, other preferences, permissions, registration and update policy
untouched, and do not request an application restart.

## Remaining automatic-setup work

The backend runner must reuse existing installation preflight, exact download
receipts, bounded live progress/cancellation and before/during-transfer disk
checks. Admission must recheck the current bootstrap version/binding and model
setup after asynchronous provider inspection, not merely trust a past policy
read. Preserve existing opt-outs/custom roles, prefer a suitable existing Ollama
setup without a second provider, and keep GGUF/llama.cpp as the CPU fallback.
Do not confuse provider-reported identity with a verified local artifact.
Policy changes should wake a backend-owned supervisor that re-reads durable
state; a dashboard timer must not own the loop. Keep core owner controls usable
when model files disappear, separating validation of unchanged unavailable models
from validation of newly selected paths.

Retain durable setup lifecycle/unknown outcomes, refuse replay after uncertain
admission, and bind post-install selection/role changes to unchanged model setup
and verified installed bytes. Automatic application must not restart into
unrelated pending settings without the owner's chosen apply/restart policy.
Offline/skip/managed behavior must leave deterministic node controls usable.

Before enabling normal fresh-install bootstrap, supply a replaceable versioned
default manifest with reviewed licensing, pinned source/bytes/digest and tested
runtime/CPU/resource/interpretation requirements. The existing failed real-model
quality runs do not authorize a default. Full onboarding, physical two-PC and
installer/signing/Defender/stable-release acceptance remain open.

## Verification

Both locked/offline workspace/all-target feature modes pass 901 tests plus one
ignored profiler each. Fifteen new regressions cover node policy/catalog/version
and opaque-state behavior, remote request validation and both CLI adapters.
Public model-bootstrap-uwjwWa checks real local and pinned-TLS coordinator
commands, owner consent/revocation, no-op byte preservation, source redaction,
stale save/runtime and strict body/origin checks, catalog drift/re-pinning, lost or
altered replies, older endpoint refusal, restart persistence without download,
offline refusal without runtime startup, and core permission changes while an
unsupported bootstrap policy remains opaque. It uses synthetic metadata, not
actual models or physical two-PC acceptance.

Existing process-lifetime-8W31Pd, local-pipelines-neMSj0,
python-policy-r20X6c, python-bridge-7qg8Dt,
permission-commands-tfoUon, node-commands-wc6YHi,
node-credentials-qvYLuV and model-downloads-IbRhZ2 suites also pass. Final
headless apps/examples build; EEF, EEFN and both native fixture helpers match
the evaluated hashes. Source boundary, formatting, JS syntax and diff checks
pass; fixture apps/helpers are terminal. No installed/remote node, visual UI,
model selection/default, version, installer or update policy changed.
