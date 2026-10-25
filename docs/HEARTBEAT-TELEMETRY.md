# Lightweight node telemetry

Unreleased command-first reliability increment after native cleanup snapshot
`cb50fe0`. No dashboard controller or new public command is required.

## Collection and lifetime

Node registration still detects hostname, CPU and RAM, and reports the existing
storage/platform fields. Heartbeats now use one CPU/RAM inventory instead of
two full system inventories. Targeted refreshes do not request process tables;
CPU sampling retains the existing minimum interval and second usage refresh.
Storage enumeration and all heartbeat/registration telemetry queries run on a
blocking worker rather than an async worker. `system.info` uses the same shared
snapshot service, not an independent UI implementation.

The pinned `sysinfo` 0.37.2 source was checked locally. Its targeted-refresh
contract is also described in the library's [System documentation](https://docs.rs/sysinfo/latest/sysinfo/struct.System.html).
No dependency upgrade, toolchain, downloaded script or model is needed.

Each heartbeat awaits its one sample before another is started; no background
sampling queue or cached report is introduced. CPU sampling and OS queries are
read-only. An already-started blocking query cannot be cancelled by dropping its
async waiter, and OS enumeration is not a guaranteed-latency operation. This is
not physical telemetry freshness, all-load timing or resource enforcement proof.

The receiver supervises the heartbeat task. A worker failure ends that task and
retires the session, allowing the existing reconnect behavior, rather than
remaining apparently connected without heartbeats. Existing output-write failures
retain their original diagnostic. Dropped execution still retains uncertainty:
reconnect never means automatic workload replay or continuation.

The frame and fields remain compatible. Existing GPU/queue placeholders and the
public legacy `current_load` fallback are unchanged, not newly measured facts.
The heartbeat's historical `latency_ms` field measures collection elapsed time,
now including storage queries; it is not network RTT or link-quality evidence.
Provider admission still requires current registration, advertised capability and
the unchanged monotonic freshness bound. Failed admission logs age, bound and
capability presence separately, without credentials or workload parameters.

## Verification scope

Focused tests check an empty process inventory before and after CPU/RAM sampling,
retained specs and finite load values. A held synthetic collector verifies that
the single-thread async executor remains responsive; a deliberate collector fault
returns an error rather than invented success. These are local source checks.

Default and dashboard-free workspace/all-target suites each pass **609 tests**:
eef library 231 plus one ignored opt-in journal profiler, owner CLI 16,
installer 6, node library 345 and node CLI 11. Fresh default executables/examples,
formatting, diff and the backend/browser source boundary pass.

Fresh public fixtures pass:

- `local-pipelines-uJumaU`: finite local dataflow, child-provider accounting,
  permission loss, unsuccessful native stage, bounded recurrence and restart.
- `process-lifetime-xHDCEn`: all seven typed-source/native lifetime cases.
- `model-metadata-eIHmw1`: model metadata and remote selection protocol.
- `model-policy-elPG1I`: synthetic owned-model lifecycle/owner policy.

Ordinary scheduling `automation-schedules-57c0Sr` fails at the final explicitly
fresh replacement activation, after its initial three native occurrences and
refusal/uncertain-resolution checks. Read-only inspection retains an interrupted
replacement job with **zero issued attempts** and schedule reason
`job_unresolved`. The new diagnostic records `system.info` still advertised,
heartbeat age 10291 ms against the unchanged 10-second freshness bound. Slow
journal checkpoints range up to about 3.8 seconds. This distinguishes staleness
from capability withdrawal, but does not isolate every cause of the lost
responsiveness or establish all-load timing. The scheduling gate remains open.

The earlier failed pipeline fixtures remain failures; a passing fresh run does
not establish their complete cause. No interval, expiry, replay rule, freshness
window, workload permission or authorization is weakened. Installed applications,
the physical remote node, release assets, feeds, models and UI are unchanged.
Physical media/radio/network, bootstrap/model quality, strong stop, resource,
peer, cluster, signing and all other stable 0.4.0 gates remain incomplete.
