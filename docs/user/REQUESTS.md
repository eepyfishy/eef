# Requests and results

[User guide](../USER_GUIDE.md) | [Installation](INSTALLATION.md) |
[Using nodes](USING_NODES.md) | [Troubleshooting](TROUBLESHOOTING.md)

This page describes **development builds after alpha.6**. Published alpha.6 has
older request behavior. Use matching updated coordinator and node builds with
private, pinned node credentials for the reviewed execution flow below.

## The request flow

Send input through a running node. A request keeps that node as its origin even
when another node executes the work. It passes through these separate stages:

1. **Review:** save what you asked, and resolve any missing information.
2. **Plan:** save the exact operations, inputs, and chosen executors.
3. **Approve:** the local eef owner approves that exact plan with explicit bounds.
4. **Execute:** explicitly request the approved plan once.
5. **Inspect:** read retained status and results.

Enabling a feature, pairing a node, or saving a model does not skip approval.
The dashboard and commands use the same backend rules.

## Try a request without a model

An unreleased `save-input` command can retain ordinary text before any model is
configured. Its command flow passed hosted verification at `4ef0935`:

```powershell
.\eefn.exe requests save-input --text 'Your request'
.\eefn.exe jobs get REQUEST_ID
```

This saves raw input only. It does not interpret, prepare, approve or execute it.
The new `eefn requests interpret-saved REQUEST_ID` follow-up is in development
and awaiting hosted verification. It explicitly uses this node's selected local
interpreter, saves a proposal under the same request ID, and starts no tool work.
The command reads and pins the current source itself; it needs no JSON, revision
or digest copied by the user. Repeated attempts inspect retained interpretation;
an uncertain attempt cannot automatically run again. Missing models leave the
input saved and core services available. Automatic remote interpretation remains
planned, and `requests input` still uses its existing local flow.

You can cancel saved input with
`eefn jobs stop REQUEST_ID`. To inspect after a lost reply, use the operation ID
with `eefn jobs find --operation-id OPERATION_ID`. Reusing an explicit
`--operation-id` with the same text returns the retained record; changing the
text is refused. This protection lasts while the job remains in history.

To create an executable plan without a model, use an explicit template instead:

This example asks for basic system information using an explicit template. It
does not need an interpreter or a JSON input file. Both apps must be running and
the execution node must support reviewed grants.

From the node's installation folder:

```powershell
.\eefn.exe requests submit --text 'Show system information' --template system_info
```

Keep the returned request ID and operation ID. This saves and compiles a plan;
it does not execute the tool. Inspect it:

```powershell
.\eefn.exe requests plan REQUEST_ID
```

Read the selected executor and exact operation. To choose a particular node
up front, the submission also accepts `--node NODE_ID`. Otherwise the compiler
chooses from eligible nodes; do not assume a request always executes locally.

On the coordinator PC, the eef owner inspects the private plan:

```powershell
.\eef.exe requests inspect REQUEST_ID
```

Use that plan's current `revision` and `plan_sha256` in the approval below. The
example bounds allow up to one second of execution, 16 KiB of result data, and
60 seconds to begin the approved request. The app rejects incompatible bounds;
choose bounds appropriate to the reviewed operation rather than copying them
onto different work.

```powershell
.\eef.exe requests approve REQUEST_ID --revision PLAN_REVISION --digest PLAN_SHA256 --valid-for-ms 60000 --max-execution-ms 1000 --max-result-bytes 16384
```

Approval saves a receipt. It still does not dispatch work. Keep its **new
revision and `approval_sha256`**, which are different from the earlier plan values.
From the original node, explicitly execute that approved revision:

```powershell
.\eefn.exe requests execute REQUEST_ID --revision APPROVED_REVISION --approval-digest APPROVAL_SHA256
.\eefn.exe jobs get REQUEST_ID
```

The local eef owner can use `eef requests execute` instead. Do not send both as
retries. Inspect first after a timeout or lost response; an acknowledgement can
be lost after execution was admitted.

## Ask in ordinary language

First [configure exactly one request interpreter](USING_NODES.md#choose-a-model).
Then submit input through the running node:

```powershell
.\eefn.exe requests input --text 'Show system information'
.\eefn.exe jobs list
```

Depending on applied input policy and the model's response, this saves a review,
a clarification question, or a prepared plan awaiting approval. Input does not
approve or execute work and does not guarantee a conversational answer.

Inspect the returned request and answer only its actual saved question:

```powershell
.\eefn.exe requests draft REQUEST_ID
.\eefn.exe requests respond REQUEST_ID --revision CURRENT_REVISION --text 'Your answer to the saved question'
```

Inspect again after each change. If the request remains in review and is ready
for explicit preparation:

```powershell
.\eefn.exe requests prepare REQUEST_ID --revision CURRENT_REVISION
.\eefn.exe requests plan REQUEST_ID
```

Preparation can itself ask for clarification. Once a concrete plan exists, the
same separate owner approval and explicit execution steps above apply. A model's
proposal is not an approval, and model output should be checked before granting
file changes, program execution, or other effects.

## Find results

From the originating node:

```powershell
.\eefn.exe jobs list
.\eefn.exe jobs get REQUEST_ID
```

For bounded text-generation output, use `.\eefn.exe jobs output REQUEST_ID`.
For an issued attempt's retained executor result, copy its grant ID from the job
and use:

```powershell
.\eefn.exe requests result REQUEST_ID --grant-id GRANT_ID
```

Read-only executor receipt inspection is also available with
`requests receipt REQUEST_ID --grant-id GRANT_ID`. Retrieving a result does not
rerun work or automatically settle an interrupted coordinator record. Advanced
explicit settlement is described in [result recovery](../RESULT-RECOVERY.md).

## Keep command receipts after uncertainty

Commands that accept `--operation-id` can use an ID you generate before sending.
This helps find a request even if its first reply is lost. In PowerShell:

```powershell
$operationId = [guid]::NewGuid().ToString()
$operationId
.\eefn.exe requests submit --text 'Show system information' --template system_info --operation-id $operationId
```

Keep the ID and the exact command. To locate retained jobs without resubmitting:

```powershell
.\eefn.exe jobs find --operation-id $operationId
```

Each command's receipt has its own rules. Reusing an exact retained receipt can
read the past outcome; it does not authorize a new attempt. Inventing a fresh ID
after an unknown outcome can create new work. Do not remove history or create
another identity to force a retry.

## Stop a request

```powershell
.\eefn.exe jobs stop REQUEST_ID
.\eefn.exe jobs get REQUEST_ID
.\eefn.exe jobs stop-status REQUEST_ID
```

Stop records intent and prevents further authorized work where supported. Read
the stop evidence: acknowledgement alone does not prove every external program,
remote service, or physical device has stopped. Actions can already have taken
effect. Closing the dashboard, pausing a connection, or restarting eef is not an
undo operation. Interrupted work is not automatically replayed on restart.

Continuous source inspection and approval commands are development primitives;
they do not provide a public continuous-session launch command. Advanced finite
audio and scheduled automation have separate setup and approval requirements.
Start with [the command reference](../COMMANDS.md) rather than treating this
one-shot example as authorization for background recording or repeated actions.
