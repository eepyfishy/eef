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

These changes need fresh hosted component and complete native acceptance. The
diagnostic side branches retain their failures and are not release acceptance.
Physical two-PC and stable release requirements remain outstanding.
