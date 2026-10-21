# Opt-in missing-package-file maintenance

Unreleased backend increment. Both EEF and EEFN expose the same local owner
commands; no dashboard or running service is required. The installed service
also evaluates its role's saved policy every 30 seconds while running.

```
eefn repair-setup show
eefn repair-setup configure --mode prompt
eefn repair-setup run
eefn repair-setup approve --operation-id ATTEMPT_UUID
eefn repair-setup configure --mode off
```

Use `eef` for the coordinator. `--json` supplies structured reports. Enabling
prompt mode uses the current managed bundle's public repair-source hint; a
publisher without a hint can be selected explicitly with `--manifest HTTPS_URL`.
No publisher, repository, model or network address is hardcoded.

## Consent and authority

The default is **off**. Reading an untouched installation creates no policy,
history, lease or network request. In prompt mode, missing required files cause
one descriptor check and a retained proposal; no artifact download is authorized
until the owner approves that exact attempt. Automatic mode requires separate
explicit consent:

```
eefn repair-setup configure --mode auto --allow-automatic-repair
```

This grants **one attempt per explicitly saved policy identity**, not an endless
retry loop. Every save creates a fresh identity and increasing revision. Use
`--expected-revision NUMBER` to reject a stale save. Saving the same active mode
again can authorize a new attempt; inspect prior evidence first.

All prerelease builds block runner/supervisor network work, including prompt
checks. Auto consent cannot be saved from a prerelease. Saving prompt policy on
an alpha is possible but does not make it applicable. Manual `repair --check`
and `repair --approve UUID` retain their separate, one-shot `--allow-prerelease`
path; automatic-update feeds still reject prereleases.

The grant pins role, exact invoked version/executable SHA-256/physical slot,
component paths and a reviewed **public HTTPS source** without credentials,
query or fragment. Later discovery-hint changes cannot redirect that grant.
Changing build/slot requires explicit reauthorization. Existing remote-update,
filesystem, shell or model permissions do not grant repair-setup authority.
This increment does not expose remote policy configuration or approval APIs.

## What it detects and does not detect

The runner checks presence of required managed package files and selected stock
runtime files from the bounded inventory: Python, node plugins, stock llama.cpp
and relevant top-level runtime libraries. It does not inspect owner config,
models or arbitrary/deep custom runtime data. Managed source hints and component
requirements resolve from the invoked bundle, never an old root or pending slot.

**Presence is not runtime health.** Present-but-corrupt files, imports, ABI,
transitive dependencies and successful inference are not diagnosed here. Existing
custom interpreter/model choices are not changed, even when missing stock files
are maintained. Artifact application still uses the existing complete verified
[package repair boundary](PACKAGE-REPAIR.md), not a new permissive downloader.

The runner never chooses/downloads a model, upgrades versions, restarts a service,
changes OS trust/Defender settings or claims publisher-signature verification.
After successful selection, restart remains a separate explicit action; selection
is historical evidence, not proof that the live service adopted the bundle.

## Durable attempts, revocation and interruption

Admission is persisted before descriptor I/O. Each record retains its original
policy snapshot, observed missing files, control UUID, exact source proposal UUID,
approval flag and lifecycle. A policy saved off later cannot erase the original
intent. Public source URLs are deliberately retained in these owner policy and
controller records; they must not contain secrets. Existing source proposals and
artifact receipts remain URL-free.

One serial role lease prevents concurrent admission; separate per-attempt OS
leases distinguish active work from earlier released/interrupted attempts.
Read-only inspection does not rewrite history or restart work. A retained
in-progress record with a released/unavailable lease has an unresolved outcome,
not proof of success or a safe automatic retry. No retained attempt is replayed
after process restart. Failed artifact work preserves reported uncertainty.

Policy/lifetime checks bound network waits and retire the owned source future
after revocation/shutdown. Build, policy, record, missing-file cause and approval
are rechecked at source/artifact boundaries. The final shared policy guard spans
artifact publication. A conflicting policy save reports not confirmed/busy; it
does not falsely report a completed revocation. These are cooperating-process,
point-in-time checks, not protection against arbitrary same-owner file edits or
a guarantee that already-committed work can be undone.

Off can be saved without reading broken setup history or starting a missing
selected child executable. Corrupt/unsupported policy itself is preserved and
blocks automation rather than silently resetting consent.

Per-role policy is bounded to 64 KiB; history to 256 KiB and 128 attempts; lease
files, including orphans, to 128 slots. Unsupported, damaged, oversized or
ambiguously missing history refuses admission. There is no automatic pruning,
reset, interrupted-operation recovery or source fallback.

`success` in a show/run report means inspection completed, not that repair or
runtime health succeeded. Inspect record phases and `outcome_unresolved`; use
`repair --status` for proposal/artifact/selection evidence. All reports explicitly
say runtime health and running service were not inspected.

## Validation boundary

Focused tests cover policy consent/CAS, prerelease refusal, source pinning,
one-attempt admission, prompt approval, owner-data preservation, immutable grant
retention, per-attempt lease observation, uncertainty, revocation/lifetime,
no-replay and corrupt/missing/quota refusal.

`tools/test-package-setup.mjs` exercises actual owner commands for both roles and
the real HTTPS metadata entrypoint using a **non-packaged test-only synthetic
stable context**. Its loopback TCP gates do not complete TLS or serve descriptors
or artifacts. Actual production alpha binaries must refuse automation. Native
evidence can establish admission-before-network, live/released leases, killed
process inspection, no replay, source pinning and opt-out; it is not successful
publisher HTTPS repair, a stable main-loop test, physical two-PC validation,
fresh installer/MSI/signing/Defender acceptance or a runtime-health certificate.

Both final locked/offline workspace/all-target feature modes pass **1,029 tests
plus one ignored each**, including nineteen setup cases and two source-authority
regressions added in this increment. Headless apps/examples build. All **twenty-two
native suites** pass against unchanged eight app/helper hashes; twenty-four
frozen runtime/harness/helper source hashes match, all twenty-two public result
files report passed, and observed app/helper fixture processes are terminal.
Format, script syntax, backend/browser source boundary and diff checks pass.
Evidence:

- `.validation/package-setup-workspace-headless-final.log`
- `.validation/package-setup-workspace-default-final.log`
- `.validation/package-setup-native-complete.log`
- `.validation/package-setup-native-results.json`
- `.validation/package-setup-source-hashes.json` (twenty-four files)
- `.validation/package-setup-native-hashes.json` (eight apps/helpers)

The new native suite is `package-setup-SUi3RE`; Python bridge validation is
`python-bridge-FHFULT`. Initial focused tests preceded the final immutable-intent
change; the final full runs, not that earlier eighteen-test result, cover the
accepted sources. No failed full/native attempt was retried or hidden here.
The test-only stable-context, non-handshaking HTTPS, hardware and release limits
above still apply. General progress/cancel/retention commands, post-restart
dependency health, remote owner-policy management, approved model/default quality
and physical/installer/signing/Defender/stable-release gates remain open.
No installed/remote node, UI, model/default, version, release asset, OS trust,
Defender setting or auto-update policy changed. CI remains local and unpublished
at the owner's request; no GitHub authorization scope was expanded.
