# Two-PC diagnostic test (v0.4.0a3)

Use the same alpha version on both PCs. These steps do not require a model, shell
permission, filesystem grants or remote access to Windows. Keep both machines on
the same reachable private LAN/overlay. Addresses are runtime configuration, not
built-in Radmin protocol modes.

## Install and pair

1. Close older eef/eefn processes and back up their config/database. Install eef
   and eefn on the first PC, and eefn on the second. Launch from Start > eef.
   Preserve existing node IDs; do not generate replacements just to change a name.
2. The first PC's node pairs locally under the same Windows account. On the
   first PC, create a private connection code for eef's reachable overlay/LAN
   endpoint. In the eef installation directory, run:
   `eef.exe invite --address YOUR_COORDINATOR_ADDRESS:51335 --json`.
   Use the complete `code` value. It contains a secret: do not publish it.
3. On the second PC's node, open Network, paste the code, and apply/restart.
   No JSON editing is needed. The advertised node address and the outgoing eef
   endpoint are different settings; the second node must connect to the FIRST
   PC's coordinator address, not its own address.
4. If desired, set the second node's advertised address using
   `eefn.exe network set --advertise-address YOUR_NODE_ADDRESS`; restart to apply.
   An advertisement alone does not open a peer listener.
5. eef's Nodes page should list both stable IDs as connected. Keep dashboard APIs
   on loopback; only the authenticated eef gateway needs private-network reachability.
   Do not expose a local management dashboard to the Internet.

eef's gateway defaults to TCP 51335; private-network/firewall rules must allow
the actual configured port. Do not disable Windows protection or use a UAC
bypass. If the private network is unavailable, the node remains controllable
locally and reports reconnecting/waiting. A successful OS ping alone does not
prove the authenticated eef protocol works.

## Collect explicit diagnostics

From the node directory on either PC:
`eefn.exe network diagnose --json`.

From the coordinator directory:
`eef.exe diagnostics --json`, then
`eef.exe diagnostics --node THE_SECOND_NODE_ID --samples 5 --json`.

These commands print local JSON. Redirect to a chosen file if desired, review,
and share privately. The probe runs only authenticated `system.ping`, records
latency/version/success per sample and exits nonzero if any sample fails. Reports
include stable node/runtime IDs but omit names, addresses, file paths, secrets,
prompts and raw errors. No analytics are uploaded automatically. NEVER attach
connection codes, raw configuration or unreviewed logs to public issues.

A developer with owner-authorized access to the first PC can run these same
local commands against the connected second node through eef. No backdoor or
public remote-control endpoint is added. Other remote configuration still
requires the node owner's approval.

## Acceptance checklist

- Both roles report `0.4.0-alpha.2`; second-node probe succeeds with matching version.
- Rename or change advertisement, restart, and confirm the stable node ID remains.
- Stop/restart the second node; eef records disconnect/reconnect and probes recover.
- Stop/restart eef; nodes reconnect without duplicate registrations or auto-replayed jobs.
- Test origin-scoped discovery with explicit eef-owner grants; self-only is default.
- Save one report from each PC and the coordinator's probe results. Record physical
  device count and which coordinator was active; one-PC fixtures do not satisfy this.

Do not turn on powerful feature permissions merely to pass a connectivity test.
Models, media, files, processes and automations need separate explicit acceptance.

## Optional coordinator on the second PC

eef may also be installed there, without making a second machine type. Test one
coordinator at a time with separate state; deliberately switch node endpoints/
private pairing when changing which coordinator owns the test. Endpoint priority
is reconnect preference, not replication, safe election or stale-leader fencing.
Do not issue the same stateful work independently from two eef instances.

MSI packaging, default lightweight model bootstrap, direct peer data paths,
continuous media and safe clustering remain future increments.
