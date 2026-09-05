"""Opt-in microphone capture returning an in-memory WAV."""

import base64
import io

CAPABILITY = "audio.capture"
ACTIONS = ["record", "devices"]
PARAMS_SCHEMA = {}


def handle(action, params):
    import sounddevice as sd

    if action == "devices":
        return {"devices": [dict(device) for device in sd.query_devices()]}
    if action != "record":
        raise ValueError(f"unknown microphone action: {action}")
    import soundfile as sf

    duration = max(0.1, min(float(params.get("duration_seconds", 5)), 20.0))
    sample_rate = max(8000, min(int(params.get("sample_rate", 16000)), 48000))
    channels = max(1, min(int(params.get("channels", 1)), 2))
    frames = int(duration * sample_rate)
    recording = sd.rec(
        frames,
        samplerate=sample_rate,
        channels=channels,
        dtype="int16",
        device=params.get("device"),
    )
    sd.wait()
    output = io.BytesIO()
    sf.write(output, recording, sample_rate, format="WAV", subtype="PCM_16")
    return {
        "mime_type": "audio/wav",
        "sample_rate": sample_rate,
        "channels": channels,
        "duration_seconds": duration,
        "data_base64": base64.b64encode(output.getvalue()).decode("ascii"),
    }
