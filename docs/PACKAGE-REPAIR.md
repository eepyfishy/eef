# Explicit Windows package repair

The subsequent [durable receipt/inspection contract](REPAIR-RECEIPTS.md) adds
admission-before-download, non-replayable operation IDs and offline status
schema 2. Its verification is tracked separately in `STATUS.md`; the evidence
below retains this original checkpoint's boundaries, not acceptance of later
work.

The later [source proposal flow](REPAIR-SOURCES.md) lets an owner check a
publisher's exact-build HTTPS descriptor and approve its retained UUID without
manually entering artifact SHA/size. It is not an automatic repair source or
controller; this document retains the original raw-artifact checkpoint below.

The backend can stage an explicitly selected **same-build** EEF or EEFN package
without replacing loaded files. This is a repair primitive, not an automatic
dependency-repair controller, application upgrade, model installer or readiness
certificate. The dashboard is not needed. No new model/provider is selected.

## Owner commands

Both applications expose these local commands, including when their normal
configuration cannot be loaded or their pending executable is missing:

```text
eefn --json repair --status
eefn --json repair --url HTTPS_INSTALLER_OR_ZIP --sha256 SHA256 --size-bytes BYTES
eefn --json repair --rollback
```

Use `eef` instead of `eefn` to repair the coordinator. A prerelease requires
`--allow-prerelease` on that specific package selection. These commands do not
change the update feed, subscribe to prereleases, restart an application or
invoke a model. Status and rollback require no server, port or configuration
file. The URL/hash/size must come from an independently trusted package source;
the application does not guess a replacement or weaken verification after an
error. Automatically discovering/approving the correct repair package remains
future setup-controller work.

This offline package status compares the **invoked executable** with the selected
bundle. Its `current_version`/`restart_required` are not an inspection of an
already-running background service; invoking an older root bootstrap can report
handoff pending even if another process already runs the selection. Use running
node diagnostics/update status to inspect that runtime. This distinction remains
part of the setup controller's outstanding usability work.

For a running remote node, the existing approved update capability also exposes
repair as a distinct action. The coordinator command is:

```text
eef --json node update --node NODE_ID --repair --version EXACT_RUNNING_VERSION --url HTTPS_INSTALLER_OR_ZIP --sha256 SHA256 --size-bytes BYTES
```

Add `--allow-prerelease` only for an intentionally selected prerelease. The node
checks its applied update permission, exact node ID, current on-disk remote
update permission, and rechecks that permission before selecting the
package. Older nodes reject `repair_explicit`; the client never falls back to
editing feeds or executing shell commands. A lost/mismatched reply is unknown,
not proof of failure; inspect node update status before any new operation.

After selection, use the existing explicit application restart command. An
ordinary restart also applies **all other pending settings** and may interrupt
work. Scoped model-activation restarts continue to skip application handoff.
Rollback selects a retained bundle, but does not start it or delete any files.

## Verification and commit boundary

The public repair request requires a canonical operation UUID, exact semantic
version, credential-free HTTPS URL, SHA-256, positive declared size up to 512 MiB
and separate prerelease consent. The version must equal the running build.
Redirects retain the HTTPS/credential policy. Downloaded bytes must match the
declared size and hash. This reuses the existing bounded installer/ZIP reader;
the outer hash covers the installer as well as its embedded ZIP.

Extraction is limited to 20,000 entries and 2 GiB expanded bytes, checks available
disk space, and rejects traversal, ASCII-case-colliding paths and symbolic links. The
staged package must have matching product/version metadata and an executable
whose SHA-256 equals the running executable. A same-version label with different
code is refused; this command is not an unreviewed software upgrade.

`files.sha256.json` is bounded to 4 MiB/20,000 entries. Every listed file must have
the declared size/hash; unsafe or duplicate inventory paths, missing required
Windows runtime/plugin files, and unlisted package files are refused. This
verifies the selected package's bytes, not Python/library ABI compatibility,
model quality, Authenticode identity or that it will work on every machine.

A cross-process `.update.lock` serializes update, repair and rollback. Pending
application selections block another repair. The verified payload is normalized
to a direct immutable directory:

```text
INSTALL/versions/VERSION.repair-REQUEST_UUID/
```

The running directory, owner configuration, model storage and previous repair
directories are not overwritten. The per-version `VERSION.bundle.json` records
the selected physical directory and previous directory (`$root` denotes the
flat installation). It is atomically replaced with the shared synchronized
same-directory writer, with **no copy-over-live-file fallback**. `current.txt`
still records the semantic version; ordinary updates retain their existing
layout. Update/rollback version pointers also use the atomic writer.

Repair accepts direct flat/versioned running bundles, not arbitrary executable
layouts. Owner-specified interpreter/plugin/model-server overrides are preserved;
a damaged custom dependency may still need owner-directed repair. An installer
rebuilt with different executable bytes is not accepted just because it reuses a
version label.

For a first flat repair, the generation is published before `current.txt`. If
the second write fails, the flat application is still selected. Failed selector
publication after staging retains the unselected directory for inspection;
replaying its operation ID cannot overwrite it. There is no automatic retry,
orphan deletion, forced restart, multi-file transaction or power-loss guarantee.
Corrupt/oversized selectors fail closed and are not silently reset. A damaged
selector may require owner-directed recovery; no arbitrary directory is guessed.

Handoff compares the selected executable path to the actual running executable,
not just semantic versions, so a same-version generation starts once and cannot
loop back through the root bootstrap. Repair rollback first selects the retained
previous physical bundle; a later rollback can use the ordinary previous
semantic version. No health-based rollback is automatic.

## Verification evidence

The original native fixture reproduced the old behavior with the published
checkpoint binaries: the root node stayed active after a same-version selection
and restart. See `.validation/package-handoff-before-observed.log` and
`.validation/package-handoff-UiCGZA/observations.json`. The latter records the
fixture-only executable path; it is not a physical two-PC test.

Focused Rust cases cover exact flat/versioned staging for both products,
rollback/no handoff loop, retained operation directories, configuration/model
preservation, hash/size/product/version/code/inventory mismatches, final grant
revocation, concurrent update locking, prerelease consent, archive bounds and
native Windows selector rename denial. `tools/test-package-handoff.mjs` exercises
real application restart/rollback and read-only CLI status with synthetic
selectors in isolated installations. `tools/test-explicit-updates.mjs` checks
remote authorization and exact reply/no-replay behavior; its successful repair
reply is mocked, not an HTTPS package repair download.

Both final locked/offline workspace/all-target feature modes pass **981 tests
plus one ignored each**, including ten added regressions. Headless apps/examples
build. Eighteen native suites pass against the same frozen apps/helpers:

- `.validation/package-repair-workspace-default-final.log`
- `.validation/package-repair-workspace-headless-final.log`
- `.validation/package-repair-build-final.log`
- `.validation/package-repair-native-complete.log`
- `.validation/package-repair-final-source-hashes.json` (ten product/harness files)
- `.validation/package-repair-native-hashes.json` (six apps/helpers)

The final new suites are `package-handoff-lVYQVB` and `explicit-updates-juFiMJ`.
All eighteen public result files report passed; owned application/helper
processes are terminal, and source/artifact hashes remain unchanged. The handoff
suite also confirms that ordinary update API calls cannot supersede a pending
repair and that coordinator status notices an offline selection.

Earlier retained harness attempts are not final acceptance: one omitted the
coordinator test secret; one used a newer-version repair fixture when testing
revocation; one omitted the trusted llama runtime path before that suite started.
Those fixture/environment issues were corrected without changing the frozen
product source or binaries. Full acceptance is the `native-complete` run above,
not the earlier `native-final`, `native-accepted` or `native-all-accepted` logs.

## Remaining setup-controller work

Automatic repair still needs a trusted current-build artifact source and consent
policy, bounded progress/cancellation and retention controls, runtime-aware
status, and post-restart dependency health verification. The subsequent
[receipt contract](REPAIR-RECEIPTS.md) supplies bounded local operation and
uncertainty evidence, not an automatic repair controller. Local failed selection
publication must not be interpreted as proof that no files were staged. No automatic replay
or orphan cleanup is implemented here.

No antivirus exclusions or bypasses are added. Exact hashes do not prove a file
benign or resolve a Defender detection. Fresh physical two-PC, packaged HTTPS
repair, signing/Defender, approved-model/default and stable-release acceptance
gates remain open.
