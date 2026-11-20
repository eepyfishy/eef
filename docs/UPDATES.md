# Updates

eef and eefn share the same verified, versioned updater. Official packages use
this repository's separate eef and eefn manifests by default. The URL and
policy are ordinary configuration values: forks and self-hosting users can
replace the feed or disable it from the dashboard.

From v0.4.0a3 onward, feed-based checks and installation are **stable-only** for
both roles, including `auto`, dashboard/API apply and approved `node.update`.
Prerelease versions (alpha, beta, RC or any hyphenated prerelease) report
`update_available:false` and `prerelease_blocked:true`. Installation independently
rechecks the downloaded manifest and rejects prereleases before downloading the
artifact or creating an installation directory. Installing an alpha does not
subscribe that machine to alpha updates. Stable feeds remain on stable releases.

Feed installation rechecks that the fetched version is newer than the running
app and refuses to replace a pending update. The verified archive must contain
`bundle.json` beside the target executable. Its `name` and `version` must match
the app role and the exact version in the feed. Forks and custom feeds must
include that metadata in each role's bundle. A missing or mismatched record
fails before the updater switches the installed selection.

Backend status records paused feeds and blocked prereleases alongside available
stable releases. A failed check clears the previous availability result until a
check succeeds. A manual feed install reports a paused feed or a prerelease
block explicitly.

Prereleases require an explicitly chosen installer or owner-pinned explicit
command, never an automatic feed subscription. A version-pinned test
manifest may be used only for an owner-supervised migration from an older node
without this guard, with automatic checks disabled throughout and its original
stable feed restored before launching the new build. It is never a default feed.

```json
{
  "version": "0.3.0",
  "url": "https://releases.example.org/eef-installer.exe",
  "sha256": "64-lowercase-or-uppercase-hex-characters"
}
```

The distribution may be a ZIP or an official self-extracting eef installer; its
payload must contain the executable being updated. Downloads are rejected if
the SHA-256 does not match. Files are extracted with traversal-safe ZIP paths
into `versions/<version>`, and
`current.txt` is switched only after validation. On the next launch, the stable
root executable hands off to the selected version. `previous.txt` supports
rollback.

A ZIP can place the executable at its root or inside one wrapper folder. When
the root lacks the executable, the updater requires a single matching wrapper.
The wrapper supplies the app's bundled files while mutable settings and update
selection stay at the existing installation root.

Explicit same-build Windows [package repair](PACKAGE-REPAIR.md) uses immutable
repair generations and slot-aware restart/rollback without changing feeds or
owner settings. Local `repair --status` and `repair --rollback` work without the
dashboard or a running service. Automatic dependency repair remains future work.

Policies:

- `off` — do not check.
- `prompt` — check and report availability; never install automatically. This
  is the default when a manifest URL is configured.
- `auto` — download and install a verified update, then report that a restart
  is required.

eef reports update events and exposes `/api/update/status`, `/check`, and
`/apply`. Both 0.3.2 dashboards offer Check for updates, Install update, and
Restart now; eefn exposes `/api/update/check` and `/api/update/apply` too.
Restart hands off to the selected installed version, without needing a user
to find and terminate a process. Automatic updates install but do not interrupt
running work by forcing a restart. eefn also supports
`--check-update`, `--rollback`, and the permission-controlled `node.update`
capability.

The updater verifies HTTPS/pinned artifact SHA-256, not an Authenticode publisher
or a signed manifest. Packaging tools support optional owner-supplied signing
credentials; this development checkpoint creates no newly signed artifacts and
does not close signing/Defender acceptance. Signed-manifest/publisher enforcement
remains outstanding.
