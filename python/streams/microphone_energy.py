"""Single-open microphone energy observations for an explicitly admitted stream.

This module has no one-shot handler or startup registration. Energy is not speech
or person recognition. Authorization and listening controls belong to the caller.
"""
import math


def _integer(params, name, default, lower, upper):
    value = params.get(name, default)
    if type(value) is not int or not lower <= value <= upper:
        raise ValueError("microphone energy parameter")
    return value


def stream(action, params):
    if action != "energy" or not isinstance(params, dict) or set(params) - {
            "sample_rate", "channels", "block_frames", "max_blocks",
            "activation_level", "deactivation_level", "device"}:
        raise ValueError("microphone energy request")
    rate = _integer(params, "sample_rate", 16000, 8000, 48000)
    channels = _integer(params, "channels", 1, 1, 2)
    block_frames = _integer(params, "block_frames", 320, 80, 4096)
    max_blocks = _integer(params, "max_blocks", 1500, 1, 100000)
    activate = _integer(params, "activation_level", 1000, 1, 32768)
    deactivate = _integer(params, "deactivation_level", 500, 0, activate)
    device = params.get("device")
    if device is not None and not (
            type(device) is int and 0 <= device <= 1023
            or isinstance(device, str) and 0 < len(device) <= 256
            and not any(ord(c) < 32 for c in device)):
        raise ValueError("microphone energy device")

    # Import and open only when an admitted caller actually advances the iterator.
    # Blocking reads have one fixed block, not a Python callback queue or a series
    # of recordings. The outer owned process enforces cancellation/deadline.
    import sounddevice as sd

    active = None
    with sd.RawInputStream(samplerate=rate, channels=channels, dtype="int16",
                           blocksize=block_frames, device=device) as capture:
        for index in range(max_blocks):
            data, gap = capture.read(block_frames)
            raw = memoryview(data).cast("B")
            if raw.nbytes != block_frames * channels * 2 or type(gap) is not bool:
                raise ValueError("microphone energy block")
            if gap:
                active, level = None, None
            else:
                samples = raw.cast("h")
                level = math.isqrt(sum(int(v) * int(v) for v in samples) // len(samples))
                del samples
                if level >= activate:
                    active = True
                elif level <= deactivate:
                    active = False
            # No PCM survives in the yielded metadata or an offline/cache queue.
            del raw, data
            yield {"source_frames": (index + 1) * block_frames,
                   "active": active, "level": level, "discontinuity": gap}
