# Owner HTTP socket boundary (unreleased)

EEF and EEFN owner HTTP controls require a real loopback socket peer as well as
the existing local Host, matching browser Origin and non-cross-site fetch checks.
The shared guard covers the owner API and browser routes, not just mutating
commands. It runs before route handlers; malformed/missing transport metadata
fails closed. `Host`, `Forwarded`, `X-Forwarded-For` and `X-Real-IP` cannot establish
that a connection came from this PC.

The production EEF listener and node `ApiServer` now supply Axum
`ConnectInfo<SocketAddr>` from the accepted TCP connection. IPv4 loopback, native
IPv6 loopback and IPv4-mapped IPv6 loopback are accepted. Remote IPv4/IPv6 peers,
including IPv4-mapped remote addresses, are refused even with loopback headers.
Ordinary localhost commands, local browser controls and private local enrollment
continue using the same services. No browser dependency, new trust grant,
credential change, model download or command retry is introduced.

Custom Rust embeddings of the EEF router must also serve it with
`into_make_service_with_connect_info::<std::net::SocketAddr>()`. A plain router
without trusted socket metadata receives HTTP 403, including on a local request.
Do not reconstruct that metadata from client-provided headers or let callers
inject a claimed peer. Test-only extension injection is not an application
transport adapter.

## Why the change is required

The previous guard checked local-looking HTTP headers and browser origin, but
not the peer socket. A wildcard-bound owner listener could therefore accept a
non-loopback TCP client that sent `Host: 127.0.0.1:PORT`. Default loopback bindings
limited direct network exposure, but the header check alone did not enforce the
intended boundary when broader interfaces were explicitly configured.

An isolated read-only baseline against the pre-change binaries records this in
`.validation/owner-boundary-vLdYUB/observations.json`: both apps returned HTTP 200
to real non-loopback socket requests using forged loopback Host, with and without
forged forwarding headers. It queried only status/redacted credential inspection;
no credential, permission, settings, model or workload mutation was attempted.
The failing evidence is retained. An earlier harness run closed its write side
too early and obtained no usable HTTP reply; that was a harness error, not proof
of either protection or exposure.

The socket fixture binds each owner listener to a wildcard in its private test
configuration and connects from/to an address actually assigned to this PC, with
that non-loopback source bound explicitly. No second host or external address is
contacted. This checks the real TCP metadata path, not just injected headers; it
is still same-PC evidence, not physical two-PC/firewall acceptance. It requires
an assigned non-loopback IPv4 address and does not certify a missing interface
as a pass. Results intentionally omit the actual interface/address.

## Scope and limits

Owner controls should still bind to loopback. The separate authenticated node
gateway remains the path for remote nodes, owner-approved node commands and
node registration; this change does not restrict that gateway to loopback or
change its framing, credentials, TLS or execution grants. Direct remote HTTP
dashboard/owner management is not made into an authenticated feature by using a
wildcard bind.

Loopback is a **machine-local transport boundary**, not authentication of the OS
user/process. Other local processes and a local reverse proxy/tunnel can originate
loopback connections; forwarded headers are neither trusted nor used to recover
an original remote identity. Do not expose owner controls via an unauthenticated
local forwarding service or run them with unnecessary elevation. Separate
multi-user isolation/authenticated remote-owner transport remains unimplemented.
This increment is not installer/code-signing, Defender, full onboarding, model,
mesh/cluster or complete stable-release acceptance.

## Verification checkpoint

Final workspace/all-target suites pass with default features and separately with
`--no-default-features`: **694 passed, one ignored in each configuration** (EEF
library 243, EEF CLI 17, installer 6, node library 413, node CLI 15). Fresh default
binaries/examples build; formatting, diff, fixture JavaScript syntax and the
backend/browser dependency source check pass.

`tools/test-owner-api-boundary.mjs` passes against those binaries, with evidence
`.validation/owner-boundary-cxdaNY/observations.json` and `results.json`. Unlike
the retained pre-fix HTTP 200 baseline, all four non-loopback/forged-header cases
return HTTP 403 on both wildcard-bound apps. Normal loopback access remains HTTP
200, foreign Origin and cross-site fetch requests remain refused, and both
configuration files remain byte-identical. No settings/permission/workload writes
are sent by this socket fixture. IPv6/mapped address and absent-peer decisions
are unit-tested with test-only trusted-metadata injection; the real socket fixture
is IPv4 on this PC, not an IPv6 or physical second-PC test.

Headless local enrollment (including lost-owner-reply/no-replay), credential and
setup regressions pass in `.validation/local-enrollment-IyM9uy/results.json`,
`.validation/node-credentials-SdKiUD/results.json` and
`.validation/setup-check-PvcC1P/results.json`. These separately exercise existing
local private node registration; the socket-boundary fixture does not itself
test the framed gateway. No installed/remote apps, release versions, installers,
update feeds, models or user configuration are changed by this increment. Test
processes exit afterward; source changes are not an installed-app update.
