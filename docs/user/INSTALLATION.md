# Installation and connection

[User guide](../USER_GUIDE.md) | [Using nodes](USING_NODES.md) |
[Requests](REQUESTS.md) | [Troubleshooting](TROUBLESHOOTING.md)

The installation steps apply to the published Windows alpha. The command-based
private enrollment section requires matching **development builds after alpha.6**.
For the released dashboard flow, use [First installation](../FIRST_RUN.md).

## Install your first PC

1. Obtain `eef-installer.exe` and `eefn-installer.exe` from the project's
   [GitHub releases](https://github.com/eepyfishy/eef/releases). Read the notes for
   the version you choose. These are test releases, not stable v0.4.0.
2. Run each installer. Keep the recommended dedicated folder, normally
   `%LOCALAPPDATA%\eef` and `%LOCALAPPDATA%\eefn`, or choose separate dedicated
   folders. Installation directly into the download folder is refused.
3. Choose whether each app should start when you sign in. Startup is optional;
   ordinary app startup runs as your Windows user.
4. Launch both apps using the installer or their installed shortcuts. Keep eef
   running whenever you want the network's coordinator available.
5. Check the node's connection, then configure its permissions and models using
   [Using nodes](USING_NODES.md).

The installers include Python for supported optional adapters. You do not need
to install Python separately for the bundled adapters. The node also includes
the llama.cpp runtime; model files are a separate owner choice and download.
Existing reachable Ollama can be used instead.

If antivirus quarantines an installer, keep it quarantined and report the exact
version and detection. The builds are unsigned, and historical Defender reports
are unresolved. See [antivirus guidance](../ANTIVIRUS.md). An unsigned-publisher
warning and an antivirus threat detection are different messages.

## Open a command terminal

Open the app's installation folder in File Explorer, then open a terminal there.
Commands do not need an administrator terminal for ordinary setup or use.

In the node folder:

```powershell
.\eefn.exe --version
.\eefn.exe --help
```

In the coordinator folder:

```powershell
.\eef.exe --version
.\eef.exe --help
```

Use matching builds on the two roles. New commands are not included in older
installers even when their version still belongs to the 0.4.0 alpha series.

For a development build without installed shortcuts, start
`.\eef.exe --no-ui` in one terminal and
`.\eefn.exe --no-ui` in another, from their respective folders. Those terminals
remain occupied by the apps; use another terminal for commands. No browser
controller is needed.

## Connect both roles on the same PC

Published alpha.6 uses same-user local discovery. Its node dashboard should show
Connected once eef is available. That older shared-key connection is not sufficient
for development reviewed execution, which requires private node credentials.

With both matching development apps running under the same Windows account,
inspect the local connection plan from the node folder:

```powershell
.\eefn.exe setup --local
```

Read the result. When you are ready to apply private pairing, and it is safe to
interrupt both apps' work:

```powershell
.\eefn.exe setup --enroll-local --restart-coordinator
```

This can provision the node's credential and restart the coordinator and node
apps to apply it. All pending settings on a restarted app can apply, not just
pairing. It does not restart Windows, enable features, or install a model.

After the apps return, inspect again:

```powershell
.\eefn.exe setup
.\eefn.exe connection show
.\eefn.exe network show
```

Confirm the connection rather than treating a restart acknowledgement as success.
The stable node ID remains the same when you rename or restart the node; its
runtime ID changes on restart. A paused connection stays paused until you
explicitly run `.\eefn.exe connection resume`.

If enrollment is refused, follow [troubleshooting](TROUBLESHOOTING.md).
Do not delete identities, replace certificate pins, or switch to shared-key
pairing to make reviewed execution connect. Details are in
[local enrollment](../LOCAL-ENROLLMENT.md).

## Add another PC

The other PC needs `eefn`; it does not also need its own eef. Both PCs must be
able to reach the coordinator over your LAN or an owner-managed private network.

- For published alpha.6, create a private connection code in eef's Network view
  and enter it in the other node's Network view.
- For development reviewed execution, provision the other PC's existing stable
  node ID and use a private, certificate-pinned invitation. Follow the exact
  [two-PC credential commands](../NODE-CREDENTIALS.md#command-based-migration).
  Same-PC `--enroll-local` is not a remote-PC discovery command.

Invitations contain reusable secrets. Transfer them privately and keep them out
of screenshots, public issues, source control, and chat transcripts. The node's
feature permissions remain the node owner's choice after pairing.

Windows may ask for permission for a network listener. Ordinary node connections
are outbound; a configured peer receiver is a separate listener. Use the actual
installed executable and intended network scope. Keep owner dashboards local.
Persistent firewall setup can require a legitimate administrator approval;
ordinary startup does not bypass that approval. See
[firewall guidance](../WINDOWS-FIREWALL.md) if prompts repeat.

## Updates and backups

Feed-based updates exclude alpha, beta, and release-candidate versions. Installing
an alpha does not subscribe the PC to later alphas. The repository's release
documentation records paused feeds; an older client can report a feed-check error.
Use an explicitly chosen matching installer for a test release.

Before upgrading development builds, stop the apps and back up their settings
and data. Keep node identities, credentials, and the coordinator's job history
together with that backup. Do not downgrade a newer job database into an older
build or run two builds against the same data. Removing history is not a way to
undo remote work. See [updates](../UPDATES.md) for advanced update and rollback
behavior.
