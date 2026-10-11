# Production interpreter quality evaluation (unreleased)

This evaluation invokes the real running node's `requests preview` command,
production prompt, selected backend and strict Rust validator. It is separate
from execution acceptance and the older small classifier benchmark. An accepted
proposal remains untrusted and cannot authorize work.

## Reproduction

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

The API-only fixture disables coordinator connections, uses CPU-only GGUF
inference, and sends twenty fixed synthetic inputs. It verifies unchanged saved
configuration, no downloads, no workload and zero successful connections. It
stops only its owned processes. Reports stay in ignored
`.validation/interpreter-quality-*` directories, not release artifacts.
New reports also record a `prompt_reference` hash emitted by the test-only Rust
fixture using the shared prompt function and the same capability vocabulary.
This is a reproducibility reference, not intercepted inference or remote
attestation. Build examples alongside the fresh production binaries.

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
