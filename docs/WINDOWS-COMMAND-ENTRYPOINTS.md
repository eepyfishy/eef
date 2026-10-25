# Windows command entrypoints (locally verified, unreleased)

The physical-node preflight exposed a command-first usability defect: direct
Windows PowerShell invocations of the GUI-subsystem role executables produced
empty redirected output and no observed exit status, while explicit child pipes
returned the expected read-only reports. The initial empty/null observations are
not evidence of denied node consent or successful commands. They remain recorded
separately from the subsequent explicit-pipe, identity-checked reads.

[Rust documents](https://doc.rust-lang.org/std/io/struct.Stdout.html) that detached
GUI processes can have null standard handles with silently ineffective writes.
[Microsoft documents](https://learn.microsoft.com/en-us/windows/console/console-allocation-policy)
the different shell waiting behavior for console and GUI subsystem images.
Attaching output alone does not establish correct command completion/exit status.

## Implementation and boundaries

Both public `eef.exe` and `eefn.exe` entrypoints now use the normal console
subsystem. Commands, help and parsing errors retain terminal/pipe handles and
ordinary shell waiting/status behavior. Their names, command grammar and owner
APIs are unchanged; no new UI controller or model behavior is introduced.

Only a successfully parsed service invocation may release a console allocated
exclusively to that process. A shared/inherited shell is never detached; valid
non-console stdin/stdout/stderr handles prevent detachment, preserving redirected
service logs and pipes. Uncertain ownership leaves the console alone. This is
normal background-service behavior, not a UAC/Defender bypass or proof of
flicker-free desktop/startup acceptance. Existing installer launch flags and
startup preferences remain unchanged.

The existing package-selection resolver still owns handoff, including refusal
of broken selections and offline repair/doctor exceptions. One-shot commands,
help and deferred parse errors now wait for the selected executable and return
its exit code, not successful-spawn status. Signal termination without an exit
code is failure. Stdin/stdout/stderr remain inherited, including private pairing
input. Node legacy `--ask`, `--check-update`, `--rollback` and
`--list-ollama-models` are one-shot commands, not services. Shared classification
keeps them out of service detachment, spawning and instance-lock behavior.
Service handoff remains non-blocking so runtime retirement/instance-lock
handoff is not converted into a parent waiting with the old lock held. Detached
parents do not allocate a new visible console for their selected child; attached
terminals remain attached. No mutation is automatically retried or resumed.

## Verification scope

Two focused console-ownership tests, the legacy-flag classification test and
fresh headless app builds pass. Corrected native `windows-shell-jq0A7E` passes
all twenty-four PowerShell cases and both API-live quiet service checks.
The earlier combined run was deliberately interrupted after review found legacy
flags incorrectly classified as services. Its two full modes passed 1,106 tests
plus one existing ignored profiler each, but do not accept the correction.
The corrected frozen-source full modes each pass 1,107 tests plus one existing
ignored profiler across fourteen groups. All thirty-one native suites pass
sequentially, with 304 source/resource/workflow and nine native artifact hashes
unchanged. The final shell fixture is `windows-shell-wzvq4t`; no matching app or
cached-model fixture processes remain observed. The independent read-only record
is `.validation/windows-console-acceptance.json`.

Cached real-CPU fixture `real-model-qaQaMt` also passes the selected owner model
check with ordinary grants, retained results/no replay, model removal,
unload/reload, explicit restart, idle unloading and on-demand regression. Its
source/app/runtime hashes match, cached weights are unchanged, no model was
downloaded or copied, and no matching app/model process remains observed. See
`.validation/windows-console-real-cpu-acceptance.json`. These are mechanical
regressions, not default-model quality or automatic first-run acceptance.
Published source `6aa2adec510462f555cd34a1ec1f3ec71b96c64b` passes all four jobs in
[hosted run 40145011857](https://github.com/eepyfishy/eef/actions/runs/40145011857).
The completed Windows headless log confirms all twenty-nine selected native
suites in exact order, independently audited in
`.validation/windows-console-ci-acceptance.json`. This is source verification,
not acceptance of subsequent catalog edits or an installed/released package.

`tools/test-windows-shell.mjs` runs real Windows PowerShell against both roles,
with root and synthetic same-version selected-package paths. It checks console
PE headers, version/help output, stderr and nonzero parse status, live read-only
JSON, a failed retained-operation receipt and rejected stdin pairing without
configuration changes. The stdin assertion requires the exact nonempty invalid
code error, distinguishing received input from lost/empty stdin. Legacy input
and update-check flag cases also verify waiting and failure status without
executing work or contacting an update server. The selected-command cases verify
child exit and stdin propagation. Separate hidden service launches remain API-responsive while a
console-attachment probe observes no retained service console. Fixtures use hard
links rather than copying payloads; cleanup is
confined to their owned paths/processes. The suite does not establish a signed
package, visual startup/installer behavior, real model quality or physical
workload acceptance. Its selected Windows CI suite passes at the published
revision above; later source changes require their own acceptance.
