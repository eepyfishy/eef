# Interpreter rejection diagnostics (unreleased)

Node commands retain `error_code: "interpretation_invalid"` for rejected model
proposals. When the local validator identifies the cause, replies additionally
include a bounded `rejection_reason` and a static, readable explanation. The
existing CLI displays these without requiring a browser, port number or JSON edit.
No model text, parser excerpt, rejected field name/value, model identifier or
request contents are included in the new diagnostic. Existing node/runtime and
request identity fields retain their normal meanings.

For example, an inconsistent proposal can produce these fields:

```json
{
  "success": false,
  "error_code": "interpretation_invalid",
  "rejection_reason": "clarification_required",
  "execution_authorized": false,
  "dispatched": false,
  "automatically_retried": false
}
```

The explanation says that uncertainty and clarification fields are inconsistent;
unknown intent/workload requires a question. A setup hint points to explicit
commands and model inspection, without choosing, downloading or changing a model.
This is a failed proposal, not permission to repair/retry it or execute anything.

## Categories and scope

| `rejection_reason` | Local validation category |
| --- | --- |
| `input_invalid` | Empty/oversized input or unsupported control characters |
| `context_invalid` | Invalid capability vocabulary |
| `output_too_large` | Proposal exceeds the existing 16 KiB output limit |
| `schema_invalid` | Invalid JSON, duplicate/extra fields, types or enum values |
| `version_unsupported` | Unsupported proposal schema version |
| `goal_invalid` | Goal fails the existing text/byte limits |
| `hints_invalid` | Invalid/excessive context hints or constraints |
| `capabilities_invalid` | Excessive, duplicate or undisclosed capability suggestions |
| `clarification_invalid` | Question fails the existing text/byte limits |
| `clarification_required` | Inconsistent question/uncertainty fields |

These categories come from typed validator errors, not matching parser/error
strings or trusting a model-authored reason. The validator still uses exactly
the previous checks and order; a reason identifies the first failing check, not
every defect in the proposal. Owner preview and network-backed review/open share
the same mapping; answer resolution also retains it before any checkpoint save.
The existing shared interpreter response-size guard can identify oversized method
preparation output, but this does not add detailed method-schema diagnostics.

Earlier input/context prechecks keep their existing error codes. Missing or
ambiguous interpreter roles, unavailable backends, incomplete responses, timeouts,
runtime changes and unclassified failures are not mislabelled as validator
categories. An older peer or generic failure without `rejection_reason` remains
unclassified. No successful report receives the field.

Structurally valid but unfaithful goals, wrong intent/workload labels or irrelevant
yet advertised capability names can still pass validation. Absence of a rejection
is **not model-quality approval**. All accepted proposals remain untrusted and
review-only; the retained [quality failures](INTERPRETER-QUALITY.md) and default
bootstrap/physical acceptance requirements remain open. Historical failures with
only the old coarse error cannot be retrospectively classified from those reports.

## Verification boundary

Unit checks exercise all ten categories, static engine-code round trips, fallback
for unrelated errors, non-reflection of rejected content, unchanged authority
flags and omission on successful/unclassified reports.

`.validation/request-preview-cEsTUs/results.json` passes the expanded API-only
CLI fixture with real node processes and mock Ollama responses. It checks eight
output categories, the retained broad error code, no private marker reflection,
one inference attempt per rejected proposal, unchanged configuration, no result,
no execution, existing capacity/deadline/restart guards and legacy compatibility.
It is not real-model inference, physical two-PC, default-model or release approval.

`.validation/request-review-QXcbO7/results.json` passes the expanded durable
review/resolution fixture. Invalid reviewed and answer-resolved proposals retain
the broad error code, carry `schema_invalid`, and do not reflect the marker text.
The failed resolution leaves the prior draft unchanged with no checkpoint save.
The larger fixture also tests separately owner-approved jobs, pinned-TLS logical
peers, source/restart/replay guards and result recovery; those operations are not
execution caused by rejection diagnostics. All processes/data are isolated on
this PC, with fake inference rather than physical two-PC/model acceptance.

Strengthened final reruns also retain the original forged `permissions` field
alongside the private marker: `request-preview-8g8vs1` and
`request-review-FTmMPq` both pass. This preserves the authority-field rejection
regression while checking diagnostic redaction; the latter process exits cleanly
after its existing restart/peer/workflow acceptance checks.

Both locked/offline workspace/all-target suites pass **703 tests plus one ignored
each**, with default features and `--no-default-features` (eef 243, owner CLI 17,
installer 6, node library 422, node CLI 15). Fresh binaries/examples build; focused
rejection tests, JS syntax, source dependency, frozen scorer, formatting and diff
checks pass. The rebuilt reference retains system-prompt SHA-256
`f9ec6ea9d7341bb7a5594959667b184485d5c9749f6a107c0228ce6866373216`.
No prompt, model selection, inference preference, installed app, release or update
feed changes accompany this increment. Existing semantic/default-bootstrap and
physical/release acceptance gaps are not closed by more precise rejection labels.
