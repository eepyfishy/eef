# Explicit network interpreter commands

The explicit command source `19cc599` passed all four jobs in its
[hosted run](https://github.com/eepyfishy/eef/actions/runs/42731181964) and an
independent source/log audit. Its caller-ceiling follow-up `006e996` also passed
[all four jobs](https://github.com/eepyfishy/eef/actions/runs/42731997375) and source
review. These are unreleased development checkpoints, absent from published
alpha installers. Automatic normal-input delegation and stable-release
acceptance remain open.

The next admission follow-up exposes a crate-private shared implementation for
input workflows that already own the node's input slot. The public command
still acquires that slot; the admitted path borrows it until the correlated
reply or runtime retirement, repeats target validation, and retains all existing
runtime and confirmation checks. It adds no public endpoint, permission, worker
or retry. Hosted regressions cover all three stages, competing public commands,
invalid/stale targets, retirement and a discarded late reply. This is an
integration prerequisite awaiting validation; ordinary input still needs its
own acceptance. The [normal-input follow-up](NETWORK-INPUT-WORKFLOW.md) now adds
an opt-in privacy preference, remote attribution and stage wiring; hosted
validation of that source remains pending.

The running input node can request one source-bound stage through an explicitly
authorized interpreter service. It needs no installed model. The provider needs
an applied interpreter-sharing permission and an explicitly selected text model
with the `request_interpreter` role. The coordinator owner must approve the
origin/provider scope and bounded use through
[interpreter service policy commands](INTERPRETER-POLICIES.md). The coordinator
does not install or load a model.

Save and interpret an initial request:

```text
eefn requests save-input --text "Inspect node status"
eefn requests interpret-network SAVED-ID
```

The backend reads and pins that saved input. No JSON, source digest, prompt,
gateway session, grant or port number belongs on the command line. The command
prints an operation ID; preserve it after uncertainty. An optional
`--operation-id UUID` explicitly supplies that receipt.

Continue a saved clarification answer or prepare a method:

```text
eefn requests interpret-network SAVED-ID --stage clarification --revision REVISION
eefn requests interpret-network SAVED-ID --stage method --revision REVISION
```

Use the revision of the retained answer/review, available through
`eefn jobs get SAVED-ID`. Clarification needs a recorded answer; method
preparation needs an eligible reviewed request. Original text, first origin and
area, previous proposals and answers remain intact. A method result either
asks a clarification question or saves a deterministic concrete plan awaiting
separate owner approval. These commands never execute tools.

Optional preferences are `--provider NODE-ID`, `--backend ollama` or
`--backend llamacpp`, `--model-id MODEL-ID`, and `--local-only`. Preferences
restrict existing advertised, authorized models. They do not install models,
assign roles or permissions, or bypass serving permission for a local provider.
An unavailable explicit selection is refused without silently choosing another.

The separate [caller-ceiling follow-up](INTERPRETER-CALLER-CEILING.md) adds
`--inference-timeout-seconds` using the existing 20-second default and narrower
service/grant limits. Exact source `006e996` is accepted as described above;
the normal-input follow-up has separate pending validation.

Reusing the same stage/source inspects retained history before checking current
provider/model/policy availability. A completed checkpoint remains inspectable
after provider loss or policy revocation. Changed source/receipt or conflicting
preferences are refused; a retained result never recreates a live continuation.
An interrupted inference or a result whose source update was lost needs explicit
reconciliation, which remains an open requirement. No automatic replay occurs.

Inspection and cancellation remain ordinary node commands:

```text
eefn jobs get SAVED-ID
eefn jobs stop SAVED-ID
```

A stop request or tracking state does not prove remote model computation or
external effects stopped. The existing exact-grant stop checkpoints and
uncertainty are preserved. No grants, serving credentials or raw prompts are
included in the command receipt; its optional source view belongs to the
authenticated originating node.

The [plain-output follow-up](INTERPRETER-COMMAND-GUIDANCE.md), awaiting hosted
validation, identifies the interpreter node, displays an actual unanswered saved
question and its revision, and offers read-only plan guidance when appropriate.
Uncertain attempts retain inspection/stop guidance without a continuation command.
JSON output may contain private original input and review data.

The local API is `POST /api/commands/requests/interpreter`, with an 8 KiB body
limit, existing browser-origin protection, and node/runtime checks. CLI and API
use the same `NodeService` backend and authenticated node submission path. The
coordinator accepts only a retained reference, stage, revision, operation and
selection preferences, builds the exact typed task, then uses existing routing,
capacity, policy, allowance, grant, completion and source-checkpoint services.
No browser controller, new worker, idle poller or dependency is introduced.

Hosted tests include private TLS input/provider clients with synthetic model
responses, all three stages and concrete compilation, offline history,
cross-origin/stale-session/runtime/selection denials, stop/no-replay, CLI parsing,
reply validation, and the shipping model-free CLI/API refusal path. Passing them
will not prove real-model quality, physical two-PC or release acceptance.

Automatic integration into `requests input` and clarification/preparation is in
[development validation](NETWORK-INPUT-WORKFLOW.md). Explicit historical-result
recovery, physical checks and the remaining release gates are still required.
