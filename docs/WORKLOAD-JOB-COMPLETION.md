# Coordinator workload completion draft

The coordinator now has an internal completion checkpoint for the existing
reviewed workload job. It uses the original `Issued` token and `JobStore`, not a
second scheduler or a dashboard controller. Authenticated dispatch and owner
execution commands still need integration; this method does not authenticate a
transport or launch a source by itself.

## Receipt and original issuer

The caller must supply issuer, session and node identities from the pinned
authenticated connection, independently of the receipt body. Completion requires
the exact issued definition, a previously saved consumed acknowledgement, a valid
schema-three node completion receipt and a lease sequence that the original
issuer actually issued. Node acceptance time must equal the saved node ACK time;
node update/recording times must not precede that ACK. These node timestamps are
never compared with the coordinator's independent wall clock.

The original issuer closes before the completion write. No further renewal is
available while storage is blocked or after it fails. A failed completion write
attempts one uncertainty checkpoint; dropping the token does not retry completion
or restore authority. An interrupted/unknown job cannot be completed through a
retired token. A future authenticated historical reconciliation command must be
separate from execution or renewal; it is not implemented by this draft.

## Existing durable job

The compact record retains the node consumption metadata and completion report,
the last sequence the issuer sent, and the coordinator's recording time. The
already immutable issued definition is reused rather than stored a third time.
Validation reconstructs the typed node receipt against that definition. The
coordinator retains a trusted node's reported event-prefix hash; it does not
independently attest physical capture or reconstruct a source from that hash.

One existing job update saves the completion and sets its single workload task
and plan to completed. Its execution record uses schema three. Old issued,
accepted and unknown workload records retain schema two and omit the optional
completion. No media or fabricated ordinary `TaskResult` is inserted. Definition,
consumed ACK and saved completion remain immutable. Reopening preserves completed
history and never recreates an issuer. Generic interruption only retires active
workload states, preserving terminal completion.

An identical receipt from the same pinned identities returns the saved plan with
no new revision. A changed receipt or peer is refused, including after completion.
Completed history cannot renew, resume or replay work. Older readers cannot
reinterpret these records as ordinary v1 execution; do not downgrade live journals.

Issuance reserves an additional 16 KiB within the existing owner job-storage quota
before any offer. The compact completion payload is bounded below that allowance,
including checkpoint overhead. Completion releases logical reservation; it does
not delete retained evidence or claim physical disk reclamation. Existing owner
limits are not increased or bypassed.

## Runtime and verification boundary

Hosted run `41146319859` exposed a stack overflow in the prepared native source
test. The candidate now boxes transport, filter completion, listening control and
consumed supervision at their composition boundaries. A new Windows regression
bounds the returned unpolled runner future and verifies unused drop retains
uncertainty without starting an interpreter. The real native source test checks
the same frame bound while retaining its process/event/completion assertions.
Thread stacks, compiler optimization and production execution limits are unchanged;
the exact overflow call site is not proven by the existing failure log.

The earlier main-ref run `41145461807` failed the synthetic process-observation
helper's unrelated five-second startup timer. Native prepared tests now observe
held process identities using the original session horizon while racing the
actual supervised source result. Original leases and transport limits still end
the source first if they expire; no fresh execution budget is minted. Other
process fixtures retain their existing five-second default. The native finite
audio pipeline's separate interrupted result remains failed and unexplained;
closed-code-only failure observations are added without weakening its assertions.

Six new coordinator regressions use real temporary job journals with explicitly
synthetic authenticated-node receipts: durable completion/reopen/idempotence,
ACK and issued-sequence fences, peer/scope/time correlation, terminal immutability,
failed writes and nine corruption variants. They do not prove network
authentication or actual source behavior. Node native regressions establish a
separate combined source boundary. All new changes require fresh hosted validation.

Public authenticated offer/lease/ACK/event/completion/stop transport, bounded
asynchronous coordinator I/O, owner commands, long sessions, physical two-PC
acceptance and stable release gates remain open.
