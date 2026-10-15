# Lightweight interpreter candidate evaluation

## Optional 2B text candidate (unreleased)

An optional `qwen3.5-2b` catalog entry pins Unsloth's text-language Q4_K_M GGUF
for Qwen3.5-2B at revision `f6d5376be1edb4d416d56da11e5397a961aca8ae`:
1,280,835,840 bytes, SHA-256
`aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223`.
The [quantizer's pinned repository metadata](https://huggingface.co/api/models/unsloth/Qwen3.5-2B-GGUF/revision/f6d5376be1edb4d416d56da11e5397a961aca8ae?blobs=true)
declares `Qwen/Qwen3.5-2B` as its base and Apache-2.0. The upstream
[pinned license](https://huggingface.co/Qwen/Qwen3.5-2B/blob/15852e8c16360a2fea060d615a32b45270f8a8fc/LICENSE)
was checked; the [model card](https://huggingface.co/Qwen/Qwen3.5-2B/blob/15852e8c16360a2fea060d615a32b45270f8a8fc/README.md)
positions the small model for research/development and prototyping. This records
publisher declarations, not legal advice or independent reproduction of the
quantization. Future redistribution still requires applicable license/notices.

This is **not an automatic default**, bootstrap approval, bundled weight or role
assignment. It is text-only in EEF's catalog: no vision projector is included and
no VLM/media capability is claimed. The pre-existing catalog order/default
behavior remains unchanged. Configured/custom catalogs remain supported.

The [Ollama alias](https://ollama.com/library/qwen3.5:2b) exists, but its manifest,
quantization, size and image components are distinct from the pinned GGUF. The
GGUF digest/bytes must never be presented as verification or a disk estimate of
that Ollama artifact. This checkpoint does not install or evaluate the Ollama
distribution or modify personal model selections.

The planned isolated evaluation uses one verified content-addressed cache file,
the existing CPU runtime and frozen production quality cases. It must record
runtime/model identity and results separately from strict schema acceptance;
successful loading or valid JSON cannot approve an interpreter default.

The initial isolated download in `.validation/real-model-J6G6W3` failed with a
premature response-body EOF. The incomplete file was removed by the existing
partial-file guard; no completed evaluation report, installation or model
selection resulted. This is retained failure evidence, not candidate acceptance.
That failure motivated the unreleased bounded byte-range recovery path before
another real download. Its local TLS fixture checks exact-offset recovery and
rejection of an ignored range; it does not prove the external transfer or model
quality. `EEF_TEST_MODEL_DOWNLOAD_TIMEOUT_MS` permits an explicit 1–60 minute
test wait (15 minutes by default), without retrying the install command.

### Completed 2B baseline evidence (not default approval)

The subsequent `.validation/real-model-wfJ5WW/results.json` passes using fresh
development binaries and the existing alpha.6 CPU runtime. The explicit owner
download command produced one exact-size/SHA-verified cache artifact, without
selecting a model as a side effect of installation. Progress samples remained on
the first transfer attempt; this successful external download is not evidence
that external byte-range recovery was exercised.

The fixture then explicitly selected/tuned the model, applied disabled thinking,
and passed private pinned-TLS reviewed inference, exact CPU thread arguments,
remote tuning refusal, unload/reload without node restart, retained results and
no completed-work replay after restart, automatic idle unload and on-demand
loading. The artifact remained on disk; personal installations/selections were
untouched. This is same-PC integration, not physical two-PC, measured RAM/VRAM
release, total resource enforcement or model-quality acceptance.

The unchanged production prompt and temperature-zero policy were evaluated on
both existing frozen case sets, reusing that one artifact without copies:

| Case set | Accepted | Label matches | Copied sample goal | Startup ms | Model peak working set bytes | Evidence |
| --- | --- | --- | --- | --- | --- | --- |
| Original v1 | 20/20 | 14/20 | 16/20 | 4,695 | 2,129,494,016 | `interpreter-quality-f5bK0p` |
| Generalization v1 | 20/20 | 12/20 | 14/20 | 4,429 | 2,130,448,384 | `interpreter-quality-bxghok` |

"Copied sample goal" counts the exact former instructional value, not all
semantic goal defects. Many clear status queries and greetings also request
unnecessary clarification. Neither valid schema output nor these label scores
establish a useful default. Working-set samples are not total system memory or
enforced quotas; quality-run thread counts were not explicitly selected.

Both reports retain system-prompt reference SHA-256
`56ec36c9a3b47393458479c216924d4d0b156aae31b80a1f5c2a1380ccd8ca5c`,
node binary SHA-256
`c52a0f9a9c6db8c638c48258eb8661e967252bc04a51b385707f1a7cd7167511`,
and the unchanged case-list hashes documented in the quality guide. These are
development cases, not independent held-out/human acceptance. The candidate
remains optional and **unapproved**. The subsequently compared field-description
prompt removed copied sample goals but regressed both scores and clarification
handling; it was rejected and the prior prompt restored. See the
[retained rejected experiment](INTERPRETER-QUALITY.md#rejected-prompt-experiment-field-descriptions-instead-of-sample-answers).

Development evidence, 2026-09-19. This does not select a default model, enable
automatic downloads, or authorize execution of model-generated instructions.

## Candidate and source

The existing `qwen2.5-small` catalog entry pins Qwen2.5-0.5B-Instruct Q4_K_M GGUF
at revision `9217f5db79a29953eb74d5343926648285ec7e67`. Its 491,400,032-byte
artifact was downloaded to the ignored development cache and verified against
catalog SHA-256 `74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db`.

The publisher's [pinned license](https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/blob/9217f5db79a29953eb74d5343926648285ec7e67/LICENSE)
and [pinned model card](https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/blob/9217f5db79a29953eb74d5343926648285ec7e67/README.md)
identify Apache-2.0. This records the upstream declaration, not legal advice,
a complete redistribution compliance review, or approval of an Ollama artifact.
Future distribution must retain applicable license/notices and record changes.

## Reproducible harness

`tools/benchmark-node-model.mjs` takes an explicit existing model, runtime and
catalog ID. It verifies size/hash before launching a hidden loopback-only CPU
server, sends only fixed synthetic prompts, bounds response bytes/time and
stops its owned process afterwards. It never downloads or selects models, calls
EEF, runs generated actions, changes installed nodes, or sends personal prompts.

```powershell
node tools/benchmark-node-model.mjs --model PATH_TO_GGUF --runtime PATH_TO_LLAMA_SERVER --catalog-id qwen2.5-small --profile baseline
node tools/benchmark-node-model.mjs --model PATH_TO_GGUF --runtime PATH_TO_LLAMA_SERVER --catalog-id qwen2.5-small --profile detailed
```

Reports are stored in ignored `.validation/model-benchmark-*` directories.
`benchmark_completed` describes harness completion, **not candidate acceptance**.
`default_approved` remains false. Do not treat exit code zero as model approval.
The Windows process peak working set is not total system RAM demand or a limit.

## Observed one-PC results

CPU: Intel Core i5-13420H, 12 logical CPUs, about 32 GiB installed RAM. Runtime:
llama.cpp build 10621 / `c1d0e7a00`, existing alpha.2 bundle; executable SHA-256
`0f706d509ce7937504d527f49cc1e68a0e45ee60cfc4153b3ff16f76a0f4e3ac`.
Both runs used 4 CPU threads, zero GPU layers, 2,048-token context, one slot,
temperature 0, seed 1 and at most 128 output tokens per request. Startup timing
is process launch to health readiness, not a controlled cold-disk measurement.

| Prompt profile | Startup | Peak working set | Valid schema | Expected classifications |
| --- | --- | --- | --- | --- |
| Baseline | 890 ms | 534,581,248 bytes | 10/12 | 4/12 |
| Detailed definitions/examples | 886 ms | 560,762,880 bytes | 12/12 | 9/12 |

Evidence: `model-benchmark-MYTDGw` and `model-benchmark-5uqQZo`. The detailed
profile's first request took 2,827 ms; later requests took 344-439 ms. Its
remaining failures were the ambiguous deletion, unspecified application target
and instruction-override cases. A well-formed schema did not make those results
semantically safe. The test strings were never executed.

## Decision and next gates

The later `--profile contract` run (`model-benchmark-gmKt3L`) used the richer
[shared Rust proposal prompt and validator](INTERPRETATION.md), with a 256-token
output limit. Startup was 881 ms and peak working set 543,375,360 bytes. Six of
twelve outputs passed the contract, but none matched both the expected intent
and clarification decision. Some merely repeated template placeholders; other
outputs had inconsistent clarification/workload fields or invented intent values.
Accepted proposals still required review and never authorized execution. This
profile is not the same schema/prompt task as the two simpler profiles above;
do not compare the counts as a controlled model-quality regression.

**Do not enable this candidate as the automatic interpreter default yet.**
The runtime is CPU-capable on this PC, but this tiny synthetic set is neither a
representative accuracy benchmark nor a security evaluation. It establishes a
repeatable baseline and a concrete failure mode, not production readiness.

Next: shared strict interpretation validation with original input preserved,
mandatory policy/clarification boundaries independent of model confidence,
broader held-out and multilingual evaluation, and a second-PC CPU/memory run.
Then compare candidate/prompt choices before recording a reviewed, replaceable
bootstrap manifest. Automatic setup must still respect skip/offline/managed
policy, existing selections, cancellation and disk limits.

## Production-node follow-up, 2026-10-11

The strict shared validator and explicit interpreter commands are implemented in
development. The new [production evaluation](INTERPRETER-QUALITY.md) uses the
actual running-node preview path and twenty fixed synthetic cases, not the
older three-field classifier or twelve-case fixture. With the original production
prompt, 9/20 proposals passed the contract and 3/20 matched semantic expectations.
An experimental example prompt performed worse (1/20 and 1/20) and was reverted.
The unchanged prompt with bounded backend generation hints accepted 10/20 and
matched 3/20. These runs are not directly comparable to the earlier profiles.

The candidate remains **unapproved for automatic default use**. More reliable
formatting does not solve semantic interpretation, faithful questions or planning.
No new model was downloaded/copied, selected in an installed node or bundled.

## Optional 1.5B candidate, 2026-10-11

The new manual catalog option `qwen2.5-1.5b` pins Qwen2.5-1.5B-Instruct Q4_K_M:

- Revision: `91cad51170dc346986eccefdc2dd33a9da36ead9`.
- File: `qwen2.5-1.5b-instruct-q4_k_m.gguf`.
- Bytes: `1117320736`.
- SHA-256: `6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e`.

The publisher's [pinned model card](https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/blob/91cad51170dc346986eccefdc2dd33a9da36ead9/README.md)
and [pinned license](https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/91cad51170dc346986eccefdc2dd33a9da36ead9/LICENSE)
declare Apache-2.0. Artifact size/digest were checked against the publisher's
Hugging Face LFS metadata, then against the downloaded bytes. This is provenance
and an upstream license declaration, not legal review or model-quality approval.
Future redistribution must retain applicable license/notices and record changes.
The mutable Ollama alias `qwen2.5:1.5b` is not the verified GGUF artifact and was
not evaluated. Owner-managed catalog/provider configuration remains available.

One explicit test-only download was admitted through public `models install-plan`
and `models install` commands, inspected by its operation ID through
`models download-status`, and found through `models installed`. The fixture
verified installation did not select any interpreter/model role. Existing
transfer disk checks, exact size/SHA verification and partial cleanup were used.
The verified artifact is retained in the ignored `real-model-9JKfMN` directory,
not copied into a bundle or personal installation. Subsequent quality runs reuse
that one artifact without downloads/copies.

`real-model-9JKfMN` passed real CPU text generation through explicit reviewed
grants, pinned TLS, node restart with a new runtime ID, retained completed output
without replay, and model withdrawal. These are isolated one-PC processes, not
physical two-PC acceptance or interpretation quality. The
[production quality results](INTERPRETER-QUALITY.md#larger-candidate-and-rejected-prompt-experiments-2026-10-11)
reject both tested value-free prompts and leave this candidate unapproved as an
automatic default. The existing first catalog entry/default behavior is unchanged;
no bootstrap, automatic role assignment or update-feed change is introduced.
