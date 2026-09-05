"""Optional Windows screen adapter. Enable it explicitly in the EEF config."""

import base64
import io

CAPABILITY = "screen.capture"
ACTIONS = ["capture"]
PARAMS_SCHEMA = {
    "format": {"type": "string", "required": False, "enum": ["png", "jpeg"]},
    "quality": {"type": "integer", "required": False, "min": 1, "max": 100},
}


def handle(action, params):
    if action != "capture":
        raise ValueError(f"unknown screen action: {action}")
    from PIL import ImageGrab

    image = ImageGrab.grab(all_screens=True)
    image_format = str(params.get("format", "png")).lower()
    output = io.BytesIO()
    if image_format == "jpeg":
        image.convert("RGB").save(output, format="JPEG", quality=int(params.get("quality", 85)))
        mime = "image/jpeg"
    else:
        image.save(output, format="PNG", optimize=True)
        mime = "image/png"
    if output.tell() > 25 * 1024 * 1024:
        raise RuntimeError("encoded screen capture exceeds 25 MB")
    return {
        "data_base64": base64.b64encode(output.getvalue()).decode("ascii"),
        "mime_type": mime,
        "width": image.width,
        "height": image.height,
    }
