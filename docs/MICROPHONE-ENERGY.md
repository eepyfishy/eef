# Microphone energy source

`python/streams/microphone_energy.py` implements a lazy `stream("energy", params)`
iterator for future admitted microphone workloads. It opens one blocking
`sounddevice.RawInputStream` for the iterator's lifetime. It reads one configured
block at a time, computes integer RMS level and yields only bounded metadata.
It does not make a series of recordings or keep a Python callback queue.

This is an energy threshold detector, not voice activity, speech recognition,
person detection or presence identification. Configurable activation and
deactivation levels provide hysteresis. A first value between the thresholds is
unknown. An input overflow clears the previous decision and reports a
discontinuity, unknown activity and unknown level; it never reports fabricated
silence. The source-frame count counts frames actually returned by reads, not
unobserved frames lost during overflow.

Parameters select sample rate, one or two channels, block frames, maximum blocks,
thresholds and an owner-selected device. Each block has exactly the configured
size and at most 16 KiB of PCM. Only source-frame count, optional activity,
optional level and discontinuity leave the iterator. PCM views are discarded
before yielding; there is no recording, transcript or offline media cache.
Outer streaming limits and deadlines still apply and may stop earlier.

Normal iterator completion, explicit iterator closure and read/validation failure
exit the input context. A blocked native device read relies on the outer owned
process deadline/cancellation; fake-device tests do not prove physical driver
release, capture quality, working-set usage or device-buffer quotas.

The module has no one-shot handler, startup registration or advertised capability.
It is not yet installed or callable by a user command. Existing microphone
recording behavior is unchanged. Integration must require an explicit resource,
microphone permission, local listening consent and an admitted durable workload.
Local listening-off and permission revocation must retire the source without
stopping typed commands or node connectivity. Coordinator event transport,
deduplication, resource accounting and real-media/second-PC acceptance remain open.

`tools/test-microphone-energy.mjs` uses an explicitly supplied existing interpreter
and eleven fake-device tests. They verify lazy single-open capture, closure, failure,
exact block sizes, limits, hysteresis, overflow uncertainty, stereo/full-scale
levels and device selection. They never access microphone hardware or require
the sounddevice package.

`tools/test-microphone-stream.mjs` separately checks the actual source through the
production Rust/Python stream transport with a fake audio device. Normal energy,
overflow uncertainty and a malformed block retain exact requests and process
outcomes. The three original file-based cases observe one input context opening
and closing. This is synthetic source/transport integration, not physical
microphone acceptance.

The pinned-source development cases embed the reviewed energy bytes in an
immutable trusted source, then replace or delete the nearby plugin file before
execution. The runtime executes the owned bytes without reopening that path.
Independent Rust/Python digest checks and the encoded request budget apply before
source effects. This closes the source-file substitution gap for that API; it
does not add durable session admission, lease renewal or physical-device proof.
Hosted validation for the new cases is pending.
