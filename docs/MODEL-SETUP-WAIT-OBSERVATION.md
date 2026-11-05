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

## Independent historical-completion fixture failure

The diagnostic candidate `69b95fea` passed the model-setup cases but failed the
Windows `blocked_historical_completion_is_bounded_without_retaining_source_authority`
case in run `42046011083`. It expired its three-second synthetic source lease
inside the journal-backed fixture, before reaching the blocked final-save test.
That is a separate failure and does not explain the earlier model-setup waits.

The completion case now uses the same existing contract and initial fixture
horizon as the other journal-backed completion cases. Its lease remains capped
by the unchanged ten-second contract ceiling. Once the source has actually
finished, the test observes its original sealed deadline through a read-only
test accessor and explicitly waits for that exact lease to expire before starting
historical persistence. It asserts the original deadline has elapsed, the sealed
boundary still validates, and reporting remains retired. No boundary is mutated,
renewed or reconstructed. Existing live-source expiry/refusal tests remain.

The production final-worker wait stays three seconds, and all blocked-save,
capacity, historical receipt and no-replay assertions remain. Only fixture staging
changes; fresh full hosted acceptance is required. The failed diagnostic run and
the still-unexplained model-setup failures remain retained.
