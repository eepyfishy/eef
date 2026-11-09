# Receiving interpreter results

This unreleased increment is accepted at source `5b9ef3a` in the
[four-job hosted run](https://github.com/eepyfishy/eef/actions/runs/42582257784)
and independent source/log audit. It adds semantic validation to the existing
grant-report confirmation path; it introduces no new command, model request,
permission or wire format.

An interpreter node already validates complete model output before returning a
candidate. The receiver must independently validate that candidate as well.
Authenticated transport and matching grant IDs establish which node sent a
reply, not whether its proposal, evidence or permission claims are valid.

For a completed interpreter task, the receiver reconstructs the existing typed
candidate from the returned semantic proposal and the exact granted source.
It compares the complete result with that reconstructed value. This checks:

- Initial, clarification and method stage, source identity/revision and digests.
- Original input or method evidence, disclosed capabilities and proposal schema.
- Exact model/backend and serving permission revision from the grant.
- Required untrusted-output and non-execution flags, without extra peer claims.

The validated types are regenerated through the existing validators; peer JSON
is not deserialized into an authority object. A valid result remains an untrusted
proposal and needs the normal review, compilation and separate tool approval.
This does not attest which model weights actually ran or make a remote executor
trusted beyond the owner's policy.

Existing byte bounds, authenticated session checks, journal checkpoints, stop
and deadline behavior remain in control. Unknown/rejected outcomes cannot carry
a successful candidate. A completed `result_too_large` report remains a failure
with null output, preserving the existing bounded-result behavior. Other grant
types retain their current confirmation rules.

Three new regression tests cover all three stages, changed bindings, forged
flags/evidence, undisclosed capabilities, extra fields and result/uncertainty
bounds. Existing private TCP/TLS interpreter tests also pass through this receiver
path when run. Synthetic checks do not replace physical/model acceptance, the
coordinator allowance ledger or the automatic broker.
