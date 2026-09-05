# EEFN capability catalog

An EEFN node advertises only capabilities that are available and permitted by
its current configuration. Except for basic status and updates, permissions are
default-deny. Changes made in the node dashboard apply after EEFN restarts.

## Core capabilities

| Capability | Actions | Important parameters | Permission/configuration |
| --- | --- | --- | --- |
| `system.ping` | `run` | none | Always available over the authenticated node link. |
| `system.info` | `get`, `read`, `run` | none | Always available; reports basic CPU, RAM, GPU, storage, platform, and current load. |
| `node.update` | `check`, `apply` | optional `manifest_url` | Uses the node's configured feed unless explicitly overridden. |
| `llm.infer` | `run` | `model`, `prompt` or `messages`, optional generation settings | Advertised only for an owner-selected text model present in Ollama or configured as a llama.cpp slot. |
| `vlm.analyze` | `analyze`, `run` | `model`, `prompt` or `messages`, `images` | Advertised only for an owner-selected vision model. Images are URLs or base64/data URLs. |
| `filesystem` | `read`, `list`, `write`, `mkdir` | `path`; `write` also accepts UTF-8 `content` | Requires read/write flags and existing explicit roots. Byte and directory-entry limits are configurable. |
| `process.exec` | `run` | absolute `program`, optional string `args`, scoped `cwd`, `timeout_seconds` | Requires an exact existing absolute executable allowlist. No shell is used. Time, argument, and output limits apply. |
| `application.control` | `list`, `launch`, `terminate` | absolute `application`, optional string `args`; terminate also needs `pid` | Requires an exact existing absolute application allowlist. `launch_application` remains a compatibility alias. |
| `http.request` | `send` | `url`, optional `method`, string `headers`, `body`, `timeout_seconds` | Requires host and method allowlists. Redirects are off. Private/non-routable targets are off unless explicitly enabled. |
| `stt.transcribe` | `run`, `transcribe` | `audio_base64` or approved `audio_path`, optional `filename` | Requires an owner-configured OpenAI-compatible transcription endpoint and model. API tokens come from the named environment variable. No STT model is bundled. |
| `network.wol` | `send`, `wake` | `mac`, IPv4 `broadcast`, UDP `port` | Requires exact MAC-address, broadcast-address, and port allowlists. No network destination is selected automatically. |

## Opt-in media capabilities

These adapters run through bundled CPython in a short-lived sidecar. They load
automatically only when their corresponding `permissions.media` switch is on.

| Capability | Actions | Important parameters |
| --- | --- | --- |
| `audio.capture` | `devices`, `record` | device, duration up to 20 seconds, sample rate, channels; returns base64 WAV. |
| `audio.play` | `play`, `stop` | `data_base64` containing a WAV up to 25 MB. |
| `tts.speak` | `voices`, `speak` | text, voice, rate, volume; uses voices installed in Windows. |
| `camera.capture` | `list`, `capture` | device, width, height, JPEG quality; returns base64 JPEG. |
| `screen.capture` | `capture` | PNG/JPEG format and quality; returns base64 image. |
| `input.control` | `type`, `press`, `hotkey`, `key_down`, `key_up`, `move`, `click`, `scroll` | Action-specific keyboard/mouse parameters; PyAutoGUI's corner failsafe stays enabled. |

## Policy example

Paths in JSON use escaped backslashes. This example illustrates configuration
shape only; owners must choose paths and hosts that are appropriate for their
own node.

```json
{
  "permissions": {
    "filesystem": {
      "read": true,
      "write": false,
      "roots": ["D:\\ApprovedData"],
      "max_read_bytes": 4194304,
      "max_write_bytes": 4194304,
      "max_list_entries": 10000
    },
    "process": {
      "enabled": true,
      "allowed_executables": ["C:\\Windows\\System32\\whoami.exe"],
      "max_seconds": 30,
      "max_output_bytes": 1048576
    },
    "applications": {
      "enabled": true,
      "allowed": ["C:\\Windows\\System32\\notepad.exe"]
    },
    "http": {
      "enabled": true,
      "allowed_hosts": ["api.example.org", "*.services.example.org"],
      "methods": ["GET"],
      "allow_private_networks": false,
      "max_response_bytes": 4194304
    },
    "stt": {
      "enabled": false,
      "endpoint": "",
      "model": "",
      "api_key_env": ""
    },
    "wake_on_lan": {
      "enabled": true,
      "allowed_macs": ["00:11:22:33:44:55"],
      "broadcast_addresses": ["192.0.2.255"],
      "ports": [9]
    },
    "media": {
      "microphone": false,
      "audio_output": false,
      "tts": false,
      "camera": false,
      "screen_capture": false,
      "input_control": false
    }
  }
}
```

The coordinator routes by advertised capability rather than model, executable,
application, device, or provider names. Forks can add Python capabilities with
`python_plugins`; plugin code is trusted code and is not an OS sandbox.
