# Prepared node-owned microphone source

This development path connects the reviewed built-in energy source, local
consent, node resource ownership and existing durable consumption runner. It is
an internal backend operation; no public command, startup or network handler
launches it yet. Public source inspection continues to report execution unavailable.

Preparation holds the existing runtime/configuration binding while checking the
owning node, current saved/applied microphone permission, local listening epoch,
applied resources and exact private-binding/source digests. It retains the compiled
adapter bytes and resolves the private device only on the owning node. It reserves
the node microphone before consumption without importing a device or starting a
process. Dropping an unused preparation releases that reservation.

Running requires the exact consumed definition, not another plan using the same
resource. The source snapshot and listening epoch are rechecked before source
effects. The original initial proof and existing short renewal/stop supervisor
drive one source future; local consent/runtime retirement also cancels it. Off/on
cannot revive an old preparation. Returning a transport summary still does not
create a durable completion receipt or authorize replay.

On Windows, finite `audio.capture` and prepared continuous microphone sources share
one reservation per node service, including resource aliases and engine rebinding.
The reservation follows the exact owned process job. Normal release requires an
observed empty job. Cancellation requests termination synchronously, then retains
the reservation during a bounded read-only cleanup observation. An aborted or
failed cleanup quarantines it instead of making it available optimistically.
This excludes cooperating sources in this node process, not other applications
or physical driver/device ownership. Non-Windows finite capture retains its
existing behavior; prepared sources requiring an owned job memory limit refuse
unsupported platforms. This is not physical source quota acceptance.

The checked configured interpreter is retained independently of optional plugin
registration. That does not grant a permission or assert device availability.
Prepared job commit memory is the tighter local/reviewed ceiling. The ordinary
Python timeout, output and observation limits remain: long-session streaming,
durable events/completion and authenticated public launch
are still required before full continuous-workload acceptance.

Four new ownership regressions include a Windows owned root/descendant test with
held process handles. Eight preparation/handoff tests use a deliberately absent
interpreter, verifying that preparation and rejected handoffs execute no source.
Two consent tests cover runtime generation retirement. Hosted compilation/native
validation is pending; no heavy local Rust build or physical capture was run.

The follow-up retirement fence latches source denial before waiting on a runtime
binding/configuration lock. Runtime generations are separate from user consent:
rebinding clears retirement while preserving a local denial, and a stale guard
cannot retire or unbind a replacement even if its UUID was reused. Completion is
also checked after a source's final poll. Three added regressions hold the actual
binding mutex while retiring/replacing guards and exercise final-poll revocation.
This follow-up still requires hosted validation before promotion.

## Activity event filtering draft

The internal prepared runner now delivers typed `microphone_activity` metadata
instead of forwarding every energy observation. It emits the first observed state,
changes between active/inactive/unknown, and every explicit discontinuity. Repeated
state and level-only changes are suppressed. Unknown remains unknown, and a gap is
never fabricated as inactivity. Event sequence and original source sequence/frame
position are separate; suppressing a repeated state never renumbers source history.
The metadata contains no audio, private device selection, transcript or path.

The existing reviewed `min_interval_ms` is a hard bound between useful events on
the node's monotonic clock, not a promise that it will debounce or discard rapid
transitions. A required event arriving earlier stops the source under the reviewed
`overflow: stop` policy. Count and canonical encoded-event byte limits are checked
before delivery; a too-small event budget can refuse the first event. Malformed,
skipped, oversized or over-budget input latches failure. It cannot resume later
by silently discarding the required transition or gap. A future smoothing policy
would need its own reviewed semantics.

The transport awaits each event callback before reading the next observation.
There is at most one pending event, tighter than every allowed `max_pending` value,
and no detached reporter/offline queue. A stalled sink remains inside the existing
lease, stop and consent supervision. Source transport bytes still have their own
unchanged aggregate bound, including suppressed observations; this does not yet
solve long sessions. Callback delivery and a returned transport summary are not
durable evidence. Event persistence, receipt deduplication and observed completion
remain required before public execution. Eight filter regressions cover uncertainty,
sequence positions, exact rate/count/byte boundaries and permanent overflow refusal;
hosted verification is pending.

Each event callback is now additionally bound to a captured conservative reporting
window from the exact consumed runner. Valid renewals keep the source alive but
cannot extend that in-flight callback. Pre/post checks reject a callback completing
after expiry or retiring its own runner in the final poll. Cancellation retires
reporting before the existing final journal save can block. This does not add
event storage or turn callback delivery into a durable completion receipt; see
[reporting lifetime](WORKLOAD-LEASES.md#reporting-lifetime-draft).

## Combined native source tests in development

An explicit Windows `native-test-fixtures` unit-test feature supplies simulated
sounddevice input to the same Python host, without changing the reviewed compiled
microphone bytes. Only `cfg(test)` Windows builds contain the private prelude
field/method; shipping builds, including builds selecting that Cargo feature,
always use the unchanged host and have no configuration/command/wire selector.
Default developer tests do not require Python or device hardware. Explicit native
mode requires the pinned test interpreter and fails if it is missing.

GitHub's Windows headless Rust-test step selects that mode using the existing
official Python 3.11.9 setup. Other jobs retain the default test set; all previous
native command gates remain. Three additional tests use actual consumed grant
receipts, prepared resource/consent checks, pinned source execution, filtered
callbacks and owned Windows jobs. A finite source must emit the exact initial,
transition and gap metadata with no private device data or raw samples. Local
consent revocation and whole-future cancellation interrupt a blocked simulated
read. Each source spawns an actual descendant, and tests hold real root/descendant
handles; microphone reuse requires both to have exited. Test pacing preserves the
approved event rate instead of relaxing it for a buffered fixture.

These are pending hosted tests, not physical microphone/driver acceptance or
authenticated public execution. Normal return still retains an unknown receipt
until durable event/completion reporting is implemented. No local heavy build,
capture, installer, model or remote-node change was run for this draft.

The subsequent [event journal draft](WORKLOAD-EVENTS.md) saves useful events before
constructing observer callbacks, with captured-window supervision of both save
and delivery. Event schema two adds relative elapsed time and counts that field
against the existing byte budget. The combined native test now checks that exact
durable prefix before acknowledging a simulated observation. Event persistence
still does not turn source return into a completion receipt; this follow-up needs
its own hosted validation.

## Typed owned-source exit evidence in development

The private reserved-source path now returns an opaque `ObservedStream`, rather
than the public serializable `StreamSummary`. It requires successful transport,
validated EOF/end counts, a successful root exit and positive observation of the
exact owned Windows job having no active processes within the existing deadline.
Its private fields bind the stream ID/counts, reviewed adapter digest and a local
non-cloneable job-exit witness. No deserialized summary, root status or termination
request can construct that witness. Public transport APIs still return the same
summary type; they acquire no workload or completion authority.

Microphone reservation release now consumes evidence matched to its exact owned
job. A witness for a different job or an already retired capture is refused.
Failed or expired observation retains the reservation until independently observed
cleanup, or quarantines it as before. The unsupported-platform reserved path refuses
before starting an interpreter. Neither ordinary Python budgets nor source,
permission, consent, lease or session bounds are extended by this change.

Two Windows regressions hold real root/descendant handles to check cross-job witness
rejection, repeated release refusal and expired observation of a still-live capture.
A non-Windows regression requires refusal before interpreter startup. The combined
native source test now receives the opaque result, checks its adapter digest and
still requires both held processes to have exited and the exact event history to
be durable. Hosted validation is pending; no physical capture was started locally.

This is local process evidence, not a physical microphone/driver guarantee or a
durable completion record. The consumed runner still persists uncertainty. The
next completion path must validate the exact consumed definition, final event
prefix, lease/consent/stop boundary and this owned evidence together before saving
completion in the existing journal. It must retain bounded final reporting and
must not infer completion from a summary or saved event prefix alone.

### Lifetime handle coverage follow-up

The first owned-exit candidate failed its two new Windows tests in hosted run
`41143002837`: a zero job active count was observed before held root/descendant
handles were signalled. The original failed run is retained. Waiting longer in
the test would hide the production gap, so its immediate held-handle assertions
remain unchanged.

The follow-up retains a duplicate of the exact root handle before resume and
captures bounded current members of the exact unnamed job before requesting
termination. Lookup PIDs are hints only: each retained handle must still belong
to that job, and kernel-object comparison deduplicates identities. Positive exit
requires the job active count to be zero, the lifetime total to equal the number
of unique held process handles, and every held handle to be signalled before the
existing deadline. There is no whole-PC enumeration, PID-based termination,
uncontained fallback, privilege change, completion-port delivery assumption or
execution budget increase.

The source tracker holds at most 128 process identities. A missed transient
descendant, failed lookup, excess lifetime count or failed wait cannot produce
evidence or release the microphone reservation. Unconfirmed cleanup remains
quarantined, with a new actual-process regression for a child that exited before
its handle could be captured. A process lookup race can therefore deny reuse;
it is not automatically repaired or treated as complete. The drop observer uses
the same stronger check. This change requires fresh hosted validation.

The checks use Microsoft's documented [job lifetime accounting](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_accounting_information),
[kernel-object handle comparison](https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-compareobjecthandles)
and [process termination signalling](https://learn.microsoft.com/en-us/windows/win32/procthread/terminating-a-process).
Job accounting and best-effort process notifications alone are not used as a
replacement for the retained process-handle observations.
