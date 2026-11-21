# Two-PC acceptance for v0.4.0

Use a matching v0.4.0 development candidate on two physical Windows PCs.
Record the source revision, executable versions and package SHA-256 values.
These steps use commands added after the published alphas. For the older
package, see the [alpha diagnostic](#older-alpha-diagnostic-v040a3).

## Verify setup and upgrade from real installers

1. Record the candidate installer hashes and the Windows and Defender versions
   on each PC. Use the normal Windows download and installation flow as a
   standard user. Record any publisher warning or threat detection separately.
   Keep a detected sample quarantined and record that install as blocked.
2. For a fresh installation, install both roles on PC A and the node on PC B.
   Confirm separate per-user directories, optional startup, and an empty model
   selection on each node. Sign out and back in to check that only the opted-in
   apps start. Complete the pairing and registration checks below.
3. For an upgrade, use a separate test installation from a previously approved
   prerelease. Record its version and installer hashes, then record the stable
   node IDs, startup choices, configured permissions and one retained job.
   Stop both roles and back up the test installations before upgrading in place
   with the candidate. Start the apps and repeat the registration checks.
4. Confirm the candidate is running, the node IDs and approved settings are
   preserved, and the retained job is still readable. Confirm the upgrade has
   neither enabled a new capability nor replayed the old job. Record fresh
   setup and upgrade as separate outcomes.

The [isolated installer regression](BUILD.md#toolchain) covers stale update
selections. With `-PreviousReleaseDir`, it also covers upgrading actual prior
installations. Record the physical registration and job checks above separately.

## Remote update preflight (2026-11-21)

The locally packaged development coordinator from `e975866` temporarily
listened on the private VPN using the existing legacy pairing. The physical
node registered and passed five application probes. The read-only
`eef node update-status` command reported running and installed versions
`0.4.0-alpha.6` with no restart pending. The coordinator's source revision
was verified locally. The preflight measured the remote node's reported version.

The node also reported `remote_updates: true` in its saved and applied
permissions, and the owner had approved remote management for a separate app
restart. These were read-only observations. The node checks its current policy
again when an update or restart is requested.

The explicit updater requires a newer version than the installed version.
The alpha.7 validation build gives the test a distinct version to target.
Keep it on the explicit [node update path](COMMANDS.md#explicit-remote-updates)
and record the built artifact hash and size when selecting it. Both public
feeds were paused and contained no artifact fields at the time of this check.

## Pair the nodes and verify the connection

1. Use two owner-approved PCs on a private LAN or overlay. Back up their
   configuration and journals before an upgrade. Run eef and the input node
   on PC A, and the executor node on PC B, as ordinary users. Exercise the
   command path with both roles running without the browser UI.
2. On PC A, use `eefn setup --local` to inspect same-PC enrollment. If needed,
   use `eefn setup --enroll-local --restart-coordinator` with owner approval for
   the restart. On PC B, follow the [private pairing flow](NODE-CREDENTIALS.md)
   using its existing stable node ID. Transfer the invitation privately and
   verify that the coordinator has applied its trust settings.
3. With both node apps running, run `eefn setup --wait-registration-seconds 30 --json`
   on each PC. Require `outcome: "accepted"` and `last_setup.transport: "pinned_tls13"`.
   Record the stable node IDs and runtime IDs. Run `eefn network diagnose --json`
   on both PCs and `eef diagnostics --node NODE_B --samples 5 --json` on the
   coordinator. Record the versions and application probe results. A successful
   OS ping checks network reachability.
4. Confirm PC A cannot see PC B's remote capabilities before disclosure. On
   the coordinator run `eef discovery grant --requester NODE_A --target NODE_B`
   and restart to apply it. Verify discovery works in that direction and is
   still denied in the reverse direction.

## Run a reviewed request across both PCs

Use an already installed, owner-selected text interpreter on PC B. Record the
exact backend, model and runtime version. Keep PC A free of an applied
interpreter for this case. Configure PC B's interpreter sharing, approve the
coordinator's origin/provider policy and apply PC A's opt-in preference.
Follow the [interpreter policy commands](INTERPRETER-POLICIES.md) and
[network interpretation setup](NETWORK-INPUT-WORKFLOW.md#choose-where-interpretation-happens).

1. On PC A, run `eefn requests input --text "Inspect system information on NODE_B"`
   using PC B's actual node selector. Follow any clarification request. Use
   `eefn requests plan JOB_ID --json` to verify PC A is still the origin and the
   plan binds the intended operation on PC B. Record incorrect plans, unavailable
   interpretation and unresolved clarification as incomplete cases.
2. On PC A, check the exact plan using `eef requests inspect JOB_ID --json`.
   Use the [owner approval command](COMMANDS.md#bounded-owner-request-approval-unreleased-after-alpha6)
   with its retained revision and digest. Confirm approval issued no grant.
   From the input node, use the new approval revision and digest with the
   [explicit execution command](COMMANDS.md#explicit-approved-request-execution-unreleased-after-alpha6).
3. On PC A, use `eefn jobs get JOB_ID --json` and
   `eefn requests result JOB_ID --grant-id GRANT_UUID --json`. Record the exact
   grant, origin, executor and final state. Compare the retained output with
   the approved operation on PC B. From PC B, inspect the same result through
   its node client and confirm that the origin check denies it.
4. With separately approved test work, exercise an executor disconnect and a
   coordinator restart. Record the last acknowledged attempt, stable node IDs
   after reconnection and retained status. Verify the old grant is never replayed
   and unknown outcomes stay unconfirmed without matching retained evidence.
5. For the executor restart case, start fresh, separately approved test work
   on PC B and have its owner allow remote management.
   From PC A run `eef node restart --node NODE_B --wait-seconds 60 --json`. Record
   the acknowledged prior runtime and the replacement runtime for the same node
   ID. On PC B, wait for accepted registration again and compare that runtime ID
   with the restart result. Record whether the last approved grant was left
   unreplayed. Restore the owner's earlier remote-management setting afterward.

## Run a bounded continuous source across both PCs

Use an owner-approved physical camera and scene on PC B. Select a resource with
`eefn workloads camera-sources` and confirm its applied workload permission and
local camera listening consent. Follow the [camera workflow](CAMERA-MOTION.md).
From PC A, submit `eefn requests submit-camera --source-node NODE_B --camera RESOURCE_ID --frames 30`.
Verify the reviewed source and limits, then use separate session approval and
execution. Record the consumed grant acknowledgement, source and event counts,
event bytes and completion state. Verify repeated camera payloads stay on PC B
while the coordinator receives bounded useful events.

For the [local vision case](LOCAL-EVENT-VISION.md), separately approve the exact
local VLM and limits. Record model call counts and bytes sent to the coordinator.
On separate admitted work, exercise stop and local listening revocation. Verify
that capture ceases, the capture reservation becomes reusable after observed
exit, and normal commands remain responsive. From PC A, record the intent with
`eefn requests stop-receipt JOB_ID --grant-id GRANT_UUID --json` and the process
observation with `eefn jobs stop-status JOB_ID --json`. Record availability and
scope separately from the job's interrupted state. On PC B, read the same grant
with `eefn grants stop-status --id GRANT_UUID --json`, including with the node
service offline. After reconnecting PC B, repeat the read from PC A and compare
the original scope and job revision. Check that inspection sends no new stop
and does not replay the source. Use independent observations for device buffers
or external effects and record their uncertainty separately.

## Record the outcome

Record pass, fail or unverified for each case with the candidate revision,
physical node roles, network path, runtime/model versions, permissions and
evidence. Redact secrets, private model input, media, paths and addresses from
shared reports. Keep loopback, fake-source and mocked-inference results separate.

Use this record alongside the [remaining v0.4.0 gates](ROADMAP-CORE.md#acceptance-and-remaining-scope),
including negative cases and real model quality. Track
[MSI rollback](MSI-ROLLBACK-VALIDATION.md), [signing](SIGNING.md) and
[Defender](ANTIVIRUS.md) evidence separately. See [STATUS.md](STATUS.md) for the
currently verified source checkpoints.

## Older alpha diagnostic (v0.4.0a3)

Use the same alpha version on both PCs. These steps do not require a model, shell
permission, filesystem grants or remote access to Windows. Keep both machines on
the same reachable private LAN/overlay. Addresses are runtime configuration, not
built-in Radmin protocol modes.

### Install and pair

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

### Collect explicit diagnostics

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

### Acceptance checklist

- Both roles report `0.4.0-alpha.3` as listed in the
  [alpha release notes](RELEASE-0.4.0a3.md). The second-node probe succeeds with
  the matching version.
- Rename or change advertisement, restart, and confirm the stable node ID remains.
- Stop/restart the second node; eef records disconnect/reconnect and probes recover.
- Stop/restart eef; nodes reconnect without duplicate registrations or auto-replayed jobs.
- Test origin-scoped discovery with explicit eef-owner grants; self-only is default.
- Save one report from each PC and the coordinator's probe results. Record physical
  device count and which coordinator was active; one-PC fixtures do not satisfy this.

Do not turn on powerful feature permissions merely to pass a connectivity test.
Models, media, files, processes and automations need separate explicit acceptance.

### Optional coordinator on the second PC

eef may also be installed there, without making a second machine type. Test one
coordinator at a time with separate state; deliberately switch node endpoints/
private pairing when changing which coordinator owns the test. Endpoint priority
is reconnect preference, not replication, safe election or stale-leader fencing.
Do not issue the same stateful work independently from two eef instances.

MSI packaging, default lightweight model bootstrap, direct peer data paths,
continuous media and safe clustering remain future increments.
