# Backend and browser boundary (unreleased)

The dashboard is an optional client. It does not own runtime state or keep jobs,
connections, model management or update operations alive. CLI and automation use
the same core operations through the local command API; executors use core
services directly. No interpreter is needed for deterministic commands.

| Layer | Node | Coordinator |
| --- | --- | --- |
| Runtime and core operations | `service.rs`, `engine.rs`, model/connection/job command modules | `runtime.rs`, model routing, durable job engine, node-control services |
| HTTP/command adapter | `api.rs` | `api.rs` |
| Optional browser adapter | `dashboard.rs` browser routes and native picker | `web_ui.rs` browser routes |

Core modules must not import the HTTP/browser adapters. The HTTP adapter may
assemble optional browser routes, but disabling them cannot disable core routes.
`NodeDashboard` remains a public compatibility alias for `NodeService`; core
code uses the service name. This does not introduce a second state container.

Node status, configuration save/restore/reset, connection pause/pairing,
proposal handling, startup selection and update check/apply are backend service
operations. The HTTP handlers adapt inputs and outputs. Configuration
read/modify/write operations use the service lock, including connection changes,
so an independent model edit is not overwritten by a stale pause request.

Both binaries accept `--no-ui`. Both crates also have an optional `dashboard`
feature, enabled by default. `cargo build -p eef -p eefn --no-default-features`
omits bundled browser assets; APIs, command services and execution still compile.
No-feature builds cannot serve browser pages even if a saved UI preference is true.

Saved UI preferences are `dashboard.ui_enabled` (node) and `web.ui_enabled`
(coordinator). They never enable a disabled HTTP listener. Legacy node
`dashboard.enabled` retains its historical listener meaning for compatibility.
After a requested node runtime restart, the HTTP adapter updates its browser
route gate without closing/rebinding the command listener. EEF applies its
preference during its existing graceful coordinator restart. Disabling the UI
does not revoke execution permissions or cancel already-authorized work.

Validation includes `tools/test-backend-boundary.mjs` (source dependency guard),
workspace tests with and without default features, API-only real-process
model/job/connection tests, and default-build first-run/browser regressions.
`EEF_TEST_UI_CONFIG=1 node tools/test-model-metadata.mjs` exercises saved UI
preferences with a default build instead of relying on `--no-ui`. These fixtures
use isolated configs and a fake inference backend, not physical two-PC inference.

This boundary is not a claim that every command family or roadmap feature is
complete. Generalized permissions, model bootstrap, interpretation, execution
leases, direct-peer sessions and cluster safety still have their own acceptance
gates. No visual redesign or release is included in this checkpoint.
