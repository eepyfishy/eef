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
  Ordinary cancellation/failure still removes the owned partial. An in-process
  node-runtime restart leaves the same service-owned transfer alive, with its
  existing receipt; there is no repeated install or new transfer budget. Full
  app replacement loses that in-memory receipt and never automatically resumes
  or replays the transfer. A crash/forced termination can leave an abandoned
  partial on disk; it is never trusted or resumed. Automatic orphan reclamation
  is not implemented. Ollama pulls retain their existing semantics.

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
- The expanded `.validation/model-downloads-waH4bW/results.json` also passes
  an actual public restart command during a mock pull: runtime changes, owner
  process remains, transfer ID/progress survive and exactly one provider request
  was issued. Exact cancellation still works afterwards. Forced replacement of
  the isolated owner during another mock pull starts idle, loses the old receipt
  and does not repeat the provider request. These are service-lifetime checks,
  not proof of GGUF cleanup on a crash or durable transfer recovery. The first
  expanded fixture (`model-downloads-y1Ujvv`) failed a wrongly placed expected
  request-count assertion; moving the process-replacement scenario after the
  existing completion scenario corrected the harness ordering, not production.

The subsequent `.validation/real-model-wfJ5WW/results.json` passes an explicit
1,280,835,840-byte external model download, complete size/SHA verification and
real CPU lifecycle checks using that one retained cache file. Progress samples
showed only the first attempt, so this proves external full-transfer integration,
not external byte-range recovery. The two unchanged-prompt quality suites complete
but show semantic failures; the candidate remains unapproved.

These checks do not establish external recovery after interruption, physical
two-PC acceptance, default interpreter quality or release readiness. See
[candidate evaluation](MODEL-CANDIDATE-EVALUATION.md) for separate artifacts,
quality failures and measured scope.
