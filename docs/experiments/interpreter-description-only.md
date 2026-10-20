# Description-only interpreter prompt experiment (2026-10-20)

Status: rejected; production prompt restored. No default/model/setup approval.

The current production prompt presents a filled unknown proposal with a sample
goal and question. The recorded CPU baseline copies that instructional goal in
25 of forty cases, while both tested backends ask unnecessary questions. The
rejected balanced-illustration experiment replaced that object with two filled
answers and introduced missing questions and continuous-workload mistakes.

This candidate removes filled proposal examples entirely. It describes the same
required fields and decision rules in prose, without an example goal, question,
intent/workload combination or JSON answer. The separate actual user message,
supplied capability vocabulary, generation schema, validator, input/output bounds,
sampling, thinking hint, context, token cap and authorization rules stay unchanged.
It does not inject known evaluation inputs/answers or rewrite model output.
The candidate text is retained in interpreter-description-only.txt; capability
vocabulary is supplied separately by the existing production formatter.

## Criteria declared before candidate inference

Reuse the exact existing 2B CPU GGUF and llama.cpp runtime; do not download or copy
weights. Run the current prompt's original-v1 CPU baseline first to check the
current native build and resource conditions. Compare the candidate using both
frozen suites and, only if the CPU gates pass, the existing selected shared
Ollama model if its current identity/residency checks permit the test. Do not
preempt a different resident model or change installed selections/defaults.

All four candidate runs must accept 20/20 structurally valid proposals. No label
total may fall below the recorded baseline (CPU 14/20 and 12/20; Ollama 13/20 and
12/20), or below a higher fresh same-backend baseline if one is observed. No
required question may be lost; no ongoing request may become one-shot. Each
backend must remove at least two unnecessary questions across its forty cases.
CPU instruction/example goal copies must fall to at most twelve across forty;
the shared baseline has none and must retain that property. Manually inspect all
candidate synthetic goals/questions, including copies of new prose definitions,
missing purposes, invented targets/state and unjustified clarification. Label
scores alone cannot establish semantic improvement.

Keep temperature zero, context 4,096, disabled-thinking hint and 512 output tokens.
CPU keeps its existing 20-second inference wait; shared Ollama retains the
explicit 120-second allowance. No retries, source-input edits, rubric/case edits,
output repair, permission changes or automatic approval/execution are permitted.
An observed gate failure on any completed suite rejects the candidate; additional
backends need not be exercised merely to repeat an already decisive failure.

Even passing these inspected development cases would not approve a default
model, complete automatic bootstrap, prove physical two-PC latency or establish
stable v0.4.0 readiness. A failed experiment must restore the production prompt
and retain the failure evidence rather than weakening acceptance.

## Result: reject at the first CPU suite

The fresh production original-v1 run interpreter-quality-KmZw8R accepts 20/20
proposals and reproduces the recorded 14/20 label matches. The candidate run
interpreter-quality-qCAAG3 also accepts 20/20, but matches only 4/20. It loses the
required question for an underspecified destructive request and classifies
eighteen inputs as information, including intended actions. No proposal is
approved or executed.

| Original-v1 observation | Fresh production | Candidate |
| --- | ---: | ---: |
| Structurally accepted | 20/20 | 20/20 |
| Frozen label matches | 14/20 | 4/20 |
| Required questions lost | 0 | 1 |
| Unnecessary questions | 6 | 3 |
| Literal instruction-goal copies | 12 | 0 |

The literal-copy count checks normalized verbatim system-text substrings of at
least ten characters, excluding text present in the actual input. It is not a
semantic faithfulness score. Fewer copied goals and unnecessary questions do not
compensate for lost safety questions and misclassified purposes. The predeclared
early rejection gate is decisive, so generalization and shared-Ollama candidate
runs are not performed. No installed provider/model or shared residency changes.

Both runs use the existing 1,280,835,840-byte 2B artifact and the same pinned
CPU runtime, SHA-256
0f706d509ce7937504d527f49cc1e68a0e45ee60cfc4153b3ff16f76a0f4e3ac.
Their generation schema hash is identical; the rubric, source inputs, context,
thinking hint, output cap and CPU inference wait are unchanged. The candidate
uses a standalone node/example build; production restoration uses the full
workspace build. Matching executable hashes across rebuilds are not asserted:
this Windows toolchain inserts fresh PE timestamps. Prompt/schema references and
source restoration are checked separately from binary identity.

The production source is restored byte-for-byte, without changing cases,
validators, limits or model policy. This rejects this candidate, not every
description-only prompt or every model. It does not establish which individual
instruction caused an error. Automatic default selection/bootstrap and the
physical/stable-release gates remain unfinished.

## Restoration verification

The full-workspace headless rebuild restores the committed production source
byte-for-byte. A fresh original-v1 run interpreter-quality-P3W8pL again accepts
20/20 and matches 14/20. Original and restored system/messages/schema reference
hashes, artifact/runtime/rubric identity, context, thinking hint and output cap
match. Fresh executable/helper hashes are retained with that new run rather than
substituted into older evaluations. The reference-capture-wzl3Ow tests pass
against the matching rebuilt helper, including wrong/substituted context refusal.
Frozen rubric checks, request-preview-ZUvdrX, format, source-boundary and diff
checks pass. Test-owned nodes and CPU runtime are terminal.

No production source, installed node, model selection, weight, provider setting,
permission, UI, version, installer or updater policy changes survive the trial.
Only this experiment and its outcome are documented; the previous full-workspace
and native acceptance checkpoint is not represented as rerun here.
