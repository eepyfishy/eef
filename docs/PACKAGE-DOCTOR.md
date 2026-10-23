# Bundled runtime doctor (unreleased)

The shared backend exposes local owner diagnostics independently of a dashboard,
saved configuration or running EEF/EEFN service:

```
eefn doctor
eefn doctor --probe
eefn doctor --probe --media
```

Use `eef doctor` for the coordinator; it checks Python, never a local LM or media
stack. `--json` supplies structured reports. Normal inspection starts no runtime
and creates no policy/history. `--probe` explicitly permits starting package
executables and importing selected core Python modules. Node-only `--media`
additionally imports the six stock media libraries as one diagnostic group.
No individual file/folder/application grants or JSON editing are required.

## Selected package, not an uninspected service

The inspector resolves the existing semantic/physical package selection and
inspects that generation's files directly. It does **not** launch the selected
EEF/EEFN application or hide diagnostic failure behind bootstrap-child exit codes.
This also supports same-version repair generations without borrowing old-root
runtime files. The inspector and selected application versions are reported
separately; selected application launch/ABI compatibility remains unchecked.

If selection is unreadable or the selected executable is missing, ordinary
invoked-package metadata can still be displayed where valid. Runtime probing is
blocked, not silently redirected to an older package. Use `repair --status` to
inspect/repair or explicitly roll back selection. This command changes none of it.

Reports identify `selected_package`, `invoked_package` or
`invoked_metadata_only` scope. `selection_pending` describes a difference from
the inspector invocation (or unknown selection), not an observed service's
restart/adoption state. `selected_target_resolved` must not be confused with a
running service or successful inference.

## What explicit probes establish

- Python starts with isolated mode, UTF-8 and bytecode writing disabled. It imports
  `json`, `base64`, `ctypes` and `importlib.util`, then returns a strict bounded
  version/64-bit/module observation.
- Node llama.cpp runs only `--version`; its build/commit response is parsed.
  Current and legacy version formats are supported, including the actual
  `0.3.0-dev (build ..., commit ...)` output of the existing bundled runtime.
- Known Python/build versions declared by the bundle marker are compared to the
  observed versions; mismatches are not accepted as passing probes.
- Explicit node media checking imports `PIL.Image`, `pyautogui`, `sounddevice`,
  `soundfile`, `cv2` and `pyttsx3`. Missing/failed imports are named without exposing
  private exception output. It does not request capture, playback, typing or other
  feature operations. Library initialization may still inspect OS/device state or
  have side effects; this is **not** a sandbox or a no-code-execution operation.

No inherited PATH/working-directory/PYTHONHOME/PYTHONPATH fallback chooses another
runtime or supplies a development DLL. Probes clear the inherited environment,
retain ordinary OS/profile/temp variables, set PATH to the runtime's own directory
and use that directory as CWD. Provider/plugin overrides and credentials are not
forwarded. Windows still supplies its ordinary OS DLLs; this is not a complete
module-origin attestation or protection against a compromised OS.
The bundle marker is bounded and checked for role/version/architecture, not treated
as publisher trust. Runtime paths reject symlinks/reparse points and escapes.
Executable observations are bounded to 512 MiB each. Before/after subprocess work,
fresh selection and application/marker/runtime-executable identities are compared.
Changed or unresolved identity invalidates all rows; work is never replayed against
a new target. These are point-in-time observations, not a lease against arbitrary
same-owner edits or a complete DLL/environment fingerprint.

## Lifetime and resource bounds

Runtime probes currently require Windows. Owned children start suspended, join
the existing kill-on-close job before running, and have a job-wide commit ceiling
(512 MiB by default, explicit range 64–2,048 MiB). This is not working-set RAM,
VRAM or external-effect containment. There is no uncontained fallback.

Each probe has an absolute pipe/root/descendant deadline (default 10 seconds,
explicit range 1–30). Each output stream is bounded to 64 KiB. Readers belong to
the probe future, not detached tasks. A successful root's remaining descendants
are terminated/observed before inherited-pipe EOF is accepted. Failure/timeouts
close the owned job and attempt bounded root retirement; unconfirmed retirement
is not reported as success. App startup and bounded filesystem inspection/hashing
are separate overhead, so the timeout is not a whole-command wall-clock promise.

```
eefn doctor --probe --timeout-seconds 15 --max-job-memory-mib 768
```

The local CLI briefly serializes/restores process error mode around synchronous
child creation to suppress critical loader-error dialogs. Children inherit that
mode, as described by [Microsoft's SetErrorMode contract](https://learn.microsoft.com/en-us/windows/win32/api/errhandlingapi/nf-errhandlingapi-seterrormode).
It does not suppress UAC, change Defender/OS trust, elevate, or span async waits.
No daemon endpoint or unattended automatic diagnostic authority is introduced.

## Reading results honestly

`success` means inspection completed and, if probes were explicitly requested,
all requested probes passed. Metadata inspection may succeed with missing files;
it is not a health certificate. Read the scope, individual states, selection and
`all_requested_probes_passed` (null if not requested). An unconfirmed result must
not become a passing result through retry, source fallback or history reset.

The command does not inspect a live node/coordinator, owner runtime overrides,
Ollama, model inference, permissions, network registration, hardware usability,
continuous media or safe migration. `system_ready` remains null. A passing
llama.cpp version probe does not make a model available or choose that backend
over the owner's Ollama configuration. Optional media packages may intentionally
be omitted; an import failure is not proof of package corruption or malware.
Do not bypass an antivirus detection because a probe passes or fails.

No model download, selection, configuration/permission change, package repair,
automatic retry, startup/registration change or restart is requested. Next-step
guidance and `repair --status` remain separate from explicit repair and restart
consent. Automatic post-restart/live dependency/inference health, remote diagnostic
authority and complete non-coder onboarding remain open requirements.

## Validation boundary

Final local checkpoint (2026-10-23): both locked/offline workspace/all-target
feature modes pass 1,045 tests plus one ignored each, and headless apps/examples
build. All twenty-four native suites pass in one sequential run, including the
doctor, scheduler and pipeline regressions. Every public fixture result reports
`passed: true`; thirty-six source/helper and nine rebuilt native hashes remain
unchanged, and no matching fixture app/helper processes remain observed.
Logs/inventory: `.validation/package-doctor-timer-{default,headless}-final.log`,
`.validation/package-doctor-timer-native-complete.log` and
`.validation/package-doctor-timer-acceptance.json`.

This includes the [single-view timer observation](AUTOMATION-SCHEDULES.md#single-view-timer-observation-2026-10-23),
but does not establish a root-cause fix for prior intermittent stalls, loaded/
physical timing, full onboarding/model quality or stable release acceptance.
The failed/unknown occurrence below is retained and never replayed.

### Retained preliminary validation history

Initial checking passed for the first draft, but final validation is pending.
One expanded-check process ended with only an incomplete trace and no test result;
it is retained rather than classified as a source error or a passing run. The
revised selected-target check/focused run is tracked separately. Do not use the
prior product checkpoint as acceptance for these new files.

The selected-target all-target check passed. Its preliminary focused run passed
thirteen of fourteen tests; the remaining assertion compared canonical Windows
runtime paths with an unnormalized expected fixture path. The expected path is
now normalized without relaxing the selected-generation requirement. The failure
is retained in `.validation/package-doctor-focused-selected.log`; the corrected
full headless run uses `.validation/package-doctor-workspace-headless-final.log`
and thirty-two final source hashes in
`.validation/package-doctor-final-source-hashes.json`. Final acceptance is still
pending; the preliminary run is not acceptance for the corrected test.

The corrected final headless workspace/all-target run now reports 1,043 passed
plus one ignored across fourteen target groups, including all fourteen doctor
cases. Its log completes and all thirty-two frozen source/helper hashes match.
The prior process handle is no longer available after continuation; no compiler
or cargo process remained, so the completed log was inspected instead of
restarting that run. Default-feature and fresh native validation are still pending.

Both pre-isolation full feature modes subsequently passed 1,043 plus one ignored
and headless apps/examples built. However, the **first native doctor suite failed**
in `package-doctor-Yr7zxr`: removing its local `python311.dll` still allowed Python
to start by borrowing the deliberately supplied donor PATH. This contradicted
the intended package-scoped dependency check. The initial native log is retained
as `.validation/package-doctor-native-complete.log`; no later suite ran in it.

The probe command now isolates CWD/PATH/environment as described above, and a new
unit test covers command scoping without modifying the parent environment. The
synthetic Rust fault helper receives its known libunwind dependency beside its
test executable rather than via host PATH, and checks that the parent's private
environment sentinel was not forwarded. This changes product behavior, so the
pre-isolation full tests are not final acceptance. Corrected full/native validation
is pending in separately named `package-doctor-isolated-*` logs; no condition was
removed or reinterpreted to make the missing-DLL case pass.

Both corrected isolated locked/offline workspace/all-target feature modes now
pass 1,044 tests plus one ignored each, including all fifteen doctor cases.
The logs are `.validation/package-doctor-isolated-headless-final.log` and
`.validation/package-doctor-isolated-default-final.log`. Thirty-two source/helper
and nine rebuilt native app/helper hashes match the isolated snapshots.

The rebuilt native run in
`.validation/package-doctor-isolated-native-complete.log` passes the doctor suite
(`package-doctor-ScnyB3`) and fifteen subsequent suites. The doctor observes real
stock Python/llama.cpp, correctly rejects the missing-DLL donor-PATH case, and
passes its bounded synthetic failure/selection cases. The broader run then fails
in the existing local-pipeline bounded recurrence fixture
(`local-pipelines-TPd8qe`): one occurrence completed; the second was
accepted/running when the harness's 45-second observation deadline expired.
The retained database does not prove that second attempt completed or failed
before fixture shutdown. No job is replayed to infer its outcome.

Read-only comparison finds slow journal checkpoints in this failed fixture
(up to about 3.6 seconds) and earlier passing fixtures; it does not yet establish
the slowdown's cause or prove package-doctor causality. Execution limits, policy
expiry, observation deadline and completion assertions remain unchanged.
The original failure is preserved; remaining native suites and a separately
identified fresh-fixture investigation do not erase it. Final broader native
acceptance and release gates remain open.

The remaining six suites subsequently pass against the same frozen binaries:
Python policy/bridge, permission commands, node commands, node credentials and
model downloads. Their log is
`.validation/package-doctor-isolated-native-remaining.log`, with each fixture's
public result reporting `passed: true`.

A separately identified fresh local-pipeline fixture (`local-pipelines-QLhLZm`)
also passes the original assertions, 45-second observation deadline, 60-second
policy lifetime and execution limits. Its log is
`.validation/package-doctor-isolated-pipeline-reproduction.log`. These are new
fixture jobs, not retries of the unknown second occurrence in the failed run.
Thus all twenty-three native suites have individual passing results for these
binaries, but the original sequential run still failed. Intermittent scheduler
latency remains an open reliability finding; neither a root-cause fix nor a
clean complete sequential acceptance run is claimed. Source-boundary, script
syntax, formatting and diff checks also pass. No release or installed-node
update follows from this diagnostic checkpoint.

A subsequent single-view timer observation edit is described
in [the scheduler tracker](AUTOMATION-SCHEDULES.md#single-view-timer-observation-2026-10-23).
The preceding full/native counts describe the pre-timer source and binaries;
they are not acceptance for that later edit.

Unit cases cover default no-execution, role/flag scope, strict replies/version
matching, missing paths, metadata refusal, identity/selection invalidation,
no fallback/replay, selected generations and restoration of error mode.
The native harness uses existing stock Python/llama.cpp in isolated hardlinked
packages and a separately compiled fault helper, without model downloads/copies.
Planned observations include actual imports/version launch, missing extension/DLL,
ignored donor environment, unsafe junction, bounded output, separate early and
post-start deadlines, inherited-pipe retirement and damaged/selected generations.
Synthetic helper protocol replies are not real Python/inference acceptance.
Physical two-PC, successful repaired-package/live-service inference, fresh full
packaging/MSI/signing/Defender and stable-release acceptance remain unproven.

CI remains local and unpublished. No installed/remote node, UI, model/default,
version, release artifact or automatic-update policy is changed by this work.
