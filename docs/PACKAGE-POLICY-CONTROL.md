# Node installation maintenance policy from eef

Unreleased after alpha.6. Commands and authenticated backend APIs own this
behavior; no dashboard is required. This controls the existing
[missing-package-file maintenance](PACKAGE-SETUP.md), not model setup or updates.

## Owner commands

On the node, explicitly authorize policy changes from eef:

```
eefn permissions allow remote-management
eefn permissions allow remote-repair-setup-policy
```

Both switches are off by default. The second switch can only be changed locally,
even when eef already has general remote-management permission. These switches
take effect without restart. eef cannot enable them through either permission
commands or generic remote settings saves. A local deny revokes future policy
saves; it does not turn off a policy already saved. To stop future maintenance,
save that policy off explicitly before revoking remote access, or use the local
offline command `eefn repair-setup configure --mode off`.

Changing this live-only consent does not block a separately approved model-only
activation or make it report a pending application restart. Only the two known
live management fields are excluded from that comparison; opaque future settings
and actual model/permission changes retain their existing fences.

From eef:

```
eef node repair-setup --node NODE_ID show
eef node repair-setup --node NODE_ID configure --mode prompt
eef node repair-setup --node NODE_ID configure --mode off
```

Prompt uses the invoked bundle's public source hint. Select another public HTTPS
descriptor explicitly with `--manifest HTTPS_URL`; credentials, query and fragment
are refused. Auto requires `--mode auto --allow-automatic-repair` and a stable
invoked build. Prereleases still refuse auto saves, and their supervisors refuse
all network work, including prompt checks. No alpha override is available here.
Nothing changes the automatic-update feed's prerelease exclusion.

This is **shared node-role installation policy**, not policy limited to one
configuration or device identity. Multiple node processes using that same
installation share it. The local consent switch authorizes eef to select off,
prompt or (on stable builds with its distinct flag) automatic maintenance for
that installation. Saving an active mode always creates a new one-attempt policy
identity, even when the mode is unchanged. A running supervisor can then evaluate
it under the existing rules. Re-saving is new authority, not a harmless retry.

The new endpoint does not expose `run` or `approve`. Prompt repair approval is
still a separate local command for one retained attempt. Version upgrades,
service restart, model selection/download, OS trust and workload permissions
remain separate. Saving policy is not successful repair or runtime health.

## Shared service and uncertainty

The eef CLI first reads the policy, then sends one save pinned to the exact
node/runtime, inspected revision and canonical policy SHA-256. The coordinator
adapter validates typed, correlated node replies. No alternate node, legacy
action, repeated save or automatic retry is attempted after loss, rejection or
unsupported replies. A failed save response may leave the outcome unknown;
inspect the policy before deciding whether to grant another attempt.

Node commands: `POST /api/commands/repair-setup-policy` (local owner adapter).
eef commands: `POST /api/commands/node/repair-setup-policy`. Bodies are bounded
to 16 KiB and retain existing owner/origin checks. Requests use schema 1, an exact
runtime and canonical request UUID; only eef read requests may omit the runtime
to discover it first. Wire actions are `repair_setup_policy_status` (show only,
reserved control capacity) and `repair_setup_policy` (work capacity). A status
action cannot smuggle a mutation through the control lane.

The node owns one bounded blocking file worker for these commands. Cancellation
does not release its slot while the worker remains active. Publication holds the
existing runtime/configuration guards through the shared package-policy file
lock and revision/digest comparison. A local revocation winning that lock order
prevents a subsequent remote save. A conflict refuses rather than claiming a
completed opt-out. No guard spans network I/O in this command.

Show does not initialize policy, history or leases. Off can still be saved when
maintenance history is damaged, without launching a pending executable. Damaged
policy itself is preserved and refuses inspection/save. Policy reports include
public source/bundle binding, but no node credentials or raw process errors.
Remote command input cannot select an executable, version or stable test context.
Inspection reports whether policy is off, prerelease-blocked, bound to the invoked
build, requires reauthorization, or cannot verify that build. A matching binding
is applicability evidence only, never an import/inference health certificate.

Adding the new feature row extends strict permission reports. Older peers that
do not understand it must report unsupported/unconfirmed, not assume remote
consent. Absent/false new consent retains the old permission digest; explicit
true is included in the digest and stale saves are rejected.

## Verification boundary

Both locked/offline workspace/all-target feature modes pass **1,089 tests plus
one existing ignored profiler each**. Fresh headless apps/helpers build. All
**twenty-nine native suites** pass in one sequential run, including the new
production-alpha policy suite `package-policy-AtcuRm`, activation consent cases,
and coordinator retirement/restart controls. All **299 source/resource/workflow
and nine native artifact hashes match**; no matching fixture processes remain
observed. Evidence is `.validation/package-policy-{headless,default}-final.log`,
`package-policy-validation.log`, the source/native hash inventories and the
read-only `.validation/package-policy-acceptance.json` audit.

Published source `fd21f14` subsequently passes all four matrix jobs and all
twenty-seven selected native suites in
[hosted run 40076717528](https://github.com/eepyfishy/eef/actions/runs/40076717528).
Exact native dispatch order is checked in `.validation/fd21f14-ci-acceptance.json`.
Separate real CPU lifecycle/inference also passes without downloading/copying a
model. These are revision-specific follow-ups, not stable-release, physical
two-PC or approved model-quality acceptance.

Earlier failures and incomplete scopes are retained below; they are not the
final acceptance evidence.
The first focused run failed because the synthetic owner config had a blank
name. An intermediate full run then found a fixture helper had used the generic
settings save, which intentionally marks restart pending; it now uses the same
local permission commands as owners. Neither failed run is acceptance. Logs are
retained in `.validation/package-policy-headless.log`; the first focused failure
is also retained in the tool transcript. The lost-reply harness uses a separate
eef command configuration pointing at its loopback proxy, not a node discovery
marker or edits to the running coordinator's settings.
Review also found that the model-only activation comparison needed to exclude
the new live consent alongside general remote-management consent. A direct
regression preserves unknown-setting/model fences; the native activation suite
checks a local permission toggle before activation and a local owner settings
save while the old executor is retired. It does not bypass runtime-bound
permission commands during that retirement interval. That source change required
fresh full validation; the preceding 1,085-test headless pass did not cover it.

The activation-adjusted source passed both Rust modes (1,086 tests plus one
ignored each), then nineteen native suites. The combined run stopped at
`model-bootstrap-bfvMBT`: the coordinator exited on restart because the preceding
job-store owner still held its exclusive database lock. Its retained fixture has
no accepted overall result. The replacement work adds an explicit bounded
retirement wait for that exact store's connection/lock, not a lock bypass,
unconditional delay or retry. Six focused retirement tests pass; rebuilt eef
also passes `model-bootstrap-l6uYv7`, observing ten distinct runtimes across
nine explicit restarts and refusing a second coordinator's database ownership.
This focused result is not the full checkpoint. The failed combined run is
archived as `.validation/package-policy-before-retirement-validation.log`, with
its original Rust logs and source/artifact hashes; it is not retrospectively
counted as green. The final full acceptance above covers the retirement change;
`model-bootstrap-WH2CyR` repeats the restart/competitor checks in the combined run.
See [coordinator retirement](COORDINATOR-RETIREMENT.md) for the ownership boundary
and its limits; the exact late holder in the exited fixture was not identified.
Synthetic stable unit contexts do not prove a stable installed supervisor can
successfully repair publisher artifacts. No physical two-PC, installer/MSI,
signing/Defender, useful model quality or stable-release acceptance is implied.
