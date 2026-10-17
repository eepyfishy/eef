# Explicit owner inference timing (unreleased)

The owner can choose a finite per-call inference allowance without changing the
default, interpreter prompt/schema, token limits or execution permissions:

```text
eefn requests preview --text "Show this node's operating system and memory usage" --inference-timeout-seconds 120 --json
eefn requests input --text "Inspect node status" --inference-timeout-seconds 60 --json
```

The same optional flag is available on `review`, `open`, `resolve`, `prepare`
and `respond`. Values must be integers **1–120 seconds**. Omitting the flag keeps
the existing **20-second** inference deadline and existing client/workflow bounds.
This is an explicit request option, not a saved preference, global timeout
increase, startup setting, model recommendation or instruction from model output.
The compatibility chat/stdin adapters continue using the default; command APIs
let interfaces choose the option without controlling backend behavior.

## Shared backend contract

Local command endpoints for preview/review/open/input/resolve/prepare/respond
accept optional `inference_timeout_seconds` in the top-level owner request. The
field is outside model-authored proposals, method/answer commands, immutable
origin context and execution grants. Unsupported types, zero or values above
120 are rejected before model consultation or workflow mutation. Omitted/null
means default. Legacy serialized preview/answer requests omit unused timing;
no persisted review/plan/receipt identity or default policy digest changes.

One selected-model call receives that allowance, including cold loading if it
occurs during inference. The same engine constructs its backend HTTP deadline
with bounded headroom (at least the previous 30 seconds, otherwise allowance
plus ten); the outer inference deadline remains authoritative. Direct model
workloads, execution leases, process timeouts, downloads and explicit Ollama
preparation retain their separate limits. Output limits remain 512 proposal
tokens and 1,024 method tokens, with the same byte/schema/completion checks.

`input` forwards the option to opening interpretation and possible method
preparation. `respond` forwards it to resolution and possible method preparation.
The workflows still honor the applied input/answer policy, source revision,
runtime invalidation, retained receipts and independent validation. A larger
allowance never permits a new step or execution that was otherwise disallowed.

## Overall wait bounds

Only an explicit allowance above 20 adds time to existing workflow/transport
bounds: one added interval per possible model call. Shorter allowances do not
shorten unrelated coordinator-save/network margins. These are maximum waits,
not promised response times or automatic retries.

| Path | Default seconds | Maximum with 120-second calls |
| --- | ---: | ---: |
| Preview CLI | 25 | 125 |
| Review/open CLI | 150 | 250 |
| Resolve CLI | 65 | 165 |
| Prepare CLI | 70 | 170 |
| Input backend / CLI | 135 / 150 | 335 / 350 |
| Answer backend / CLI | 180 / 195 | 380 / 395 |

The original overall bounds can expire before every worst-case coordinator wait
is exhausted; the extension preserves that design rather than inventing fresh
leases per stage. Runtime retirement still interrupts immediately. Inference
timeout drops EEF's request/wait; it is **not proof that shared provider work or
external effects stopped**. No automatic reload, retry, provider switch or
unconstrained fallback follows timeout. Previously retained answer/input commands
still inspect history without reinference, even if the caller supplies a new
allowance. A timing change is not a new recovery/execution authorization.

Older strict nodes may reject the new field. Omitting it preserves old request
shapes; adapters never strip it and silently retry. After uncertain transport
outcomes inspect the original retained request/operation where applicable.

This does not implement automatic latency-aware model selection, saved timing
preferences, successful model quality, reliable first-run bootstrap, physical
two-PC acceptance or a stable v0.4.0 release. The full two-backend quality
comparison remains required. Model loading can be separated explicitly using
[Ollama preparation](OLLAMA-PREPARATION.md).

## Verification: 2026-10-17

Both locked/offline workspace/all-target suites pass **732 tests plus one ignored
each**, including no-dashboard; fresh binaries/examples build. New unit checks
cover shared bounds, all transport/workflow margins, seven CLI actions, legacy
serialization and rejection in all five backend entrypoints before runtime or
file writes. Source dependency, syntax, format and diff checks pass.

Public `request-preview-KMiBnr` accepts a deliberately **31-second** synthetic
provider response under an explicit 45-second allowance, crossing the former
20-second service, 25-second CLI and 30-second provider limits without changing
them by default. It also checks one-second cancellation of the node's wait,
unchanged default timeout, invalid-budget refusal, capacity release, runtime
invalidation and existing strict generation/redaction/no-retry regressions.

Public pinned-TLS `request-input-IsehrU` checks one-second limits during initial
interpretation, input method generation, answer resolution, answer method
generation and standalone resolve/prepare. Partial source/answer revisions stay
retained, and repeating input/answer receipts with 120 seconds performs only
historical inspection—never another model call. Existing approval/source-lineage,
lost-reply, restart and no-execution regressions pass. Observer admission guards
and synthetic preparation/preview tests pass too. These fixtures use fake
providers on one PC, not physical two-PC or real-model quality acceptance.

The separate opt-in real run `real-ollama-context-NRcbqM` requests **120 seconds**
explicitly in a new isolated experiment, not as an automatic retry or a new
default. Existing Ollama 0.40.4 / Qwen3-VL
preparation completes in **5,823 ms** with the same provider-reported digest
`901cae73216286ea8c5aba8b46d307ff7188f737285ec500c795a12f05225d28`
and resident context 4,096. The following one preview returns after **39,689 ms**,
past the previous timeouts, but **fails** as `interpretation_incomplete`.

The narrowly inspected provider log records 512 generated tokens at about 13.05
tokens/second, matching the unchanged proposal cap, and HTTP 200. The public
failure report does not retain the raw provider finish reason; the log alone
does not prove whether thinking, repetitive output or another model behavior
consumed that allowance. There is no accepted proposal, semantic quality score
or default approval. No text repair, token-limit increase, prompt/schema change,
fallback or automatic retry follows the failure. The earlier 20-second failed
runs are retained unchanged, rather than reclassified as successes.

The tested node executable SHA-256 is
`434ca404fd321425474988bcef0617cbd710f1a95a2c66946ff8b61fcc570ad7`.
Saved scratch configuration is unchanged by preview. No model downloads/copies,
installed app/physical node updates, shared-service changes, forced unloading,
dashboard changes, release or update-feed changes occurred. Reliable interpreter
completion/quality remains an open gate; explicit timing removes a fixed wait
restriction, not that gate.
