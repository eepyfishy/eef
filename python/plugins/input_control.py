"""Opt-in keyboard and mouse control with PyAutoGUI's corner failsafe."""

CAPABILITY = "input.control"
ACTIONS = ["type", "press", "hotkey", "key_down", "key_up", "move", "click", "scroll"]
PARAMS_SCHEMA = {}


def handle(action, params):
    import pyautogui

    pyautogui.FAILSAFE = True
    if action == "type":
        text = str(params.get("text", ""))
        if len(text) > 10000:
            raise ValueError("text exceeds 10000 characters")
        interval = max(0.0, min(float(params.get("interval", 0.01)), 2.0))
        pyautogui.write(text, interval=interval)
        return {"typed": len(text)}
    if action == "press":
        key = str(params.get("key", ""))
        if not key:
            raise ValueError("press requires 'key'")
        pyautogui.press(key)
        return {"pressed": key}
    if action == "hotkey":
        keys = [str(key) for key in params.get("keys", [])]
        if not keys or len(keys) > 8:
            raise ValueError("hotkey requires between 1 and 8 keys")
        pyautogui.hotkey(*keys)
        return {"hotkey": keys}
    if action in {"key_down", "key_up"}:
        key = str(params.get("key", ""))
        if not key:
            raise ValueError(f"{action} requires 'key'")
        (pyautogui.keyDown if action == "key_down" else pyautogui.keyUp)(key)
        return {action: key}
    if action == "move":
        x, y = int(params.get("x", 0)), int(params.get("y", 0))
        duration = max(0.0, min(float(params.get("duration", 0.2)), 5.0))
        pyautogui.moveTo(x, y, duration=duration)
        return {"x": x, "y": y}
    if action == "click":
        button = str(params.get("button", "left"))
        if button not in {"left", "middle", "right"}:
            raise ValueError("button must be left, middle, or right")
        clicks = max(1, min(int(params.get("clicks", 1)), 10))
        pyautogui.click(params.get("x"), params.get("y"), clicks=clicks, button=button)
        return {"button": button, "clicks": clicks}
    if action == "scroll":
        amount = max(-100, min(int(params.get("amount", 0)), 100))
        pyautogui.scroll(amount)
        return {"amount": amount}
    raise ValueError(f"unknown input action: {action}")
