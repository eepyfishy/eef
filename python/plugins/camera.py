"""Opt-in webcam capture returning an in-memory JPEG."""

import base64

CAPABILITY = "camera.capture"
ACTIONS = ["capture", "list"]
PARAMS_SCHEMA = {}


def handle(action, params):
    import cv2

    if action == "list":
        available = []
        for index in range(max(1, min(int(params.get("scan", 8)), 16))):
            camera = cv2.VideoCapture(index)
            try:
                if camera.isOpened():
                    available.append(index)
            finally:
                camera.release()
        return {"devices": available}
    if action != "capture":
        raise ValueError(f"unknown camera action: {action}")
    device = max(0, min(int(params.get("device", 0)), 16))
    camera = cv2.VideoCapture(device)
    try:
        if not camera.isOpened():
            raise RuntimeError(f"camera {device} is unavailable")
        width = max(160, min(int(params.get("width", 1280)), 3840))
        height = max(120, min(int(params.get("height", 720)), 2160))
        camera.set(cv2.CAP_PROP_FRAME_WIDTH, width)
        camera.set(cv2.CAP_PROP_FRAME_HEIGHT, height)
        frame = None
        for _ in range(3):
            ok, frame = camera.read()
            if not ok:
                frame = None
        if frame is None:
            raise RuntimeError("camera did not return a frame")
        quality = max(30, min(int(params.get("quality", 85)), 100))
        ok, encoded = cv2.imencode(".jpg", frame, [cv2.IMWRITE_JPEG_QUALITY, quality])
        if not ok:
            raise RuntimeError("could not encode camera frame")
        if encoded.size > 25 * 1024 * 1024:
            raise RuntimeError("encoded camera frame exceeds 25 MB")
        actual_height, actual_width = frame.shape[:2]
        return {
            "mime_type": "image/jpeg",
            "width": actual_width,
            "height": actual_height,
            "data_base64": base64.b64encode(encoded.tobytes()).decode("ascii"),
        }
    finally:
        camera.release()
