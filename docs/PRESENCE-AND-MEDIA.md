# Planned presence, audio routing and supervised media sessions

Owner direction recorded 2026-09-22, after `v0.4.0a6`.
This is a design record, not implemented functionality or a release promise.
The 2026-09-22 request authorized documentation only. Backend development resumed
on 2026-09-26; the media/presence features below remain planned, not implemented.

This extends steps 6-9 of the [command-first roadmap](ROADMAP-CORE.md).
Reuse its origin/area/resource model, durable jobs, execution grants and command
services. Do not introduce a parallel media authority or dashboard-owned runtime.

## Coordinator authority, optional peer relay

The latest direction allows nodes to relay media directly, but only after EEF
approves the session. This supersedes the intermediate idea that every media
packet must pass through EEF. EEF remains responsible for admission, routing
decisions and supervision; peer connectivity is not independent permission.

Support a user-selected coordinator-relayed route as well as approved direct
peer routing. A fallback relay must be explicitly allowed and bounded, not
silently expose a listener or bypass limits. No default routing mode is chosen
by this document.

```text
node requests session -> EEF authorizes participants, resources and limits
                              |                         |
                              v                         v
                          source node <--- media ---> destination node
                              +---- status/events ----> EEF

optional media route: source node <-> EEF relay <-> destination node
```

EEF must be able to request pause, stop, rerouting, quality changes and revocation
while other jobs run. Nodes enforce local permission and session limits without
consulting EEF for every frame. Use the roadmap's separately authenticated,
short-lived scoped grants, not the shared network PSK as a new authority system.

Control requests need acknowledgement and observable effective state. During a
partition, instant intervention cannot be guaranteed: obey the session's approved
disconnect policy and bounded expiry. A restart must not silently revive an old
grant. Route changes must retire the old destination safely rather than broadcast
private audio to several rooms while handoff is uncertain.

## Persistent virtual audio devices and user preferences

Example: a call application runs on a living-room PC node, while the user speaks
and listens through a bedroom node. The call application keeps fixed virtual
input/output devices; EEF changes internal routes, not the application's selected
microphone every time the assistant speaks.

```text
bedroom microphone + assistant TTS -> mixer -> living-room virtual mic -> call
bedroom headphones/speakers        <- route <- living-room virtual output <- call
```

These are logical paths; either approved transport route above may carry them.
Call participants outside the EEF network use the existing call application;
they do not need EEF accounts or node access. No call-platform account automation
or integration is implemented or authorized by this design record.

Expose preferences through shared commands/configuration, with UI as a client:

- Microphone-only, TTS-only or mixed outgoing audio; independent levels/mutes.
- Mix speech, lower the microphone level while TTS speaks, or wait to speak.
- Select input and output nodes/resources independently, including a supported
  Bluetooth headset attached to a node. Changing an EEF route is distinct from
  changing Windows' system-wide default device or pairing an accessory.
- Route incoming call audio and local assistant feedback separately. Prevent
  accidental echo/feedback and prevent private assistant feedback from entering
  the call unless explicitly selected for transmission.
- Manual destination, suggested handoff, or explicitly enabled automatic handoff.

Ordinary voice bridging is deterministic and requires no language model. An
optional assistant participant may process explicitly approved call audio through
STT/model/TTS and speak into the same virtual input. Permission to relay audio
does not by itself permit interpretation, recording or autonomous participation.
Listening and speaking controls remain separate; make the assistant's identity
clear to participants and provide visible session state and immediate local mute.

Loss of a chosen source must not fall back to an unrelated room microphone.
Report unavailable/muted state. Virtual-device implementation, driver deployment,
signing/licensing and OS constraints remain unresolved engineering work. Existing
short-lived WAV/snapshot adapters are not a continuous streaming implementation.

## Reusable, lightweight presence

Presence is a resource/event source usable by multiple approved workflows:
welcoming, locating an occupied area, suggesting audio destinations and other
automations. It must not be a special case owned by call routing.

Prefer inexpensive available signals; camera-based sensing is optional and
permission-controlled. If a camera is used, perform bounded local processing
at a configurable rate, with more expensive analysis only when justified. A
heavy LLM/VLM must not be necessary just to keep presence or routing operational.
Share an approved observation pipeline when possible rather than opening the
same sensor and loading a separate model for every consumer.

Report small observations to EEF using existing node/area/resource identities,
with observation time, freshness/expiry, confidence and explicit unknown state.
Exact protocol fields are to be designed and tested, not declared here.
"Someone is present" does not mean "the owner is present"; do not silently add
face recognition or assume an identity. Camera access alone does not authorize
locating a person, greeting aloud, recording, or moving their call audio.

EEF decides any resulting action against the user's enabled automation and
routing preferences. Ambiguous/stale observations should retain a still-approved
route or ask for direction, not guess a room or expose private content. Presence
is evidence for a decision, never authentication or an execution grant.
Minimize retention and disclosure; raw video and recordings are not required
merely to report occupancy. Define retention explicitly for any later history.

## Concurrent sessions, bandwidth and lightweight operation

Reuse job/session identities and ownership so stop/cancel affects the intended
activity. Define exclusivity/sharing for microphones, cameras and outputs, and
make conflicts visible. Admission must account for aggregate CPU, memory and
network use, not merely whether each individual stream fits its own budget.

Bound stream rate, resolution, bitrate, buffers, concurrency and duration.
Keep control traffic responsive; priorities should allow a call to take precedence
over an optional background video stream. Add bounded buffering, backpressure,
echo handling and visible quality/drop status; no unbounded offline accumulation.
Budget coordinator relay capacity as well as both endpoints. No fixed performance
or hardware-footprint guarantee is made before measurement.

Presence detection and video sharing are separate permissions/workflows. Local
presence processing can avoid uploading ordinary frames; actual remote video
still consumes bandwidth. Video relay must be explicitly requested/configured,
not automatically enabled because a camera permission exists.

## Nearby ESP and accessory gateways

A PC node may eventually act as an approved local gateway to ESP devices or
supported Bluetooth accessories. Local processing can keep bulk sensor/media
traffic off a remote tunnel while EEF receives bounded events/results. A requested
remote feed still needs a network path; gateway routing does not eliminate its
traffic. Do not assume a direct route bypasses the tunnel without measuring it.

Do not require every accessory to run a full PC EEFN. Represent supported
resources behind their owning gateway with explicit adapter/protocol and local
permission boundaries. Pairing, OS authorization and hardware support still apply;
discovering a device is not authority to control it. Wi-Fi, Bluetooth, ESP firmware,
ESP-NOW bridges and multi-hop support remain later implementation work.

## Acceptance before claiming support

- Real two-PC, two-way call audio using fixed virtual input/output, microphone/TTS
  mixing, independent destination choice, mute, handoff and disconnect behavior.
- Direct and coordinator-relayed sessions with observed packet/byte paths;
  wrong, expired, replayed and revoked grants fail without permission widening.
- Concurrent calls/background streams/automations: resource conflicts, aggregate
  limits, control responsiveness, bounded buffers and no cross-session leakage.
- EEF disconnect/restart, stale commands and uncertain handoff: truthful state,
  bounded authorization lifetime, no unexpected source or destination fallback.
- Presence absent/ambiguous/stale/unknown-person cases, reuse across approved
  consumers, local-only processing where configured, and no implicit recording.
- Measure idle/active CPU, RAM, network use and audio latency on target hardware;
  distinguish fake-backend tests from real microphones, cameras and accessories.

Current alpha.6 does not provide virtual audio endpoints, live peer media sessions,
presence automation or these acceptance results. No release, model selection,
permission, installed node or update policy is changed by this document.
