"""Optional Windows keyboard adapter. Enable it explicitly in the EEF config."""

CAPABILITY = "keyboard"
ACTIONS = ["type", "press", "hotkey", "key_down", "key_up"]
PARAMS_SCHEMA = {
    "text": {"type": "string", "required": False},
    "key": {"type": "string", "required": False},
    "keys": {"type": "list", "required": False},
    "interval": {"type": "number", "required": False, "min": 0, "max": 2},
}


def handle(action, params):
    import pyautogui

    pyautogui.FAILSAFE = True
    if action == "type":
        text = str(params.get("text", ""))
        pyautogui.write(text, interval=float(params.get("interval", 0.01)))
        return {"typed": len(text)}
    if action == "press":
        key = str(params.get("key", ""))
        if not key:
            raise ValueError("press requires 'key'")
        pyautogui.press(key)
        return {"pressed": key}
    if action == "hotkey":
        keys = [str(key) for key in params.get("keys", [])]
        if not keys:
            raise ValueError("hotkey requires non-empty 'keys'")
        pyautogui.hotkey(*keys)
        return {"hotkey": keys}
    if action in {"key_down", "key_up"}:
        key = str(params.get("key", ""))
        if not key:
            raise ValueError(f"{action} requires 'key'")
        (pyautogui.keyDown if action == "key_down" else pyautogui.keyUp)(key)
        return {action: key}
    raise ValueError(f"unknown keyboard action: {action}")
