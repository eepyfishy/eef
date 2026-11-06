# Running-origin admission failure observation

The origin-admission candidate `5ee5e1d`, run `42224709777`, failed its new
positive running-origin submission test on both Linux modes and Windows/dashboard.
The node correctly returned an unconfirmed result, but that wrapper did not retain
the coordinator's rejection reason in the test output. The failed evidence remains;
other jobs and full terminal reconciliation are independent requirements.

One additional test sends the same concrete request through the actual origin
mailbox and preserves a synthetic fixture's backend rejection on failure. It
requires a valid confirmed coordinator receipt and no approval/execution on success.
The existing end-to-end node wrapper and source admission tests remain unchanged.
No production code, authority, deadline, retry, permission, assertion or CI gate is
weakened. This is diagnostic evidence collection, not a proven fix or release.

The initial diagnostic source `439e7ab` incorrectly used a crate-private node
method from the coordinator test. The follow-up uses the existing public mailbox
request and validates its returned durable operation ID. Production APIs and all
original tests remain unchanged; the earlier source and its CI result are retained.

The uncorrelated public request is not equivalent to the normal command: without
an explicit operation ID, the gateway uses the short transport ID as context, which
the concrete authoring backend correctly rejects. That result does not identify
the original correlated admission failure. The next follow-up makes the existing
correlated mailbox method public for backend clients and preserves the original
explicit operation ID. Its method body, validation, queue limits and authorization
are unchanged. All original node-wrapper and admission assertions remain required.
