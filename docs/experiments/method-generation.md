# Method generation guidance (2026-11-23)

This is development evidence using the owner's existing shared Gemma model.
It does not approve a default interpreter, authorize generated actions, or close
physical two-PC or stable v0.4.0 acceptance.

## Observed failure

The [ordinary network input test](interpreter-property-order.md#normal-network-input-finding-2026-11-23)
correctly interpreted a clear system-information request but copied the method
prompt's example clarification question instead of preparing a method. The
method stage supplied no structured-output hint. The preceding initial preview
scores did not test this stage.

## Controlled screens

Twenty synthetic retained reviews were prepared before inference, with expected
method/question choices, templates and selected parameter values. Twelve cases
have concrete inputs across system information, screen capture, speech, files,
application launch, HTTP, Wake-on-LAN, text generation and keyboard input. Eight
need missing input or have no supported one-step method. Capability lists and
retained initial interpretations are synthetic; these are not twenty complete
ordinary user workflows or held-out model acceptance data.

Both screens use existing `gemma4:latest` through loopback Ollama `0.40.17`,
provider-reported digest
`c6eb396dbd5992bbe3f5cdb947e8bbc0ee413d7c17e2beaae69f5d569cf982eb`.
Context is 4,096 tokens, output is bounded to 512 tokens, temperature is zero,
the disabled-thinking hint is set, and each inference has a twenty-second wait.
The shared service is neither stopped nor forcibly unloaded. No model is
downloaded/copied and no workload is executed.

The candidate replaces the filled clarification illustration with a description
of the two permitted alternatives, uses an empty parameter object for templates
without parameters, and asks for missing required input rather than optional
settings. The catalogue, history, validator and generation bounds are fixed.

| Screen | Prompt | Structurally valid | Expected outcomes | Unnecessary questions | Missing required questions |
| --- | --- | ---: | ---: | ---: | ---: |
| No schema hint | Existing | 20/20 | 8/20 | 12 | 0 |
| No schema hint | Candidate | 0/20 | 0/20 | Not assessable | Not assessable |
| Method schema hint | Existing | 20/20 | 8/20 | 12 | 0 |
| Method schema hint | Candidate | 20/20 | 18/20 | 1 | 0 |

All twenty prompt-only candidate responses contain Markdown fences, so the
unchanged validator rejects them. They remain failed responses; stripping their
fences would change the acceptance boundary. Both schema-guided conditions use
the same lexically serialized schema. It describes the method/question union,
eligible template names, untrusted parameter objects and area/display-name
constraint hints. Empty catalogues omit the method alternative.

The schema-guided candidate still asks which node is meant for a screenshot of
"this node" and selects `read_file` for a directory listing. Those scores remain
failures. Structural guidance and reduced copying do not prove semantic truth,
permission, safe parameter values, correct resource routing or suitability as a
lightweight default.

## Evidence and limits

The ignored fixtures are
`.validation/method-prompt-comparison-20261123-v1/` and
`.validation/method-schema-comparison-20261123-v1/`. Each retains a pre-inference
plan, script/helper/case hashes, private cases and complete responses. Separate
audits reconcile all eighty request hashes, revalidate every response with the
bounded Rust `method_fixture`, and regrade against the saved expectations.
The helper used unchanged production validation and prompt code from `63f6788`
plus a test-only stdin adapter. The second screen reuses the first screen's
development case definitions; it is not independent generalization evidence.

The original baseline, failed candidate and their audits remain retained. The
candidate is a generic generation change with no hardcoded model or user-request
matching.

## Source integration and ordinary workflow

The development implementation adds the same hint to selected local preparation
and hash-bound network interpreter tasks. All twenty compiled prompt/schema
references match the measured candidate exactly, including schema property order.
Sixty-two focused Rust regressions pass across preparation, task binding,
serialization, adapters, serving, response completion and sampling. A schema
refusal causes one failed call without unconstrained fallback; differently pinned
method tasks cannot substitute the new definition. The subsequent compatibility
review adds method task version two and retains the published version-one prompt
and absent schema for old task/report validation. This prevents a new prompt
definition from invalidating retained evidence. New constructors select version
two; relabeling a task cannot change the reconstructed input or authorize fallback.
The frozen synthetic version-one task preserves the captured pre-change prompt
digest. Extended grant-report tests confirm legacy results while rejecting altered
authority, source, model and attribution fields. An initial fixture used a
non-UUID request ID and correctly failed grant validation. Its corrected context
uses a valid UUID and preserves the original inference-spec digest; no production
validation rule was relaxed. Separate reference checks preserve all twenty old
definitions, match twenty current definitions, and reject forty version relabelings.

The separate public-command fixture `network-input-commands-Dk1P9D` passes with
matching coordinator/node binaries, a simulated model and two logical nodes on
one PC. It covers model-free input, separate provider inference, private TLS,
prepared review, explicit owner approval, system-information execution, retained
result retrieval and receipt reuse without extra inference. This is framework
behavior with simulated semantics.
After task versioning, the fresh `network-input-commands-lxBsqs` fixture passes
the same complete public-command path using the versioned source.

The real-model fixture `real-model-method-guidance-20261123-v1` still returns
`clarification_required` for the clear system-information request. Its initial
interpretation is correct, but method preparation now asks for a node name or
identifier. This run used the guidance implementation before task versioning;
its prompt/schema content matches the new definition, but it did not test the
version-two wire rollout with a real model. The ambiguous cancellation also remains in clarification. The fixture
fails its `awaiting_approval` assertion before requesting owner approval or tool
execution. This is an unresolved model usability failure, despite the improved
direct-screen score. The node-name question is not the earlier copied example.

Two subsequent direct screens compare the current candidate with one further
change at a time. Each repeats the twenty development cases and adds the retained
real review before inference. A generic explanation of existing origin-relative
placement reduces matches from 18/21 to 13/21 and still fails the real review.
Projecting history to semantic proposals/questions/answers also reduces matches
from 18/21 to 13/21. It fixes that real review while introducing six additional
unnecessary questions. Both alternatives are rejected; neither is integrated.

Their separate audited fixtures are `method-origin-guidance-20261123-v1` and
`method-history-projection-20261123-v1` under `.validation/`. All 84 responses
validate structurally and their saved request hashes/scores reconcile. They are
reused development inputs, not new generalization acceptance. The original full
review remains in the production prompt and source/task identity remains bound.
Real-model semantic quality, hosted acceptance of this source, physical two-PC
and all other stable-release gates remain open.

## Native integration attempts

The broader hosted run
[45182291340](https://github.com/eepyfishy/eef/actions/runs/45182291340)
exposed nine coordinator exchange/application/recovery failures. Their shared
synthetic model identified initial interpretation by the presence of a schema,
which now also occurs in method preparation. It returned an interpretation where
a method was required; the validators refused that wrong response. The fixture
now identifies the method stage from the reconstructed system message and checks
the exact schema against the pinned task alongside its messages. This preserves
the existing completion, recovery, lineage and no-replay assertions. Fresh full
hosted results remain required.

The first full `test-request-input.mjs` invocation omitted its explicit Python
dependency and stopped before camera lifecycle checks. A fresh run with the pinned
Python then exited natively with `0xc0000409` on the installed Node.js `24.15.0`.
Neither is a passing full-suite result.

A subsequent fresh fixture used the existing portable `24.20.0` executable,
rechecked against the previously recorded SHA-256 and valid OpenJS signature.
It passed the camera and distributed vision sections, then timed out waiting for
the cold-model placement observation. The required `model_startup_fixture.exe`
had not been built in this binary directory. The fixture now checks its explicit
Python and helper paths before starting services. With the complete helpers,
the final `request-input-he6IKr` run passes the entire native input fixture on
the cached portable runner. It covers both camera/vision layouts, source and
answer lineage, partial/uncertain outcomes, restart, retained results, actual
synthetic model process ownership and cold-memory observations. Devices and
model responses are simulated. The prior failures remain retained; no timeout,
assertion or production RAM check is relaxed. No global runner, installed model
or app was changed by these runs.
