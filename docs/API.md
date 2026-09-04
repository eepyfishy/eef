# HTTP API

The coordinator listens on `127.0.0.1:51334` by default.

Opening that address in a browser displays the accessible dashboard. It edits
the full coordinator configuration and generates node configurations without
requiring source changes.

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/api/status` | Runtime, adapters, models, tiers, and world summary |
| GET/PUT | `/api/config` | Read or save complete coordinator configuration |
| GET | `/api/world` | Event-derived world state |
| POST | `/api/chat` | Handle one user message through the brain |
| GET/POST | `/api/rules` | List or create assistant rules |
| DELETE | `/api/rules/{id}` | Remove a rule |
| POST | `/api/rules/{id}/toggle` | Enable or disable a rule |
| POST | `/api/assistant/execute` | Route a capability locally or remotely |
| GET | `/api/capabilities` | Capabilities and ranked providers |
| GET | `/api/node/status` | Connected and previously registered routes |
| POST | `/api/node/{id}/invoke` | Invoke a specific connected node |
| GET | `/api/memory` | Identity, facts, notes, and recent conversation |
| POST | `/api/memory/reset` | Reset mutable/working memory only |
| GET | `/api/tasks` | Plans and task results |
| GET | `/api/logs?limit=100` | Recent structured events |
| POST | `/api/firmware/generate` | Resolve a description and store ESP source |
| POST | `/api/firmware/push` | Stream stored/source firmware to a node |
| GET | `/api/firmware/list` | Stored firmware versions |
| GET | `/api/update/status` | Update policy and last check result |
| POST | `/api/update/check` | Check the configured release manifest now |
| POST | `/api/update/apply` | Install a verified update for the next restart |

Errors use a uniform JSON object with `success: false`, `error`, and
`error_type`. Node responses preserve the same shape.

Each EEFN also exposes a local configuration dashboard on
`http://127.0.0.1:51336` by default. It remains available without a coordinator;
saved changes apply after EEFN restarts.
