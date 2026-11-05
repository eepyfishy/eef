# Troubleshooting

[User guide](../USER_GUIDE.md) | [Installation](INSTALLATION.md) |
[Using nodes](USING_NODES.md) | [Requests](REQUESTS.md)

Most commands here require development builds after alpha.6. Check `--version`
and `--help` before using them on an older installed alpha.

## The command is not found

In PowerShell, open a terminal in the app's installation folder and include the
current-folder prefix: `.\eefn.exe setup`, rather than just `eefn setup` unless
you explicitly added it to PATH. Use the coordinator folder for `eef.exe`.
An "unrecognized subcommand" error usually means this executable predates the
feature. A local command working while a remote command is unsupported can mean
the other role is older. Use matching builds; do not use a legacy execution
path to bypass the missing feature.

## The node is not running or the local service cannot be reached

Launch the installed node app, then run `.\eefn.exe connection show`.
Commands target the running node; they do not normally start a replacement node.
If you use a custom configuration, pass the same full `--config` path that the
service uses. Do not change a port number or start duplicate app copies to fix
local discovery. If an app just restarted, wait for it to return before checking.

## eef is waiting for a node, or the node is disconnected

1. Confirm eef and the intended node app are running.
2. Inspect `.\eefn.exe connection show` and `.\eefn.exe network diagnose`.
3. If the connection is paused and you want it active, run
   `.\eefn.exe connection resume`.
4. On one PC, use the [local enrollment flow](INSTALLATION.md#connect-both-roles-on-the-same-pc).
   On two PCs, check the private network and the invitation's coordinator endpoint.
5. From eef, run `.\eef.exe diagnostics --node NODE_ID --samples 1` to probe an
   already-connected node. This tests authenticated control traffic, not a model.

A successful Windows ping does not establish pairing or protocol support.
Network reachability, credentials, TLS trust, and version compatibility can fail
separately. Do not replace certificate trust or delete node identity as an
automatic repair. Preserve the error and use [credential guidance](../NODE-CREDENTIALS.md).

## Connected, but ordinary-language requests fail

Run `.\eefn.exe setup` and `.\eefn.exe models show`. Check that:

- A text-capable model is selected and installed on the node doing interpretation.
- Exactly one active selection has the `request_interpreter` role.
- The chosen provider activates that model's backend.
- Saved changes have been applied by an intentional app restart.

Follow [model setup](USING_NODES.md#choose-a-model). Connection does not prove
model health or quality. An explicit `system_info` template can help distinguish
model interpretation problems from the reviewed execution path.

If interpretation times out or reports incomplete output, keep its receipt and
inspect history. Do not assume the model's internal reasoning is a usable answer.
Development commands offer an explicit `--inference-timeout-seconds` option;
see [inference timing](../INFERENCE-TIMING.md). Increasing a wait is not a model
quality fix or permission to repeat an uncertain action.

## A permission was enabled, but the action is refused

Inspect `.\eefn.exe permissions show` on the actual executor, not just the origin
node. Compare saved and applied values. Review pending settings, restart that
node if appropriate, and inspect again. Also check the request's current plan
and approval, required service/model, and Windows camera/microphone privacy
settings. Feature permission and exact request approval are separate.

Whole-feature access does not bypass Windows account rights or install drivers.
Old scoped grants do not silently become unrestricted on upgrade; use an
explicit feature `allow` only if you want broader access.

## Remote settings or restart is refused

The target node owner must allow `remote-management` locally. eef cannot enable
its own management permission. Remote updates need their separate consent and
an explicitly approved artifact. After an uncertain settings or restart reply,
inspect current settings and the runtime ID before requesting another mutation.
Check [using nodes](USING_NODES.md#manage-a-node-from-eef).

## A request says review, awaiting approval, or unknown

`review` can need clarification or preparation. `awaiting_approval` needs the
local eef owner's exact plan approval. `approved` still needs a separate explicit
execution request. See the [complete walkthrough](REQUESTS.md).

For `unknown`, `interrupted`, or an unconfirmed reply, keep the request, grant,
and operation IDs. Inspect `jobs get`, `requests receipt`, or `requests result`
as appropriate. An error or missing result is not proof that nothing happened.
Do not blindly repeat execution, erase history, or resume via an older command.
Explicit result recovery or tracking abandonment are separate advanced decisions.

## Downloads fail or storage is low

Use `models install-plan` before a new model download, then inspect its exact
download status. Keep enough room for the payload and temporary installation
work; the app cannot guarantee that other programs leave that space available.
There is no mandatory fixed 5 GB reserve.

Removing a model selection does not delete weights. Avoid deleting app settings,
node identities, job databases, or installed runtime files to free space. Stop
unneeded downloads by their exact ID and inspect the outcome. Shared Ollama
layers may remain. Clearing a developer checkout's compiler output only applies
to rebuildable build files, not an installed app's data or runtime payload.

## Firewall or antivirus prompts recur

Firewall rules refer to executable paths. A new version or temporary development
copy may use a different path. Use the installed copy and review the actual
listener/network scope; keep dashboards local. See [firewall guidance](../WINDOWS-FIREWALL.md).

If Defender reports a threat, keep the file quarantined. Collect the release
version, filename, detection name, and hash when available, then follow the
[antivirus investigation guide](../ANTIVIRUS.md). A clean scan on another PC
does not clear the detection on this PC.

## Collect a useful problem report

For development builds, basic inspection can include:

```powershell
.\eefn.exe --version
.\eefn.exe setup
.\eefn.exe network diagnose --json
```

From the coordinator folder, `.\eef.exe diagnostics --json` provides its
diagnostic report. Package checks use `eef doctor` or `eefn doctor`; their default
inspection does not launch runtime probes. An unpackaged source build may not
have the package inventory those checks expect.

Include what you tried, the expected result, the observed result, versions of
both roles, and whether the test used one or two physical PCs. For a Windows
crash, include its time and stop code if available; that alone does not identify
the cause. Preserve crash evidence and avoid heavy repeat tests until investigated.

Review reports before sharing them. Do not post private invitations, raw
configuration, credentials, database files, full request inspections, personal
paths, or model prompts to a public issue. Diagnostic IDs can also identify your
network; share only what is needed. eef does not automatically upload these
reports.
