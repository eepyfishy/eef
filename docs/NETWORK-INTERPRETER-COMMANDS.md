# Explicit network interpreter commands

This is unreleased development source awaiting hosted validation. It is not in
the published alpha installers and does not enable automatic normal-input
delegation or complete the stable-v0.4 roadmap.

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
service/grant limits. That increment awaits its own hosted validation.

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

Automatic integration into `requests input`, existing local interpretation and
clarification/preparation workflows, explicit historical-result recovery,
physical checks and the remaining release gates are still required.
