# Managed Python import caches

The production bridge previously used isolated UTF-8 Python processes but did
not suppress automatic import caches. A controlled native test against `32d2c02`
observes `sys.dont_write_bytecode=false` and a generated `__pycache__` beside its
synthetic plugin. This is an import side effect, not evidence of malware or a
claim that all earlier cache differences have a proven cause.

Both bridge availability checks and plugin inspection/invocation now share the
same command factory: `-I -B -X utf8`. Isolation, UTF-8, deadlines, bounded pipes,
Windows owned-process cleanup and optional memory ceilings remain unchanged.
The explicit `-B` option is used because isolated mode ignores Python environment
settings. See [the Python command-line documentation](https://docs.python.org/3.11/using/cmdline.html#cmdoption-B).

The legacy bundle builder's validation imports also use `-B`, alongside its
existing pip `--no-compile` policy. Before copying role payloads or generating
their inventories, the read-only `tools/assert-python-payload.ps1` guard refuses
filesystem `__pycache__` directories and reparse points without following their
targets, deleting files or changing hashes. A clean staging directory is
required; the builder does not sanitize a previously used payload into apparent
integrity. The MSI planner/inspector retain their existing generated-cache
omission and verified-source requirements.

## Scope and limits

`-B` prevents automatic `.pyc` writes on source import. It does not ignore or
validate existing cache files, remove old caches, guarantee plugin behavior,
prevent deliberate filesystem writes or constrain independently launched plugin
children. Python plugins remain an explicitly trusted extension boundary under
the existing owner permissions and bridge limits, not a sandbox.

Existing installations, cached runtime bundles and failed integrity observations
are untouched. In particular, the earlier broad cached-Python inventory preflight
remains failed; suppressing future writes does not recertify those historical
bytes or authorize a repair. No application inventory is loosened, cache mismatch
ignored, automatic repair/retry enabled, or signing/Defender approval inferred.
The official embedded standard-library archive remains a pinned immutable
artifact; this guard does not reject bytecode stored inside that archive.

## Verification boundary

The native bridge regression first fails on its new no-bytecode assertion while
the old bridge otherwise returns a successful bounded reply. The new assertion
also requires no import-cache directory after both inspection and invocation.
All pre-existing UTF-8, byte limits, memory, deadlines, cancellation, descendant
and redaction checks remain in that same fixture.

Nine focused Rust tests pass, including the shared command's exact arguments.
The fresh actual-interpreter bridge run also passes the new flag/cache assertions
and all original memory, UTF-8, deadline, cancellation, descendant and redaction
checks against unchanged evaluated source/helper hashes.
Seven read-only native PowerShell checks cover clean/literal paths, empty and
case-variant cache directories, non-directory/missing roots and root/child links;
source bytes, caches and link targets are not deleted or rewritten. CI runs this
staging check in every OS/build mode. Full workspace/native, actual builder,
installer/recovery, physical-PC and release acceptance remain separate gates.
No full bundle rebuild/download, installer or installed-node update is implied.
