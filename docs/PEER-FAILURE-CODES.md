# Finite peer failure evidence

Consumed finite peer operations now retain fixed failure categories in the
existing grant report's `error_code`. Reports keep the same schema, exact grant
identity and unknown state. They retain no audio, private callback messages,
socket addresses, credentials or interpreter output as error text. The code is
selected from trusted local error context, never from a node-supplied parameter.

| Code | Operation that could not complete |
| --- | --- |
| `peer_listener_unavailable` | Opening the already approved destination listener |
| `peer_connection_unavailable` | Accepting or connecting the approved socket |
| `peer_readiness_unavailable` | Delivering readiness through the existing control channel |
| `peer_channel_unavailable` | Establishing the scoped authenticated peer channel |
| `peer_audio_capture_failed` | Returning from the approved capture adapter |
| `peer_audio_capture_invalid` | Validating/canonicalizing its bounded WAV result |
| `peer_transfer_failed` | Transferring frames or acknowledgements |
| `peer_audio_manifest_invalid` | Reading a valid manifest within reviewed bounds |
| `peer_payload_invalid` | Validating exact finite payload layout, digest or WAV |
| `peer_audio_playback_failed` | Returning exact completion from the approved playback adapter |

Existing Python bridge codes take precedence through nested peer context. Stop,
deadline, policy-loss and failed-checkpoint outcomes retain their existing
categories and priority. Permission checks, readiness order, sockets, original
execution deadlines, payload limits and cleanup ownership are unchanged.

These categories describe where an operation failed. They do not prove rollback,
that capture/playback never happened, or that physical effects stopped. No category
authorizes retry or reconnect. A consumed grant remains fenced, even if listener
setup or readiness failed before a source ran. Historical receipt/result commands
can inspect the retained outcome without issuing another operation.

Three new regressions require retained unknown outcomes for a real occupied
listener, failed readiness including nested Python errors, and invalid peer
handshake bytes. They verify capacity/socket release, no private error leakage and
no readmission. Existing substituted-payload tests also assert the fixed payload
category. They do not prove physical media behavior. Fresh hosted acceptance is
required, and the existing finite audio interruption is not claimed fixed.
