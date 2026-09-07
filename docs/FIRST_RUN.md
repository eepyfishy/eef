# First installation: EEF 0.4.0a alpha

The alpha uses “node,” puts user input in the node app, provides feature-wide
permission switches and Stop/Resume connection controls, and retains old scoped
grants until explicitly changed. See [alpha release notes](RELEASE-0.4.0a.md) and
[STATUS.md](STATUS.md). This is not completion of the full v0.4.0 roadmap.

The node app also has a **Jobs** page for saved planned actions.
Open a job's steps to see its status and assigned node. Pause/Stop prevent later
steps, but actions already sent may finish. After an EEF restart, interrupted
jobs wait for review; Resume is available only when the unfinished actions are
safe to repeat. Successful steps are retained. This page requires a connection
to the EEF that holds the history; it does not share history across coordinators.

Settings also provides **Location and resources**. Area
names use a path such as `Home / Upstairs / Office`. Add a named camera,
microphone, screen or output there; renaming it keeps its resource identity.
Resource areas can inherit the node area. Save and restart to advertise changes.
This does not enable permissions or start capture, and it does not yet provide
live media relaying. EEF's Configure node view offers the same forms with the
existing owner-approval rules.

EEF manages your network; the **node app (EEFN)** supplies this PC's models
and allowed tools. They can run together on one PC. Another PC may run only
the node app. At least one EEF and one connected node are needed to work
as a network; EEF itself does not run a language model.

## Start on one PC

1. Download the two installers from the project's Releases page. If antivirus
   reports a threat, stop and report it. Do not add an exclusion or restore it.
2. Open each installer. Keep its recommended folder, or Browse to a dedicated
   folder. The download folder cannot be the installation folder. Startup is
   optional and does not require administrator access.
3. Select **Open app when finished**, or open EEF and EEF Node from
   the Start menu later. Both programs run under your own Windows account.
4. In the node app, Home should change from **Waiting for EEF** to
   **Connected**. In EEF, Nodes should show this PC's detected name.
5. Open Permissions in the node app. Enable only what you want to share.
   Each switch grants its feature within your Windows account's rights. Existing
   limited grants stay limited until explicitly switched off and on. Click
   **Apply and restart**. Saved permission preferences are not active until
   the node services restart. Windows privacy controls still apply.
6. Open Models. If Ollama is already reachable, choose an installed model or
   explicitly install one. Otherwise, choose the small catalog model or a
   local GGUF file. Watch the download progress, click **Use model**, then
   **Apply and restart**. Installation alone does not select a model.
7. The node app's Home page accepts messages. Try a short message there.
   A connection without a model can provide enabled tools but cannot answer
   using a language model. CPU inference may be slow on older PCs.

You do not need to edit JSON, run PowerShell, or type a port number for these
steps. Advanced remains available for less common settings and custom models.

## Configure nodes from EEF

In EEF, open Nodes and choose **Configure node**. The same permission,
model, and name forms now refer to that PC, not the PC displaying EEF.
By default, Save sends a proposal: the owner reviews and saves it in that
PC's node app. To permit immediate remote settings changes, model installs,
and restarts, enable **Allow EEF to apply settings and restart this node**
locally in Settings and save. This trust change applies when saved. Remote
management cannot enable itself. Local file pickers and startup remain local.

## Add your second PC

In EEF, open Network and create a connection code. Keep it private: it contains
your network credential. On the second PC, install the node app, paste the
code into Network, and save. The PCs must be reachable on your private network;
Windows Firewall may ask for approval. Do not disable the firewall. If names
cannot be resolved, the connection details support a reachable LAN address.

Multiple EEF connections can have priorities under Network's connection
details. These control the order of reconnect attempts, not a shared election
or replicated EEF state. Each connection must use the network's shared key.

## Saving, restarting, and updates

Unsaved edits have a visible Save bar. **Save changes** stores preferences;
**Apply and restart** also restarts services. Settings can remember automatic
restart after saving in this browser. Running requests may be interrupted.
You never need to find and kill a process to apply normal settings changes.

Updates default to asking first. Use Settings to check/install, then Restart
to run the installed version. Automatic updates install in the background but
still require Restart. No elevation bypass or antivirus bypass is used.

If Home reports an error, read Network's connection details, correct the
settings, and Retry. Advanced offers a redacted diagnostic export and previous
configuration restore. Renaming, restarting, and upgrades keep the node ID;
deleting the configuration creates a new identity. Downloaded models stay on
disk when deselected or when node settings are reset.
