# Conditional local stages

This increment adds explicit data conditions to a finite graph. It is a
prerequisite for local event gating, not continuous capture, ambient listening,
an automatic detector, a heavy planner or new execution authority. Local public
commands and full regression pass. Published `25475b3` also passes
[all four hosted jobs](https://github.com/eepyfishy/eef/actions/runs/40599793148);
these results do not establish continuous or physical workload acceptance.

`conditional_pipeline` is an explicit concrete input template, separate from
`local_pipeline`. Its single `pipeline.when / run` parent contains:

```json
{
  "schema_version": 1,
  "graph": {
    "schema_version": 1,
    "max_retained_bytes": 2048,
    "steps": [
      {
        "id": "sample",
        "capability": "filesystem",
        "action": "read",
        "params": { "path": "approved-status.txt" }
      },
      {
        "id": "analyze",
        "capability": "llm.infer",
        "action": "run",
        "model_id": "YOUR_SELECTED_MODEL",
        "backend": "ollama",
        "params": { "prompt": "Run the explicitly approved check.", "max_tokens": 16 }
      }
    ]
  },
  "conditions": {
    "analyze": {
      "source_step": "sample",
      "pointer": "/content",
      "operator": "equal",
      "expected": "ready"
    }
  }
}
```

The file path and model above are owner-chosen example values, not defaults.
An actual submission wraps this definition in the existing versioned concrete
input with its title, original text and explicit executor constraints. It uses
the normal running-node submission, separate owner approval and execution
commands. Neither a model nor a dashboard compiles or authorizes this graph.

## Condition semantics

Conditions name an existing target step and an earlier source step. Bounded RFC
6901 pointers select source data only. Operators are `equal`, `greater_than` and
`less_than`; ordered comparisons require numbers. Expected values are bounded
booleans, finite numbers or text. Strings compare exactly, including whitespace.
Integer comparisons stay exact; mixed comparisons that would lose integer
precision fail rather than silently rounding.

A false condition skips that operation. Data or guard dependencies on an already
skipped stage propagate a dependency skip. This is not an implicit block around
later steps: an unrelated, explicitly unconditional step can still run. Missing,
null, wrong-type, oversized or unavailable guard data is uncertainty and stops
execution; it is not false, truthiness or permission to choose another operation.
A known unsuccessful native stage still stops the entire graph, including later
unconditional stages.

## Authority and data bounds

The existing compiler binds the whole graph to one current disclosed executor.
All possible stage capabilities, model/backend/role and resource identities must
be disclosed, approved, available and locally permitted before dispatch, even
when a condition may skip them. Scheduling reserves the full static provider
union atomically. Conditions never select another model, executor, program,
path, URL, permission or graph. Source identity and its conditions are immutable.

One original job and consumed grant own the execution, clock checks, deadlines,
permission watching, result checkpoint and replay protection. The existing
30-second grant maximum is unchanged. `pipeline.when` is not ordinary RPC and
cannot be nested as a normal child operation. Older nodes without its exact
advertisement are not substituted or retried through `pipeline.run`.

Guard reads join the same last-use result accounting as data inputs; their
values are released after the final declared use. The finite graph's 16-step,
64 KiB definition and local encoded-data limits remain. The parent result budget
must also reserve 4 KiB for bounded outcome metadata. This is encoded-data
accounting, not proof of a physical RAM, CPU or GPU quota. No new local cache,
model selection, permission default, retry, continuation or lease renewal is
introduced.

## Results and remaining scope

Final receipts contain ordered `completed`, `skipped_condition`,
`skipped_dependency` or known-native `failed` outcomes. They record a guard match
or skipped dependency ID, not the sampled value. A skipped final stage returns
null output, not private upstream data. A successful parent means the declared
finite workflow finished, which can include intentional skips; it does not mean
every stage ran. Intermediate model text stays local unless the owner explicitly
makes it the final output, subject to existing data limits.

The parent is durably accepted before effects and its final result uses the
original receipt. Conditional stages do not yet have their own durable live
progress observations; they never fabricate completed stages in the legacy
finite-progress schema. A crash, lost checkpoint, failed lease or stop remains
unknown under existing rules. Recorded skips are not stop acknowledgements,
outside-effect proof or rollback. Historical inspection cannot rearm the work.

Twelve added focused tests, both full local modes (1,229 passed plus one existing
ignored test each), all 38 native cases, cached CPU mechanics and separate
default-build browser checks pass. Independent read-only audits correlate four
conditional outcomes with their exact single-parent grants and retained reports.
The public command fixture proves false skips without model/sink effects, true
execution once, missing-data uncertainty, known native failure, immutable source
conditions and restart history without replay. The exact hosted checkpoint passes
32 Windows native suites, separate connection-close and firewall mock gates;
source, compiled-input, binary, runtime/model and retained-journal audits pass.
Continuous sessions, listening controls, light detector
adapters, event transport/backpressure, strong stop/resource budgets, physical
two-PC, approved model onboarding and installer/signing/Defender release gates
remain open. Visual dashboard development remains paused.
