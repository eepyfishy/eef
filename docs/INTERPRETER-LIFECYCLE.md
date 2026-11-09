# Interpreter continuation and stop checkpoints

This is an unreleased internal increment toward the network interpreter broker.
Source `b1b1c70` passes all four jobs in the
[hosted run](https://github.com/eepyfishy/eef/actions/runs/42630292907) and an
independent source/log audit, including 1,703 Windows/headless Rust tests and
all 37 native suites. It does not add a public inference command,
connect ordinary input to a network model, send a stop itself or complete broker
delivery, promotion and recovery. The [allowance ledger](INTERPRETER-ALLOWANCES.md)
remains the authority and storage boundary.

An exact continuation can be retired without inventing an executor report.
Unsent work becomes `not_sent`; delivery-intent or accepted work becomes
`interrupted`. Matching initial ownership is retired in the same transaction.
Retirement never refunds aggregate allowance or reconstructs delivery authority.

The internal stop checkpoint reuses the existing exact-grant stop contract.
It saves one send intent before returning the original grant for the gateway's
`stop_grant` operation. Reading or repeating that intent returns no new send
continuation. A source still known to be unsent is retired without network work.
The eventual broker must invoke the gateway and record its authenticated reply.

Stop observations must confirm the original grant and cannot be erased,
retargeted or overwritten with different evidence. A confirmed stop receipt
records cancellation signalling, never proof that computation or external
effects stopped. Unknown work keeps its concurrency slot and recovery space.
If a final inference report races ahead of a stop observation, bounded headroom
within the original allocation remains available for that late observation,
including after quota reduction. No new quota admission or automatic retry is
used to record it.

The shared task engine also allows an active review-only input to become
cancelled directly. Such input has no executable plan that can enter the
execution-specific `stopping` state. The future broker can therefore join
existing active-job admission and shutdown without making raw input executable.

These are journal and lifecycle prerequisites. Wiring the existing grant
transport, scheduler capacity, owner-policy and source-change notifications,
result promotion for all three stages, model-free normal input, explicit
recovery and physical two-PC acceptance remains required.
