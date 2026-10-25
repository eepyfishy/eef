# Explicit interpreter assignment (unreleased after alpha.6)

The node core now provides one atomic owner operation to assign the
`request_interpreter` role to an existing selected model. This reduces setup
steps without choosing a default model, changing permissions or creating a
dashboard controller. CLI, local API and approved coordinator commands share
the same service and configuration lock.

```powershell
eefn models installed --json
eefn models show --json
eefn models interpreter --backend ollama --model ACTUAL_SELECTED_MODEL_ID --json
eefn restart --wait-seconds 60 --json
eefn models show --json
```

Use the actual backend and selected model ID. Installed inventory and saved
selections are different: select the model first with existing selection
commands if necessary. For an existing selected GGUF use `--backend llamacpp`.
Without `--activate-backend`, provider selection remains independent: `auto`
prefers available Ollama and
does not activate a llama.cpp selection merely because it has this role.
Choose a provider explicitly with `models provider` if needed, then restart.

## Save the interpreter and its backend together

An explicit `--activate-backend` also saves the specified backend as provider in
the **same validated configuration write** as the role move:

```powershell
eefn models interpreter --backend ollama --model ACTUAL_SELECTED_MODEL_ID --activate-backend --json
eefn models interpreter --backend llamacpp --model ACTUAL_SELECTED_MODEL_ID --activate-backend --json
```

For a node-installed text GGUF, selection, role assignment and provider choice
can share that one save, without file paths or port numbers:

```powershell
eefn models select-installed-gguf --model ACTUAL_INSTALLED_MODEL_ID --interpreter --activate-backend --json
eefn restart --wait-seconds 60 --json
eefn setup --json
```

The flag means **save this backend for the next app start/restart**, not load it
now or prove interpreter quality. Omitting it still preserves the saved provider;
`auto` still prefers reachable Ollama. Activation selects one backend, not
simultaneous operation of both: other-backend selections remain saved but will
not be activated by that provider. A restart applies all pending app settings,
not only the interpreter. Inspect pending settings before restarting.

The same flag works on approved `eef node models --node NODE_ID ...` commands.
It cannot grant remote management, widen model capabilities/permissions, download
weights, infer, restart or override an invalid target. Invalid role/metadata or
cache checks leave both provider and role unchanged. Matching repeated saves are
no-ops, not requests to restart or replay work. A changed provider does not select
an automatic/default artifact or approve a candidate's semantic quality.

For either `interpreter` or `select_installed_gguf`, the wire field is optional
`activate_backend: true`. Omitted/false retains the old wire shape and behavior;
installed selection requires `interpreter: true` when activating. Adapters check
the saved provider as well as the role before confirming activation. Older nodes
may reject the new field; there is no fallback sequence of provider/role writes.
Lost or substituted replies remain uncertain; inspect `models show` before a
new explicit attempt. Correlation does not deduplicate configuration writes.

## Scope and atomicity

- Move only `request_interpreter` to the exact selection, removing that role
  from every other saved selection across both backends in one validated save.
  Backend is part of identity; the same model ID on two backends is not the same
  selection. Other roles, capabilities, files, ports and configuration survive.
- Require the target's existing metadata to support text input/output and
  `llm.infer`. Never widen disabled capabilities, infer capability from a name,
  load a backend or test model quality as part of assignment.
- Reject absent targets, malformed/duplicate selections and a role-list overflow
  without modifying even the staged configuration. Concurrent commands merge
  against current configuration under the existing lock; no lost unrelated edits.
- `eefn models clear-interpreter --json` removes only this role, preserving
  selections and other roles. It creates no missing backend configuration.
  Repeated assignment/clear is a no-op and does not write the configuration again.

Saved roles do not imply applied readiness, installed availability, safe model
output, a running connection, owner request approval or workload execution.
Explicit restart applies all pending settings, not just this role. An unavailable
model leaves deterministic core commands usable. Manual `hints` remains available
and can introduce ambiguity; interpretation still refuses multiple candidates
instead of silently choosing one.

## Adapters and remote authority

```powershell
eef node models --node ACTUAL_NODE_ID interpreter --backend BACKEND --model ACTUAL_SELECTED_MODEL_ID --json
eef node models --node ACTUAL_NODE_ID clear-interpreter --json
```

Remote changes require the node owner's existing local management approval.
eef inspects support/approval; the node rechecks current approval under its
configuration lock at application time. These commands cannot enable approval.
Use a separate approved node restart to apply changes. Offline local commands
hold the existing instance lock and save only for the next node start.

Local-owner-protected `POST /api/commands/models` retains schema version 1:

```json
{
  "schema_version": 1,
  "expected_node_id": "ACTUAL_NODE_ID",
  "command": {
    "operation": "interpreter",
    "backend": "ollama",
    "model_id": "ACTUAL_SELECTED_MODEL_ID"
  }
}
```

Clear uses `command: {"operation":"clear_interpreter"}`. Strict commands reject
caller provider/permission additions. Responses retain `saved_selections` and
`registered_models` separately and omit private file paths/service URLs/secrets.
Node CLI and coordinator adapters validate exact saved assignment/clear replies,
including metadata bounds, identity and uniqueness, before confirming a change.
They do not claim the registered model already changed.

Older builds do not implement these commands. Unsupported, lost or substituted
replies cause no legacy save fallback, client-side sequence of hint writes or
automatic retry. Configuration operations are not a permanent receipt ledger;
inspect `models show` after uncertainty before deciding whether to send a new
owner command. After an unconfirmed local HTTP reply, the CLI emits
`local_model_role_reply_unconfirmed`, `outcome_unknown: true`, and the complete
role command. A 404 returns `unsupported_command` without claiming uncertainty
or trying another path. These flags do not prove rollback or application.
Coordinator role reports also preserve the requested role command even when
permission, support or saved-state confirmation fails.

## Acceptance and remaining setup

The explicit backend-choice follow-up passes **714 workspace/all-target tests
plus one ignored each**, with and without dashboard features. Fresh default
binaries/examples build; format, JS syntax, source boundary and diff checks pass.
`installed-selection-COTfk0` passes public local/offline commands, saved-versus-
applied checks, activation no-op/no-write and corrupt-cache refusal without a
partial provider change. `model-metadata-6b2ZUj` passes approved remote Ollama and
GGUF activation and owner denial alongside existing protocol regressions. These
two fixtures use mock Ollama and synthetic cache bytes, not real inference.

`real-installed-selection-R6vXTz/results.json` reuses the existing verified
1,280,835,840-byte 2B development cache without copying/downloading weights or
changing its installed index. Public offline selection with explicit activation
verifies the artifact in **17,019 ms**. Starting the isolated node with the bundled
CPU runtime reaches ready in **3,234 ms**, applies that interpreter despite a
reachable mock Ollama service, and subsequently completes explicit unload with
the owned native PID observed exited. Only a new unique test configuration is
changed. No inference, model-quality/default approval, physical two-PC or complete
installer/onboarding acceptance follows. Job commit accounting in the result is
not total/current RAM, VRAM, a resource quota or a general termination guarantee.

The test script defaults to offline selection without activation or runtime
loading. Real startup requires explicit `EEF_TEST_ACTIVATE_BACKEND=1` and
`EEF_TEST_RUNTIME_BUNDLE` plus its existing-cache directory/catalog-ID arguments;
it never enables downloading. All test-owned processes are stopped afterward.

Rust checks cover cross-backend role preservation, same-name identity, clear/no-op,
invalid/disabled targets, duplicate selections, role overflow, concurrent writes,
remote owner denial, strict CLI/JSON and corrupted saved-role reports. The public
metadata fixture checks local and coordinator commands without inference, denied
remote changes, no-op assignment/clear, saved/applied separation and restart.
It also drops/substitutes local HTTP replies after an actual role save and checks
read-only inspection, exact pre-send targets, no replay and older-endpoint refusal.
The input fixture checks interpreter withdrawal with retained-history inspection.

This is explicit role setup, not completion of non-coder onboarding or the default
lightweight model requirement. Automatic first-run integration of private pairing,
approved configurable
bootstrap/model quality, physical two-PC acceptance and MSI/Defender/signing
remain release gates. No published alpha.6 artifact or update feed is replaced.
