# Reviewed continuous job issuance

The coordinator draft extends the existing review, concrete planning, approval
and `Plan.execution` records. It uses the existing `JobStore` and task assignment
identities, not a second job database or a separate timer-loop recording system.
A shared exact template carries the source/resource/adapter pins and all session,
memory, source-frame and event limits before a grant ID or node session exists.

Internal checkpoint APIs compile that template as one continuous task, save a
distinct schema-two owner approval, and store the full issued definition before
returning an issuer token. The existing v1 scope/start paths refuse that approval.
Legacy records omit the new optional fields and retain their original serialized
definitions and approval hashes. Old readers cannot interpret the new records as
v1 execution authority; do not downgrade journals containing them.

Schema-two approval retains the exact template and its approved plan digest. Its
common `limits` fields mirror the short lease window and reporting bound; they
are not a legacy execution grant or an increase of v1 duration limits. The owner
validity must cover the entire requested session. Issuance pins the original
request context, one task/attempt, chosen executor/resource and authenticated
session identity. It reserves storage before recording authority. An ordinary
operation grant cannot be derived from this approval.

The issuer token is neither serializable nor cloneable. It retains the original
monotonic session end across database I/O, checks local clock continuity and
reads the current durable issuance before producing a lease. The first offer
can precede consumption; subsequent windows require the exact validated node
consumption ACK to be saved first. ACK timestamps are node-local evidence and
are not compared to the coordinator's wall clock. Retained identical ACKs are
read-only; changed scope or an interrupted job cannot reactivate renewals.
An observed local liveness failure permanently retires both lease and ACK
authority before attempting an uncertainty checkpoint. Correcting the clock or
recovering database access cannot revive the same token, even if that checkpoint
failed. A malformed peer ACK alone does not retire an otherwise live issuer.

Generic job checkpoints cannot erase or rebind an issued definition/session,
change its execution command or overwrite a retained ACK. Lifecycle transitions
cannot return from uncertainty to active. Retiring the issuer attempts one
uncertainty checkpoint. Opening a database with an interrupted running job uses
the existing restart handling and never reconstructs the token or resends work.
These are tracking/authority guarantees, not proof a node or physical device
stopped. There is no workload completion record or strong stop ACK yet.

The existing request compiler now calls the review checkpoint after a fresh
authenticated [source inspection](WORKLOAD-SOURCES.md) and origin/disclosure
checks, under the engine shutdown gate. Approval and issuance remain internal
and have no product/network caller. Those persistence helpers do not by themselves
authenticate an owner or peer, check listening/microphone permission or execute
a source. Execution integration
must hold existing engine admission/quiescence gates, obtain identities and ACKs
through pinned authenticated sessions, and verify the node's actual support.
The normal runtime compiler can retain an exact continuous source plan, but
ordinary operation approval still refuses it. A schema-valid template or
synthetic receipt is not permission to capture. Model-based method
authoring does not advertise this new template. No default listening, model,
permission, updater or installer behavior changes.

Focused tests use real temporary job databases with synthetic peer identities
and ACKs. They cover commit-before-offer, ACK-before-renewal, exact retained
retries, immutable issuance, legacy-path refusal, quota failure without issuance,
interruption and a copied committed crash snapshot that reopens as uncertain.
The clock regression first demonstrated that a corrected clock could revive a
rejected issuer; it now exercises irreversible retirement before and after
consumption through both lease and ACK entry points, with no OS clock changes.
The snapshot test is not a supported replication or cluster mechanism. Source
runtime/permissions, authenticated transport, bounded event delivery, real media
and physical multi-node acceptance remain required.
