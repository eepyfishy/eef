# First installation: EEF 0.3.2

EEF manages your network; the **device app (EEFN)** supplies this PC's models
and allowed tools. They can run together on one PC. Another PC may run only
the device app. At least one EEF and one connected device are needed to work
as a network; EEF itself does not run a language model.

## Start on one PC

1. Download the two installers from the project's Releases page. If antivirus
   reports a threat, stop and report it. Do not add an exclusion or restore it.
2. Open each installer. Keep its recommended folder, or Browse to a dedicated
   folder. The download folder cannot be the installation folder. Startup is
   optional and does not require administrator access.
3. Leave **Open app when finished** selected, or open EEF and EEF Node from
   the Start menu later. Both programs run under your own Windows account.
4. In the device app, Home should change from **Waiting for EEF** to
   **Connected**. In EEF, Devices should show this PC's detected name.
5. Open Permissions in the device app. Enable only what you want to share.
   File and app access also require choosing allowed folders/programs. Click
   **Apply and restart**. Saved permission preferences are not active until
   the device services restart. Windows privacy controls still apply.
6. Open Models. If Ollama is already reachable, choose an installed model or
   explicitly install one. Otherwise, choose the small catalog model or a
   local GGUF file. Watch the download progress, click **Use model**, then
   **Apply and restart**. Installation alone does not select a model.
7. EEF Home shows connected devices and available models. Try a short message.
   A connection without a model can provide enabled tools but cannot answer
   using a language model. CPU inference may be slow on older PCs.

You do not need to edit JSON, run PowerShell, or type a port number for these
steps. Advanced remains available for less common settings and custom models.

## Configure devices from EEF

In EEF, open Devices and choose **Configure device**. The same permission,
model, and name forms now refer to that PC, not the PC displaying EEF.
By default, Save sends a proposal: the owner reviews and saves it in that
PC's device app. To permit immediate remote settings changes, model installs,
and restarts, enable **Allow EEF to apply settings and restart this device**
locally in Settings and save. This trust change applies when saved. Remote
management cannot enable itself. Local file pickers and startup remain local.

## Add your second PC

In EEF, open Network and create a connection code. Keep it private: it contains
your network credential. On the second PC, install the device app, paste the
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
configuration restore. Renaming, restarting, and upgrades keep the device ID;
deleting the configuration creates a new identity. Downloaded models stay on
disk when deselected or when device settings are reset.
