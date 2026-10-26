# Python startup observations

The node status and explicit `diagnostics` export include an additive
`python_startup` object. The command/backend owns this data; no browser is
required. It is null before observations exist or if the stored shape is invalid.

The constant-size record contains:

- `complete`: the startup attempt finished, including an unsuccessful attempt.
- `interpreter.state`: not requested, checking, not configured, invalid limits,
  available, or failed. Failed checks include a fixed reason: invalid limits,
  unsupported memory limit, spawn, wait, deadline, cleanup or unsuccessful exit.
- Separate built-in/custom plugin requested, loaded and failed counters. The last
  failure is a selected-bundle lookup failure, an existing typed bridge failure,
  or `other` for an untyped error whose cause is not inferred.

Counters saturate at the unsigned 32-bit maximum. No plugin names, paths,
exception text, credentials or child output are included. Existing private local
logs are not made safe by this record. Diagnostic exports still contain the
existing stable node/runtime IDs for correlation; they are not anonymous exports.
There is no automatic upload.

`available` means the configured interpreter's bounded no-import probe succeeded.
It does not prove plugin inspection succeeded. `complete`, a runtime ID, bridge
enabled status and plugin metadata do not certify ongoing health, usable physical
media, permissions, model quality or effect completion. Failed startup attempts
do not acquire authority, repair a package, retry, change grants or widen limits.
The existing five-second interpreter probe and bridge deadlines are unchanged.

## Hosted failure and evidence boundary

[Run 40291186483](https://github.com/eepyfishy/eef/actions/runs/40291186483)
at `5b48055` failed its Windows bundled-component fixture: the selected plugin's
inspection marker was absent after the runtime ID appeared. Three other jobs
passed. Embedded and regular trusted Python 3.11.9 passed the same fixture
locally, but the hosted failure's exact cause remains unconfirmed. The private
runner logs were not uploaded. Do not replace this failure with a later result.

The fixture retains its original marker, old-root/PATH refusal, capability and
deadline checks, adds controlled interpreter refusal and plugin import failure,
and prints only an independent whitelist of startup state, counts, fixed issue
codes, marker-presence booleans and process outcomes on failure. Names, IDs,
paths and private child logs are excluded. Pure hostile-input tests exercise the
whitelist; backend tests cover typed errors, saturated counters, strict parsing
and read-only status/export behavior. A green later diagnostic revision alone
would not explain the earlier failure or prove repeated physical reliability.

The first local strict-parsing regression failed because internally tagged unit
enum variants accept extra fields. Empty struct variants retain the same JSON
shape while enforcing rejection. That failed local run is retained; the expanded
negative test checks every empty state/failure variant before fresh acceptance.

Fresh full-regression and exact-revision hosted acceptance remain separate.
No release/feed, model/default, installed-node, firewall, signing/Defender, MSI
rollback or remaining roadmap gate is changed by these observations.

## Independent CI observations, not relaxed acceptance

At `4a9a24f`, both local Rust modes pass 1,157 tests plus one existing ignored
profiler each. Thirty-seven native cases and a separate existing regular-Python
component check pass; independent reconciliation verifies 450 source hashes,
twelve artifact hashes and all ten component observations. The first regular
check's signature-path preflight failed before any interpreter/node executed;
the separately corrected helper and original failed record are retained.

[Run 40298967494](https://github.com/eepyfishy/eef/actions/runs/40298967494)
still fails Windows headless at the existing grant-cleanup unit test's required
first poll. It never reaches native Python testing; the other three jobs pass.
The test starts a one-second lease before durable setup and thread/runtime
startup, while production correctly refuses already-expired work before polling.
The precise hosted timing stage is not recorded. Do not renew a production
lease, increase limits or remove cleanup assertions to make this test green.

CI now identifies the Rust-test and fresh-native-build steps separately. On an
uncancelled Windows headless run, native building may follow either a passed or
failed Rust test step, but not skipped setup/tests. Native diagnostics require
that fresh build to succeed. Cancellation still stops work, and Rust failure
still fails the job: no `continue-on-error` or skipped acceptance gate is added.
The existing 31 native suites and distinct connection-order case remain required.
Closed-expression tests cover 288 canonical state combinations; these are not a
general workflow parser or proof of a hosted run.

This uses GitHub's documented
[status-check semantics](https://docs.github.com/en/actions/reference/workflows-and-actions/expressions#status-check-functions)
to collect independent evidence, not reinterpret failure as success. Neither
previous hosted failure is explained or accepted by this workflow change.
