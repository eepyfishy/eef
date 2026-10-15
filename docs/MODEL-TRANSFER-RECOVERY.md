# Bounded model-transfer recovery (unreleased)

Local GGUF downloads now recover a broken HTTPS response inside the one
owner-approved install operation. Previously a premature response EOF discarded
all received bytes and failed that operation. This was observed in
`.validation/real-model-J6G6W3` while evaluating an optional larger interpreter;
the partial was cleaned up and no model was installed or selected.

## Integrity and operation boundary

- At most eight GET attempts share a one-hour total transfer budget. Neither a
  lost command reply nor a node restart resubmits the install command.
- Received bytes, the running hash and the offset live only in this operation.
  A recovery request uses `Range: bytes=OFFSET-` and requests identity encoding.
- Before appending, the response must be an unencoded `206` whose single
  `Content-Range` starts at that exact offset and ends at the pinned final byte,
  with the pinned total. A supplied content length must equal the remaining
  bytes. Ignored ranges (`200`), mismatched ranges and unsupported responses fail
  closed; the downloader never appends a duplicate full prefix.
- Every chunk remains subject to exact-size, current disk-space and cancellation
  checks. The final complete SHA-256 must match the catalog before the existing
  flush/sync/rename/index publication path can mark the model installed.
- Progress adds `transfer_attempt` (one-based) and `recovering_range` phase.
  Cancellation/failure still removes the owned partial. There is no trusted
  cross-restart partial cache, install retry, model selection or backend change.
  Ollama pulls retain their existing semantics.

Pinned HTTPS metadata and final full-file verification are integrity checks,
not independent reproduction of model weights or approval of interpreter
quality. A source without range support can serve the initial full response,
but interrupted recovery from it fails explicitly rather than silently restarting
or accepting an unverifiable prefix.

## Verified scope

- Both locked/offline workspace suites pass: **700 passed, one ignored each**,
  with default features and with `--no-default-features`. Fresh workspace binaries
  and examples build; formatting, diff whitespace and harness syntax checks pass.
- Header tests reject ignored, malformed, mismatched, unknown-total and encoded
  ranges. A real local TLS fixture trusts only its generated localhost root and
  deliberately truncates a 65,536-byte response after 3,333 bytes. The next request
  starts at byte 3,333; a correct response produces the exact complete file/hash,
  while a full `200` response is refused without appending its body.
- `.validation/model-downloads-QdQQyb/results.json` passes public command/API
  regressions for progress, exact cancellation, retained receipts after lost
  replies, no automatic install retry, no selection change and backend preflight.
  That fixture uses mock Ollama streams, not real Ollama weights or a physical
  second PC.

These checks do not establish successful external large-artifact recovery,
physical two-PC acceptance, default interpreter quality or release readiness.
The optional candidate and subsequent external results are tracked separately in
[candidate evaluation](MODEL-CANDIDATE-EVALUATION.md).
