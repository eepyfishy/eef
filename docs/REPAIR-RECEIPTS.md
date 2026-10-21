# Durable repair admission and inspection

Repair is still explicit, not an automatic dependency-repair controller. This
increment adds durable admission/evidence to the same backend used by local EEF,
local EEFN and approved remote node repair. No dashboard is needed to own the
operation or inspect an interrupted attempt.

## Owner interface

```text
eefn repair --status
eefn repair --status --operation-id UUID
eefn repair --operation-id UUID --url HTTPS_PACKAGE --sha256 SHA256 --size-bytes BYTES
eef node update-status --node NODE_ID --operation-id UUID
eef node update --node NODE_ID --repair --operation-id UUID --version EXACT_RUNNING_VERSION --url HTTPS_PACKAGE --sha256 SHA256 --size-bytes BYTES
```

Use `eef repair` for local coordinator repair. `--operation-id` is optional for
new local/remote repair; an omitted ID is generated. An explicit ID lets an
owner inspect the same operation after a lost reply without repeating it. IDs
are non-nil canonical UUIDs. An admitted ID is never replayed, including after
failure, process exit, a changed source or a selected/rolled-back package.
An intentional new repair needs a new ID and the existing authorization checks.

Prereleases still require one-shot `--allow-prerelease`; no feed, preference or
automatic-update policy changes. A normal application update cannot use the new
repair-only operation-ID option. Repair does not restart, run inference, edit
owner configuration or choose/install a model. Ordinary explicit restart still
applies all pending settings. No automatic retry or rollback is added.

Local failures return a structured report with the operation ID even if no
admission was confirmed. Human-readable output points to `repair --status
--operation-id UUID`. Potentially private artifact URLs/errors are not copied
into durable receipts. Remote status exposes retained receipts even when package
selection needs attention; its restart flag refers to that running node process.

## Offline status is not runtime health

Offline `package_status` now uses schema **2**. It reports `scope:
invoked_executable`, `invoked_version`, and
`handoff_required_for_invoked_executable`. `application_restart_required` is
null and `running_service_inspected` is false. The old offline schema-1
`restart_required` field is intentionally absent: a root bootstrap can differ
from a selected generation already running in another process. That is not
evidence that the background service needs a restart.

Selection and repair-receipt inspection are independent. A damaged/missing
selection target can produce a failed status report with useful receipt data;
damaged receipt data can likewise leave selection data available. Neither
condition authorizes a reset, fallback repair, manual JSON rewrite or replay.

## Storage and lifecycle

The shared installation `.update.lock` serializes package repair with update and
rollback. `.package-repairs.json` holds up to 128 immutable operation identities
and their latest observations, bounded to 256 KiB. Each identity has one empty
lease file under `.package-repair-leases/`; that directory is independently
bounded to 128 files, including pre-admission crash orphans. No record/lease is
automatically pruned. Full quotas refuse admission before artifact I/O.

Schema, record count/size, IDs, sequence, product, version, artifact digest/size,
timestamps and phase/outcome combinations are validated before admission. The
journal retains a digest binding the exact request (including source/consent),
product, invoking executable hash and running slot; it does not retain the URL,
owner settings, credentials or model data. It is atomically replaced with the
shared synchronized writer, not copied over a live file. Later writes fence the
last durable bytes; changed, damaged, missing, future-schema and oversized
evidence is retained without reset or overwrite. Initial empty-journal
publication uses synchronized same-directory staging with a no-clobber commit,
before creating a lease. A missing journal with retained leases is ambiguous,
not a fresh installation; admission is refused rather than silently resetting
operation history.

An operation's lease is acquired before admission publication. Admission must
be durable before download begins. Pre-action boundaries must persist before
their corresponding action; post-action evidence follows it. The sequence is:

```text
admitted → downloading → verifying → staging → staged → selecting → selected
```

`verifying` includes download size/hash, archive, product/version, executable and
inventory verification. `staging` precedes the permanent directory rename;
`selecting` precedes selector publication; `selected` is recorded only after
publication succeeds. A caught error records a failed outcome when possible.
If that write fails, the last durable step remains unresolved. There is no
copy-overwrite fallback and no claim of a multi-file/power-loss transaction.

Inspection tests the per-operation OS lease, not a PID/state-file guess. A
released in-progress lease is observed as interrupted without changing the
retained record. An inaccessible lease is unknown, not proof of a live or
finished repair. Inspection never recovers/resumes an attempt or obtains new
execution authority. Pre-admission lease orphans beside a valid journal are
visible but not deleted; missing/damaged journal evidence remains unavailable.

`selection_outcome_unknown` remains true for an uncertain selecting step,
even when a current pointer observation shows that generation selected.
`package_may_be_staged` is conservative around the permanent rename. Neither
flag is proof about leftover temporary extraction files. `selected_now` and
`bundle_present` are read-only observations separate from the historical
receipt; rollback does not rewrite a previous selected outcome. Nothing here
certifies that a runtime is running, healthy or using working dependencies.

## Verification boundary and outstanding work

The native fixture uses the public HTTPS entrypoint and a local TCP/TLS gate
that never serves a handshake/artifact. It terminates only its fixture process,
checks the released lease/read-only interruption view, and proves that replay
cannot contact that source. It is not a successful HTTPS package repair,
publisher signature check, real-model test or physical two-PC acceptance.

The frozen sources pass workspace/all-target type-checking and the complete
locked/offline default-feature and headless workspace/all-target test runs:
**990 passed plus one ignored each**, including nine new regressions. Final
headless apps/examples build. All **nineteen native suites** pass against the
same frozen apps/helpers, including real-process interruption/replay checks for
both roles and approved remote repair failure/receipt inspection. Evidence:

- `.validation/repair-receipts-workspace-default-final.log`
- `.validation/repair-receipts-workspace-headless-final.log`
- `.validation/repair-receipts-build-final.log`
- `.validation/repair-receipts-native-complete.log`
- `.validation/repair-receipts-native-results.json` (nineteen passed public results)
- `.validation/repair-receipts-source-hashes.json` (thirteen product/harness files)
- `.validation/repair-receipts-native-hashes.json` (seven apps/helpers)

The final new suites are `repair-receipts-UKl33O` and
`explicit-updates-g7W7LA`; the updated handoff suite is `package-handoff-9LCdQR`.
All source/artifact hashes remain unchanged and owned validation app/helper
processes are terminal. Format, affected JavaScript syntax, backend/browser
source boundary and diff checks pass. Earlier superseded compilation was
intentionally stopped; it is not acceptance evidence.

The `.github` workflow stays a local draft at the owner's request and is excluded
from the backend publication. Its earlier validation-branch push was rejected
because the isolated OAuth login lacks the workflow scope. No permission was
expanded and no CI run is claimed. Local verification cannot substitute for the
missing release/physical gates.

Trusted current-build artifact discovery/consent, automatic repair,
byte progress/cancellation, owner-directed retention/capacity controls,
post-restart dependency health, approved defaults/model quality, physical
two-PC, packaged HTTPS repair, signing/Defender and stable-release gates remain.
No antivirus exclusion/bypass or public self-upgrading agent is introduced.
