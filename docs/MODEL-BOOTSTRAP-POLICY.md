# Model bootstrap preferences (unreleased after alpha.6)

These node-owned preferences now feed an opt-in [backend setup runner](MODEL-SETUP-RUNNER.md).
An approved project default and complete first-run automatic setup remain
unfinished. Earlier inactive auto/prompt preferences stay inactive on upgrade:
automation requires a new explicit opt-in and an exact owner catalog binding.
Manual installation and interpreter-assignment commands remain separate.

## Commands and interfaces

Run these through an already-running node; no port numbers or JSON editing:

    eefn model-bootstrap-policy show
    eefn model-bootstrap-policy configure --mode off
    eefn model-bootstrap-policy configure --mode managed
    eefn model-bootstrap-policy configure --mode prompt --model CATALOG_ID
    eefn model-bootstrap-policy configure --mode auto --project-default
    eefn model-bootstrap-policy configure --mode auto --model CATALOG_ID --enable-automation
    eefn model-bootstrap-policy configure --mode auto --model CATALOG_ID --enable-automation --restart-after-setup
    eefn model-bootstrap-policy configure --manual-apply
    eefn model-bootstrap-policy configure --disable-automation
    eef node model-bootstrap-policy --node NODE_ID show
    eef node model-bootstrap-policy --node NODE_ID configure --mode off

Add --json for structured reports. A catalog model and --project-default conflict;
omitting both preserves the existing binding. Omitting --mode preserves the
existing mode. Empty configuration commands fail before contacting the service.
Plain output includes retained setup state and pending restart, not readiness.
CLI and future UI clients share the node-owned service; neither drives the loop.

The desired modes are preserve, off, managed, prompt and auto. A missing saved
policy is preserve, not implicit download consent. This stage does not alter
fresh-installer defaults or upgrade existing installations into automatic setup.
Off records an opt-out; managed records external model management. Neither
disables core commands or prevents a separate owner-requested manual install.
Without --enable-automation, prompt/auto remain inactive preferences. With it,
prompt awaits an explicit owner change to auto; auto permits one setup attempt
per policy epoch for its exact catalog binding. Disabling requests cancellation
only of its own exact admitted transfer, never a different manual transfer.
The default remains manual application. The separate --restart-after-setup
opt-in permits a [scoped runtime restart](MODEL-ACTIVATION.md) after a newly
eligible setup; --manual-apply disables that preference. It does not retroactively
apply an already-completed manual setup. Unrelated pending runtime settings block
automatic activation. An admitted restart may finish after later policy edits;
those edits remain saved/pending, never overwritten by its private snapshot.

Local-owner APIs use POST /api/commands/model-bootstrap-policy. The EEF owner
adapter uses POST /api/commands/node/model-bootstrap-policy and existing
node.configure control framing. Remote inspection is read-only. Remote writes
require the current node owner's existing remote-management consent, checked
again under the configuration lock; they cannot enable that consent themselves.
Unsupported, lost or altered replies cause no client fallback, replay or restart;
an enabled backend attempt may already exist after a lost policy-save reply.
Inspect show and its installer receipt. An offline CLI never launches a runtime.

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
successful inference or semantic quality. Automatic-setup availability follows
the explicit opt-in. Default approval, license review and runtime verification
remain false. The policy reply's download/selection/inference/execution/restart/
retry flags describe the preference command, not completion or absence of
asynchronous backend effects. Retained setup and installer receipts describe
the separate attempt. No flag establishes quality or readiness.

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
replay it or interpret uncertainty as rollback. The atomic policy save leaves
model configuration, other preferences, permissions, registration and update
policy untouched and does not itself restart the node. An enabled backend may later
install/configure its pinned target and, with separate restart consent, activate
that new setup. Generic saves cannot enable or retarget an
automatic epoch, including stale enabled snapshot restoration; use the typed
current-runtime/version command.

## Backend runner and remaining automatic-setup work

The [runner contract](MODEL-SETUP-RUNNER.md) covers existing installation preflight,
exact receipts, progress/cancellation, disk checks, async admission fences,
preserved selections/opt-outs, Ollama preference, verified CPU cache selection
and durable no-replay lifecycle. Provider metadata is not a GGUF or Ollama
manifest checksum. Configuration saves wake its backend supervisor; no dashboard
timer owns it. The earlier
[unavailable-model control fix](UNAVAILABLE-MODEL-CONTROLS.md) separates retained
references from introduced artifacts so missing files need not block unrelated
owner saves; this is not general model-health acceptance.

The separately opted-in scoped activation path refuses unrelated pending runtime
settings and does not piggyback a selected application update. Its durable runtime
binding observation is not loading, inference or readiness evidence.
Offline/skip/managed behavior must leave deterministic node controls usable.

Before enabling normal fresh-install bootstrap, supply a replaceable versioned
default manifest with reviewed licensing, pinned source/bytes/digest and tested
runtime/CPU/resource/interpretation requirements. The existing failed real-model
quality runs do not authorize a default. Full onboarding, physical two-PC and
installer/signing/Defender/stable-release acceptance remain open.

## Policy-foundation verification (earlier checkpoint)

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
