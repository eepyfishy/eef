# Packaged public repair source discovery (unreleased)

An optionally configured publisher package now supplies the source for these
shared local backend commands, without owner URL, hash, size or JSON entry:

```text
eefn repair --check
eefn repair --status
eefn repair --approve PROPOSAL_UUID
```

Use `eef repair` for the coordinator. Prerelease builds still require explicit
`--allow-prerelease` for checking/approval. No automatic update/repair path gains
prerelease consent. Existing `--manifest HTTPS_DESCRIPTOR` remains an explicit
per-call override, including private query-bearing descriptor links; it does not
rewrite the hint. Raw artifact repair, receipt inspection and rollback remain.
Hint-less packages with these commands ask for an explicit source, not JSON edits.
Older binaries may need an upgrade before the command implementation is available.

This closes the publisher-source **lookup** gap. It is not automatic dependency
diagnosis/repair, a configured background repair policy, an approved model,
post-restart health verification, remote node source planning or stable release
acceptance. It does not start a daemon, connect/register a node, choose a model,
enable a capability, grant remote-update permission or restart either process.

## Current-bundle and consent boundaries

`RuntimeLayout` selects the **invoked executable's bundle**, never the pending
selector, old installation root, working directory or environment. An unmarked
flat developer layout has no implicit packaged source. Managed flat bundles use
their marker; direct versioned bundles use their own directory. Product-specific
filenames avoid borrowing the other role's source. For example,
`eefn.repair-source.json` contains:

```json
{
  "schema_version": 1,
  "program": "eefn",
  "version": "EXACT_PACKAGED_VERSION",
  "manifest_url": "https://YOUR_PUBLISHER/EXACT_VERSION/eefn-repair.json"
}
```

The strict file is at most 8 KiB. The schema, product and exact invoked version
must match. Non-file/symlink metadata, unknown fields, invalid/oversized data,
HTTP, URL credentials, query strings and fragments are refused. The file is
**public package metadata**, never a secret store or a permission grant. Its
availability is not a signature, network reachability or runtime-health claim.
The normal descriptor fetch still uses verified HTTPS and the existing 20-second,
16 KiB/five-HTTPS-redirect bounds. Full exact-build descriptor/artifact checks
remain in [source approval](REPAIR-SOURCES.md) and [repair](PACKAGE-REPAIR.md).

The owner sees the packaged public publisher URL in status/check output before
choosing to approve. Explicit/private URLs are still not echoed; neither form
enters proposal/receipt stores as raw URLs. A valid hint is only lookup data, not
automatic publisher trust. No source is inferred from an ordinary update feed.
Nothing here verifies a publisher signature or proves a file benign.

Approval resolves the current source again and requires the original proposal's
URL hash and exact descriptor bytes/build/slot/consent. A different hinted URL
is rejected before descriptor fetch. For implicit sources, the selected hint's
exact bytes are additionally fenced across asynchronous metadata retrieval and
the existing pre-admission/final-publication authorization boundaries. A detected
changed, removed or damaged hint refuses publication. These are point-in-time
fences under the existing update lock, not an atomic file-revocation transaction
or protection against arbitrary same-owner writes after the last observation.
No source fallback, repair retry, reset, retention cleanup or restart follows.
Proposal UUIDs still become the existing non-replayable repair operation IDs.

Offline `repair --status` adds `repair_source_hint`. Its configured/not-configured/
unavailable states are independent of selection, retained proposals and receipts.
The public source, `authority_granted: false`, `network_inspected: false` and
`publisher_signature_verified: false` describe the invoked build, not a separately
running service. An absent/invalid optional hint does not alone fail otherwise
valid package status, disable core controls or block an explicit source override.
Inspection does not create, repair or rewrite the hint.

## Publisher packaging

`tools/new-repair-source-hint.ps1` accepts `-Product`, `-Version`,
`-ManifestTemplate` and a new `-Output` file in an existing directory. Both
`{program}` and `{version}` placeholders are required, with no unresolved braces,
credentials, queries or fragments. Uppercase product input is normalized. The
UTF-8 hint uses synchronized same-directory create-new staging and refuses
replacement. `-ResolveOnly` validates/resolves a public template without writing
a file. The helper does not fetch, execute, upload, sign or change a feed.

The Windows bundle builder accepts these optional **paired** parameters:

```text
-RepairSourceTemplate 'https://YOUR_PUBLISHER/v{version}/{program}-repair.json'
-RepairArtifactTemplate 'https://YOUR_PUBLISHER/v{version}/{program}-installer.exe'
```

Neither publisher nor repository is hardcoded. Omitting both preserves existing
packaging behavior. Supplying just one, invalid URLs/templates, or an existing
`eef-repair.json`/`eefn-repair.json` output refuses preflight before downloads/builds.
Choose a fresh release directory instead of replacing published checksum data.
The builder:

1. Adds the correct role/version hint before generating the bundle inventory.
2. Builds the existing installer artifacts and performs the existing optional
   signing and scan stages, without bypass/exclusion changes.
3. Runs `new-repair-source.ps1` against each actual packaged executable and final
   installer to emit `eef-repair.json` and `eefn-repair.json` sidecars. Those
   exact artifact size/SHA pins therefore follow signing/scanning.

These small descriptors accompany the two application installers; no model or
third installer is added. Publish the descriptors and corresponding installers
at their configured URLs **only after the whole build succeeds**. No uploader or
release action is invoked by these helpers. Multiple output publication is not
an atomic transaction: a failed build may retain newly created partial metadata
for investigation; it is not a successful/publishable release. Templates must
refer to the actual supported installer/ZIP artifacts, not unrelated MSI bytes.
Fresh complete packaging, signing/Defender and installer acceptance remain gates.

## Verification boundary

The all-target compile check passes. Both final locked/offline workspace/all-target
feature modes pass **1,008 tests plus one ignored each**, including six added
regressions. Headless apps/examples build, and all **twenty-one native suites**
pass against the same seven frozen apps/helpers. Twenty runtime/harness/helper
source hashes remain unchanged, all twenty-one public results report passed,
and owned app/helper processes are terminal. Format, affected script syntax,
backend/browser source boundary and diff checks pass. Evidence:

- `.validation/repair-hints-workspace-default-final.log`
- `.validation/repair-hints-workspace-headless-final.log`
- `.validation/repair-hints-native-complete.log`
- `.validation/repair-hints-native-results.json`
- `.validation/repair-hints-source-hashes.json` (twenty runtime/harness/helper files)
- `.validation/repair-hints-native-hashes.json` (seven apps/helpers)

The final hint suite is `repair-hints-Uw6cd4`. Six added Rust cases cover
managed/current-bundle/product resolution, strict public hint validation/bounds,
optional status and explicit private-source compatibility, implicit check output,
changed-hint approval refusal, and final guard invalidation. Native fixtures
exercise actual owner commands, real publisher helpers, negative builder
preflight and static inventory/sign/scan ordering. Their proposal store is
synthetic and TCP gates never serve a TLS handshake or artifact. They are not a
successful public descriptor/artifact transfer, full packaging run, physical
two-PC, signed/Defender-cleared installer or complete first-run acceptance.

Trusted automatic setup/repair policy, progress/cancellation/retention,
post-restart dependency health, approved model/default quality, fresh physical
tests and stable-release gates remain open. No installed/remote node, visual UI,
model/default, version, release asset or automatic-update policy is changed.
CI stays local at the owner's request.
