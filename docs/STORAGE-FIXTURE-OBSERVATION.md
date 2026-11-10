# Storage fixture observation windows

The explicit interpreter command source `09d6574` passes both Linux jobs. Its
[Windows dashboard job](https://github.com/eepyfishy/eef/actions/runs/42728889098)
failed two older metadata-persistence tests: waiting for actual journal retirement
after runtime destruction exceeded three seconds, and positive completion-fixture
issuance exceeded its two-second observation budget. The full failure log is
retained. No evidence yet establishes whether scheduling, storage latency or
another condition caused those elapsed deadlines.

The database connection permits a five-second SQLite busy wait. Positive fixture
setup and observation of historical I/O should allow that wait to finish before
classifying the expected final state. Those three fixture waits now allow ten
seconds: creating the completed-workload fixture and observing final ownership
release in the issuance and completion runtime-destruction cases.

All three files are included only by coordinator tests. Production timeouts,
leases, original caller windows, consent, quotas and queue ownership are
unchanged. Main still allows five seconds for final journal retirement and
refuses replacement if it cannot prove release. This change does not prove that
physical storage or restarts meet that five-second allowance.

The tests still require prompt runtime destruction, refusal to open an owned
database, the exact retirement signal before reopening once, and the original
terminal state and revision. They do not repeat a write or open, fabricate a
completion or release a lock on timeout. Existing original-deadline refusal,
late-save, cancelled-waiter and unknown-outcome assertions remain intact.

This fixture follow-up needs its own complete hosted validation. It does not
turn the failed run into a pass or complete interpreter command, automatic
normal-input, physical or stable-release acceptance.
