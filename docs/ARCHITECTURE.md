# Architecture

This page describes the v0.4.0a baseline. The accepted next architecture and
source-module mapping are in [Command-first core roadmap](ROADMAP-CORE.md).
The target makes CLI, automation and UI clients of shared core commands; nodes
interpret input and execute locally, while EEF plans/authorizes and controls
scoped peer data paths. Visual UI work is paused. Execution leases, continuous
pipelines, peer authorization and default-LM bootstrap remain pending. The first
development slice now extracts `eefn::NodeService` and adds independent network
advertisements plus [headless network commands](COMMANDS.md). Model management
uses runtime-owned state instead of a dashboard-owned object.

This implementation keeps the coordinator and node in one Rust workspace so
their protocol types cannot silently diverge.

## Process topology

```text
user -> originating eefn.exe -> eef.exe -> selected eefn.exe node(s)
             ^                    |                  |
             +------ result ------+<---- results ----+

admin/dashboard -> HTTP :51334 -> eef.exe

eefn.exe -> owner-selected Ollama / llama.cpp model
eef/eefn -> python.exe -I -c <embedded host> <explicit plugin.py>
```

`eef` owns the event bus, world state, identity/mutable/working memory,
capability registry, adapter policy, model registry, planner, task engine,
assistant rules, firmware service, node gateway, and Axum API.

The coordinator never discovers, installs, loads, warms, or invokes a local
model. Its `LlmService` only routes requests to models explicitly advertised by
authenticated connected nodes.

The coordinator is not implicitly a node. It starts its dashboard and gateway
in `waiting_for_node` state and becomes operational only after at least one EEFN
connects. Device actions and model inference therefore always come from nodes.

`eefn` owns authenticated failover/reconnect, live load/spec reporting, scoped
filesystem and process/network/application capabilities, model and configurable
STT provider access, encrypted framing, OTA streaming, versioned updates, and
opt-in Python media adapters using the bundled interpreter.

Coordinator endpoints have an owner-configured integer priority. EEFN sorts
them highest-first on every connection attempt, so loss of the preferred EEF
causes a retry against the next reachable EEF. String endpoint entries remain
supported and have priority zero.

## Hard-rule mapping

| Contract from the current docs | Rust implementation |
| --- | --- |
| Tasks actually run | `TaskEngine` refuses to run without a wired `TaskExecutor`; `Runtime` wires `Dispatcher` at startup. |
| Selection uses real load | Capability and model registries rank live load, latency, capacity, hardware, warm state, and constraints. |
| Arguments are validated | Provider `params` schemas validate required fields, types, bounds, and enums before invocation. |
| Sensitive abilities default-deny | Adapter policy rejects keyboard, screen, filesystem mutations, shell, GPIO, servo, motor, relay, and LED unless explicitly allowed. |
| Failures are typed/structured | Node responses and API errors contain `success`, `error`, and `error_type`; events retain structured history. |
| API before UI | The implementation exposes `/api/*`; no UI is coupled to the runtime. |
| Green milestones | Workspace tests cover crypto, protocol, routing, policies, execution, persistence, firmware, and intent parsing. |

## Python boundary

CPython is a plugin sidecar, not part of the Rust process. Each inspect or invoke
operation starts the bundled interpreter with isolated Python flags, exchanges
one JSON request/response over standard I/O, enforces a timeout, and exits.
Plugin output is redirected away from the JSON channel.

This is process isolation for reliability, not an operating-system sandbox.
Only trusted plugin paths should be configured.

## Interaction path

Normal user input originates on a node. `eefn --ask "..."` authenticates,
registers the node, submits the message, continues servicing capability calls
needed by the plan, and prints the final result returned by EEF. The HTTP chat
route remains an administrative/dashboard interface, not the required network
topology.

## Conformance boundary

The single-coordinator execution path is implemented. Partition-safe
active/standby coordinator election and the coordinator-independent node mesh
are not represented as complete in this release; see `STATUS.md`. Those need a
quorum/lease design and a separately authenticated peer protocol. Merely
starting a second unrestricted coordinator would not satisfy the split-brain
requirement, so the project does not claim that it does.
