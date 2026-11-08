# Origin admission for the built-in microphone source

Explicit continuous microphone requests now use their own node-owned source
snapshot during admission. They no longer require the separate one-shot
`audio.capture` plugin to be installed or advertised. The built-in adapter and
the one-shot plugin have different execution paths.

Before saving a new review, eef checks that the source node is fresh and disclosed
to the origin, reads its resource and adapter digests over the existing independently
authenticated TLS session, and checks the exact requested template against that
snapshot. Disclosure is checked again after inspection. Missing, unavailable,
changed or privately rebound resources cannot produce a new review. The existing
compiler independently checks the source snapshot before freezing the plan.

Reviewing a configured source does not require permission to execute it. A node
can prepare a request with listening, microphone permission and workload policy
still off, or without Python configured. Owner approval and explicit execution
remain separate. The execution driver requires all current runtime, permission,
listening, policy and compatible completion conditions before issuing work.
Admission never starts capture or modifies those settings.

Exact retained concrete requests remain usable before any current provider lookup.
They keep their immutable origin, source choices, compiled plan and receipt after
disconnect. Reusing a receipt neither rebinds a source nor executes the request.
Changing an operation's concrete input remains a conflict.

Two shared credential/TLS tests exercise the running node's actual submission
mailbox without models, Python or media plugins: successful preparation and offline
receipt inspection; and refusal of wrong adapter/resource digests, absent resources,
undisclosed nodes and saved/applied binding changes before job creation. The opt-in
Windows combined source test now also submits and compiles through that mailbox,
then uses separate owner HTTP approval before origin execution. It no longer stages
its review directly through an internal test helper.

Changed-source hosted verification is pending. This repairs the existing explicit
template path; JSON-free continuous-source authoring, complete non-coder onboarding,
physical two-PC/long-source acceptance and the remaining stable release gates remain
required. No installed node, permission, model, version, update feed or OS policy is
changed by this development increment.
