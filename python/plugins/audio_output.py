"""Opt-in WAV playback using the Windows multimedia service."""

import base64

CAPABILITY = "audio.play"
ACTIONS = ["play", "stop"]
PARAMS_SCHEMA = {}


def handle(action, params):
    import winsound

    if action == "stop":
        winsound.PlaySound(None, 0)
        return {"stopped": True}
    if action != "play":
        raise ValueError(f"unknown audio output action: {action}")
    encoded = str(params.get("data_base64", ""))
    if not encoded:
        raise ValueError("play requires WAV data_base64")
    audio = base64.b64decode(encoded, validate=True)
    if not audio.startswith(b"RIFF") or len(audio) > 25 * 1024 * 1024:
        raise ValueError("audio must be a WAV no larger than 25 MB")
    winsound.PlaySound(audio, winsound.SND_MEMORY | winsound.SND_SYNC)
    return {"played": True, "bytes": len(audio)}
