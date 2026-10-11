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
Provider selection remains independent: `auto` prefers available Ollama and
does not activate a llama.cpp selection merely because it has this role.
Choose a provider explicitly with `models provider` if needed, then restart.

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
EEF inspects support/approval; the node rechecks current approval under its
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

Rust checks cover cross-backend role preservation, same-name identity, clear/no-op,
invalid/disabled targets, duplicate selections, role overflow, concurrent writes,
remote owner denial, strict CLI/JSON and corrupted saved-role reports. The public
metadata fixture checks local and coordinator commands without inference, denied
remote changes, no-op assignment/clear, saved/applied separation and restart.
It also drops/substitutes local HTTP replies after an actual role save and checks
read-only inspection, exact pre-send targets, no replay and older-endpoint refusal.
The input fixture checks interpreter withdrawal with retained-history inspection.

This is explicit role setup, not completion of non-coder onboarding or the default
lightweight model requirement. Friendly private pairing, approved configurable
bootstrap/model quality, physical two-PC acceptance and MSI/Defender/signing
remain release gates. No published alpha.6 artifact or update feed is replaced.
