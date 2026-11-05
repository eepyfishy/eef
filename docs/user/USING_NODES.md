# Using nodes

[User guide](../USER_GUIDE.md) | [Installation](INSTALLATION.md) |
[Requests](REQUESTS.md) | [Troubleshooting](TROUBLESHOOTING.md)

Commands below describe the **development build after alpha.6**. Use the
published alpha's dashboard instructions in [First installation](../FIRST_RUN.md)
when your executable does not offer them in `--help`.

## Check your node

From the node's installation folder:

```powershell
.\eefn.exe setup
.\eefn.exe connection show
.\eefn.exe network show
```

`setup` explains connection and model configuration without running inference.
`network show` identifies the node; use its stable ID when another command asks
for `NODE_ID`. The hostname is a friendly name, not an interchangeable ID.

## Allow a feature

Each feature has its own whole-feature switch. You do not need to approve every
folder, file, program, or host when choosing feature-wide access.

```powershell
.\eefn.exe permissions show
.\eefn.exe permissions allow filesystem-read
.\eefn.exe permissions deny camera
```

These are examples, not required first-run permissions. Choose the features you
want this node to provide:

| Feature name | Allows the supported operations for |
| --- | --- |
| `filesystem-read`, `filesystem-write` | Reading or changing files |
| `process-execution`, `applications` | Running programs or controlling supported apps |
| `http-requests` | HTTP requests |
| `wake-on-lan` | Sending a wake request to compatible configured computers |
| `microphone`, `speech-to-text` | Microphone capture or configured speech recognition |
| `audio-output`, `text-to-speech` | Playback or speech output |
| `camera`, `screen-capture`, `input-control` | Camera, screen, or keyboard/mouse operations |
| `peer-transfers` | Separately authorized peer transfers |
| `remote-management` | Allowing eef to apply supported node settings and request restart |
| `remote-updates` | Separately approved remote updates |

A feature switch does not install missing services, override Windows privacy
settings, give administrator rights, or approve a request. Program/app control
can itself affect files; these switches are not a sandbox around other apps.

Inspect saved and applied values. Most changed preferences need an explicit
node restart before the running executor uses them:

```powershell
.\eefn.exe restart --wait-seconds 60
.\eefn.exe permissions show
```

Restart can interrupt work and apply other pending settings. Revoking remote
management takes effect for subsequent remote changes immediately. Revoking a
feature can block new affected requests before restart, but does not undo
effects already produced. Older scoped permissions are preserved until you
explicitly allow that feature broadly. See [permission details](../FEATURE-PERMISSIONS.md).

## Choose a model

Model installation, selection, interpreter assignment, and activation are separate
steps. Inspect first:

```powershell
.\eefn.exe models installed
.\eefn.exe models show
```

Use IDs returned by your node. There is no model name you must use, and this
guide does not prescribe one model for every PC.

For an installed, text-capable Ollama model:

```powershell
.\eefn.exe models select-ollama --model MODEL_ID --modality text
.\eefn.exe models interpreter --backend ollama --model MODEL_ID --activate-backend
```

The second command assigns the selected model to interpret normal-language input
and saves Ollama as the provider. Selecting a model alone does not assign that
role. When the provider is `auto`, reachable Ollama takes precedence over
llama.cpp; an explicit provider choice removes that ambiguity.

For a catalog GGUF that the node already installed:

```powershell
.\eefn.exe models select-installed-gguf --model MODEL_ID --interpreter --activate-backend
```

For your own existing GGUF file, select it first, then assign the interpreter:

```powershell
.\eefn.exe models select-gguf --model MODEL_ID --file 'C:\models\chosen.gguf'
.\eefn.exe models interpreter --backend llamacpp --model MODEL_ID --activate-backend
```

Replace the file path with the real file on that node. A remote selection uses
paths on the target node. Vision models can also require a compatible projector;
do not label a text-only model as vision-capable to satisfy a request.

Apply your intended changes when it is safe to interrupt the app:

```powershell
.\eefn.exe restart --wait-seconds 60
.\eefn.exe setup
.\eefn.exe models show
```

Exactly one active, text-capable selection should have the
`request_interpreter` role. A configured model is not proof it loads successfully
or interprets requests reliably. See [interpreter setup](../INTERPRETER-SETUP.md)
and [model lifecycle](../MODEL-LIFECYCLE-POLICY.md) for advanced controls.

## Install a model explicitly

Choose a model ID from your node's reported catalog or supported backend, then
inspect its installation plan before requesting a download:

```powershell
.\eefn.exe models install-plan --model MODEL_ID
.\eefn.exe models install --model MODEL_ID
.\eefn.exe models download-status
```

Keep the returned download ID. To inspect or cancel that exact transfer:

```powershell
.\eefn.exe models download-status --id DOWNLOAD_ID
.\eefn.exe models cancel-download --id DOWNLOAD_ID
```

Inspect again to confirm the outcome. Cancellation is a request; shared Ollama
layers or transfers can outlive it. A completed installation does not select the
model, assign its interpreter role, or restart the node. Removing a selection
also does not delete the model file or recover its storage. More detail:
[node models](../NODE_MODELS.md), [installation receipts](../MODEL-INSTALL-RECEIPTS.md).

## Manage a node from eef

Read-only inspection does not require allowing remote changes. From the
coordinator's installation folder:

```powershell
.\eef.exe diagnostics
.\eef.exe node setup --node NODE_ID
.\eef.exe node permissions --node NODE_ID show
.\eef.exe node models --node NODE_ID show
```

To permit supported remote settings changes and app restarts, the node owner
must explicitly run this **on the target node**:

```powershell
.\eefn.exe permissions allow remote-management
```

Then the eef owner can, for example, request:

```powershell
.\eef.exe node permissions --node NODE_ID allow filesystem-read
.\eef.exe node restart --node NODE_ID --wait-seconds 60
```

Review pending settings before restarting. Remote management does not grant
workload approval or remote-update consent, and cannot enable itself. To revoke
management, run `.\eefn.exe permissions deny remote-management` locally.

## Pause connections and close the dashboard

```powershell
.\eefn.exe connection pause
.\eefn.exe connection show
.\eefn.exe connection resume
```

Pause disables the outgoing coordinator connection while keeping local commands
available. It is not a general stop command for external programs or ongoing
effects. Resume reconnects using saved endpoints and trust; it does not replay
interrupted jobs. Close the browser whenever you like; the running apps continue
to own the backend state.
