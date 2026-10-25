# Core controls with unavailable model files (unreleased after alpha.6)

An unavailable selected model must not make ordinary node controls depend on
repairing or deleting that model first. The previous settings validator checked
every configured GGUF path on every save. Deleting one selected file therefore
blocked even permission revocation and unrelated settings changes. It also
checked relative paths against the process working directory instead of the
configuration directory used at startup, and did not check new projector paths.

## Structural validation versus artifact changes

Core saves continue validating identity, authentication, policy, limits, model
metadata, unique model IDs and nonzero/unique model ports. Artifact availability
is checked separately against the latest saved configuration under its existing
lock. Unchanged, unambiguous typed bindings retain the same model ID, model path
and optional projector path. Their files may be unavailable without blocking
unrelated owner changes. No reference, model role or cache file is implicitly
cleared, replaced, normalized or recreated.

New identities, model paths or projector paths require existing regular files
for the new binding. A different ID cannot borrow another selection's missing
reference; a removed reference cannot be resurrected by an old settings snapshot
unless its artifacts are available again. Relative references in generic settings
resolve against the configuration directory, exactly as at startup. Typed
select-GGUF commands still require absolute existing paths; installed-GGUF
selection retains its existing index and checksum checks.

The central writer and remote proposal/save preflight use the same distinction.
Remote changes still need current local owner consent or remain only proposals.
All changes merge against current saved state. Credential, permission, bootstrap
and other controls do not gain authority from a missing file. Existing saved
permission changes continue fencing affected grants without assuming an app
restart already applied everything or that ongoing external effects stopped.

## What this does not prove

Keeping a saved reference is not successful installation, model readiness, file
integrity, runtime compatibility, license review or semantic quality. Basic
regular-file checks on a new raw path do not establish those facts either.
Model preparation/loading and verified installed-artifact selection remain
separate operations. No save implicitly loads, downloads, unloads or restarts a
model. Removing a file is not proof an already-running model/service terminated;
owned-process lifecycle and shared-provider limits are unchanged.

This does not repair arbitrary malformed node settings, permit invalid model
metadata/ports, introduce new missing references, or bound every filesystem
probe on an explicitly selected new network path. It removes filesystem-health
dependency for retained valid reference metadata, not all configuration errors.
Automatic bootstrap, default-model quality, continuous workloads, physical
two-PC and installer/signing/Defender/stable-release gates remain open.

## Regression evidence

With the original implementation, model-reference-before-corrected-fixture.log
under .validation records all three intended failures: deleted weights block a
permission save, a new config-relative existing file is rejected, and a new
missing projector is accepted. The initial fixture had an unrelated incomplete
HTTP permission preference; that was corrected before this product baseline.
No HTTP request is executed by the policy check.
The final fixture also validates a supported HTTP send grant before its policy
probe; the earlier policy-only probe did not establish grant admissibility.

After the fix, model-reference-after.log passes all three. The revocation case
checks the affected grant's policy before and after the save, unchanged model
references and unrelated settings, and absence of model-file recreation. The
other cases check correct path resolution and refusal before any settings write.
These synthetic temporary files are not real model weights or runtime acceptance.
The final package-only headless run model-reference-after-final.log also passes
all three named tests, including the supported grant's structural validation.

The public native suite tools/test-unavailable-model-controls.mjs runs independent
node/coordinator processes with pinned TLS and no visual UI. It creates and
deletes only its own two synthetic artifact files before startup. The final
unavailable-model-PdWpTG run verifies an applied eager-startup policy, usable core
controls after model startup fails, local permission revocation, ordinary owner
saves, unapproved remote proposals without configuration mutation, approved
remote bootstrap/name changes, unchanged model references, rejection of new or
rebound missing artifacts, reviewed system-information work, explicit restart,
explicit model removal and refusal of stale missing-reference restoration.
It neither downloads a model nor demonstrates model readiness or physical
two-PC acceptance. A failed eager attempt is not a claim that a runtime process
never starts, only that no model becomes loaded/ready.

Existing model-bootstrap-egUddG, model-metadata-FsgHLl,
installed-selection-ztmNBZ, process-lifetime-qEhNZh,
local-pipelines-nnX1BE, python-policy-xDJsx7, python-bridge-lsg4Oo,
permission-commands-6cBdic, node-commands-tLEPSv, node-credentials-XuvfL3
and model-downloads-Eh9KJA also pass. These are isolated native regression
fixtures, not installed applications or the physical remote node.

Both locked/offline workspace/all-target feature modes pass 904 tests plus one
ignored profiler each: eef 268, owner CLI 21, installer 6, eefn 584 and node CLI
25. The final default run follows the fixture's supported-grant validation fix;
the final headless run also includes that fix. Logs are retained under
.validation/model-reference-workspace-default-final.log and
.validation/model-reference-workspace-headless-final.log. Final headless
applications/examples build. These checks do not close the model bootstrap,
continuous-workload, physical-device or stable-release gates above.

The final eef/eefn executables and both native process/Python fixture helpers
match all four hashes captured before the twelve native suites in
.validation/model-reference-native-hashes.json. Source boundary, formatting,
JavaScript syntax and diff checks pass. No evaluated application/helper or
llama-server process remains live. No installed/remote node, model
selection/default, visual UI, version, installer or update policy changed.
