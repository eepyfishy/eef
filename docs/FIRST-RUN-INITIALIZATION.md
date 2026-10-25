# Backend-owned coordinator initialization (under development)

The coordinator now has a backend initialization path, independent of its
browser UI and of an MSI custom action. It is a prerequisite for MSI payloads
that ship distribution templates without owning mutable user configuration,
not an implemented MSI installer or complete automatic onboarding.

```text
eef initialize
eef --config ABSOLUTE_CONFIGURATION_PATH --json initialize
```

This explicit offline command requires the coordinator using that configuration
to be stopped. It creates missing saved configuration from the bundled template
with a distinct random 256-bit legacy secret. It does not start networking,
register a node, enable permissions, choose/download a model or request a restart.
Reports never print a generated or externally provided secret. Mapping/policy
validation and satisfying the legacy-secret requirement are not runtime health,
private enrollment, TLS connectivity or model readiness.

Existing configuration bytes and credentials are preserved, including formatting
and invalid settings. Invalid saved credentials without a usable explicit
external secret produce an unsuccessful report; initialization does not silently
rotate them. Invalid settings are not replaced by defaults, and parser errors
are reported without quoting configuration content. An existing directory or
symlink is refused rather than replaced. Existing environment overrides retain
their runtime meaning; their values are not copied into the generated file.

The command is offline like repair/doctor: it does not execute a selected
version, repair a broken selection or alter `current.txt`. It operates on the
invoked installation's configuration. Existing explicit non-default config paths
remain explicit; normal command behavior is not an initialization side effect.

## Ordinary startup and shared installer behavior

Coordinator service startup performs missing-file creation while holding its
existing per-config instance lease. Existing settings are not repaired or
rotated. Creation stages and synchronizes complete bytes before no-clobber
publication. A concurrent existing owner file wins, with no copy-over-live-file
fallback. This is not a multi-file transaction or a directory power-loss guarantee.

Both roles now resolve a managed package's default mutable configuration under
the persistent installation root even when it does not exist yet. They do not
create it under the caller's unrelated working directory or a versioned bundle.
Unmanaged development lookup and explicit non-default paths retain their prior
meaning. A bundle marker determines lookup policy, not publisher trust.
The implicit firmware repository for a managed default coordinator configuration
also follows the persistent installation's `data/firmware` directory. The stock
relative value is a default even though it appears in the bundled template.
Non-default repository settings and unmanaged/external configuration defaults keep their
previous meaning; no old files are copied or migrated. The first native run
exposed a directory leak into the unrelated working directory; that run is
retained as failed evidence, not accepted.

The EXE installer's explicit placeholder-secret preparation uses the same shared
backend helper. It still preserves valid owner secret bytes without rewriting.
Its explicit preparation remains separate from ordinary startup's create-only
behavior; this does not implement concurrent install/service migration, rollback,
MSI repair, uninstall, startup preferences or signing/Defender acceptance.

New files inherit the destination's filesystem protection on Windows; this change
does not claim Windows ACL hardening or isolation from other local OS accounts.
Use dedicated per-user installation/data directories. Existing same-PC private
enrollment keeps its explicit trust/restart consent and all saved/applied fences.
Automatic legacy registration is not proof of reviewed private workload readiness.

## Verification status

Three focused offline report tests, four creation tests, four shared coordinator
tests and the existing configuration/lookup-focused group pass. Fresh headless
apps/helpers build, and `first-run-LfIP4P` passes the new public-command managed
first-run fixture after the stock firmware-path correction. Earlier compilation
and working-directory failures remain retained, not accepted. Both full feature
modes passed 1,130 tests plus one existing ignored profiler each, across
fourteen groups. The subsequent native run passed ten suites, including the new
initialization test, before package-setup failed its unchanged 15-second metadata
connection limit. That failed run and its evaluated hashes are retained; it is
not a complete regression pass.

A separate bounded diagnostic with a new synthetic policy observed connection
only about twenty seconds after launch under substantial unrelated host load.
Same-file measurements identify repeated unoptimized executable hashing as
avoidable cost: the prior implementation takes 3.8–4.5 seconds for the 68 MB
debug app, while the existing `ring` SHA-256 implementation takes 0.15–0.19
seconds with identical digests. Streamed file checks now use `ring`, still
rereading the full file each time with no digest cache, deadline extension or
authorization removal. Known-vector, compression/read-buffer boundary,
same-size changed-file and read-error coverage is added. Fresh headless apps and
helpers build; the unchanged package-setup fixture `package-setup-NJbCQb` passes
for both roles with the original deadline. Both focused digest-correctness tests
also pass. Both corrected full feature modes now pass 1,132 tests plus one
existing ignored profiler each across fourteen groups. All thirty-two native
suites pass sequentially with the evaluated 307 source/resource/workflow files
and nine headless artifacts unchanged. Independent acceptance is recorded in
`.validation/initialization-acceptance.json`; no matching fixture apps remain.
The final managed first-run fixture is `first-run-Y6Br2G`.

The cached real-CPU owner workflow also passes at the matching headless apps,
including ordinary grants, retained results without replay, model withdrawal,
unload/reload and idle/on-demand lifecycle. No weights were downloaded or copied;
no matching model/app process remains. This is mechanical workflow evidence,
not default/interpreter-quality approval. The separate default-feature app epoch
passes the unchanged browser regression in `first-run-FAHpRo`, including copying,
focus, forms and retained job state. Both epochs have independent hash/process
audits; rebuilding the browser apps does not replace the earlier headless proof.

The subsequent standalone MSI payload planner has thirty-four separate local
synthetic cases; its CI step is an explicitly separate workflow extension.
No Rust application source changes follow those accepted app epochs. Published
`4ea7964` passes all four jobs in
[run 40176167808](https://github.com/eepyfishy/eef/actions/runs/40176167808), with
thirty native suites and the separate planner step verified. Subsequent MSI
authoring work and broader release acceptance remain separate. Full automatic onboarding,
approved interpreter/default quality, physical two-PC and stable v0.4.0 gates
remain open.

`tools/test-coordinator-initialization.mjs` adds this coverage without replacing
the existing `tools/test-first-run.mjs` test. Its fresh managed app launches
require the default local ports to be free; a conflict fails the test rather than
stopping another application. State/discovery/credentials are fixture-specific,
and no models or installed nodes are involved. Initial automatic registration
uses existing legacy pairing; the subsequent private enrollment explicitly
authorizes the existing coordinator/node runtime restart workflow. This does not
claim automatic private enrollment, selected-model health or physical networking.
