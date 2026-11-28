# Installation and connection

[User guide](../USER_GUIDE.md) | [Using nodes](USING_NODES.md) |
[Requests](REQUESTS.md) | [Troubleshooting](TROUBLESHOOTING.md)

These steps apply to matching [v0.4.0a9 Windows installers](https://github.com/eepyfishy/eef/releases/tag/v0.4.0a9).
For alpha.6, see [the older dashboard setup](../FIRST_RUN.md).

## Install your first PC

1. Obtain `eef-installer.exe` and `eefn-installer.exe` from the project's
   [GitHub releases](https://github.com/eepyfishy/eef/releases). Read the notes for
   the version you choose. These are test releases, not stable v0.4.0.
2. Run each installer. In the new setup wizard, select **Next** to choose a
   dedicated install folder, whether the app starts at sign-in, and whether
   **Finish** opens it. The default folders are `%LOCALAPPDATA%\eef` and
   `%LOCALAPPDATA%\eefn`. Choose a folder outside Downloads.
3. Review your choices and select **Install**. Select **Back** to edit the
   options before installing.
4. Open both apps using setup or their Start menu entries. Keep eef
   running whenever you want the network's coordinator available.
5. Check the node's connection, then configure its permissions and models using
   [Using nodes](USING_NODES.md).

The alpha.9 installers explain invalid folder choices on **Installation
options**, before **Review setup**. Correct the folder and select **Next**
again; no installation files are written until **Install**. If the installed
app fails to open, those builds offer **Retry opening** or **Close**. Retrying
opens the installed app without reinstalling it.

Setup also attempts to restore its earlier file changes if a later
installation step fails. If it reports that recovery needs attention, close the
apps and keep the reported backup files. After an interrupted setup, run the
matching installer again, choose the same folder and select
**Install** to recover before continuing. Recovery after power loss is not yet
verified. See [setup recovery](../EXE-SETUP-ROLLBACK.md).

The alpha.9 apps also refuse to start when setup has not finished or
its recovery state cannot be verified. Run the matching installer in the same
folder; keep its journal and backups. If setup completed and only backup
cleanup is pending, the app can start with a cleanup warning.

If setup says the installation is in use, stop every running copy of that app
from the same installation folder and retry setup. Multiple profiles can share
one installation. Closing a dashboard tab does not stop an app process.

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

To run the apps without opening their dashboards, start
`.\eef.exe --no-ui` in one terminal and
`.\eefn.exe --no-ui` in another, from their respective folders. Those terminals
remain occupied by the apps; use another terminal for commands. No browser
controller is needed.

## Connect both roles on the same PC

With both matching alpha.9 apps running under the same Windows account,
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

- For alpha.6, create a private connection code in eef's Network view
  and enter it in the other node's Network view.
- For alpha.7 and later reviewed execution, provision the other PC's existing stable
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

Before upgrading test releases, stop the apps and back up their settings
and data. Keep node identities, credentials, and the coordinator's job history
together with that backup. Do not downgrade a newer job database into an older
build or run two builds against the same data. Removing history is not a way to
undo remote work. See [updates](../UPDATES.md) for advanced update and rollback
behavior.

## Remove a Windows EXE installation

1. Let the app finish its work or stop the work first. If you started the app
   in a terminal, press Ctrl+C there. If the installer or Start menu launched it,
   use Task Manager's **Details** tab. Check **Open file location** on
   `eef.exe` or `eefn.exe` to confirm it belongs to the installation you
   want to remove, then end that process.
2. In File Explorer, open
   `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup`. Remove
   `eef Coordinator.cmd` for eef or `eef Node.cmd` for the eef node if the
   matching file is present.
3. In `%APPDATA%\Microsoft\Windows\Start Menu\Programs\eef`, remove the
   matching Start menu entry, `eef coordinator.cmd` or `eef node.cmd`.
4. Move that app's dedicated installation folder to a backup location on the
   same drive if you want to keep its local settings, data, and models. You can
   delete the folder when you no longer need it. Models managed separately by
   Ollama stay in Ollama's storage.
