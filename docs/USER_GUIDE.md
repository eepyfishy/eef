# eef user guide

eef connects your computers into a network of **nodes**. You send requests through
a node; eef coordinates the work, and nodes provide the models and tools you allow.

This guide describes the **current development source after alpha.6**. The
published `v0.4.0a6` installers contain fewer commands and use the older setup
flow. For those installers, start with [First installation](FIRST_RUN.md) and
[their release notes](RELEASE-0.4.0a6.md). A development feature documented here
does not become available by changing an alpha install's settings.

## Start here

| What you want to do | Read |
| --- | --- |
| Install, start, or connect your first node | [Installation and connection](user/INSTALLATION.md) |
| Choose models, enable features, or manage another node | [Using nodes](user/USING_NODES.md) |
| Submit a request, review it, run it, and find its result | [Requests and results](user/REQUESTS.md) |
| Fix a connection, model, permission, or command problem | [Troubleshooting](user/TROUBLESHOOTING.md) |
| Look up advanced commands | [Command reference](COMMANDS.md) |

## What to install

| App | Purpose | Where it is needed |
| --- | --- | --- |
| `eef` | Coordinates requests and keeps their history | At least one PC in the network |
| `eefn` | Makes this PC a node, with its own identity, permissions, models, and tools | Every PC you want to use as a node |

For your first PC, install both. Another PC can run only `eefn`. Installing `eef`
alone does not make that PC a node. eef uses models offered by connected nodes;
you do not need a separate model just for the coordinator.

Neither alpha.6 installer bundles a model. Development builds add explicit model
setup choices, but there is no approved universal default model. You can inspect
and configure a node, or submit supported explicit tool requests, without a model.
Interpreting ordinary language needs a selected request interpreter.

## How commands and the dashboard fit together

The running apps own configuration, requests, and automation. Commands and the
dashboard are interfaces to those services. Keep the apps running when issuing
commands that contact them. Closing a dashboard tab does not stop an app.
Development builds support `--no-ui` to run command/API services without the
browser interface. UI work is paused, so some newer features have commands only.

Examples use Windows PowerShell. Open a terminal in the app's installation
folder and use the `.\` prefix shown below:

```powershell
.\eefn.exe --version
.\eefn.exe --help
```

Run `eef` examples from the coordinator's folder and `eefn` examples from the
node's folder. For a custom installation, use your chosen folder. Advanced users
with multiple configurations can pass `--config` followed by the full path to
the intended configuration. Normal setup does not require editing JSON or
entering local API ports. Add `--json` when you need structured command output.

Words such as `NODE_ID`, `MODEL_ID`, and `REQUEST_ID` are placeholders. Replace
them with the exact values returned by your app; do not type them literally.

## Understand the main states

| State or message | Meaning | What to do |
| --- | --- | --- |
| Waiting for a node | eef has no usable connected node | Start and connect `eefn` |
| Connected | The node has a connection to eef | Check models and permissions separately |
| Restart required | Saved settings differ from the running settings | Review pending changes, then restart the app |
| Review / clarification | A request still needs interpretation or an answer | Inspect it and answer the saved question |
| Awaiting approval | A concrete plan has been saved | Have the eef owner review and approve it |
| Approved | Conditional approval is saved | Explicit execution is still a separate step |
| Completed | Final evidence was retained | Inspect the result; check whether the task succeeded |
| Interrupted / unknown / unconfirmed | The app cannot establish the final outcome | Inspect existing history before sending more work |

A connection, enabled feature, installed model, and approved request are different
things. None substitutes for the others. Stopping a request records stop intent;
already-sent actions can have effects before stop is observed.

## Current limits

Stable v0.4.0 is not yet released. Continuous microphone sessions, automatic
room-following audio, virtual call inputs, general camera/screen relays,
Bluetooth/Wi-Fi device control, ESP mesh routing, safe coordinator clustering,
and self-coding updates are not ready for ordinary use. Development finite audio
and automation commands have narrower requirements; follow their specific docs
before trying them. Multiple coordinator priorities order reconnection attempts;
they do not share job history or provide a replicated cluster.

The supported Windows release assets are two EXE installers. MSI installation,
full recovery acceptance, signing, and resolution of historical antivirus
reports remain open work. See [installation](user/INSTALLATION.md) and the
[active roadmap](ROADMAP-CORE.md) for the relevant boundaries.
