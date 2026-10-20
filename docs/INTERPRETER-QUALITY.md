# Production interpreter quality evaluation (unreleased)

This evaluation invokes the real running node's `requests preview` command,
production prompt, selected backend and strict Rust validator. It is separate
from execution acceptance and the older small classifier benchmark. An accepted
proposal remains untrusted and cannot authorize work.

The retained [two-backend frozen-case baseline](#two-backend-frozen-case-baseline-2026-10-18-no-default-approval)
extends evaluation to existing shared Ollama. A subsequent
[balanced-illustration experiment](#rejected-balanced-illustrations-2026-10-18)
was rejected on both backends; its gains on clear inputs lost required questions.
Production interpreter, expectations and scorer are restored unchanged. Neither
evaluated model is approved.

## Rejected balanced illustrations (2026-10-18)

The [experiment and criteria fixed before inference](experiments/interpreter-balanced-illustrations.md)
replace the baseline's ambiguous-only shape with clear/underspecified complete
illustrations and distinguish optional presentation choices from essential missing
details. Both frozen suites run on both existing backends under the same schema,
validator, capabilities, sampling, context and output bounds as the retained
baseline. No model download/copy or installed-node changes occur.

All eighty proposals are structurally accepted. CPU label scores fall to **7/20
and 6/20**; shared Ollama gives **11/20 and 12/20**. Although unnecessary questions
and exact sample-goal copies drop to zero, **eighteen required questions are lost**
across the four runs. Finite actions become continuous and one CPU question treats
illustration text as possible actual request context. Full result tables,
identities, review limits and private fixture/reference locations are in the
experiment record. All four reference-capture regression checks pass before
candidate binaries are replaced. No proposal authorizes or executes work.

**Rejected:** the candidate source and experiment-only test are removed, leaving
all production source unchanged from `c0151b3`. Restored focused interpretation
tests pass **11/11**, and fresh node/reference-helper builds succeed. The rebuilt
helper's entire messages/schema reference exactly matches the preceding baseline,
including prompt SHA
`f9ec6ea9d7341bb7a5594959667b184485d5c9749f6a107c0228ce6866373216`
and schema SHA
`2660caf1caed38b74c11e266d1c79de1060ed80284d8ba270977c5b4aa84aa8b`.
The archived rejected text also matches the evaluated candidate bytes/hash.
Binary reproducibility is not claimed. Expectations, grading, validation,
permissions and generation limits are never loosened to retain the candidate.
The restored public preview regression passes in `request-preview-FUD6HR`,
including completion rejection/redaction, bounded inference waits and no replay.
Frozen scorer, argument-admission, backend/browser source-boundary, formatting
and diff checks also pass. Test-owned processes are terminal after cleanup.

No default model/bootstrap, installed/physical node, UI, release or stable feed
changes. The latest full Rust checkpoint remains 735 passed plus one ignored
in each mode; those full suites are not rerun for this source-restored evidence
checkpoint. Further interpreter work needs stronger method/model/grounding
evidence, not repeated prompt additions alone. Physical/bootstrap/release gates
remain open.

## Reproduction

The same evaluator supports owned CPU GGUF and explicitly opted-in existing
loopback Ollama. These are developer test tools, not end-user setup requirements.
Neither mode downloads/copies models, changes installed nodes or approves quality.

### Owned CPU GGUF

Build fresh default-feature binaries, then supply an existing, exact catalog
artifact and a runtime bundle. The harness verifies the artifact's size/SHA-256,
records runtime/node binary hashes, and uses the real owner command to assign the
interpreter role in its disposable configuration. It never downloads or copies
a model, modifies installed nodes, or contacts a coordinator.

```powershell
node tools/test-interpreter-quality-rubric.mjs
$env:EEF_TEST_BINARY_DIR = 'ABSOLUTE_FRESH_BINARY_DIRECTORY'
$env:EEF_TEST_MODEL_FILE = 'ABSOLUTE_EXISTING_CATALOG_MODEL_FILE'
$env:EEF_TEST_MODEL_ID = 'qwen2.5-small'
node tools/test-interpreter-quality.mjs ABSOLUTE_RUNTIME_BUNDLE_DIRECTORY
```

### Existing shared Ollama

Use an already-installed text-capable model. Clear CPU-artifact variables so a
provider-reported tag/digest cannot be mistaken for a hash-verified GGUF:

```powershell
$env:EEF_TEST_MODEL_FILE = $null
$env:EEF_TEST_MODEL_ID = $null
$env:EEF_TEST_ALLOW_SHARED_OLLAMA = '1'
$env:EEF_TEST_THINKING_MODE = 'disabled'
$env:EEF_TEST_INFERENCE_TIMEOUT_SECONDS = '120'
$env:EEF_TEST_QUALITY_SUITE = 'original-v1'
node tools/test-interpreter-quality.mjs --ollama INSTALLED_MODEL_ID http://127.0.0.1:11434
$env:EEF_TEST_QUALITY_SUITE = 'generalization-v1'
node tools/test-interpreter-quality.mjs --ollama INSTALLED_MODEL_ID http://127.0.0.1:11434
```

Thinking remains an owner-request hint, not proof of provider behavior; omission
still inherits policy. An explicit per-call wait is recorded, while omission
retains the existing 20-second default. Prompt/schema, 512 output-token cap,
rubric and case sets are the same in both modes. No repair, expected-label change
or automatic inference retry makes a run pass. Completed harness execution is
not a model-quality approval, and accepted JSON is not faithful interpretation.

Shared mode checks loopback IP-literal origin, preinstalled tag/digest/size,
reported non-cloud identity, completion capability, observed free system RAM and
unrelated resident models before fixture creation. Metadata has a five-second
absolute deadline, strict UTF-8 parsing and a 4 MiB bound. No redirect is followed.
Owner/provider inspection uses an explicit direct HTTP agent; the Node 24.5+
[`proxyEnv` option](https://nodejs.org/docs/latest-v24.x/api/http.html#new-agentoptions)
is empty, and the fixture's shared-provider child has proxy environment settings
cleared. The tested portable toolchain is Node 24.20.0. These checks do not reserve
memory, attest provider identity or prevent another shared client from changing
state between observations.

Only a scratch node is started. Its ordinary owner commands assign the role,
save/apply context 4,096 and retention 60 seconds, confirm the applied selection
through `setup`, then request one explicit preparation. API startup is reported
separately from provider preparation; paused advertisement lists do not prove
model health. The tag/residency/context are checked around each case. A tag,
context or competing-resident change aborts without another inference. Timeouts,
backend/runtime/transport failures and unfinished/unconfirmed provider outcomes
also stop further cases rather than compound uncertain shared work.

Reports distinguish `backend:"ollama"`, `shared_ollama_provider:true`, reported
identity/residency and `model_file_hash_verified:false` from owned CPU file SHA,
runtime hash and process samples. They do not invent GPU-layer counts, thread
settings or CPU-only measurements for an unowned service. Reference capture
supports both typed evidence forms and legacy CPU reports, and rejects CPU
artifact/resource claims in shared-provider reports. Configured context is a
request preference; absent/null observed context stays unknown, not zero or
confirmed. Cleanup stops only test-owned node/CLI processes;
it never stops the shared service or forces a model unload. Ordinary shared
retention/other clients remain in charge. New outputs stay in ignored validation
storage; do not publish private reference files as model attestation.

Optional `EEF_TEST_THINKING_MODE=inherit|disabled|enabled` uses the public owner
policy command in the isolated fixture before launch; inheritance remains the
default. Reports record the choice and check saved/applied agreement. The same
option is available to `tools/test-real-model.mjs` with explicit restart and no
token-budget change. It is a request hint, not proof that the backend honored it.
Compare runs at the same prompt, rubric and bounds; never infer model quality from
a useful short generation or from reasoning-mode support alone.

`EEF_TEST_QUALITY_SUITE=original-v1|generalization-v1` selects a fixed case set;
omission retains the original twenty inputs/expectations and their SHA. The
additional twenty cases in `tools/interpreter-generalization-cases.mjs` were
defined before their first model evaluation, after the latest candidate prompt
was written. They vary wording/languages, finite observations, ongoing relay/
monitoring, missing resources/attachments/history and authority overrides. The
fixture supplies no feed, attachment or prior-task context. This is additional
development evidence, **not independent human review or a default-approval gate**.
Neither case set executes the requests. The same unchanged grader is used.

Optional `EEF_TEST_INTERPRETER_SAMPLING_ARGS` is a JSON array of explicit sampling
CLI flag/value strings. The harness accepts only the seven documented sampling
flags, bounded numeric text and no duplicate flags; it cannot inject config paths,
restart, execution or other commands. It passes those choices to the public owner
policy command in the disposable config and checks saved/applied agreement after
start. New reports record normalized `interpreter_sampling`. Omitting the variable
retains temperature-zero interpretation and omitted optional hints. JSON here is
test-harness input, not a requirement for configuring a real node: the normal
commands accept human decimals, without editing JSON.

New reports record `case_set` and the exact case-list SHA. Private reference
capture selects only these known sets and verifies the case SHA before emitting
the matching shared Rust prompt. Older reports without a set ID mean original-v1.
`tools/test-interpreter-reference.mjs BINARY_DIR COMPLETED_QUALITY_FIXTURE` tests
capture against a fresh metadata-only scratch copy: matching capture, unknown/
substituted case sets, corrupted hashes and exclusive output refusal. It leaves
the original evaluation unchanged and performs no inference or execution.

The API-only fixture disables coordinator connections, uses CPU-only GGUF
inference, and sends twenty fixed synthetic inputs. It verifies unchanged saved
configuration, no downloads, no workload and zero successful connections. It
stops only its owned processes. Reports stay in ignored
`.validation/interpreter-quality-*` directories, not release artifacts.
New reports also record a `prompt_reference` hash emitted by the test-only Rust
fixture using the shared prompt function and the same capability vocabulary.
This is a reproducibility reference, not intercepted inference or remote
attestation. Build examples alongside the fresh production binaries.
The next evaluation-tool increment also records the whole message-sequence hash,
message count and an exclusive private full-prompt reference, so system-only
hashes cannot conflate examples added in separate chat turns. Existing reports
remain unchanged. A matching existing run can be captured before its binaries
are replaced using `tools/capture-interpreter-reference.mjs BINARY_DIR FIXTURE_DIR`.
The tool requires an existing isolated quality fixture, matching node/helper
hashes and matching system/full-message hashes, and refuses to overwrite evidence
or write outside the validation directory. It performs no inference or workload.

Schema-bearing evaluations additionally record `prompt_reference.schema_sha256`
and save `PRIVATE-generation-reference.json` containing both `messages` and
`schema`. Capture checks the schema against the same matching helper, rejecting
changed or malformed hashes before writing. Matching case-set, exclusive-output
and node/helper/system/message checks still apply. Older prompt-only report
shapes continue to capture `PRIVATE-prompt-reference.json`, without claiming a
schema reference. Metadata-only tests cover both shapes, preserve the source
evaluation and perform no inference. These references identify shared code
output, not intercepted requests or provider attestation.

Rubric version 1 is defined in `tools/interpreter-quality-cases.mjs`; expectations
were fixed before candidate runs. It covers status, conversation, Thai/Spanish,
missing targets/inputs, continuing camera requests and instruction overrides.
`accepted_count` counts strict contract acceptance. `matched_count` additionally
checks allowed intent/workload, clarification decision and required capability
hints. Failed proposals count as mismatches. Question specificity, faithful goal
paraphrase, useful planning and prompt-injection resistance require independent
review; this rubric does not measure them. Once used to tune a prompt, these are
development cases, **not held-out evaluation**. Do not retune expected answers
to a candidate's observed output.

`harness_completed` means the fixture's checks finished. Exit zero, accepted
JSON, low memory use or a semantic match does not approve a model.
`default_approved` is always false; approval requires separate reviewed evidence.
Latency includes CLI overhead. Windows peak working set is a sample of the owned
model process, not total RAM demand or an enforced memory limit. Thread count is
unknown; these runs did not set a measured CPU budget.

## Observed production runs, 2026-10-11

Candidate: existing verified Qwen2.5-0.5B-Instruct Q4_K_M, 491,400,032 bytes,
SHA-256 `74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db`.
CPU: Intel Core i5-13420H, 12 logical CPUs, about 32 GiB RAM. Runtime:
llama.cpp build 10621 / `c1d0e7a00`, SHA-256
`0f706d509ce7937504d527f49cc1e68a0e45ee60cfc4153b3ff16f76a0f4e3ac`.
All completed runs used a 4,096-token context, zero GPU layers and the production
512-token output limit. Rubric SHA-256:
`20c3665b0599ae0935128a16285d4d0f28bb0561c1924c22670e4746fec7b189`.

| Production variant | Accepted | Semantic matches | Startup | Peak working set | Evidence |
| --- | --- | --- | --- | --- | --- |
| Original prompt, no generation schema | 9/20 | 3/20 | 3,863 ms | 570,347,520 bytes | `interpreter-quality-8BJVVk` |
| Experimental definitions/examples | 1/20 | 1/20 | See local report | See local report | `interpreter-quality-xY1AGY` |
| Original prompt, bounded generation schema | 10/20 | 3/20 | 4,259 ms | 597,053,440 bytes | `interpreter-quality-XS9UE4` |

The worse example prompt was rejected and reverted; it is not production code.
The original prompt is unchanged. The schema is a formatting aid, not evidence
of improved semantic quality. These are individual development runs, not a
statistically controlled comparison or a physical second-PC test. The initial
`interpreter-quality-TfP8NQ` invocation failed a harness assertion and is not
counted as a completed run.

**This candidate is not approved as an automatic default.** Next gates include
better candidates/prompts, independently reviewed held-out multilingual cases,
second-PC CPU/memory measurement, and a configurable verified bootstrap with
offline/skip/cancellation/disk behavior that leaves deterministic commands usable.
See [candidate provenance](MODEL-CANDIDATE-EVALUATION.md) and
[interpretation boundaries](INTERPRETATION.md).

## Larger candidate and rejected prompt experiments, 2026-10-11

An optional `qwen2.5-1.5b` catalog entry pins the publisher's 1,117,320,736-byte
Q4_K_M GGUF. Its source/license and exact digest are recorded in
[candidate provenance](MODEL-CANDIDATE-EVALUATION.md#optional-15b-candidate-2026-10-11).
It is not a default, bundled model, automatic download or interpreter-role
selection. The Ollama alias does not identify the evaluated GGUF bytes.

The same production harness, CPU, runtime, context, generation schema and frozen
rubric were used. These are individual development runs, not held-out tests,
controlled accuracy comparisons or physical second-PC acceptance.

| 1.5B production variant | Accepted | Semantic matches | Startup | Peak working set | Evidence |
| --- | --- | --- | --- | --- | --- |
| Original prompt, bounded generation schema | 18/20 | 12/20 | 4,311 ms | 1,868,947,456 bytes | `interpreter-quality-a5yFhr` |
| Value-free ten-field definitions | 19/20 | 0/20 | 4,748 ms | 1,882,906,624 bytes | `interpreter-quality-0f6ATc` |
| Value-free definitions with explicit intent/workload guidance | 18/20 | 1/20 | 4,491 ms | 1,933,471,744 bytes | `interpreter-quality-wTy4XM` |

Independent inspection of the first run found all eighteen accepted goals copied
the template's placeholder goal and fourteen questions copied its generic
question. The 12/20 label score therefore overstates usefulness. The value-free
prompts produced more request-specific goals/questions, but repeatedly labeled
actions and greetings as information, requested an address for this-node status,
or failed to ask about underspecified ongoing/destructive work. Thai output also
needs language/faithfulness review. Neither experimental prompt is retained;
the production prompt and independent validator remain unchanged.

Reference prompt hashes for the rejected variants are
`3fa19f536c9ee2b26093163de6993f3e71f0d873c821ef2c4f6b8dc5db138579`
and `614de73f39e10efe690e89aa965d55bd91e2b8f7b25a33a48844d252298a8cbe`,
respectively. Their local reports retain actual synthetic inputs/proposals and
binary hashes; current source reproduces the retained prompt, not the rejected
variants. No expected answers, validator rules or approval boundaries were
relaxed to improve these counts. All runs completed without coordinator access,
workloads, downloads/copies or saved configuration changes.

**Neither evaluated candidate is approved for automatic default use.** Further
work must improve faithful interpretation and specific clarification, then use
independently reviewed held-out multilingual cases and physical CPU/memory
acceptance. Successful reviewed text generation is not this quality gate.

## Further rejected variants, 2026-10-14

Four new production-node runs reused the exact 1.5B artifact and runtime above,
CPU-only context/output bounds and the unchanged version-1 twenty-case rubric
(`20c3665b0599ae0935128a16285d4d0f28bb0561c1924c22670e4746fec7b189`).
The private configurations had no coordinator connection, workload, download,
model copy or saved-configuration mutation. No installed node/service changed.
These are development cases already used for tuning, not independent held-out
approval evidence or a statistically controlled quality comparison.

| Experimental variant | Accepted | Matches | Startup ms | Model peak bytes | Evidence |
| --- | --- | --- | --- | --- | --- |
| Value-free definitions with four inline examples | 19/20 | 7/20 | 5,185 | 1,906,327,552 | `interpreter-quality-M8naN6` |
| Same definitions/examples in separate chat turns | 11/20 | 7/20 | 4,769 | 1,906,122,752 | `interpreter-quality-fgy9bc` |
| Original prompt, decision-first generation-property order | 16/20 | 3/20 | 4,276 | 1,906,544,640 | `interpreter-quality-1RTlcV` |
| Compact value-free definitions, decision-first order | 20/20 | 11/20 | 4,254 | 1,919,172,608 | `interpreter-quality-R1ZA1g` |

The inline version generated more request-specific goals, but missed action,
clarification, capability and language expectations. The chat-turn version
improved some ambiguous-action labels but rejected many ordinary information and
conversation replies. Ordering alone still copied the old placeholder goal.
The compact ordered version accepted every JSON proposal, but misclassified or
asked unnecessary questions for basic status/conversation and missed ongoing
camera expectations. Thai language/faithfulness still requires review. None is
approved for default use; all experimental prompt and generation-serialization
changes were restored to the retained production implementation. Validator,
byte/token bounds, expected answers, permission and execution rules are unchanged.

Source inspection of the exact runtime's [schema-to-grammar converter](https://raw.githubusercontent.com/ggml-org/llama.cpp/c1d0e7a00/common/json-schema-to-grammar.cpp)
and [JSON wrapper](https://github.com/ggml-org/llama.cpp/blob/c1d0e7a00/common/json.h)
motivated the ordering experiment: object properties determine required-field
generation order, whereas the current Rust schema map serializes alphabetically.
The tested adapter altered only generation-hint property order, with tests proving
equivalent parsed values and unchanged ordinary payload bytes. It did not alter
canonical stored/wire authority JSON. Its poor observed quality is why it is not
retained; field ordering is not a semantic validator or an approval mechanism.

The last variant's matching full reference was captured privately before restoring
the source. Capture checks passed for matching binaries/hashes and refused an
existing output and an outside-validation directory without changing evidence.
New evaluations automatically capture the full reference. Model/node binary and
message-sequence hashes remain in the ignored fixture reports; no rejected binary,
model weights, user data or private report is published as a release artifact.

Restored-source default and dashboard-free workspace/all-target suites each
pass **631 tests**: EEF library 237 plus one ignored opt-in profiler, owner CLI
16, installer 6, node library 361 and node CLI 11. Fresh default binaries/examples,
formatting/diff, JS syntax, the unchanged semantic rubric and backend/browser
source boundary pass. Public `request-preview-vxTurt` and
`request-review-yZirQ8` pass with verification-chain exit 0, preserving original
input, clarification/answer lineage, no automatic execution and authority checks.
The production prompt, schema ordering, inference adapters and library exports
have no diff from the preceding source checkpoint. Only evaluation tooling,
position-independent input/answer assertions and evidence documentation remain.

The next quality work needs a stronger candidate or separately evaluated method,
then independent held-out multilingual review and physical CPU/memory acceptance.
Automatic verified bootstrap, friendly setup and the full stable 0.4.0 roadmap
remain incomplete. No production model was selected or approved by these runs.

## Qwen 3.5 candidate and thinking request hints, 2026-10-14

Added an optional **text-only experimental** catalog selection, not a new default:
`qwen3.5-0.8b`, `Qwen3.5-0.8B-Q4_0.gguf`, 563,036,064 bytes, SHA-256
`57d1997790d1744fba5b40a7317df71ea5e2acee28c47e78f0cce39c0703f8cf`.
The URL pins [ggml-org's converted artifact and model card](https://huggingface.co/ggml-org/Qwen3.5-0.8B-GGUF/blob/8fea620810c4afa23dd6443f999a48574c1611a3/README.md)
at revision `8fea620810c4afa23dd6443f999a48574c1611a3`. Its Apache-2.0/base-model
metadata was checked against [the original publisher](https://huggingface.co/Qwen/Qwen3.5-0.8B).
No vision projector is included; the Ollama alias is a separate, unevaluated
artifact. Existing catalog selections/defaults and published bundles are unchanged.

The first isolated public-command run, `real-model-NSEurh`, downloaded and
verified exactly that artifact, selected/tuned it only in the disposable fixture,
and loaded it successfully. It **failed** its first 24-token text assertion:
the persisted grant operation completed but returned empty content with
`finish_reason:"length"` and `completion_complete:false`. This is not a useful
inference success. That failed fixture/artifact remains intact.

The follow-up adds generic [owner thinking preferences](MODEL-LIFECYCLE-POLICY.md#thinking-request-preference-unreleased),
not model-name special cases, a token increase, reasoning-as-final-content, or
weaker validation. `model-policy-Ec9VTm` passes the headless public-command
fixture: remote-owner refusal/approval, saved/applied separation, explicit
restart, runtime/CAS fencing, and saved-only reset to inheritance. Unit tests
check both backend payloads, a real synthetic Ollama HTTP exchange, discarded
reasoning fields, strict enum choices, prior inherited policy digests, forged
report rejection and rejection of unapplied runtime preferences.

`real-model-GmkvV2` reuses the verified artifact with no download/copy. With the
owner preference explicitly applied as `disabled`, the same 24-token first
generation returns a nonempty greeting with `finish_reason:"stop"` and
`completion_complete:true`. The real CPU/pinned-TLS fixture also passes selected
load/unload/reload, retained results across restart, historical receipt/no replay,
automatic idle unload and authorized cold on-demand inference. Its two-thread
launch is checked; this is one-PC inference/lifecycle acceptance, not default
quality approval, physical memory release, resource quotas or two-PC acceptance.

`interpreter-quality-aU4FgT` then evaluates the **unchanged production prompt,
schema, validator, 512-token output limit and fixed 20-case rubric**, with the
disabled hint and CPU-only context 4096. It completes with **16/20 accepted,
8/20 matching**. Startup is 4,233 ms; the owned model peak working-set sample is
1,284,046,848 bytes. Threads are unspecified for this quality run; the sample is
not total application RAM or an enforced limit. Rubric SHA remains
`20c3665b0599ae0935128a16285d4d0f28bb0561c1924c22670e4746fec7b189`;
system prompt SHA remains
`56ec36c9a3b47393458479c216924d4d0b156aae31b80a1f5c2a1380ccd8ca5c`.
Full-message references and binary hashes are retained privately by the harness.

Four proposals fail the strict validator. Eleven accepted goals still copy the
placeholder wording (including escaped variants); six questions copy the generic
question. Ordinary health/status and greetings still request unnecessary
clarification or wrong workloads/capabilities. **Not approved as the automatic
interpreter default.** No expectations, validator or prompt were retuned to make
this candidate pass. Further method/model work, held-out multilingual review,
friendly verified bootstrap and physical CPU/memory acceptance remain required.

Fresh workspace/all-target suites pass **638 tests** in both default-feature and
dashboard-free modes (EEF 237 plus one ignored opt-in profiler, owner CLI 16,
installer 6, node library 367, node CLI 12). Fresh default binaries/examples,
format/diff, JS syntax, fixed-rubric checks and backend/browser source guard pass.
Public `request-preview-Hh1NRT` and `request-review-dgaZ97` pass with chain exit 0.
No installed app, physical node, shared model store, release artifact, automatic
update selection or default interpreter was changed. Stable 0.4.0 remains open.

## Rejected definition-only prompt and additional cases, 2026-10-14

The next candidate replaced the filled example object with field/decision
definitions, distinguished clarification from authorization, requested faithful
user-language goals and explained that a reference to this node is not a missing
node name. It did not change message roles/input, schema, limits or validator.
`interpreter-quality-BsQoNO` evaluates it with the same Qwen 3.5 artifact, disabled
thinking hint and original twenty cases: **12/20 accepted, 0/20 matching**.
Startup is 4,441 ms; owned model peak working-set sample is 1,244,704,768 bytes.
System prompt SHA is
`4f6fe275b8bf980f198fb3051cde048de1c6af638333f3f854ec13364a6a2351`;
the full private message reference and binary hashes remain in that fixture.
The candidate and its candidate-specific unit test were **restored to f01a099**.
No unsuccessful prompt change is retained in production.

Before reading that run's output, twenty additional development inputs and their
expected labels were defined in `generalization-v1`. With the restored production
prompt, `interpreter-quality-O2LRKu` completes with **14/20 accepted, 7/20 matching**,
startup 4,278 ms and owned model peak working-set sample 1,289,216,000 bytes.
Its case-list SHA is
`bba9b21940e00c46dd31db02f01ae2c3e5d44c8785d0142022ba3f78ab924174`;
the system SHA returns to the retained
`56ec36c9a3b47393458479c216924d4d0b156aae31b80a1f5c2a1380ccd8ca5c`.
Thread count remains unspecified; these are not resource-limit or total-RAM proofs.
The weak generalization result reinforces **not default-approved**, rather than
providing independent held-out human/physical acceptance.

Retained work is limited to fixed case-set selection/recording and reference
verification. `reference-capture-gRBbvY` passes matching capture, unknown and
known-but-substituted case-set refusal, corrupted case-hash refusal and exclusive
output preservation in a fresh metadata-only scratch copy. The original evaluation
is unchanged. Existing reports without a set ID retain original-v1 interpretation;
the original case-list SHA, scorer and expected answers are unchanged.

All production Rust/config files have no diff from f01a099. Restored interpreter
unit tests pass **7/7**, and the production node/reference helper rebuild succeeds.
JS syntax, both case-set scorer checks, diff and backend/browser source guard pass.
The preceding 638-test workspace suites were not rerun for this tooling-only
checkpoint. No installed node/model, release, bootstrap default or execution
authorization changes. A stronger candidate/method and investigation of backend
template/generation behavior remain next quality work; the full roadmap stays open.

## Formatting-only backend diagnostic, 2026-10-14

`tools/test-interpreter-template.mjs RUNTIME_BUNDLE COMPLETED_QUALITY_FIXTURE`
requires `EEF_TEST_MODEL_FILE` and optionally `EEF_TEST_BINARY_DIR`. It verifies
the existing artifact/runtime hashes and known case-set hash, then asks the gated
Rust reference helper for the exact shared messages and generation schema.
The new read-only `generation_reference` function returns those two artifacts;
it neither infers nor authorizes anything, and adds no public HTTP route.

The tool starts a disposable connection-paused, API-only node with one selected
CPU-only model (context 4096, two worker/batch threads). It reuses the node's native
model-process ownership. Its backend route allowlist permits only GET health,
properties and slots, and POST formatting; no generation endpoint is called.
Responses are bounded, readiness/cleanup waits have deadlines, model weights are
not copied/downloaded, saved node config and source evaluation remain unchanged,
and the owned node is stopped afterward with backend unreachability checked.
It never starts a coordinator or touches installed nodes.

The pinned runtime's [formatting handler](https://github.com/ggml-org/llama.cpp/blob/c1d0e7a00/tools/server/server-context.cpp#L4932)
uses the same [chat parameter parser](https://github.com/ggml-org/llama.cpp/blob/c1d0e7a00/tools/server/server-common.cpp#L1128)
without inference. That parser passes the JSON schema to template processing and
handles thinking kwargs/reasoning effort, so the diagnostic includes the actual
shared schema rather than reconstructing a partial contract. The request body
is a constructed formatting reference, **not an intercepted production wire
capture or remote attestation**. Preserved formatting is not semantic quality.

`interpreter-template-cQAt6V` passes against the already-verified Qwen 3.5 0.8B
artifact and `b10621-c1d0e7a00` runtime. The restored system/full-message hashes
match `interpreter-quality-O2LRKu`. System instructions and original user text
are preserved in order in all three renders. Inherit and enabled render 1,315
bytes with the same SHA; disabled renders 1,326 bytes with a different SHA and
prefills the end of thinking. Runtime metadata reports system-role support and
no vision/video/audio modality for this text-only selection. No inference/workload
is issued, coordinator connections stay at zero, node stop is confirmed and
the backend becomes unreachable. Full private templates/properties stay ignored.

This narrows the missing-text/template hypothesis for the reference, not the
complete actual-wire or model-quality investigation. A separate controlled
sampling investigation is justified next: the [pinned publisher card](https://huggingface.co/Qwen/Qwen3.5-0.8B/blob/2fc06364715b967f1860aea9cf38778875588b17/README.md)
describes non-greedy text sampling settings and cautions about this small model's
thinking loops. EEF's present interpreter explicitly uses temperature zero.
Those differences are hypotheses, not proof of the observed failures or a reason
to change defaults, enlarge bounds, bypass validation or hardcode model names.

Fresh workspace/all-target suites pass **639 tests** in each feature mode (EEF
237 plus one ignored profiler, owner CLI 16, installer 6, node library 368,
node CLI 12). Default production binaries/examples, formatting/diff, JS syntax,
frozen case-set scorer checks and backend/browser source guard pass. With those
fresh binaries, `interpreter-template-9s6E9D` passes again with explicit
reference-not-wire metadata, and `request-preview-rU26p0` passes; chain exit is 0.
Neither diagnostic changes prompts, schema, validation, inference parameters,
permissions, selection/defaults or installed nodes. Stable release, physical
acceptance and the remaining roadmap are not complete.

## Owner interpreter sampling and rejected sampled profile, 2026-10-14

Added command-configurable [interpreter sampling preferences](MODEL-LIFECYCLE-POLICY.md#interpreter-sampling-preferences-unreleased)
to the existing node policy service, not a named-model preset or default change.
The owner can choose temperature, top-p, top-k, min-p, presence/repetition penalty
and a seed hint using ordinary decimal/integer CLI arguments. Bounds, exact
integer-thousandth storage, old default serialization/digests, explicit reset,
node/runtime/CAS fences and saved/applied separation are validated before use.
Sampling applies only to the selected interpreter's interpretation/clarification/
method calls; ordinary reviewed inference parameters are unchanged. No token,
byte, deadline, schema, validator, permission, selection or execution rule changes.

`model-policy-BbVuHi` passes headless public owner commands over pinned TLS:
unapproved remote changes are refused, approved choices save without changing
the running engine, explicit restart applies the exact sampling group, and
reset is saved-only until another restart. Unit tests check scalar/decimal bounds,
partial patches, reset conflicts, old digest preservation, forged saved reports
even with recomputed hashes and rejection of unapplied runtime claims. A real
synthetic Ollama HTTP exchange checks that the owner options reach interpretation
but not ordinary inference; both adapter builders preserve request bounds/schema
and exact default payloads. These checks are not real-model quality approval.

The isolated Qwen 3.5 0.8B experiment explicitly chooses temperature 1, top-p 1,
top-k 20, min-p 0, presence penalty 2 and repeat penalty 1, based on the publisher's
non-thinking text guidance, plus seed 42 as an experimental backend hint.
Thinking stays disabled. Both runs reuse the same verified artifact/runtime and
the unchanged production prompt, schema/validator, context 4096, output limit
512 and fixed case sets. No download/copy, coordinator or execution is involved.

| Case set | Accepted | Matches | Startup ms | Model peak bytes | Evidence |
| --- | --- | --- | --- | --- | --- |
| Original v1 | 16/20 | 4/20 | 4,034 | 1,229,099,008 | `interpreter-quality-M0MKx0` |
| Generalization v1 | 13/20 | 5/20 | 4,028 | 1,404,895,232 | `interpreter-quality-TgPPbQ` |

These are worse label scores than the preceding temperature-zero runs (8/20 and
7/20 respectively). The profile is **not approved or made default**, and the
candidate remains experimental. Seeded sampling is still stochastic/backend
dependent; a single seed/run does not establish quality, reproducibility or a
complete explanation for failure. Unspecified thread counts/working-set samples
are not enforced resource limits or total application RAM. Both case-list SHAs
and the system/full-message reference hashes remain unchanged; reports record
the normalized owner options and matching saved/applied state. Failed proposals
remain failures; no expectations or validation were relaxed to improve scores.

Configurable preferences are retained as backend functionality needed for
replaceable models. Defaults remain temperature zero with optional sampling
hints omitted, and there is no model-name-specific logic. Further candidate/
method evaluation, independent review and physical/bootstrap acceptance remain
required before selecting an automatic default or declaring stable 0.4.0.

Fresh default-feature and dashboard-free workspace/all-target suites each pass
**648 tests** (EEF 237 plus one ignored profiler, owner CLI 16, installer 6,
node library 376, node CLI 13). Fresh default binaries/examples, format/diff,
JS syntax, frozen scorer and backend/browser source guard pass. Public
`request-preview-PpY1bW` and `request-review-4YfzHv` pass; chain exit is 0.
`reference-capture-KaSXp0` verifies the new run's matching reference and refusal
of substituted/unknown sets, hashes and overwrite without changing evidence.
`tools/test-interpreter-quality-options.mjs` rejects malformed, duplicate,
non-sampling and oversized-value arguments before model/config/fixture work.
No installed node, release, selected model, default or permission was updated.

## Rejected prompt experiment: field descriptions instead of sample answers

An isolated source experiment removed the example proposal object from the shared
interpretation prompt. In the retained temperature-zero 0.8B baseline
`.validation/interpreter-quality-aU4FgT/results.json`, multiple accepted proposals
copy the literal instructional goal or generic sample question. Some of these
also match the frozen label rubric, which intentionally does not grade goal
paraphrase or question specificity. This is not evidence of useful interpretation.

The experimental prompt derived its exact field list from the unchanged generation
schema, described their meaning without sample answers, preserved original input
as a separate message, and distinguished missing context from not knowing an
answer or current system state. It asked for a concrete goal and a specific
missing-detail question, not a response to the user or a claim of execution.
No schema, validator, capability, authority, inference limit or rubric changes.

The changed node library passed **420 library tests** in each feature mode.
Both full workspace/all-target suites then passed **701 tests plus one ignored**
each, fresh binaries/examples built, and public API preview regression
`request-preview-B3xqnN` passed. These checks did not establish model quality.
The frozen scorer, option rejection and backend/browser source checks also passed.

Using the same verified 2B cache, runtime, temperature-zero policy, disabled
thinking hint, context/output bounds and unchanged case lists produced:

| Case set | Baseline accepted / matches | Experiment accepted / matches | Copied sample goals | Experiment evidence |
| --- | --- | --- | --- | --- |
| Original v1 | 20/20 / 14/20 | 19/20 / 9/20 | 0/20 | `interpreter-quality-86iK16` |
| Generalization v1 | 20/20 / 12/20 | 18/20 / 6/20 | 0/20 | `interpreter-quality-vAtAC3` |

Clear greetings/status queries improved, but underspecified actions lost needed
clarification. For example, an unspecified application close was marked clear
and suggested `system.info`; an unspecified microphone relay was marked clear
and suggested configuration/update/pipeline capabilities. A fabricated authority
request also lost clarification. These are untrusted proposals, not grants or
executed actions, but they are unacceptable interpretation regressions.

**The experiment was rejected and its source changes and experiment-only unit
test removed; the previous prompt is restored.** No weakened validator, rubric,
permissions or automatic default was retained. Both baseline and experiment still
fail default acceptance; do not trade away missing-target/authority handling to
remove placeholder strings or improve only the easy cases.

Reports preserve the complete synthetic replies and shared prompt references.
Baseline prompt SHA-256 is
`56ec36c9a3b47393458479c216924d4d0b156aae31b80a1f5c2a1380ccd8ca5c`;
experimental prompt SHA-256 is
`582bac72e1627c8613d9d4097658f61970de1b0f7ad43389ccc6551cd57dab24`.
Baseline node/reference hashes are
`c52a0f9a9c6db8c638c48258eb8661e967252bc04a51b385707f1a7cd7167511` /
`bec5f74904dba3306f7571e8b40a7843b5c8b76a469b4d0852bbd67fea4faeae`;
experimental hashes are
`e8b10de7d77538133a8fe8d4ce49e052c54b0b3ad20376a63cf56b5900e470b4` /
`9bd615da1257a11315c9a056eabc3906bdb0f84b12fd8f7346bce73cba97b5c3`.
No model download/copy, coordinator, execution, installed-app update or release
was involved in the prompt comparison. Further interpreter work and independent
human/physical/bootstrap acceptance remain required.

After restoration, fresh workspace binaries/examples built, all eight focused
interpretation tests passed, and public preview regression `request-preview-xL8Ou4`
passed. The rebuilt reference executable emits system-prompt SHA-256
`56ec36c9a3b47393458479c216924d4d0b156aae31b80a1f5c2a1380ccd8ca5c`,
exactly matching the retained baseline; at that restoration checkpoint the
interpretation source had no diff against the pre-experiment commit. Rebuilt
executable hashes differ from the
earlier builds, so prompt restoration is established by source/content checks,
not an assertion of binary reproducibility. The rejected prompt is not shipped.

## Application-context increment (unreleased; no default approval)

A smaller follow-up retains the original example, field definitions and
ambiguity/authority instructions. It appends factual application context: an EEF
node is a user-owned endpoint device, "this node" means the request's origin,
interpretation does not require knowing the requested answer/current state, and
unresolved references or unavailable inputs must not be invented. Original text
stays in a separate message. These are model instructions, not authenticated
origin proof, target assignment, a prompt-injection boundary or execution authority.
No schema, validator, policy, output bounds, sampling default or role changes.

Both frozen suites reuse the same verified 2B cache/runtime with disabled thinking
and temperature zero. They complete with no model downloads/copies, coordinator,
workload execution or personal configuration changes:

| Case set | Accepted | Label matches | Copied sample goals (baseline → context) | Startup ms | Model peak working set bytes | Evidence |
| --- | --- | --- | --- | --- | --- | --- |
| Original v1 | 20/20 | 14/20 | 16 → 12 | 4,455 | 2,138,529,792 | `interpreter-quality-fJDbY6` |
| Generalization v1 | 20/20 | 12/20 | 14 → 13 | 4,427 | 2,247,856,128 | `interpreter-quality-i3o6Ci` |

Aggregate label scores are unchanged, not an accuracy improvement. In the second
suite, the Thai transfer intent improves, while a continuous feed-monitor intent
regresses to information; both still request clarification. No expected-ambiguous
case loses clarification in these two runs, unlike the rejected field-description
rewrite. Clear requests still ask unnecessary questions and many goals still copy
the sample. This change supplies correct application context and modestly reduces
one observed failure mode, but does not solve interpretation or approve a default.
The small single-run differences are not statistical or held-out acceptance.

System-prompt reference SHA-256 is
`f9ec6ea9d7341bb7a5594959667b184485d5c9749f6a107c0228ce6866373216`;
node/reference executable hashes are
`3b421e26c109784b294b4a295b63351babb0b3b1e36c966ac11094f371248ecb` /
`baa611b6c9985fb1251b053e9d519eefa03670aed3f676dd683d1d05f9a0c699`.
The existing case-list hashes are unchanged. Process peak working set is not total
application RAM, physical memory release or an enforced resource quota.

Both locked/offline workspace/all-target suites pass **701 tests plus one ignored
each**, with default features and without dashboard features (EEF 243, owner CLI
17, installer 6, node library 420, node CLI 15). Fresh binaries/examples build.
Public preview regression `request-preview-W0Iv0h`, frozen scorer, source dependency
boundary, format and diff checks pass. The new unit checks shared context wording,
unchanged input/message separation and retained clarification/authority rules;
it does not establish semantic quality or resistance to prompt injection.
No model is automatically selected, no bootstrap default is approved, and no
installed app, permission, release or update feed changed.

## Rejected two-example prompt experiment

A subsequent isolated experiment kept the committed application-context prompt
and appended two synthetic input/proposal examples: a general-knowledge question
and an action missing both its setting and value. Both example proposals passed
the existing validator with an empty capability vocabulary and no execution
authority. The prompt explicitly marked examples as neither the actual request,
conversation history nor supplied facts; the actual input remained separate.
Ten focused interpretation tests passed and fresh binaries/examples built. No
schema, validator, sampling, output-limit, permission or frozen-case change.

The same verified 2B artifact/runtime, disabled thinking and temperature zero gave:

| Case set | Accepted | Label matches | Copied sample goals (committed context → examples) | Evidence |
| --- | --- | --- | --- | --- |
| Original v1 | 19/20 | 14/20 | 12 → 5 | `interpreter-quality-VprtK3` |
| Generalization v1 | 19/20 | 12/20 | 13 → 5 | `interpreter-quality-ifDdq2` |

Fewer literal placeholder goals did not improve label totals. A Thai greeting
and an English acknowledgement respectively failed validation. Continuous camera
watching/microphone-relay requests also became `one_shot` in the two suites,
where the committed prompt had retained a compatible workload/unknown value.
The examples both used one-shot proposals; this pattern does not establish why
the model made those errors. No benchmark expectation was relaxed or rewritten.

**Rejected:** the examples and experiment-only unit test were removed. The
committed application-context prompt remains the baseline. No failed output was
accepted, repaired, retried automatically or executed. Default bootstrap remains
unapproved. The public `interpretation_invalid` label alone does not distinguish
schema/bounds/clarification-consistency failures; better bounded, non-sensitive
failure diagnostics are a useful reliability follow-up, not grounds to relax
validation.

System-prompt reference SHA-256:
`80be8e0d3b171dabe9582e6e6b0f354acb7cd4bfbee85e55f3519a8cdcb68b51`.
Node/reference executable hashes:
`b3a85e85c6566a797f71c1f166176e6ad19383805dd9015f860584db46794446` /
`edac1f891995e33f4f218c27257f28df1761515534644436927793e22f7b2069`.
The case-list hashes remain unchanged; these are development experiments, not
independent human or physical acceptance. No model download/copy, coordinator,
installed-app change, release or update-feed change occurred.

After removal, fresh binaries/examples built, all nine focused interpretation
tests passed, and public preview regression `request-preview-dBFxoZ` passed.
The rebuilt reference emits the committed application-context system-prompt hash
`f9ec6ea9d7341bb7a5594959667b184485d5c9749f6a107c0228ce6866373216`;
the interpretation source has no diff against the committed baseline. The
example-based prompt is not retained or shipped.

The subsequent [typed rejection diagnostics](INTERPRETER-REJECTIONS.md) now make
future structurally rejected proposals more understandable without raw model text
or relaxed validation. Historical failures recorded only as `interpretation_invalid`
remain unclassified; no new quality result or default approval is inferred.

## Rejected decision-checklist experiment (2026-10-17)

Before evaluation, the candidate kept the committed application-context prompt,
shape illustration, ambiguity and authority instructions, then appends a decision
checklist. It asked for a request-specific goal/question rather than copied
illustration text; distinguishes facts/finite observations, social exchanges and
actions; separates one-shot from ongoing work; and distinguishes missing request
details from not yet knowing an answer or having execution approval. Missing
references, attachments/history and unsupported authority still require questions.
This is model guidance, not an authority boundary or changed validator.

The comparison used the existing pinned 2B cache and runtime, 4,096-token context,
512-token output bound, disabled thinking hint and temperature zero. No weights
are copied/downloaded, no coordinator is started, and no workload is executed.
Original-v1 and generalization-v1 cases, frozen grader and expectations stay
unchanged. Historical committed-context runs `fJDbY6` / `i3o6Ci` are the reference
(20 accepted each; 14/20 and 12/20 label matches; 12 and 13 literal copied goals).
This is development comparison, not independent held-out or default approval.

Retention criteria are declared before the first candidate inference: do not
reduce acceptance or either label total; do not lose a required question or turn
ongoing work into one-shot; materially reduce copied goals and inspect all
synthetic goals/questions for invented targets/state or missing purpose. Label
totals alone cannot justify retaining a regression. A failed candidate is removed
without changing expectations, bounds, permissions or the independent validator.
Even a retained improvement leaves multilingual human/physical/default-bootstrap
acceptance open.

| Case set | Baseline accepted / matches | Candidate accepted / matches | Literal copied goals (baseline → candidate) | Evidence |
| --- | --- | --- | --- | --- |
| Original v1 | 20/20 / 14/20 | 19/20 / 14/20 | 12 → 9 | `interpreter-quality-I9mW1I` |
| Generalization v1 | 20/20 / 12/20 | 19/20 / 11/20 | 13 → 9 | `interpreter-quality-McFMrY` |

The English greeting improved, and ongoing observation/relay proposals became
explicitly continuous. These gains did not meet the declared retention bar.
One Thai status query and the English acknowledgement respectively failed
clarification-consistency validation (`rejection_reason: clarification_required`).
The public reports do not expose rejected raw output, so the precise inconsistent
field combination is not inferred. Finite English/Thai transfer requests and a
fabricated grant request were incorrectly labeled continuous. Clear status and
resource requests still asked unnecessary questions and omitted `system.info`;
many goals still copied the illustration. One additional goal merely rephrased
the instruction as "Summarize the user's wanted outcome," illustrating why the
literal-copy count alone is not a goal-faithfulness score. Reviewing the full
synthetic replies did not establish independent multilingual/default quality.

**Rejected:** the appended checklist and experiment-only unit test were removed;
the committed application-context prompt is restored. No case, expectation,
generation schema, validator, capability, output/sampling limit, permission or
automatic-default policy was changed to make these runs pass. No accepted
proposal was authorized or executed. Further model/candidate work is needed;
additional prompt instructions are not evidence that the default-model gate is
solved.

For reproducibility only, [the exact rejected appendix](experiments/interpreter-decision-checklist.txt)
is retained as a static document, never imported by production or a runtime
selector. Join its lines with single spaces, trim, and append it to the committed
context system prompt with one separating space. With the evaluation capability
vocabulary, the resulting system-prompt SHA-256 is
`c7252bef546cf0d7a32be15420e0af2f56e52b5807883019f3fdf583bdb01171`.
Both runs used node/helper hashes
`b857c8b1b5db8808cadec0fdd50ebf316d3bf12e8f119ee716e009f36fdcd2d1` /
`7455bc22e3c3cb49514ec0e591b7064b33bf2e3d6d27b6dc3331adb07c2cbd39`.
Full synthetic prompt/reply evidence remains in the ignored fixture directories.
The historical baseline is not a new controlled replicate or held-out comparison.
No model download/copy, installed-app/physical-node update, coordinator, workload,
release or update-feed change occurred.

After removal, both locked/offline workspace/all-target suites pass **714 tests
plus one ignored each**, and fresh default binaries/examples build. Public preview
regression `request-preview-vG7wF4` passes. The rebuilt reference emits the exact
committed-context system-prompt hash
`f9ec6ea9d7341bb7a5594959667b184485d5c9749f6a107c0228ce6866373216`;
`interpretation.rs` has no diff against the pre-experiment commit. The static
appendix also exactly matches the recorded evaluated suffix, whose preceding
baseline hashes to that same reference. Formatter, diff, frozen scorer,
evaluation-argument and backend/browser boundary checks pass. These prove
restoration and regression checks, not model-quality or default approval.

## Generation clarification consistency (2026-10-17; unreleased)

The system prompt remains byte-identical. The generation schema now expresses
the **existing validator's** clarification decisions as two complete alternatives.
A clear proposal has false `needs_clarification`, a null question and known
intent/workload. A questioning proposal has true `needs_clarification`, a nonempty
question and may retain unknown labels. Every alternative requires the same ten
fields and refuses extra fields. Text/array/capability limits are unchanged.

Complete objects avoid mixing root properties with `anyOf`, a documented
limitation of the [pinned llama.cpp converter](https://github.com/ggml-org/llama.cpp/blob/c1d0e7a00/grammars/README.md).
No unsupported conditional/reference extensions or global JSON-key-order change
are introduced. Providers may ignore or reject grammar: the independent byte,
cross-field and authority validator remains mandatory, with no repair, fallback
or automatic retry. Correct grammar cannot decide whether a request is clear.

Before evaluation, retention required no lost acceptance/label total, no lost
required questions and no ongoing-to-one-shot regression against the existing
committed-context references. The same verified 2B artifact/runtime, disabled
thinking hint, temperature zero, 4,096-token context, 512-token output bound and
unchanged frozen cases were used. These are development runs, not held-out or
physical acceptance.

| Case set | Accepted | Label matches | Literal copied goals | Evidence |
| --- | --- | --- | --- | --- |
| Original v1 | 20/20 | 14/20 | 12/20 | `interpreter-quality-Bvx75l` |
| Generalization v1 | 20/20 | 12/20 | 13/20 | `interpreter-quality-iDUGub` |

All forty parsed proposals equal their historical committed-context counterparts.
No required question is lost and no ongoing request becomes one-shot. This meets
the limited retention bar for expressing existing format rules; **semantic
quality does not improve** and the same default-model gate remains failed.
The real llama.cpp requests complete successfully, but these runs are not an
exhaustive grammar-conformance test or real-Ollama acceptance.

System-prompt SHA remains
`f9ec6ea9d7341bb7a5594959667b184485d5c9749f6a107c0228ce6866373216`.
The schema reference for the evaluation vocabulary is
`2660caf1caed38b74c11e266d1c79de1060ed80284d8ba270977c5b4aa84aa8b`;
node/helper hashes at evaluation are
`2c408cd206e390e724a5ed207d9b445c2d867469ad09756bc2bd26c0647191e3` /
`f1bdd8e69d1c3d7d3193580b58a026687ae5c1e013a8281319595c74fae35011`.
Metadata-only reference regressions `reference-capture-wt1Luc` and
`reference-capture-MNqmfD` pass exact schema capture, changed/malformed schema
refusal, case-set/hash checks, legacy shape compatibility and exclusive output.
No model copies/downloads, coordinator, workload, installed app or physical node
was involved; no release or update feed changed.

Final locked/offline workspace/all-target suites pass **715 tests plus one ignored
each**, with default features and without dashboard features (EEF 244, owner CLI
17, installer 6, node library 433, node CLI 15). Fresh binaries/examples build;
their node/helper hashes still match the evaluated artifacts above. Public
`request-preview-Ece8DH` passes forwarding both alternatives and rejecting
inconsistent output from a grammar-ignoring mock provider, with no retry or
rejected-text exposure. Pinned-TLS `request-input-wIkILV` passes durable input,
clarification/answer workflows, source lineage, unchanged method generation and
backend-schema rejection without unconstrained fallback. Both are isolated
same-PC fake-inference fixtures, not real-Ollama or physical two-PC acceptance.
The 48 cross-field combinations agree with the unchanged validator in the unit
test; maximum 64-by-64-byte capability vocabulary stays within the schema budget.
Format, JS syntax, frozen scorer, argument-rejection, source boundary and diff
checks pass. Full model quality/bootstrap and release acceptance remain open.

## Qwen3 1.7B candidate (2026-10-17; no default approval)

The optional [pinned candidate](MODEL-CANDIDATE-EVALUATION.md#optional-qwen3-17b-candidate-2026-10-17-not-default-approved)
is an existing-in-test-cache 1,107,409,472-byte Q4_K_M artifact after one explicit
download in `real-model-GJWNnk`. Four production quality runs reuse it without
copies or further downloads. The committed prompt, clarification-consistent
schema, CPU-only 4,096-token context, 512-token output limit, disabled thinking
hint, frozen cases and grader stay unchanged. Separate disposable owner policies
were declared before quality evaluation:

- Production-default profile: temperature zero, optional sampling hints omitted.
- Alternate non-thinking profile: temperature 0.7, top-p 0.8, top-k 20, min-p 0,
  presence penalty 1.5 and fixed seed 1. Saved/applied hints are checked; this
  does not attest the backend's internal reasoning. Global defaults are unchanged.

| Profile | Case set | Accepted | Label matches | Literal copied goals | Evidence |
| --- | --- | --- | --- | --- | --- |
| Production default | Original v1 | 20/20 | 9/20 | 18/20 | `interpreter-quality-NLIMRP` |
| Production default | Generalization v1 | 20/20 | 7/20 | 17/20 | `interpreter-quality-jmp4ZK` |
| Alternate | Original v1 | 20/20 | 8/20 | 17/20 | `interpreter-quality-MxrvEu` |
| Alternate | Generalization v1 | 20/20 | 6/20 | 17/20 | `interpreter-quality-blwdBT` |

These results do not pass the application/default gate. All required ambiguous
cases still ask a question, but many goals/questions copy the illustration.
For example, a clear OS/memory request receives "What should be done?" and the
template goal. An ongoing camera-watch request becomes information/one-shot in
the production-default run. File-move/transfer requests are also repeatedly
misclassified as information. Valid structure and conservative review do not
make these faithful interpretations. The fixed 2B reference still matches 14/20
and 12/20, with fewer copied goals; neither model is default-approved. These are
individual development runs, not controlled statistical model/profile rankings,
independent human review, physical second-PC or general coding/chat acceptance.

Startup is 4,281–4,560 ms in these runs. Sampled owned-process peak working sets
are 2,367,696,896–2,397,163,520 bytes, not total/current RAM, VRAM, a resource quota
or a measured second-device budget. Thread count is not recorded in the quality
harness; the separate lifecycle test explicitly uses two threads. No quality run
starts a coordinator or executes the synthetic user requests.

Prompt/schema SHA-256 remain
`f9ec6ea9d7341bb7a5594959667b184485d5c9749f6a107c0228ce6866373216` /
`2660caf1caed38b74c11e266d1c79de1060ed80284d8ba270977c5b4aa84aa8b`.
All runs use node/helper hashes
`6ac2167023994530bbd04a503d6444669c8d7e19fd2b67a5fa421bd0f9befd2a` /
`50ad0854fe61967957fdcd44084dcd77f6bb8f10fc4fadf8b841b4e2a60b4137`.
Reference/case-set/schema capture checks pass in `reference-capture-qHQGJ5`,
`reference-capture-mymFkn`, `reference-capture-kS0qUO` and
`reference-capture-RPSU6Q`, without altering the source evaluations or inferring.

**No automatic default, role assignment or sampler change is approved.**
The new catalog entry is manual-only. No installed/physical node, release, UI or
update feed changed; full interpreter/bootstrap and release acceptance remain.

Both full workspace/all-target suites pass 715 tests plus one ignored each,
with default and no-dashboard features, and fresh binaries/examples build with
the same evaluated node/helper hashes. `model-metadata-KU5Yaj` passes public
inventory/selection and approved remote protocol regressions with mock Ollama,
not physical or real-Ollama model acceptance. Formatting, diff, frozen grader,
evaluation-argument and source dependency-boundary checks pass. None changes the
failed default-interpreter quality decision.

## Two-backend frozen-case baseline (2026-10-18; no default approval)

The evaluator now runs the same production preview, frozen expectations and
grader on either a verified owned CPU GGUF or an explicitly selected existing
shared Ollama model. No Rust interpreter, prompt, generation schema, scorer or
case set changed in this checkpoint. All eighty proposals below are structurally
accepted; that does not make them useful or authorize work.

| Candidate/backend | Cases | Accepted | Label matches | Unnecessary questions | Required questions lost | Exact template-goal copies |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Existing 2B GGUF / CPU | original-v1 | 20/20 | 14/20 | 6 | 0 | 12 |
| Existing 2B GGUF / CPU | generalization-v1 | 20/20 | 12/20 | 7 | 0 | 13 |
| Existing Gemma tag / Ollama | original-v1 | 20/20 | 13/20 | 7 | 0 | 0 |
| Existing Gemma tag / Ollama | generalization-v1 | 20/20 | 12/20 | 8 | 0 | 0 |

The CPU runs `interpreter-quality-XezHEU` / `interpreter-quality-QZu3r5` reproduce
all forty parsed proposals from the preceding `Bvx75l` / `iDUGub` references,
including their defects. An earlier original-suite harness regression
`interpreter-quality-6NMvfP` also reproduces all twenty baseline proposals.
The 1,280,835,840-byte cached file is verified against
`aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223`;
the existing CPU runtime hash remains
`0f706d509ce7937504d527f49cc1e68a0e45ee60cfc4153b3ff16f76a0f4e3ac`.
No copies/downloads are used. CPU inference retains its default 20-second limit.

The Ollama runs `interpreter-quality-klktwv` / `interpreter-quality-rbhOIM` use
the owner's already-installed Gemma tag on provider version 0.40.4. The provider
reports 9,608,350,718 installed bytes and digest
`c6eb396dbd5992bbe3f5cdb947e8bbc0ee413d7c17e2beaae69f5d569cf982eb`;
these are **not independently verified GGUF size/hash or pinned tag provenance**.
Both runs explicitly request a 120-second allowance, disabled-thinking hint,
context 4,096 and 60-second retention. The 512-token output cap and temperature-zero
sampling remain unchanged. The original suite starts with no resident model;
the second observes the same model resident. Preparation observations are 6,776
and 261 ms respectively, not comparable cold-start benchmarks. No CPU-only,
GPU-layer, thread, owned-process or exclusive-resource claim is made for Ollama.

Gemma asks a clarification question on all forty inputs, including all fifteen
expected-clear cases. Its goals no longer exactly copy the instructional sample,
but unnecessary questions and one additional intent error remain. The table's
label score does not assess every aspect of goal faithfulness; zero exact copies
is not general semantic approval. Both backends retain required questions in
these cases, but neither meets default interpreter/bootstrap quality acceptance.
The next interpretation change should target unnecessary clarification while
retaining required questions and checking both fixed suites, without changing
expected labels, validation, output limits or authority to produce a green score.

All four runs use node SHA-256
`6bfb37c0e536fcb7009eb93dc2db3d591ef6d3034e805b4b54e62a55cd073010`,
system-prompt SHA-256
`f9ec6ea9d7341bb7a5594959667b184485d5c9749f6a107c0228ce6866373216`
and generation-schema SHA-256
`2660caf1caed38b74c11e266d1c79de1060ed80284d8ba270977c5b4aa84aa8b`.
Reference checks `reference-capture-XTRQC8`, `reference-capture-57AoG5`,
`reference-capture-ytLmGL` and `reference-capture-EeQZkM` verify matching reference
captures, old CPU/reference shapes, mixed-profile refusal, source case/hash/schema
substitution refusal and exclusive output without overwriting evaluations.

Synthetic evaluator checks exercise all twenty fixed inputs without assuming
good semantic scores, timeout/tag/context/duplicate/unrelated-resident aborts,
null context as unknown, proxy-environment bypass, no redirects, oversized and
slow-drip metadata refusal before node/config work, and no shared-service stop,
forced unload or inference replay. Argument rejection, frozen-rubric, syntax,
diff and backend/browser source-boundary checks pass. Production Rust is unchanged
from the 735-test-plus-one-ignored checkpoint; no new application test-count or
release-completion claim follows this tool-only change.

No installed/physical node, UI, model default, shared-service setting, release or
stable update feed changed. Default model quality, automatic setup, physical
two-PC and remaining v0.4.0 acceptance gates remain open.

## Description-only prompt trial (2026-10-20; rejected)

Removing filled answer illustrations did not solve the current CPU interpreter's
semantic quality gap. A fresh original-v1 production baseline matches 14/20;
the description-only candidate matches 4/20 and loses one required question,
despite both structurally accepting 20/20 and the candidate eliminating literal
instruction-goal copies. It fails the predeclared early gate. Generalization and
shared-Ollama candidate runs are not needed to reject it.

The production prompt is restored byte-for-byte; a new run again matches 14/20
with identical prompt/messages/schema reference hashes and evaluation context.
Generation-reference, frozen-rubric and public preview checks pass. No default,
model selection, installed node or release changes. See [exact criteria, evidence
and build/reference limitations](experiments/interpreter-description-only.md).
