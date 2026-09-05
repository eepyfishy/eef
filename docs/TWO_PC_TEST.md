# Two-PC test guide

Both Windows PCs can run EEFN. Either PC can also run EEF; installing EEF and
EEFN on the same PC is supported because their executables, configuration,
dashboards, and ports are separate.

## Install and join

1. Run `eef-installer.exe` on the PC that will be the preferred coordinator.
   It installs per-user files in `%LOCALAPPDATA%\EEF` by default and generates a
   random network secret in `config\default_identity.yaml` under `node.psk`.
2. Optionally install EEF on the second PC. Give it a unique `coordinator.id`,
   set a lower `coordinator.priority`, and replace its generated `node.psk` with
   the same secret as the first EEF.
3. Run `eefn-installer.exe` on both PCs. No model is included or selected.
4. Open each node dashboard at `http://127.0.0.1:51336/`. Set a unique
   `node_id`, enter the shared `psk`, and list coordinator addresses in priority
   order. Example:

   ```json
   "endpoints": [
     {"address": "192.0.2.10:51335", "priority": 100},
     {"address": "192.0.2.11:51335", "priority": 50}
   ]
   ```

   Replace the documentation addresses with the PCs' actual LAN or tunnel
   addresses. Both PCs must be able to reach TCP port 51335. EEF/EEFN does not
   depend on any particular VPN product.
5. Save and restart EEFN. The preferred reachable EEF should list both nodes at
   `http://127.0.0.1:51334/`.

## Model and capability tests

- Leave every model list empty to verify that initial node installation works
  without a model.
- On a PC with Ollama, select only model IDs already installed there. On a PC
  without Ollama, add an owner-selected local GGUF path to a llama.cpp slot.
- Enable one permission at a time in the node dashboard. Add the corresponding
  filesystem, executable, host, media, or Wake-on-LAN allowlist before saving.
- For Wake-on-LAN, firmware/BIOS and the network adapter must support it. Add
  the sleeping PC's MAC, the real subnet broadcast address, and the desired UDP
  port to `permissions.wake_on_lan`, then invoke `network.wol` action `wake`.

## Failover test

With both coordinator addresses configured, stop the higher-priority EEF. EEFN
reconnects to the next reachable endpoint. Restart the preferred EEF and then
restart or reconnect EEFN to restore the normal priority order.

Priority reconnect is implemented. Quorum election, replicated coordinator
state, and split-brain prevention are not yet implemented, so this release must
not be treated as a partition-safe active/standby cluster. During this test,
avoid submitting stateful work independently to both coordinators.
