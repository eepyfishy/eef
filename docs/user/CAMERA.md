# Camera motion requests

These commands are **unreleased development features after alpha.6**. They are
not available in the published alpha.6 installers. The complete executable camera
lifecycle is still under hosted verification; physical webcam/RTSP acceptance is
also unfinished. See [current status](../STATUS.md) before testing a development
build.

This workflow observes image changes and reports small motion events. It does not
identify people, send video, call a vision model, or follow you between rooms.
No language model is needed. Lighting changes can count as motion, and an unknown
observation is not proof that a room is empty.

Keep both apps running and use the installation folders as described in the
[user guide](../USER_GUIDE.md#how-commands-and-the-dashboard-fit-together).
Examples below use the same node for input and capture. Another connected source
node can be selected explicitly with `--source-node NODE_ID` when submitting.

## Choose the camera on its node

From the source node's installation folder:

```powershell
.\eefn.exe camera show
.\eefn.exe camera configure --resource camera --device 0
```

For a network camera, use its own RTSP address instead of `--device`:

```powershell
.\eefn.exe camera configure --resource camera --rtsp rtsp://camera-host/live
```

Replace the example address with your camera's actual endpoint. A URL supplied on
the command line may be retained in shell history or visible to local process
inspection. Command reports omit the private endpoint; the source binding stays
in the owning node's settings. Configuring a camera does not open it.

The `camera` resource name in these examples is your chosen name. Use the same
name when submitting. With multiple resources, select the intended one explicitly
instead of relying on automatic selection.

## Enable the source and apply its settings

The source node owner enables the camera feature, ambient listening consent and
bounded workload policy separately:

```powershell
.\eefn.exe permissions allow camera
.\eefn.exe listening enable
.\eefn.exe workload-policy enable --max-session-ms 60000 --max-lease-ms 10000
.\eefn.exe restart --wait-seconds 20
.\eefn.exe workloads camera-sources
```

Review pending settings before restarting: a restart interrupts current work.
The workload limits are configurable maxima, not a request to begin a session.
Listening consent also governs microphone workloads; each source still requires
its own feature permission and approved request. None of these setup commands
alone authorizes camera capture.

The final inspection reports saved/applied permission and binding state, Python
configuration, consent and configured execution availability. Resolve a reported
unavailable prerequisite before submitting. Availability is not a physical camera,
OpenCV installation or network-camera connectivity test; passive inspection opens
no source. See [Python startup diagnostics](../PYTHON-STARTUP-DIAGNOSTICS.md) if the
interpreter is unavailable.

## Submit, review and approve

From the input node, submit a small bounded request:

```powershell
.\eefn.exe requests submit-camera --camera camera --frames 30 --interval-ms 200
.\eefn.exe requests plan REQUEST_ID
```

Replace `REQUEST_ID` with the returned job ID. Submission retains the exact source
and limits for review; it does not capture images. Inspect the source node,
resource, duration and event limits in the plan. A source or settings change can
require a new review.

From the coordinator's installation folder, the eef owner approves the exact
reviewed revision and plan digest:

```powershell
.\eef.exe requests approve-session REQUEST_ID --revision PLAN_REVISION --digest PLAN_SHA256 --valid-for-ms 120000
```

Use the values from the plan, then keep the approved revision and approval digest
returned by this command. The validity period is configurable. Approval is a
separate decision from execution and does not start the camera.

## Execute and inspect

Back on the input node:

```powershell
.\eefn.exe requests execute REQUEST_ID --revision APPROVED_REVISION --approval-digest APPROVAL_SHA256
.\eefn.exe jobs get REQUEST_ID
```

Execution requests the bounded approved session. An acknowledged execution command
is not proof of successful capture or completion. Inspect the retained job's
workload state and events. An initial unknown observation establishes the image
baseline; subsequent observations describe image change. Events contain motion
metadata, not images or a video feed.

When the source is this same node, inspect its retained grant receipt too:

```powershell
.\eefn.exe requests receipt REQUEST_ID --grant-id GRANT_ID
```

Use the grant ID from the job. Receipt inspection is read-only and cannot resume
capture. `completed` describes retained bounded-work evidence; it is not proof
that every external driver or physical effect stopped. Remote-source receipt
inspection uses the coordinator's executor-receipt commands in the
[command reference](../COMMANDS.md#executor-receipt-inspection-through-eef-unreleased-after-alpha6).

## Stop and recover from uncertainty

```powershell
.\eefn.exe jobs stop REQUEST_ID
.\eefn.exe jobs get REQUEST_ID
```

Stop records intent. A stop reply, disconnect or restart can leave the session
interrupted with an unknown outcome. Inspect its retained history and receipt;
do not treat uncertainty as permission to replay capture. Keep the full command,
operation ID and job ID after an unconfirmed reply. Exact execution-receipt reuse
reads retained history rather than granting a fresh capture. A new session needs
a separately reviewed and approved job.

To revoke ambient capture on this node, use `eefn listening disable`. To deny the
camera feature, use `eefn permissions deny camera`. These revoke authority; an
acknowledgment alone does not prove physical release. Typed commands and other
authorized node work remain independently available.

For detailed bounds and checkpoint history, see
[the camera source](../CAMERA-MOTION.md) and
[combined command acceptance](../WORKLOAD-FULL-COMMAND-ACCEPTANCE.md).
