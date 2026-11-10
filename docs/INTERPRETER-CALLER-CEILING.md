# Caller timing for network interpretation

This unreleased follow-up awaits its own hosted validation. It does not enable
automatic normal-input delegation or complete interpreter recovery or the
stable-v0.4 release gates.

Network interpreter commands can specify `maximum_execution_ms`, a positive
ceiling no larger than the existing grant maximum. The field is optional: an
omitted value retains service-controlled limits and older serialized commands
remain readable. It is a restriction, not an authorization or requested lease
extension.

The actual reservation uses the shortest of the caller ceiling, owner service
policy, node grant policy, and remaining policy validity. Both the grant expiry
and original monotonic continuation deadline use that same duration. Persistent
worst-case execution charges use the issued duration, without refunds or another
reservation on history inspection. No journal schema or task/prompt definition
changes are required; the exact existing grant already pins its own limits.

`eefn requests interpret-network` now accepts the existing
`--inference-timeout-seconds` option. Its default remains 20 seconds. The service
or grant protocol may impose a shorter limit, including the existing 30-second
hard maximum for these grants. The shared budget conversion leaves local model
timeouts and backend headroom unchanged.

The attempt summary includes its issued `execution_limit_ms`. A client asking
for a ceiling cannot confirm a reply missing that limit or reporting a larger
one. Reusing a retained attempt under a shorter-than-issued ceiling is a
conflict, never a reason to issue another grant, rewrite its limits or restore
an expired continuation. Selection, scope, credential/session, role, sharing
permission, aggregate/concurrency, source and completion checks still apply.

A deadline does not prove a shared Ollama process stopped computing. Unknown
outcomes retain their existing concurrency and recovery semantics; stop and
physical-effects claims are unchanged. No automatic retry or replacement
provider is introduced.

New hosted regressions cover invalid ceilings without mutation, exact issued
duration/expiry and charges, retained-history refusal without replacement, and
an actual private node/model-service exchange with a caller-limited grant and
read-only replay. Existing timing/parser tests cover conversion of ordinary
caller budgets. No local model, app or compilation is used for validation.

This closes a prerequisite for using the network service from ordinary input,
clarification and preparation. Those flows still need explicit integration,
honest remote attribution, local-only/privacy controls, shared admission and
their own complete end-to-end validation.
