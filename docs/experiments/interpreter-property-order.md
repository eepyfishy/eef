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
