# Explicit requests without model interpretation (unreleased after alpha.6)

Caller-supplied inputs can now enter the existing durable review/compiler
workflow without an interpreter, proposed-input JSON or direct RPC execution:

```powershell
eefn requests submit --text "Show status" --template system_info --operation-id UUID --json
eefn jobs request-text --prompt "Explain this topic" --role USER_SELECTED_ROLE --operation-id UUID --json
eefn requests submit-process --text "Check this tool" --program TOOL --arg=--version --node NODE_ID --operation-id UUID --json
eefn requests submit-audio --to NODE_ID --duration-ms 2000 --operation-id UUID
eefn requests plan JOB_ID --json
```

`request-text` optionally accepts exact `--target-node`, `--backend` and `--model`.
They are caller selections, not inferred model hints. For other parameter-bearing
templates, `requests submit --text ORIGINAL_TEXT --file concrete-plan.json` accepts
the existing bounded `PlanInput` file. `--template` is parameter-free; optional
`--node` is exact and conflicts with file mode. `--title` is configurable display
text. Omitted operation IDs are generated and returned; retain the exact receipt.

`submit-process` is the subsequent typed adapter for `execute_process`. Repeat
`--arg` for separate literal arguments (use `--arg=-flag` for a leading hyphen);
spaces, empty arguments and original input text are retained without shell
joining. Optional `--cwd`, `--timeout-seconds` (1-3600), `--node` and `--title`
become the same exact concrete input used by the existing submit service.
No JSON file, model, ephemeral node or new execution runner is needed. Argument
count/byte and wire bounds match existing helpers; invalid/NUL input rejects
locally before submission. Executor permissions, working-directory scope, local
timeout and separately approved grant duration still apply independently.
Submission/receipt reuse cannot start the program or grant permission. Changing
any parameter/title/text under a retained operation ID conflicts. This is native
command authoring for review, not a command-line shortcut to owner approval.

The backend does not interpret or execute these inputs. It describes the explicit
template using the shared capability catalogue, leaves compute complexity unknown,
and preserves unchanged original text and concrete input in the existing review.
Input is not permission. No request originates from a fabricated node, and neither
classifier attribution nor model authority is manufactured.

EEF stamps authenticated origin/area, validates fresh disclosed capability names,
then compiles against fresh self/directionally disclosed executors before ranking.
Selected model/resource and provider parameters still pass the existing compiler.
No coordinator-local adapter, hidden-provider fallback, model inference, download,
workload reservation, owner approval, grant or dispatch occurs during submission.
The resulting plan is `awaiting_approval`; use the separate existing owner approval
and origin execution commands. Approval/execution requires an owner-pinned TLS
executor. History remains origin-owned, even when another node executes it.

## Persistence and uncertainty

There are two journal checkpoints: explicit source admission, then compilation.
A compiler failure or lost reply can leave only a `review` source. This is reported
as unconfirmed/uncertain, not absent or completed. Inspect:

```powershell
eefn jobs find --operation-id UUID --json
eefn requests plan JOB_ID --json
```

The source retains `concrete_input` even before a concrete plan exists. It is
immutable: changing prompt, title, model/routing choices or parameters under the
same receipt conflicts. Existing compilation cannot substitute a different input;
semantic `prepare` rejects explicit-input sources. Stop/cancel the old request and
create a new receipt to change intent rather than rewriting it silently.

Compilation uses a separate deterministic custom UUID derived from origin, opening
receipt and saved job ID. This is correlation, not authentication or authority.
It cannot overwrite the opening receipt. Concurrent identical submissions retain
one source/plan while that job history exists; removing history ends that scope.
An explicitly repeated exact command may finish a still-eligible partial admission.
It cannot compile against a later edited revision or revive cancelled work.

Already-compiled exact receipts inspect frozen history without fresh provider
lookup, model selection or authority renewal. They work after restart and later
approval/execution, with no new model call or runner. The source and compiled input
are definition-digest bound; old records omit the optional input field and retain
their old digest bytes. Older binaries cannot read new marked records: preserve
backups and do not downgrade that journal.

## Command/API boundary and remaining migration

Guarded node API: `POST /api/commands/requests/submit`, with exactly
`schema_version:1`, `expected_node_id`, and
`command:{operation_id,request:{schema_version:1,original_text,title,input}}`.
No caller origin, approval or grant fields are accepted. Cross-site/extra-field/
wrong-node calls reject. `request-text` uses the same service through typed jobs
commands; conflicting top-level/command receipt IDs reject before submission.
An unsupported or unconfirmed command never falls back to legacy jobs/chat/RPC.

This is a usable explicit grant-aware entry point. The subsequent unreleased
[execution boundary](EXECUTION-BOUNDARY.md) migrates `generate-text`, moves normal
input to node-owned durable review, and retires unreviewed job/task/chat/rule and
ordinary workload RPC admission. Automatic source-bound workflows remain open.
Existing lifecycle regressions remain required for release; granted unknown
effects cannot be safely resumed just because a legacy path allowed it.
Delegated approval, automatic policies, real-model quality, physical two-PC and
complete executor termination/reconciliation remain separate release gates.
