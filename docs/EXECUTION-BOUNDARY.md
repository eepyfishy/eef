# Reviewed workload boundary (unreleased after alpha.6)

The application and public EEF core constructors now require reviewed grants for
workloads. This is a deliberate compatibility change, not a configuration toggle.
Published alpha.6 installers have not been replaced or updated by this increment.
Both roles need a new, versioned release before physical network acceptance.

## Input and execution

- `eefn --ask TEXT` is a client of the **already running node**, using the same
  durable-review service as `eefn requests open --text TEXT`. It never starts an
  ephemeral runtime, registers a competing identity, pairs, or changes models.
  An unavailable interpreter/coordinator is an explicit failure, not permission
  to use a keyword planner or another model.
- Node `POST /api/chat` remains a compatibility input adapter. Its strict body is
  `{ "message": "original text", "request_id": "optional canonical UUID" }`.
  It returns the durable-review report plus a human-readable `reply` that states
  whether review/clarification is needed. This is not fabricated inference output.
  Original whitespace, authenticated origin/area and the save receipt survive.
- `eefn jobs generate-text` now shares `request-text`'s explicit, model-free
  source admission/compilation. It preserves the prompt and exact caller choices,
  returns `awaiting_approval`, and does **not** start inference. Both commands use
  the existing journal, not a second job system. Retain the returned operation ID.
- Preparation/clarification, owner approval and exact execution remain separate
  commands. Model selection, feature switches and pairing are not plan approval.
  See [commands](COMMANDS.md), [explicit inputs](EXPLICIT-REQUESTS.md), and
  [result recovery](RESULT-RECOVERY.md).

Natural input opening is now the default path, but automatic method preparation,
answer-trigger policies and reusable bounded approvals are **not implemented**.
This increment must not be described as a finished non-coder automation workflow.

## Retired paths

Raw `jobs.create` (including owner/node `POST /api/jobs`), legacy plan admission,
legacy resume, ordinary dispatcher execution and coordinator model chat cannot
execute or create new workload records through the shipping constructors.
EEF raw message submissions and coordinator `/api/chat` are rejected before the
legacy Brain handles input. Users submit input through a real running node;
EEF does not fabricate a coordinator-node origin.

This also fences workload-bearing legacy response rules and local adapters.
Non-workload core state/configuration remains available. Automatic rules need
source-bound durable definitions, approval and a reviewed execution path before
being restored; merely calling the former dispatcher is not sufficient.

Historical low-level scheduler/journal tests explicitly use private `cfg(test)`
legacy fixture factories. They are not compiled into production, exposed as an
SDK opt-out, enabled by configuration, or used for application acceptance. The
public `TaskEngine`, `Dispatcher`, and `LlmService` constructors enforce the same
boundary as the shipping runtime. Production job views never advertise legacy
resume as available. Existing records are inspected, not rewritten or replayed.

## Independent sender and receiver enforcement

Ordinary authenticated RPC allows the exact deterministic control capabilities
`system.ping`, `node.configure`, and `node.update`. Existing action validation,
owner permission, update authorization and reserved-control/work limits still
apply. This is not blanket permission for configuration changes or updates.

Every other ordinary capability, including model inference, system information,
filesystem, processes, network requests, camera/audio and plugins, is rejected
with `execution_grant_required`. The sender rejects before queueing/correlation;
the node independently rejects before capacity admission or engine execution,
even if an older/malicious authenticated sender bypasses its sending guard and
the corresponding feature permission is enabled. No grant/RPC downgrade occurs.
The response's `not_started` describes this rejected invocation, **not** proof
that effects from previously issued work have stopped.

The one-shot `NodeClient::submit_message` API is retired without connection or
inference. Its former standalone registration/inbound ordinary execution loop
has been removed. Local interpreter services remain available: inference for
bounded input interpretation is not remote workload authority.

## Exact numeric evidence

The shared JSON dependency now enables its existing `float_roundtrip` feature,
without a dependency version change or additional downloaded library. A valid
fractional result could previously be written, parsed with a slightly changed
floating-point value, then fail its integrity checksum. A deterministic 512-case
regression reproduced this before the fix. Exact numeric round trips retain the
original bytes/checksum; no validation rule is weakened and no evidence is
rewritten to fit a checksum. Old malformed/mismatched evidence remains denied.

## Verification boundaries

Rust tests cover rejection before persistence/capacity/local adapters/models,
unchanged legacy records on resume refusal, independent node rejection of a
forged ordinary filesystem/model request, and control ping after that rejection.
Existing granted execution/replay/receipt/recovery tests remain required.

Public-process fixtures now exercise reviewed grants for scheduling, shutdown,
credentials and model startup. Discovery tests exercise source admission and
fresh compilation rather than legacy direct dispatch. Request-review acceptance
also checks normal input, reviewed text creation and direct legacy-path refusal.
These are isolated loopback/fake-backend tests, not real model-quality, physical
two-PC, strong native termination, MSI or Defender/signing acceptance.

For this increment, `cargo test --workspace --all-targets --locked` passes all
389 tests both with default dashboard features and with `--no-default-features`.
Fresh API-only executables also pass request review, durable requests, scheduling,
shutdown, peer discovery and synthetic model-startup fixtures. Credentials are
checked independently. Formatter, JavaScript syntax and the backend/browser
dependency guard are checked separately. Historical legacy fixture factories
do not count as public-process execution acceptance.

Older `test-model-metadata`, `test-first-run`, and `test-real-model` fixtures still
contain retired workload entry points and require migration before they can be
used as current acceptance evidence. Do not count their previous passes as
validation of this boundary or skip their remaining requirements for release.

Remaining work includes those regressions and client adaptation, automatic
source-bound workflows, heavy planning, stronger termination, continuous
pipelines, authenticated peer data paths, full model lifecycle and cluster
fencing. The full [roadmap](ROADMAP-CORE.md) remains active; stable v0.4.0 is not
declared complete by closing legacy admission.
