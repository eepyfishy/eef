# Model setup fixture wait observations

Main-ref run `42039659527` for `c668ecc8` failed three Windows model-setup cases:
shared setup guidance, revocation during provider inspection, and revocation of an
admitted transfer. All three failed the same three-second fixture activity wait
at `model_setup_tests.rs:687` with an elapsed timeout. The log does not establish
whether the worker was delayed, ended early, failed provider inspection or stalled
at another boundary. That run remains failed.

The same tests passed in the complete subsequent `9b637b06` source run
`42039781246`, whose full four-job and native acceptance is recorded separately.
The lease-read and naming changes in that candidate do not demonstrate a causal
fix for these model-setup failures.

The shared test helper now retains the original three-second deadline and
ten-millisecond poll interval. On failure only, it reports elapsed observation
time, poll count, target/provider/pull counters, retained journal phase/reason,
whether journal inspection failed, whether the original worker/server finished,
and fixture hold flags. Configuration, model sources, credentials, paths and raw
worker errors are not printed. It does not consume or await the original worker
again; the test's existing joins and assertions remain intact.

No production setup behavior, download, permission, runtime deadline, retry policy
or installer is changed. No extra work is launched when observation fails. This
is diagnostic coverage, not a resolution or evidence that extending a wait would
be correct. Fresh hosted validation is required, with any observed failure and
its bounded diagnostics retained before selecting a production or fixture fix.
