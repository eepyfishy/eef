# Lightweight interpreter candidate evaluation

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
