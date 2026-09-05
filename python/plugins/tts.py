"""Opt-in text-to-speech using voices installed on the node."""

CAPABILITY = "tts.speak"
ACTIONS = ["speak", "voices"]
PARAMS_SCHEMA = {}


def handle(action, params):
    import pyttsx3

    engine = pyttsx3.init()
    if action == "voices":
        return {
            "voices": [
                {"id": voice.id, "name": voice.name, "languages": list(voice.languages or [])}
                for voice in engine.getProperty("voices")
            ]
        }
    if action != "speak":
        raise ValueError(f"unknown TTS action: {action}")
    text = str(params.get("text", ""))
    if not text or len(text) > 10000:
        raise ValueError("text must contain between 1 and 10000 characters")
    engine.setProperty("rate", max(50, min(int(params.get("rate", 180)), 400)))
    engine.setProperty("volume", max(0.0, min(float(params.get("volume", 1.0)), 1.0)))
    voice = params.get("voice")
    if voice:
        known = {item.id for item in engine.getProperty("voices")}
        if voice not in known:
            raise ValueError("requested voice is not installed")
        engine.setProperty("voice", voice)
    engine.say(text)
    engine.runAndWait()
    return {"spoken": len(text)}
