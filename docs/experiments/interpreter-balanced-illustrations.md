# Rejected balanced-illustration experiment (2026-10-18)

The committed baseline (`c0151b3`) illustrates only an unknown proposal with
clarification enabled. Both current models over-question clear inputs. This
candidate replaces that object with two complete, equally explicit illustrations:
a clear general-knowledge explanation and an underspecified recurring action.
They are not frozen test inputs or actual request history. It also distinguishes
optional presentation choices and permission review from essential missing details.
The application-context instructions, separate actual user message, generation
schema, independent validator, capability vocabulary, bounds and authority remain
unchanged. This differs from the previously rejected appended examples: it removes
the ambiguous-only shape instead of leaving that shape as the primary template,
and illustrates both finite and continuing work.

## Criteria declared before inference

Compare both frozen suites on both existing backends against the recorded
2026-10-18 baseline, using the same artifact/provider identity, context 4,096,
temperature zero, disabled-thinking hint and 512-token cap. CPU keeps its default
20-second inference wait; shared Ollama keeps the explicit 120-second allowance.
No models are downloaded/copied and no installed node or coordinator is changed.

Retain only if all four runs accept 20/20 proposals and no label total decreases
(CPU 14/20 and 12/20; shared Ollama 13/20 and 12/20). No required question may be
lost and no ongoing request may be reduced to one-shot. Each backend must reduce
unnecessary questions by at least two across its forty cases. CPU literal
instruction/example goal copies must fall to at most half of its baseline 25/40;
shared baseline has none and must retain that property. Inspect all resulting
synthetic goals/questions for missing purpose, invented targets/state and copied
illustration answers; label totals alone are insufficient. Any failure rejects
the candidate rather than relaxing cases, grading, validation, authority or caps.

These are development cases already inspected during earlier work, not held-out
acceptance or independent language review. Even an improved candidate does not
approve a default model, bootstrap, physical nodes or stable release.

## Result: reject and restore the committed prompt

All eighty proposals passed the unchanged structural validator, but label scores
fell and necessary questions were lost. No case was executed or automatically
retried. Zero unnecessary questions is not an improvement worth retaining when
essential missing details are silently assumed.

| Backend | Suite | Baseline matches | Candidate matches | Required questions lost | Unnecessary questions | Finite requests labeled continuous | Evidence |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| CPU 2B GGUF | original-v1 | 14/20 | 7/20 | 4 | 0 | 8 | `interpreter-quality-l8E1Br` |
| CPU 2B GGUF | generalization-v1 | 12/20 | 6/20 | 1 | 0 | 8 | `interpreter-quality-kZ9bEG` |
| Existing Gemma / Ollama | original-v1 | 13/20 | 11/20 | 6 | 0 | 1 | `interpreter-quality-C5eBYp` |
| Existing Gemma / Ollama | generalization-v1 | 12/20 | 12/20 | 7 | 0 | 0 | `interpreter-quality-gJMINW` |

No exact baseline/example goal string was copied and no ongoing request became
one-shot. Neither property rescues the result. Review of all synthetic goals and
questions additionally found the CPU Thai delete question asking which of the
two illustration inputs should be deleted. Its English delete goal also guessed
that the missing object was a node/device. Shared Ollama treated an unspecified
job as the currently running job without supplied history. These demonstrate
lost grounding, not execution or permission grants. Capability mismatches use
the unchanged conservative rubric; for example, suggesting only `system.ping`
does not meet a case requiring `system.info`. No expectations were revised to
increase scores.

The candidate passed 12 focused Rust contract tests and built the production
node/reference helper before inference. Exact shared-reference, case-substitution,
corrupted-hash and exclusive-write checks pass for each run in respectively
`reference-capture-nCXsR5`, `reference-capture-IA6WLc`,
`reference-capture-yqipxy`, and `reference-capture-EM3og9`.

All four runs use candidate node SHA-256
`9416d5dea0323b412c1a55034d1a0e6fb84b0760ac88e90e416a3f05ec623135`
and reference helper
`67c3501a30d392e66c664708134ce844563bb2df7d2db0c6630b2cc0039f0fab`.
System prompt SHA is
`67cff6212fb110c4e468f01d3680fc09eb76c12f89747314265f8fc7ee02d377`;
generation schema remains
`2660caf1caed38b74c11e266d1c79de1060ed80284d8ba270977c5b4aa84aa8b`.
The [exact rejected system text](interpreter-balanced-illustrations.txt) uses the
six-capability evaluation vocabulary; hash its UTF-8 content without the trailing
newline. It is an archival experiment, not a runtime prompt/configuration option.
Full case-specific generation references and replies remain in ignored validation
storage, not release artifacts or model attestations.

The CPU file/runtime and provider digest are the same as the
[baseline](../INTERPRETER-QUALITY.md#two-backend-frozen-case-baseline-2026-10-18-no-default-approval).
Shared preparations take 6,739 ms and 272 ms; these do not establish CPU-only
performance or quality. No downloads/copies, shared-service stop/forced unload,
installed-node updates, release or stable-feed changes are involved.

The candidate helper, instructions and candidate-only unit test were removed;
production source returns exactly to `c0151b3`. Restoration verification is
recorded in the linked main quality tracker: 11 focused tests, fresh builds, exact
baseline message/schema equality, archived-candidate equality and the public
`request-preview-FUD6HR` regression pass. No full Rust-suite rerun is claimed.
Next work must improve grounding
without trading away essential questions; neither an ambiguous-only shape nor
this balanced replacement establishes a reliable default interpreter. Repeated
prompt additions alone are not an acceptance strategy.
