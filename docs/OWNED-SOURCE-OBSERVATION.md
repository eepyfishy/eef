# Observing capture processes before exit

Hosted diagnostics `41197726372` and `41198310676` reproduced the reserved
one-shot failure through the shipping Python host with a synthetic WAV producer.
The second run recorded lifetime process count two, retained handle count one
and active process count zero. The previous implementation waited for the root
to exit before enumerating the owned job. An additional member that had already
exited was absent from that final enumeration, so the strict completion check
correctly refused to release the microphone reservation as successful.

## Live observation under the original bound

Reserved one-shot and streaming exchanges now observe their exact owned job while
awaiting the root. The bounded member list is sampled at the existing cleanup
observer's ten-millisecond cadence. Each retained handle still requires membership
in that job and kernel-object identity deduplication. A raced member lookup does
not prevent observing the remaining entries. It never counts as evidence itself.
The root wait and observations share the caller's original absolute deadline;
they grant no extra execution time or termination authority and spawn no detached
observer task. Expiry keeps the existing Python bridge deadline category.

After root exit, the existing termination and witness path remains mandatory:
the complete lifetime count must equal the unique retained handle count, the job
must be empty, and every retained handle must be signalled within that deadline.
A member still missed before it exits continues to make completion unconfirmed.
The existing deliberately missed transient-child regression is unchanged. No
root-only or empty-accounting fallback is introduced. The fixed 128-handle bound
is unchanged, and ordinary calls without a capture reservation keep their existing
wait path. No broad process scan, PID-based termination or new device permission
is used.

This observation can support release of this application's source reservation.
It does not attest driver completion, physical silence or effects in other apps.
Cancellation still drops the source and uses the existing separate bounded cleanup
path. Failure/cancellation never grants restart, replay or additional lease time.

## Regression and acceptance boundary

The existing opt-in two-capture synthetic WAV case must now pass without changing
its six-second parent bound, byte limits or immediate-reuse assertions. A new
native proxy-root/helper case waits for both identities to be held, releases the
helper, waits for both to exit, and requires the strict witness. An expired-wait
case requires a typed deadline error while the source is still alive, followed
by explicit existing cleanup. A shared mapper case preserves the bridge's public
deadline category through private error context. Existing coverage, missed-child,
source cancellation, budget and finite audio completion checks remain required.

## Identity-based proxy fixture follow-up

The unchanged reserved one-shot case passed in isolated hosted run `41198928428`.
Full run `41198879726` also passed all 36 native suites and the separate
pre-registration-close case, including finite audio and listening revocation.
Both Linux jobs passed. Both Windows Rust suites failed the new proxy fixture,
so this is not accepted full-source validation.

Diagnostic run `41199617600` kept that fixture's release predicate and deadline
unchanged and recorded three retained processes before the original ten-second
wait expired. The fixture incorrectly waited for exactly two, despite the owned
job containing another member. This result does not establish the third member's
executable or role.

The fixture now uses read-only, test-only kernel-handle identity comparisons to
confirm the intended root and helper are both retained before releasing the
helper. Before polling the live observer it requires this condition to be false;
after exit it requires it to remain true. No process-count threshold can substitute
for either identity. The final production witness still requires complete
lifetime coverage and every retained handle, including additional members, to
signal under the original deadline. No shipping runtime behavior, execution time,
permission, source budget, or fixture fallback time changes.

Fresh full-source hosted validation is required for this correction. Earlier
failed runs and diagnostic side branches remain retained; component diagnostics
are not release acceptance. Physical two-PC and stable release requirements
remain outstanding.
