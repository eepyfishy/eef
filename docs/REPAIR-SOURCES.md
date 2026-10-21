# Exact-build repair source proposals (unreleased)

The command backend can check an owner-selected publisher descriptor and retain
an exact-build proposal before approval. This removes manual package SHA/size
entry from this local flow. It is not an automatic setup/repair controller,
default source, publisher signature check or stable-release acceptance.

```text
eefn repair --check --manifest HTTPS_DESCRIPTOR
eefn repair --status
eefn repair --manifest SAME_HTTPS_DESCRIPTOR --approve PROPOSAL_UUID
eefn repair --status --operation-id PROPOSAL_UUID
```

Use `eef repair` for the local coordinator. For a prerelease running build,
both check and approval additionally require `--allow-prerelease`. That is
one-shot consent, never an automatic-feed or update-policy change. The existing
explicit URL/SHA/size repair path and separate rollback command remain available.
These source commands do not currently add a remote node source-planning API;
approved remote node repair still uses its existing exact-artifact command.

## Source and approval contract

Choose a publisher you trust. No repository, host, credential, model or source
URL is hardcoded. The backend does not discover a source from an ordinary update
feed or silently substitute a different version. The descriptor uses HTTPS
without URL credentials/fragments, normal TLS verification, at most five safe
HTTPS redirects, a 20-second request bound and a 16 KiB body bound. Its strict
schema binds the product, exact running version and executable SHA-256:

```json
{
  "schema_version": 1,
  "program": "eefn",
  "version": "EXACT_RUNNING_VERSION",
  "executable_sha256": "EXACT_PACKAGED_EXECUTABLE_SHA256",
  "artifact": {
    "url": "https://YOUR_PUBLISHER/EXACT_PACKAGE",
    "sha256": "EXACT_PACKAGE_SHA256",
    "size_bytes": 123
  }
}
```

Checking fetches metadata only and **writes a proposal**, not a repair admission.
It never downloads the package, selects files, restarts, probes dependencies,
loads/infers a model or edits owner configuration. Pending/corrupt package
selection blocks a new source operation rather than guessing which build to use.

Approval requires the same source URL hash, exact descriptor bytes/hash, product,
version, executable hash, physical package slot, retained proposal and prerelease
choice. Changed whitespace is changed metadata too. Expired/future-clock,
missing, damaged or changed proposals refuse approval, not replacement selection.
Approval re-fetches the descriptor and checks the build/proposal again before
artifact admission and at the final existing publication authorization boundary.
It then uses the existing [verified package repair](PACKAGE-REPAIR.md) backend,
including full archive/inventory validation and same-build executable matching.

Refusal reports scope their no-new-admission/no-download fields to the current
call, retain a valid approval ID for correlation, and do not deny an earlier
attempt's admission. Inspect the independent receipt history for that ID.

The proposal UUID becomes the repair operation ID. An already admitted operation
or unavailable repair evidence blocks another approval before descriptor fetch.
Descriptor-fetch interruption alone has not admitted repair; a still-valid
proposal can be approved explicitly again. Once admitted, even a failed or
interrupted artifact attempt cannot replay that ID. Inspect its retained
[repair receipts](REPAIR-RECEIPTS.md); no automatic retry, recovery, rollback or
restart follows. Publication is not a running-runtime or dependency-health claim.

## Bounded retention and inspection

`.package-repair-plans.json` has schema 1, at most 128 proposals and at most
256 KiB. Proposals expire for approval after 24 hours but remain inspectable.
They contain digests, build/slot, size, consent and timestamps, **not source or
artifact URLs, query tokens, owner configuration or model data**. Digests are
bindings, not encryption or a defense against an attacker controlling the same
owner's files. Public errors/guidance never copy raw transport errors/URLs.

Publication shares the existing update lock, synchronized same-directory atomic
replacement and a prior-bytes fence. First creation refuses overwrite. Damaged,
future or full stores are not reset, pruned or deleted automatically. Loss of a
proposal means that ID cannot be approved; new checks generate new UUIDs, never
recover or reauthorize the missing proposal. Cooperating process serialization
does not claim protection against arbitrary same-owner tampering/power loss.

Offline `repair --status` exposes retained proposals independently of selection
and receipt inspection. It does not create metadata or grant approval; a damaged
proposal store leaves independently available package/receipt evidence visible.
Owner-directed retention/capacity tools remain future work. These bounded files
must not be treated as an unbounded event log or cleaned by incidental startup.

## Publisher metadata tool

After building and validating the exact package, run
`tools/new-repair-source.ps1` with `-Product`, `-Version`, `-Executable`,
`-Artifact`, `-ArtifactUrl` and a new `-Output` path in an existing directory.
It hashes the supplied packaged executable and final installer/ZIP, bounds the
artifact size, rejects unsafe artifact URLs and refuses output replacement.
It does not execute the inputs, download, publish, sign, use credentials or
change an update feed. The publisher is responsible for pairing the actual
packaged executable with its artifact and distributing metadata over trusted
HTTPS. HTTPS and matching digests do not prove a file benign, establish publisher
signature trust or resolve Defender detections.

## Verification boundary

Both final locked/offline workspace/all-target feature modes pass **1,002 tests
plus one ignored each**, including twelve new regressions. Headless apps/examples
build; all **twenty native suites** pass against the same seven frozen apps/helpers.
Sixteen product/harness/helper source hashes remain unchanged, all twenty public
result files report passed, and owned app/helper processes are terminal. Format,
affected JavaScript/PowerShell syntax, backend/browser source boundary and diff
checks pass. Evidence:

- `.validation/repair-sources-workspace-default-accepted.log`
- `.validation/repair-sources-workspace-headless-final.log`
- `.validation/repair-sources-native-accepted.log`
- `.validation/repair-sources-native-results.json`
- `.validation/repair-sources-source-hashes.json` (sixteen source/helper files)
- `.validation/repair-sources-native-hashes.json` (seven apps/helpers)

The final source suite is `repair-sources-lefU5R`; the Python suite is
`python-bridge-FCgM9v`. Rust regressions cover the backend's exact-byte
proposal/approval path through a private descriptor transport seam, while the
existing artifact admission/receipts path is unchanged. The native source harness
checks actual owner CLI source/build fences, metadata-only TLS fetch interruption,
read-only proposal inspection and the real publisher metadata writer. Its
proposal store is synthetic and its TCP gate never serves a TLS handshake or
artifact. Neither is a successful public HTTPS descriptor/package transfer,
physical two-PC, fresh installer, signing/Defender or complete first-run test.

The first native regression run stopped in the pre-existing Python bridge
fixture: an unchecked `.data` access hid the initial probe's actual failure.
Its original cause is unclassified, not retrospectively called a product fix.
A subsequent diagnostic retained responses and showed the one-second deadline
expiring without the plugin's start marker. Inspection and invocation each have
their own bridge deadline; the fixture incorrectly assumed the plugin/child
must have started. The test now retains private per-probe evidence, uses an
eight-second ordinary **fixture** budget instead of two, and keeps the explicit
one-second/under-four-second startup-inclusive deadline check. It records whether
that short attempt entered the plugin, never assumes it did.

A separate eight-second post-start deadline case requires real parent/child
markers and verifies both observed PIDs are retired; marker-triggered
cancellation, detached/inherited child cleanup, allocation ceilings, UTF-8 and
output/request/error bounds remain asserted. That diagnostic fixture passes in
`python-bridge-y58ZTD`. The twelve-second outer fixture guard and all product
runtime defaults are unchanged; there is no execution retry or deadline widening
in application code. The earlier failed `native-complete`/diagnostic runs are
not final acceptance. The complete final native rerun is `native-accepted` above.
Its short deadline observed parent/child startup, and the separate post-start
deadline independently confirmed both observed processes retired. Memory controls
still verify 192 MiB allocations succeed without a ceiling and fail under the
128 MiB job ceiling in both the parent and child cases.

Automatic trusted-source discovery/consent policy, repair progress/cancellation,
owner-directed retention, post-restart dependency health, approved model/default
quality and physical/installer/stable-release gates remain open. No installed
node, remote node, UI, version, release asset or automatic-update policy is
changed. CI remains local at the owner's request.
