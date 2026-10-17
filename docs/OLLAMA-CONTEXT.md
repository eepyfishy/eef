# Owner-controlled Ollama request context (unreleased)

An owner can save an explicit context window for this node's EEF-owned Ollama
requests without changing the shared Ollama server or a model definition:

```text
eefn model-lifecycle-policy show --json
eefn model-lifecycle-policy --runtime RUNTIME_UUID set --expected-digest SAVED_DIGEST --ollama-context-tokens 4096 --json
eefn restart --wait-seconds 60 --json
```

Offline policy setup holds the existing instance lock and omits `--runtime`;
the next node start applies it. Approved `eef node model-lifecycle-policy
--node NODE_ID ...` uses the same backend validation/save/report path. Remote
management must be enabled by the node owner locally; the command cannot enable
it. The existing runtime and versioned-digest fences, revision advancement,
no-op/no-write behavior and saved/applied distinctions still apply. Saving does
not restart, infer, load, change model/provider/role, or grant execution.

`--ollama-context-tokens` accepts integers 512–1,048,576. This is a request-option
bound, **not a claim that a selected model supports that context or available
memory can hold it**. Model/server limits still apply. Large windows can increase
memory use; inspect resources and choose an appropriate value. Reset with
`--ollama-context-default` and explicitly restart to inherit backend behavior.
The two options conflict; malformed, out-of-range or stale commands fail without
changing settings. Restart applies all pending node settings, not just context.

The saved policy field is optional `ollama_context_tokens`. Absent/null means
inherit, and inherited values are omitted from normalized serialized policy, so
old default-policy digests remain unchanged. `set` uses the optional integer or
`ollama_context_default: true`; a null integer alone does not request reset.
Old binaries cannot read a non-default new field: reset with this build before
downgrading rather than silently losing a saved owner preference.

The applied engine sends the value as `options.num_ctx` on Ollama requests,
including interpretation, preparation, text and supported vision generation.
Ollama documents [context length](https://docs.ollama.com/context-length) and the
[`num_ctx` parameter](https://docs.ollama.com/modelfile#valid-parameters-and-values).
No shared-service settings or model files are edited, and no process ownership
is inferred. Caller/model/grant parameters cannot override this owner choice;
if inherited they cannot inject their own context option through this adapter.
llama.cpp retains its existing per-slot context allocation and receives no
Ollama context option. A new runtime cannot claim an applied policy that its
engine did not receive.

This is node-wide, not per-model tuning, a tokenizer-aware input budget, provider
attestation, total RAM/VRAM quota or a forced unload/termination guarantee. EEF
does not rewrite/truncate original text or messages; a backend may reject or
truncate requests under its own context behavior. Do not assume every accepted
input fits a small window. Unsupported backend options fail through the existing
backend error path without unconstrained fallback or automatic inference retry.

## Verification and real-provider test scope

Unit checks cover bounds/conflicts/reset, default serialization/digests, exact
saved-value confirmation even with a substituted valid digest, request-override
refusal and no llama.cpp injection. Runtime binding refuses an engine/policy
mismatch without replacing the current owner. The headless policy fixture
checks remote approval, saved/applied context, explicit restart and saved-only
reset. Preview checks inspect the actual mock-provider request's `num_ctx`.

`tools/test-real-ollama-context.mjs MODEL_NAME [LOOPBACK_ORIGIN]` separately tests
one synthetic owner preview against an already installed local Ollama model.
It requires explicit `EEF_TEST_ALLOW_SHARED_OLLAMA=1`; it never downloads,
creates/deletes models, stops the service or forces an unload. It refuses remote/
wildcard/DNS/proxied-by-test endpoints, credentials, paths/query strings, unrelated
resident models and insufficient inspected free system RAM. Admission checks
are observations, not exclusive leases: concurrent shared-service activity can
still change resources/state. The test clears proxy environment variables for
its own node, checks provider-reported model identity before/after, saves only
disposable node settings, requests 4,096 context tokens and 60-second retention,
and records reported residency/context when available. Inference can load the
model and affect shared resource/retention behavior. Only test-owned node/CLI
processes are stopped afterward; normal service expiry/other clients retain
ownership. Provider digests/size are reports, not verified GGUF-file hashes.

The test does not establish faithful interpretation, model/default approval,
physical two-PC acceptance or an exact resource quota. The full two-backend
quality evaluator and stable-release acceptance remain separate work.

### Measured checkpoint: 2026-10-17

Both locked/offline workspace/all-target suites pass **720 tests plus one ignored
each**, including no-dashboard, and fresh binaries/examples build. Public preview
`request-preview-GSCkYn` passes actual mock-request context forwarding and existing
schema/redaction/no-retry regressions. Public policy fixture `model-policy-luqHLg`
covers denied/approved remote context changes, save/restart/reset and existing
synthetic GGUF lifecycle behavior. Observer
admission guards and synthetic-provider success/rejection tests pass; even failed
previews retain their typed reply without retry or disclosing rejected model text.

The one real cold-provider attempt `real-ollama-context-Pp7uS6` **fails** with
`interpretation_timeout`. It uses existing Ollama 0.40.4 and the owner's already
installed Qwen3-VL tag (provider reports 6,140,415,879 bytes), not a newly obtained
or hash-verified GGUF. Saved/applied context was confirmed as 4,096 before the
single preview. A narrowly inspected local provider log reports `n_ctx = 4096`,
then cancellation while loading at the existing **20-second** preview deadline;
the immediate `/api/ps` inspection is empty. Log evidence is not the fixture's
successful residency/context assertion, and no accepted proposal or quality score
was obtained. The original failed run's private logs/results are retained, not
rewritten after improving failure capture. No retry, timeout widening, preload,
forced unload, service stop or model download follows this attempt.

This establishes a cold-load usability gap: saved interpreter selection does not
mean a larger shared Ollama model is ready for a bounded preview. Explicit model
preparation/readiness and the full quality comparison remain necessary before
claiming reliable automatic setup. No physical node, installed app, public
release, update feed or dashboard changed in this checkpoint.

Ollama's [documented empty-request preloading](https://docs.ollama.com/faq#how-can-i-preload-a-model-into-ollama-to-get-faster-response-times)
is a candidate for the next backend-owned preparation operation, not implemented
or performed by this checkpoint. That operation still needs explicit owner
admission, applied-selection/context checks, bounded status/cancellation and
truthful shared-service side-effect reporting; it must not retry inference or
claim exclusive ownership of shared residency.
