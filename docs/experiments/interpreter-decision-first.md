# Decision-first interpreter feasibility screen

Declared before inference, 2026-10-22. Production baseline: `02d9947`.

The current single response mixes classification, free-form goal text and a
clarification question. Repeated prompt-only experiments lost necessary questions
without fixing semantic quality. Screen a different method before changing the
production interpreter: ask for only intent, workload, ambiguity and capability
hints, with no filled answer illustration or prose-generation fields. The small
schema serializes intent/workload before clarification, unlike the production
schema's alphabetically sorted properties. This screen changes both factor size
and generation order; it cannot identify their individual causal effects.

The screen reuses both frozen twenty-case development suites, unchanged labels
and capability requirements, and the existing verified 2B GGUF/runtime. It starts
an isolated CPU-only server: four threads, 4,096 context tokens, temperature zero,
disabled-thinking hint, a requested 128-output-token limit and twenty-second call
wait. Only one stop-completed, bounded, strictly parsed decision is accepted;
duplicate/unknown keys, malformed data and reported token overruns are rejected.
No downloads/copies, Ollama changes, installed-node changes, coordinator or
workload execution. Reports retain exact model/runtime, prompt/schema/case hashes,
all decisions and timing, without personal inputs.

Before inference, require at least 18/20 label matches **in each suite**, all forty
strictly valid decisions, no lost required question and no continuous-to-one-shot
regression. Do not change expectations or count missing/rejected responses as
matches. The baseline references must match the catalog artifact/runtime and
frozen cases. A baseline match is a comparison, not a default approval.

Passing this screen would justify a production experiment, not ship the method.
That experiment must generate a faithful full proposal and clarification question,
retain the independent Rust validator and all authorization boundaries, and fit
within the existing whole-request deadline and total 512 output-token budget
(128 decision plus at most 384 proposal tokens). No fallback/retry or deterministic
label rewrite may make a failed decision pass. Original text stays immutable.
Required question quality and invented goals/targets need separate review.

This is direct-backend diagnostic evidence, **not** the public node preview path,
held-out/human acceptance, a usable default, bootstrap or stable release. Keep the
production interpreter unchanged unless the full production experiment passes.

## Initial harness failure, not production acceptance

`interpreter-decisions-mNWsr6` used only the thinking template hint, omitting
production's additional `reasoning_effort=none`, and used the alternate OpenAI
schema wrapper. Its flat schema also did not express the validator's existing
unknown-label/clarification relationship. These are uncontrolled differences,
not evidence for shipping the method. Some real returned decisions were wrong,
including information/unknown/false for a clear node-health request and missed
required questions. Missing thinking controls have not been proven their cause.

The identified owned runtime was deliberately stopped after the mismatch was
found. The initial harness then incorrectly counted transport failures after
retirement as processed cases and reported completion. That completion claim is
invalid; its 4/20 and 0/20 totals are not a complete comparable quality run. Its
unchanged report, runtime log, first-run log and evaluated runner are retained.
Do not rewrite or promote them to acceptance.

The corrected screen uses the production llama.cpp schema wrapper and both
thinking controls. Its schema now encodes the same cross-field consistency
already enforced by the independent decision validator, without changing labels,
criteria or that validator. Runtime retirement or a backend request failure stops
the harness without replay; exact evaluated source is retained before inference.
This is a revised candidate schema as well as a harness correction, not a clean
causal comparison to the first attempt.

## Result: reject the reduced decision-first method

The corrected owned-CPU run `interpreter-decisions-CALkYm` completed without
backend failures. All forty decisions passed its strict structural validator.
It **fails** the declared screen bar:

| Frozen suite | Production baseline labels | Decision-only matches | Required clarification missed | Continuous reduced to one-shot |
| --- | ---: | ---: | ---: | ---: |
| Original v1 | 14/20 | 12/20 | 5 | 0 |
| Generalization v1 | 12/20 | 11/20 | 6 | 0 |

The original baseline is a full public-node proposal; the new run is a reduced
direct-backend decision screen. These are development comparisons, not a controlled
ranking or a claim that changed token budget, schema/order and threading have no
effect. The earlier production harness did not record thread count. The screen
used four CPU threads, temperature zero, seed 1 and startup took 6,544 ms.

Observed defects include asking for clarification about clear node-health
questions, treating unresolved send/delete/camera references as clear, and
suggesting `node.update` for a folder rename or an unspecified deletion. The
frozen label rubric does not comprehensively score capability appropriateness.
No actual goal or clarification prose was generated; omitting those fields is
not evidence that faithful full proposals would improve.

Model SHA: `aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223`.
Runtime SHA: `0f706d509ce7937504d527f49cc1e68a0e45ee60cfc4153b3ff16f76a0f4e3ac`.
Prompt SHA: `06d74acf5a62c1204a1089f4b35b15a654ecdc77ae860bcaec2e070a1b6faa84`.
Schema SHA: `82e1f84d5b6e379f4b0a86b668f4c4790b0675360ee40f0888cb7635f0a25dd4`.
Evaluated runner SHA: `f3ea84ccf2a264f7a8d61949e012abc827b3e515998fea4d6eadaa151bcd6b11`.
The exact runner/reference and results are retained under that fixture; the
execution log is `.validation/interpreter-decision-screen-second.log`. The
earlier noncomparable/interrupted run remains separately retained in
`interpreter-decisions-mNWsr6` and `interpreter-decision-screen-first.log`.

The final evaluator factors the loop into a regression-testable helper and adds
explicit complete-case/identity checks. It was not the exact evaluated runner;
the unchanged prompt/schema and all forty decisions can be independently regraded
with `tools/test-interpreter-decision-evidence.mjs FIXTURE_DIRECTORY`. This does
not replay inference or attest backend behavior. Metadata tests cover strict
parsing, duplicate/unknown fields, no retry, absent/overrun token counts,
incomplete responses, backend failure, owner retirement before/after a response,
missing/substituted/duplicate suites and failed screen gates. Evidence regressions
also refuse altered completion/approval flags, identities, source/schema references,
inputs, scores and truncated or changed recorded decisions. Failures expose only
a static diagnostic, not private model text. Those tests are not model-quality
acceptance. The initial noncomparable report is rejected by the final verifier.

Metadata/lifetime, frozen-rubric, evaluation argument-admission, JavaScript syntax,
backend/browser source-boundary and diff checks pass. The unchanged production
preview regression passes in `request-preview-6k0RGu`; it uses fake inference,
not a fresh real-model acceptance run. Product sources and the preceding frozen
app/helper hashes are unchanged, so the previous 1,029-plus-one-ignored in each
feature mode and twenty-two-native checkpoint remains the product baseline;
full Cargo/native acceptance is **not rerun** for this tools/documentation-only
increment. Owned model/preview fixtures are terminal. No model/source reference,
score or case expectation was loosened to keep a candidate.

No two-stage implementation is integrated and no production prompt/schema,
validator, inference budget, model selection/default or bootstrap behavior changes.
Do not spend the saved proposal budget or loosen clarification to ship this
candidate. Future semantic work needs stronger model/method/grounding evidence;
repeat prompt additions alone have repeatedly failed. Dependency/runtime health,
physical two-PC and installer/signing/Defender/release gates also remain open.

## Developer reproduction

Run `node tools/test-interpreter-decisions.mjs` before inference. The opt-in runner
is `tools/interpreter-decision-screen.mjs`; it requires all six explicit options:
`--model`, `--catalog-id`, `--runtime`, `--runtime-sha256`, `--baseline-original`,
`--baseline-generalization`. Supply existing pinned local artifacts and the two
matching public production-baseline reports. No option downloads/copies models.
Reference inputs in this run were `interpreter-quality-Bvx75l/results.json` and
`interpreter-quality-iDUGub/results.json`. Routine users do not need these tools.

CI remains local; no version/release or update-feed change.
