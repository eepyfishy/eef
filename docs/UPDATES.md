# Updates

EEF and EEFN share the same verified, versioned updater. Official packages use
this repository's separate EEF and EEFN manifests by default. The URL and
policy are ordinary configuration values: forks and self-hosting users can
replace the feed or disable it from the dashboard.

From v0.4.0a3 onward, feed-based checks and installation are **stable-only** for
both roles, including `auto`, dashboard/API apply and approved `node.update`.
Prerelease versions (alpha, beta, RC or any hyphenated prerelease) report
`update_available:false` and `prerelease_blocked:true`. Installation independently
rechecks the downloaded manifest and rejects prereleases before downloading the
artifact or creating an installation directory. Installing an alpha does not
subscribe that machine to alpha updates. Stable feeds remain on stable releases.

Prereleases require an explicitly chosen installer. A version-pinned test
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

The distribution may be a ZIP or an official self-extracting EEF installer; its
payload must contain the executable being updated. Downloads are rejected if
the SHA-256 does not match. Files are extracted with traversal-safe ZIP paths
into `versions/<version>`, and
`current.txt` is switched only after validation. On the next launch, the stable
root executable hands off to the selected version. `previous.txt` supports
rollback.

Policies:

- `off` — do not check.
- `prompt` — check and report availability; never install automatically. This
  is the default when a manifest URL is configured.
- `auto` — download and install a verified update, then report that a restart
  is required.

EEF reports update events and exposes `/api/update/status`, `/check`, and
`/apply`. Both 0.3.2 dashboards offer Check for updates, Install update, and
Restart now; EEFN exposes `/api/update/check` and `/api/update/apply` too.
Restart hands off to the selected installed version, without needing a user
to find and terminate a process. Automatic updates install but do not interrupt
running work by forcing a restart. EEFN also supports
`--check-update`, `--rollback`, and the permission-controlled `node.update`
capability.

Release signing beyond HTTPS plus manifest/artifact SHA-256 is not implemented
yet. Projects with a stronger threat model should publish signed manifests and
verify them before enabling `auto`.
