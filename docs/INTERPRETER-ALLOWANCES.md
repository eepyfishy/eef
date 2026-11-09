# Durable interpreter allowances

This unreleased development increment implements the core journal reservation
boundary and read-only usage commands. Source `af4ba6c` passes all four jobs in
the [hosted run](https://github.com/eepyfishy/eef/actions/runs/42627311859) and an
independent source/log audit, including 1,699 Windows/headless Rust tests and
all 37 native suites. Ordinary
input does not delegate automatically yet; the three-stage broker still needs
integration with delivery, result promotion, cancellation and explicit recovery.
No new dependency, worker, model download, polling service or dashboard logic is
required by this ledger.

## Inspect usage

Use the running coordinator's configuration:

```powershell
eef --config <coordinator-config> interpreter-policies usage
eef --config <coordinator-config> interpreter-policies usage <policy-id> --limit 25
eef --config <coordinator-config> interpreter-policies usage <policy-id> --after-sequence 25
```

Omitting the ID inspects the latest policy; an explicit ID inspects historical
usage even after supersession or revocation. Pages contain at most 100 attempts.
The local-owner API uses the same backend, pins the coordinator runtime and
limits request bodies to 4 KiB. Browser origin checks still apply. Neither a
current model nor a live provider is needed for inspection.

Reports contain conservative reserved calls, output tokens, execution time and
result bytes, occupied slots and bounded attempt summaries. They do not contain
raw requests, answers, prompts, complete grants or model output. These are
reserved upper bounds, not measured resource consumption or proof of which
weights a remote operator ran. Inspection starts no grant or inference.

## Reservation boundary

The internal reservation path checks the latest exact policy generation and
authorization digest, current private origin/provider snapshots, directional
disclosure, exact selected backend/model, serving permission revision, applied
grant limits and the immutable retained source. Saved and applied coordinator
authentication must match. Existing shared prompt builders define all three
stages; callers cannot supply arbitrary service prompts.

One journal transaction reserves aggregate allowances and creates an immutable
source-bound attempt. Initial interpretation also claims the raw input in that
transaction. Failure leaves both source and budget unchanged. Per-call limits
intersect policy and serving-node limits. Global, origin and provider concurrency
checks include unresolved work from previous policy generations.
An expiring policy also caps the grant's wall-clock and monotonic execution
deadlines at its remaining validity; starting before expiry cannot extend it.

Each reservation charges its worst-case call, output-token, execution-time and
result-byte budgets. Charges survive known-unsent cancellation, failure, restart,
policy revocation and removal of the original finished job. Separate retained
accounts are reconciled against sequenced, digest-linked attempt bindings;
missing or inconsistent retained data is refused without resetting allowances.
These checks detect inconsistent journal history; they are not protection
against an operator who can replace the entire database and its history.

Accounts, attempts and bounded checkpoint headroom share the existing journal's
count and byte limits with jobs, policies, automation and setup records. Recovery
that stays within already reserved space remains possible after quota reduction.
Logical headroom cannot guarantee writes when the physical disk is full.

## Delivery and recovery

A fresh reservation returns a nonserializable in-process continuation. Reading
history or reusing its source returns an observation, never another continuation.
Before returning an internal dispatch object, the backend rechecks source,
policy, participants, permission revision, exact model and original deadlines,
then durably records delivery intent. That object does not send a grant itself.
The eventual broker must use the existing authenticated grant/lease exchange.

Known-unsent retirement marks the attempt `not_sent` and retires its initial
source claim in the same transaction. On journal reopen, a reservation without
delivery intent becomes `not_sent`; delivered or accepted work becomes
`interrupted`. Neither path reconstructs delivery authority or retries work.
The original wall-clock and monotonic deadlines are never renewed.

Reports must confirm the exact reserved grant and existing result validators.
Completion needs its matching durably saved acceptance. Ordinary observer
reports cannot settle an interrupted attempt after reopen; that needs the
separately authenticated explicit recovery path still to be integrated.
Accepted and final checkpoints use reserved storage. Failed final observations
retire initial ownership without inventing a successful proposal. An unknown
completion continues occupying a concurrency slot: an observer timeout does
not prove shared Ollama computation stopped. It also retains its original
checkpoint headroom for explicit recovery. Explicit uncertainty resolution
remains part of broker recovery work, not an automatic budget refund.

These source mechanics do not establish physical two-PC operation, actual model
quality, complete setup, continuous-work/strong-stop behavior, installer recovery,
signing/Defender acceptance or a stable release.
