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
useful event filtering, durable events/completion and authenticated public launch
are still required before full continuous-workload acceptance.

Four new ownership regressions include a Windows owned root/descendant test with
held process handles. Eight preparation/handoff tests use a deliberately absent
interpreter, verifying that preparation and rejected handoffs execute no source.
Two consent tests cover runtime generation retirement. Hosted compilation/native
validation is pending; no heavy local Rust build or physical capture was run.
