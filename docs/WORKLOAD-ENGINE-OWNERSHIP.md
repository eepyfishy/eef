# Coordinator ownership before workload persistence

Product workload issuance now enters through `issue_owned`, which reserves a job
lifetime in the existing task engine before the first asynchronous journal wait.
This registration is not approval, a lease proof, source availability or durable
history. Exact review, approval, source/session and storage validation still apply.
The original issuer window and engine lifetime remain on the caller; neither is
sent to native metadata readers or writers. An unowned construction entry remains
only under `cfg(test)` for the existing isolated journal/window regressions.

Pending and running workload identities exclude duplicate workload admissions,
legacy runners, tracking abandonment, recovery and occurrence reservation. The
registration capacity derives from the existing journal final-reporting capacity;
no new worker, persistence store, source factory or unbounded pending queue is
created. Retired registrations are removed on later admission, without restoring
an issuer or changing retained job history.

Each lifetime has a separate active flag and live control gate. Stop revokes
control before attempting its journal checkpoint, including when that checkpoint
fails, and notifies the runner. Shutdown revokes every workload gate before
waiting for active lifetimes to retire. An already validated control enqueue uses
the same short in-memory gate as revocation. It cannot await, read the journal or
return a deferred future from inside that callback. The issuer checks its original
clock/reporting bounds again within control delivery; no lease budget is reset.

Stopping a still-preparing, unissued plan cancels its pending tasks without adding
an execution record. A registration stays tracked until its original owner drops
it. Drop closes control and clears the active flag without waiting on SQLite.
The original issuer/committed metadata packet still owns the separately reserved
uncertainty checkpoint and actual journal lifetime. Engine quiescence alone is
not proof that final I/O or a remote source has stopped: the existing runtime
retirement barrier must still observe that exact journal ownership release.

This extends engine lifetime handling for workloads while preserving ordinary job
cleanup. It does not complete public owner start/stop dispatch, stop delivery
receipts, capability advertising or authenticated source execution acceptance.
The runtime integration must retain the owned issuer for the whole source,
serialize actual lease enqueue through `deliver`, process bounded authenticated
frames and retire control before stop/disconnect/shutdown cleanup. Saved rows,
registration flags and successful metadata delivery cannot reconstruct an issuer.

Nine new cases cover duplicate/legacy exclusion without creating history, the
existing reporting capacity, stop before issuance, shutdown tracking, a failed
read-only journal stop checkpoint, dropping registration while the real journal
mutex is held, control/stop ordering across threads, original owned issuance
revocation, and cancelling owned issuance during blocked storage. They use real
engine/journal mechanics with no network offer, model, source or device. Existing
issuance, ACK, completion, node source and native regressions remain required.

Formatting and source checks run locally. Full changed-source hosted and final
main acceptance remain required; no local compilation/runtime test, installed
app/node update, release/feed/version or permission change is part of this work.
Physical two-PC, long-source and the remaining stable v0.4.0 roadmap stay open.
