# Selected-bundle component resolution (unreleased after alpha.6)

Application updates select an executable under `versions/VERSION`; persistent
configuration and model data remain outside that bundle. Previously, built-in
plugins and the stock llama.cpp binary could still come from the old installation
root. A current executable, old plugin and another Python environment are not a
coherent component selection. The backend now resolves bundled components from
the **running executable's directory**, not a pending `current.txt` selection.

## Resolution contract

| Component | Managed bundle lookup | Explicit owner choice |
| --- | --- | --- |
| Python | Current bundle's `python/python.exe` on Windows; existing `python/bin` names elsewhere | `--python` overrides `config.python`; `EEF_PYTHON` follows when neither is set |
| Built-in plugins | Current bundle's `python/plugins` | Additional configured `python_plugins` keep their existing config-relative behavior, not a new built-in replacement mechanism |
| Stock llama.cpp binary | Current bundle's `tools/llama-server.exe` | Absolute and non-stock relative executables retain owner resolution; see below |
| Configuration/model data | Existing configuration/data directory | Unchanged; updates do not copy model weights into bundles |

`versions/VERSION` layouts are managed. A flat executable directory containing a
`bundle.json` marker is also managed. The marker determines lookup policy, not
publisher trust, file integrity, runtime compatibility or model readiness. Even
an unreadable/malformed marker must not enable implicit fallback.

An enabled component missing from a managed bundle is reported unavailable.
Built-in plugins never borrow the old installation root or the caller's working
directory. Default Python keeps the expected bundle path when missing, so its
availability check fails there instead of launching an unrelated PATH interpreter.
An explicitly chosen interpreter remains explicit, including a failing choice;
failure does not substitute another interpreter.

Unmarked flat developer layouts retain the existing source-tree plugin and PATH
Python fallbacks. This preserves local development rather than applying installed
bundle assumptions to every executable. An unknown executable layout does not
authorize an implicit interpreter; explicit choices remain possible.

## Configuration and override behavior

The literal stock `tools/llama-server.exe` setting with configuration in the
installation root denotes the selected bundle's binary, even if an old root copy
still exists. If intentionally using a custom executable at that old filename,
choose its absolute path. Other relative custom binary names preserve the previous
configuration-directory-first, installation-root-fallback lookup. A configuration
outside the installation can retain an existing config-relative executable,
including one using the stock filename. A missing stock bundled executable is not
replaced by an old root binary.

Relative `config.python` paths now use the configuration directory, consistent
with configured plugin/model paths, instead of depending on the process working
directory. Relative `--python` and `EEF_PYTHON` choices keep their command-line/
environment working-directory intent. Absolute choices are unchanged. This is
path resolution, not a configuration rewrite or an automatic custom dependency
installation. Ollama preference/provider policy is unchanged; llama.cpp lookup
matters only when that backend is used.

An ordinary application handoff still owns executable-version changes. A pending
selection cannot make a running older executable load the pending version's
components. Scoped model-setup restarts keep using their current executable's
bundle and retain their existing no-application-update boundary.

## Owner guidance and remaining work

`eefn setup` now explains Python, built-in/custom plugin, local model runtime and
peer startup issues in plain language using the existing bounded observations.
For missing packaged dependencies, it advises repairing this node package;
custom choices require inspecting their explicit source/dependencies. The check
does not repair, download, change permissions, restart, infer or execute work.
Core commands remain available when optional components cannot initialize.

This does **not** implement an automatic component repair/download controller,
approve a default LM, verify each component's inventory/hash at startup or prove
ABI/dependency compatibility. Use the supported installer for the selected node
version when repairing a package, and preserve existing owner settings/models.
Fresh install/repair and physical two-PC release acceptance remain separate gates.

## Verification

The corrected native pre-fix regression fails on the old running source: in
bundled-components-tkwZtO, a versioned fixture inspects the old root plugin, not
its selected bundle plugin. The assertion is retained in
.validation/bundled-components-before-native.log and sanitized marker observations
in .validation/bundled-components-before-observations.json. The earlier harness-only
variable-shadowing failure is not product evidence.

Twelve focused regressions pass in .validation/bundled-components-focused-final.log,
including pending-version isolation, explicit override/config-relative behavior,
managed/development/malformed-marker handling, leaf-only built-in names and
read-only setup guidance. Both locked/offline workspace/all-target feature modes
pass **971 tests plus one ignored profiler each**: eef 271, owner CLI 21, installer
14, eefn 637 and node CLI 28. Final logs/build:

- .validation/bundled-components-workspace-default-final.log
- .validation/bundled-components-workspace-headless-final.log
- .validation/bundled-components-build-final.log

Final headless apps/examples build. Bundled-components-OUkNoW passes eight native
scenarios with actual Python metadata inspection and refusing executable
sentinels: current versus old root plugins, missing-plugin no root/source fallback,
flat bundles, config-relative Python launched from another working directory,
managed flat/versioned missing Python without PATH execution, current bundled
llama.cpp selection and an explicit absolute owner binary. Read-only setup
guidance is verified on missing-component cases. These tests do not invoke media,
perform inference or prove actual model/DLL/optional-package compatibility.

Fifteen existing native suites also pass on that final build:

- settings-storage-ACBnON, model-setup-WW5YKS, model-activation-ZoCSYL
- model-bootstrap-Smt0Lg, unavailable-model-oznrVE, model-metadata-i4gfeq
- installed-selection-bkaCoa, process-lifetime-8M1nC3, local-pipelines-nGgLrW
- python-policy-tPXEUp, python-bridge-SpivXH, permission-commands-042byM
- node-commands-2NpMwD, node-credentials-mk9Gr3, model-downloads-zNZMzL

Each fixture retains results.json (Python bridge uses result.json). The combined
accepted log is .validation/bundled-components-native-accepted.log. Six evaluated
app/helper hashes in .validation/bundled-components-final-native-hashes.json and
source hashes in .validation/bundled-components-accepted-source-hashes.json match
after acceptance. Source-boundary, formatting, JS syntax and diff checks pass;
owned fixture processes are terminal.

The initial bundled-components-XgzPs7 probe remains an intermediate checkpoint.
A subsequent final-run CLI loader failure in bundled-components-mDKYWw was a test
environment defect: the setup CLI lacked the development DLL PATH supplied to
the fixture node. Only that harness environment changed; product sources/binaries
remained identical for the accepted sixteen-suite rerun. Neither that failure
nor the earlier variable-shadowing error is a product regression.

No installed/remote node, visual UI, default model, version, release installer
package or automatic-update policy is changed by this increment. Model quality,
full first-run dependencies/repair, physical tests and installer/signing/Defender/
stable-release acceptance remain open.
