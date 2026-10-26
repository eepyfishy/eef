# Yield no-op timer observations during an owned launch

The corrected grant-clock draft passed paired audio and native process stages
but its native fixture later stopped a bounded recurring schedule with
`job_unresolved`. No abnormal native process exit occurred. Logs showed repeated
multi-second journal waits from validated timer/status snapshots. The execution
request-to-grant-save interval was about 6.07 seconds, while about 2.16 seconds
of the grant's actual five-second window elapsed between issue and durable save.
Those are different intervals. No endpoint acknowledged or consumed that grant.
These observations show substantial contention, not proof of every cause of
the missing confirmation. The failed run is retained.

While a launch callback is pending, the timer already cannot claim another
occurrence or settle its job. It previously still acquired the same journal
mutex and fully validated history on every periodic observation. That no-op
reader could compete with the callback's required durable admission/dispatch
checkpoints. A held-journal test reproduces this unnecessary wait without a
performance benchmark or real external effects.

The periodic checkpoint now yields immediately only when that launch is already
owned and no independent clock discontinuity was reported. It returns no trigger
or execution authority, performs no journal write and does not cache or trust a
previous snapshot. Once the callback finishes, normal validated inspection and
settlement resume. Actual launch admission, policy/delegation fences, durable
grant checkpointing and no-replay behavior are unchanged.

Clock discontinuity is still handled before this early return. Explicit stop
and shutdown paths are unchanged; a pending callback remains owned and bounded
by its existing timeout. Read-only user inspection still validates retained data.
This is not a new authority source, a general journal-latency guarantee, a larger
lease, a faster polling setting or permission to retry unconfirmed work.

Two focused tests hold the actual journal mutex: the no-op observer must finish
before its release, returning no work and changing no rows; the clock-fence
negative control must still wait for the journal and reject a missing schedule.
The first test fails on the old path, while the negative control passes. Fresh
corrected focused/native/full/hosted validation remains required.
