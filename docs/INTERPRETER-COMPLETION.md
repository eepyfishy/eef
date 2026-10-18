# Text-free interpreter completion diagnostics (unreleased)

An interpreter reply must finish before its proposal can be validated or saved.
The existing `interpretation_incomplete` error now optionally includes a static
`completion_reason` and readable explanation. It never includes generated text,
reasoning, parser excerpts, private paths or arbitrary provider finish strings.

| `completion_reason` | Observation |
| --- | --- |
| `reasoning_only_output_limit` | Provider reports a length limit, empty/whitespace final content and nonempty reasoning |
| `output_limit` | Provider reports a length limit without that reasoning-only condition |
| `tool_call` | Provider reports a tool/function-call completion |
| `content_filter` | Provider reports filtering |
| `not_finished` | Provider explicitly reports `done:false` |
| `unconfirmed_finish` | Missing/unsupported metadata does not establish a completed answer |

These are bounded classifications of **untrusted provider reports**, not proof
of model behavior, resource enforcement, semantic correctness or authority.
They use neither model names nor provider-reported token counters. A reasoning
request hint is not proof that reasoning was disabled. Ollama documents that
[`think:false` depends on model support](https://docs.ollama.com/capabilities/thinking).
Provider/model capability metadata and actual observed output may disagree.

The same engine and annotations serve preview, clarification resolution and
method preparation, including input/answer workflows. Reasoning is never
substituted for final content; a partial answer is never repaired into a proposal.
No tool call, alternative model, extra inference, increased token limit, download,
restart or configuration write follows these diagnostics. Existing source/answer
history remains intact after a failed later stage. Reusing a retained input/answer
receipt still only inspects history.

Normal completed inference retains its previous normalized response shape, and
successful owner commands receive no completion-failure annotation. Unknown
internal failure codes keep their prior behavior. The original generic failure
code remains compatible; clients can display the optional classification without
using it to authorize work. Old strict successful-report validators continue
rejecting injected failure diagnostics on otherwise successful replies.

## Opt-in private test observer

For the fixed synthetic test, response capture can be explicitly enabled:

```powershell
$env:EEF_TEST_ALLOW_SHARED_OLLAMA = '1'
$env:EEF_TEST_CAPTURE_OLLAMA_RESPONSE = '1'
$env:EEF_TEST_INFERENCE_TIMEOUT_SECONDS = '120'
node tools/test-real-ollama-context.mjs INSTALLED_MODEL_ID http://127.0.0.1:11434 --prepare
```

`tools/ollama-test-observer.mjs` is a **test helper**, not a production proxy or
network service. It listens only on loopback and forwards raw bounded bytes to
the declared IP-literal loopback provider, without redirects, proxies, response
repair or replay. Routes are restricted to tags/ps/show/chat. Selected model,
declared synthetic input, one preparation and one generation are checked;
generation retains 512 tokens, context 4,096 and retention 60 seconds. Tool/image
requests and enabled-thinking overrides are refused by this narrow fixture.

Requests are bounded to 256 KiB and responses to 4 MiB. Raw bytes (base64, plus
UTF-8 renderings) are saved only in ignored `PRIVATE-observed-chat-*.json` test evidence, using
exclusive creation. Do not publish those files: they can contain model reasoning
and the complete synthetic prompt. Public observation metadata contains hashes,
sizes, bounded counters and allowlisted completion categories, never provider
text. Counters and digests are reports/fingerprints, not artifact verification or
semantic approval. Cancellation drops the observer's request, not ownership of
shared provider work. The existing explicit shared-service opt-in/admission and
no-pull/no-stop/no-forced-unload rules still apply.

## Diagnosed existing-model failure

The separately captured run `real-ollama-context-SwNAbc` retains its failed result.
It uses the existing Ollama 0.40.4 and owner-selected Qwen3-VL tag, not downloaded
or bundled weights. Preparation completes in 5,546 ms; the single preview returns
in 37,852 ms with HTTP 200, `done:true`, `done_reason:"length"`, 512 evaluated
tokens, **zero final-content bytes** and **2,432 reasoning bytes**. The exact
request contains `think:false`, `num_predict:512` and `num_ctx:4096`.

The installed provider metadata advertises thinking values `[false,true]`, but
the observed request still returns reasoning only. The
[upstream maintainer discussion](https://github.com/ollama/ollama/issues/16945)
describes this kind of thinking-variant limitation. The
[v0.40.4 renderer](https://github.com/ollama/ollama/blob/v0.40.4/model/renderers/qwen3vl.go)
accepts the supplied flag when rendering; neither that implementation nor the
metadata proves that the trained model will produce a bounded non-reasoning
answer. This local observation does not establish every variant/version's behavior.

No reasoning text was used as a proposal, no token limit or validation was
relaxed, and no replacement model was automatically selected. Reliable/default
interpretation, full two-backend quality evaluation, bootstrap, physical testing
and stable v0.4.0 release acceptance remain unfinished.

## Verification checkpoint: 2026-10-18

Both locked/offline workspace/all-target suites pass **735 tests plus one ignored
each**, including no-dashboard; fresh binaries/examples build. New units check
both adapters' classifications, no reasoning promotion, completed-shape
compatibility, closed enum/code mapping and unchanged error/authority flags.
Public `request-preview-z0eK0F` covers all six categories, private text/unknown
finish-string suppression, no retries, unchanged timing/schema checks and runtime
invalidation. Pinned-TLS `request-input-8QpPHC` verifies reasoning-only failures
during method preparation and answer resolution preserve source/answer revisions
and inspect retained receipts without another inference.

Synthetic observer tests pass byte/hash equivalence, loopback route/model/input
and output-option checks, no replay/redirect, bounded request/response bodies,
strict UTF-8 classification and private-only raw capture. The oversized-response
test caught an end/error race in the new helper before publication; it now rejects
the body without forwarding a truncated HTTP success. Capture admission guards
and the public synthetic prepare/preview wrapper also pass. These are isolated
same-PC tests, not physical two-PC or generalized model quality evidence.

The separate real run `real-ollama-context-bswWyD` uses the owner's existing
Gemma tag on Ollama 0.40.4, with no download/copy or production default change.
Provider reports digest
`c6eb396dbd5992bbe3f5cdb947e8bbc0ee413d7c17e2beaae69f5d569cf982eb`,
9,608,350,718 installed bytes and a 4,096-token resident context. Explicit
preparation completes in **13,726 ms**. The one synthetic preview completes in
**7,196 ms**, reports `stop`, 89 evaluated tokens and 381 final-content bytes,
with no reported reasoning field. Request capture confirms `think:false`, the
same 512-token cap and unchanged synthetic input. No workload was dispatched.

The proposal passes structural validation and preserves the original text, but
asks an unnecessary output-format question for a clear system-information
request. This is **one successful completion, not semantic/default approval**;
the full frozen-case quality comparison remains necessary. The requested wait
is explicitly 120 seconds; the observed warm response also fits the unchanged
20-second default, but cold/physical and general latency are not proved.
The tested node binary SHA-256 is
`6bfb37c0e536fcb7009eb93dc2db3d591ef6d3034e805b4b54e62a55cd073010`.

The earlier captured Qwen failure uses the preceding binary and remains failed;
its exact provider response shape is covered by the new synthetic production
checks. Neither that raw reasoning nor any completed-answer substitute was
published. Model names occur only in evidence, not diagnostic logic. No installed
app/physical node, UI, shared-service setting, release or update-feed change.
