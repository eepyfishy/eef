# Interpreter property-order screen: 2026-10-26

Production remains unchanged. This is a direct native-runtime experiment, not
public-node integration, an approved interpreter/default, bootstrap or release.

## Fixed comparison

The current Rust schema serializes object properties lexically, placing
`clarification` before `goal` and `intent`. The cached runtime's pinned
[JSON wrapper](https://raw.githubusercontent.com/ggml-org/llama.cpp/c1d0e7a00/common/json.h)
preserves insertion order, and its
[grammar converter](https://raw.githubusercontent.com/ggml-org/llama.cpp/c1d0e7a00/common/json-schema-to-grammar.cpp)
emits required properties in that order. This motivated the hypothesis that
early clarification generation might impair interpretation; it did not prove
that order caused the existing semantic failures.

The plan was saved before inference and its SHA recorded in the preflight.
Use the existing verified `qwen3.5-2b` artifact and native runtime, with no model
download or copy. The matching compiled `interpretation_fixture` emits the
actual Rust messages/schema and independently validates every completed proposal.
Both complete frozen twenty-case suites and their unchanged scorer are used.
The recorded six-capability baseline context is held fixed, not claimed to be a
new live inventory. Source revision is `32d2c02`.

Compare exactly two conditions: current lexical property order and
`schema_version, goal, intent, context_hints, constraints, suggested_capabilities,
complexity, workload, needs_clarification, clarification`. Prompt messages, schema
values, required arrays and anyOf alternatives are identical. Before inference,
parsed/canonical JSON equality and distinct intended wire order are checked.
A bounded loopback observer captures actual HTTP request/response bytes without
rewriting them. No global serde_json ordering feature is enabled or changed.

Each condition/suite gets a separate owned CPU server: two decode and prompt
threads, zero GPU layers, one slot, 4,096 context, temperature zero, the same
unspecified provider seed policy, disabled-thinking controls and a 512-token cap.
Each synthetic input is submitted once under the existing twenty-second wait.
The runner checks native idle between cases and would stop that run on uncertain
idle state. No generated instruction executes anything.

## Results: useful diagnostic, insufficient quality

All eighty requests complete and pass the unchanged Rust contract. Independent
reconciliation recomputes the references, validations and grades, verifies all
captured requests and unchanged source/helper/app/runtime/model hashes, and
observes no remaining owned model/app processes.

| Condition | Suite | Label matches | Unnecessary questions | Required questions lost | Exact template-goal copies |
| --- | --- | ---: | ---: | ---: | ---: |
| Current order | original-v1 | 14/20 | 6 | 0 | 13 |
| Current order | generalization-v1 | 13/20 | 7 | 0 | 13 |
| Goal first | original-v1 | 16/20 | 4 | 0 | 6 |
| Goal first | generalization-v1 | 13/20 | 5 | 0 | 5 |

These are this experiment's concurrent direct-backend baselines, not replacements
for earlier public-node runs with different thread settings or source versions.
Do not compare its 13/20 generalization baseline to the earlier 12/20 as though
only property order changed between those historical runs.

Goal-first improves some English requests and reduces copied placeholders, but
still misses eleven labels, asks nine unnecessary questions and copies eleven
template goals. Review of all forty goal-first goal/question pairs finds clear
system requests asking for optional metric/format details, a Thai greeting asking
which node is meant, and thanks interpreted as an underspecified confirmation
task. Some ambiguous requests invent an active job/current-node context or
suggest irrelevant capabilities. A required question being present does not
establish that it is grounded or useful.

**Do not integrate this as the interpreter-quality solution or approve a default.**
It is a limited, single-pass diagnostic with no independent held-out/human
acceptance, no other backend/model comparison and no production-path integration.
Keep the production prompt, schema, validator, property serialization, model
catalog/default, permissions and generation limits unchanged. Retain the measured
gain without claiming semantic readiness or tuning another order against these
same development cases.

## Evidence and audit boundary

The ignored fixture is `interpreter-field-order-gcExWp`: preflight input/plan/script
hashes, eighty prepared requests, eighty actual HTTP captures, four native process
records, raw private results and `independent-audit.json`. Its source epoch has
330 source files and the same model/helper/native runtime identities used by the
preceding accepted development checkpoint. That checkpoint's four-job hosted
[run 40284296697](https://github.com/eepyfishy/eef/actions/runs/40284296697),
full Rust/native, CPU and browser acceptance are separate from this quality run.

The first metadata auditor incorrectly required a numeric exit code after
intentional native termination. Its failed source is retained unchanged. A
separate corrected auditor leaves those codes unconfirmed, reconciles the close
records and independently observes no live matching processes; it does not infer
exit zero, rerun inference or relabel the original auditor as passed. All model
outputs, cases, bounds and scoring remain unchanged. No physical node, installer,
update feed, model policy or UI development is changed by this experiment.

## Existing Gemma comparison (2026-11-23)

The subsequent packaged alpha.7 evaluation of the existing `gemma4:latest`
model motivated a second-backend comparison using the same two property orders
defined above. The plan and script hashes were recorded before inference.
Both frozen suites ran once per condition through loopback Ollama `0.40.17`.
The model digest was
`c6eb396dbd5992bbe3f5cdb947e8bbc0ee413d7c17e2beaae69f5d569cf982eb`.
The shared production reference supplied the prompt, capabilities and schema.
Context was 4,096 tokens, output was bounded to 512 tokens, temperature was zero,
and the disabled-thinking hint and twenty-second inference wait were fixed.

| Condition | Suite | Label matches | Unnecessary questions | Required questions lost |
| --- | --- | ---: | ---: | ---: |
| Current order | original-v1 | 13/20 | 7 | 0 |
| Current order | generalization-v1 | 12/20 | 8 | 0 |
| Goal first | original-v1 | 17/20 | 0 | 0 |
| Goal first | generalization-v1 | 18/20 | 0 | 1 |

All eighty outputs passed the shared Rust validator. A separate audit revalidated
and regraded every output, reconciled all eighty reconstructed request hashes,
and checked the script, helper and reference hashes. Output keys followed the
requested property order in all eighty responses. Schema values, required arrays
and alternatives remained semantically identical between conditions.

Manual review of the five goal-first label mismatches found four proposals choosing
`system.ping` for node status or responsiveness, where the frozen rubric requires
`system.info`. Those scores remain recorded as mismatches. The other proposal
interpreted an unresolved cancellation as "Cancel the previous task" and omitted
the required question. These results motivate production-path testing and further
review of unresolved references. Interpreter/default approval remains open.

This was a sequential, single-pass direct-provider comparison with fixed synthetic
inputs. The retained fixtures are
`.validation/gemma-property-order-f5fbcd6a80de460f9c7fcd0ac22a2c9d/`, including
the predeclared plan, private results and `audit.json`. The experiment used the
already installed model through the existing shared service and issued zero
workload grants. Source revision was `4f0136d`.

## Public-node candidate (2026-11-23)

Source `389e32b` adds a backend request serializer for the complete ten-field
proposal schema. Both Ollama and llama.cpp request adapters serialize those
properties in the measured goal-first order. Message objects, unrelated schemas,
schema values, required arrays and protocol/task definitions retain their
existing representations. The independent Rust interpretation validator owns
acceptance. The serializer introduces no model-specific selection or permissions.

The candidate's actual node executable then ran both complete suites through
`requests preview`, with the same existing Gemma model and bounds as above.
All forty proposals were accepted structurally. Scores were 17/20 and 18/20,
with zero unnecessary questions and the same one lost cancellation question.
Both prompt and schema reference hashes matched the preceding packaged-node
baseline. The run recorded execution, dispatch and automatic retry as false.

The candidate binary SHA-256 was
`67fd5b732e4fd2c33215f2ba24feb91eebc231bbecc726ec7becf7905950723e`.
Local reports are `interpreter-quality-gsj12o` and `interpreter-quality-xi6nGY`
under `.validation/`. Sixty-six focused Rust regressions passed across schema
serialization, interpretation, task binding, response completion, adapter
behavior, interpreter serving and sampling. Full hosted acceptance and model
quality approval remain open. Physical-network and owned CPU-runtime evaluation
of this source remain separate requirements.

## Normal network input finding (2026-11-23)

The candidate node executable above was also tested through ordinary
`requests input` using two logical nodes on one PC. The input node had no model.
A separate provider shared the already installed Gemma interpreter under an
explicit owner-approved policy. Both registered over pinned private TLS.
The coordinator used the published alpha.7 executable, whose interpreter task
definitions remain compatible with this serializer-only candidate.

For "Show the operating system and memory usage of this node," the initial
interpretation correctly selected `system.info` without asking a question.
Method preparation then copied its prompt's example question, "Which concrete
input should be used?", and returned `clarification_required` without a plan.
For "Cancel the previous one," initial interpretation asked which request was
meant and did not advance to method preparation. The three interpreter service
reports completed and released their slots. No workload execution was approved
or dispatched, and no automatic retry was requested.

The successful transport and service accounting do not constitute successful
method preparation. The initial preview scores above cover an earlier stage.
The method prompt currently supplies a filled clarification example and no
output schema hint; copying that example is an observed failure, while its
precise cause remains a hypothesis. Useful end-to-end model behavior remains an
open acceptance gate.

The retained run is
`.validation/real-model-network-input-417149ba6d0345c9a2782eaf09bb1ef1/`,
including `results.json`, private source/results and a separate `audit.json`.
An earlier fixture omitted model selection and stopped before inference when
the public role command refused it. That fixture remains retained separately.
This finding covers one PC with two logical nodes and an existing large model.
It provides no physical two-PC, lightweight default-model or release approval.
