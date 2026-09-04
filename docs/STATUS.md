# Architecture conformance status

This file distinguishes working behavior from planned architecture. It prevents
the open-source project from presenting an unsafe approximation as complete.

## Implemented in 0.2.0

- Rust coordinator and PC node executables in one workspace.
- Normal `node -> EEF -> selected node(s) -> EEF -> originating node` requests.
- Authenticated, encrypted node transport with configurable endpoints and
  priority-ordered reconnect/failover across the configured endpoint list.
- Live capability/model registration and load, latency, capacity, hardware,
  permission, modality, and constraint-aware routing.
- Dependency-aware task plans, concurrent ready subtasks, timeouts, retries,
  cancellation state, structured results, and node reassignment on retry.
- Structured identity, mutable, working, event, conversation, and task state.
- Default-deny sensitive adapters and explicit filesystem roots.
- Owner-selected Ollama and llama.cpp models; no coordinator model and no model
  in the node bundle.
- Optional isolated Python plugin sidecars using the bundled CPython runtime.
- Configurable `off`, `prompt`, and `auto` verified updates for both programs,
  with version handoff and rollback.
- Local EEF and EEFN dashboards for full configuration and optional startup.
- Standalone EEFN operation with zero coordinators; EEF only becomes
  operational when at least one node connects.

## Not yet complete

- Automatic coordinator discovery, quorum election, replicated coordinator
  state, and partition-safe split-brain prevention. Configured node priorities
  already provide deterministic failover when the preferred EEF disconnects.
- A separately authenticated EEFN peer mesh that remains online when every EEF
  coordinator is unavailable.
- Transport plugins beyond TCP. The protocol is not coupled to a VPN vendor,
  but the pluggable transport interface is still future work.
- Durable restoration of an in-flight task graph after coordinator process
  failure.
- Signed release manifests. Updates currently require HTTPS plus an exact
  artifact SHA-256.

Reliable coordinator election cannot guarantee both availability and a single
leader during arbitrary network partitions without quorum. The intended next
milestone is a fixed-membership, majority-lease coordinator cluster with a
durable term/vote log, followed by state replication. The mesh is a separate
node role and must not silently promote ordinary nodes into coordinators.
