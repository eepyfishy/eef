"""Private vision iterator, appended to the pinned camera_motion source bytes.

This is not the motion protocol, a model runner, or workload authorization.
The caller must own the reviewed vision scope, camera reservation and process
limits. Its private image records are for the vision host's local consumer,
not the legacy metadata stream.
"""
import base64 as _vision_base64
import os as _vision_os


def stream(action, params):
    if action != "vision" or not isinstance(params, dict) or set(params) - {
            "device", "width", "height", "sample_width", "sample_height",
            "max_frames", "interval_ms", "activation_level", "deactivation_level",
            "io_timeout_ms", "max_duration_ms", "analysis"}:
        raise ValueError("camera vision request")
    device = _device(params.get("device", 0))
    width = _integer(params, "width", 640, 160, 1920)
    height = _integer(params, "height", 480, 120, 1080)
    sample_width = _integer(params, "sample_width", 64, 8, 160)
    sample_height = _integer(params, "sample_height", 48, 8, 120)
    max_frames = _integer(params, "max_frames", 100, 1, 100000)
    interval = _integer(params, "interval_ms", 200, 100, 10000) / 1000
    activate = _integer(params, "activation_level", 15, 1, 255)
    deactivate = _integer(params, "deactivation_level", 5, 0, activate)
    duration = _integer(params, "max_duration_ms", 60000, 100, 3600000) / 1000
    io_timeout = _integer(params, "io_timeout_ms", 2000, 100, 10000)
    analysis = params.get("analysis")
    if (not isinstance(analysis, dict) or set(analysis) != {
            "width", "height", "max_image_bytes", "jpeg_quality", "max_images",
            "min_interval_ms", "trigger"} or io_timeout / 1000 > duration):
        raise ValueError("camera vision scope")
    image_width = _integer(analysis, "width", 320, 16, 640)
    image_height = _integer(analysis, "height", 240, 16, 480)
    image_bytes = _integer(analysis, "max_image_bytes", 65536, 128, 2 * 1024 * 1024)
    quality = _integer(analysis, "jpeg_quality", 70, 1, 95)
    max_images = _integer(analysis, "max_images", 4, 1, 100)
    image_interval = _integer(analysis, "min_interval_ms", 1000, 0, 3600000) / 1000
    trigger = analysis["trigger"]
    if (trigger not in ("rising", "active_interval")
            or image_width > width or image_height > height or max_images > max_frames):
        raise ValueError("camera vision scope")
    deadline = time.monotonic() + duration

    def check_time():
        if time.monotonic() >= deadline:
            raise TimeoutError("camera vision deadline")

    # This source has its own admitted process. Bound native workers before
    # import; no model module, endpoint or prompt is passed into this process.
    for variable in ("OPENBLAS_NUM_THREADS", "OMP_NUM_THREADS", "OPENCV_FOR_THREADS_NUM"):
        _vision_os.environ[variable] = "1"
    import cv2
    check_time()
    cv2.setNumThreads(1)
    capture = None
    try:
        check_time()
        if isinstance(device, str):
            capture = cv2.VideoCapture(device, cv2.CAP_FFMPEG, [
                cv2.CAP_PROP_OPEN_TIMEOUT_MSEC, io_timeout,
                cv2.CAP_PROP_READ_TIMEOUT_MSEC, io_timeout])
        else:
            capture = cv2.VideoCapture(device)
        check_time()
        if not capture.isOpened():
            raise RuntimeError("camera vision unavailable")
        capture.set(cv2.CAP_PROP_FRAME_WIDTH, width)
        capture.set(cv2.CAP_PROP_FRAME_HEIGHT, height)
        previous, dimensions, active = None, None, None
        images, last_image_at = 0, None
        next_read = time.monotonic()
        for index in range(max_frames):
            delay = next_read - time.monotonic()
            if delay > 0:
                time.sleep(min(delay, max(0, deadline - time.monotonic())))
            check_time()
            ok, frame = capture.read()
            check_time()
            if type(ok) is not bool or not ok or frame is None:
                raise RuntimeError("camera vision read")
            current_dimensions = _frame(frame, width, height, 3)
            small = cv2.resize(frame, (sample_width, sample_height), interpolation=cv2.INTER_AREA)
            if _frame(small, sample_width, sample_height, 3) != (sample_height, sample_width):
                raise ValueError("camera vision sample")
            gray = cv2.cvtColor(small, cv2.COLOR_BGR2GRAY)
            if _frame(gray, sample_width, sample_height, 1) != (sample_height, sample_width):
                raise ValueError("camera vision sample")
            pixels = gray.tobytes()
            if type(pixels) is not bytes or len(pixels) != sample_width * sample_height:
                raise ValueError("camera vision sample")
            gap = dimensions is not None and dimensions != current_dimensions
            was_active = active
            if previous is None or gap:
                level, active = None, None
            else:
                level = sum(abs(a - b) for a, b in zip(pixels, previous)) // len(pixels)
                if level >= activate:
                    active = True
                elif level <= deactivate:
                    active = False
            previous, dimensions = pixels, current_dimensions
            state, image = "not_requested", None
            requested = active is True and (trigger == "active_interval" or was_active is not True)
            if requested:
                if images >= max_images:
                    state = "budget_exhausted"
                elif last_image_at is not None and time.monotonic() - last_image_at < image_interval:
                    state = "rate_limited"
                else:
                    frame_height, frame_width = current_dimensions
                    ratio = min(1, image_width / frame_width, image_height / frame_height)
                    size = (max(1, int(frame_width * ratio)), max(1, int(frame_height * ratio)))
                    image_frame = cv2.resize(frame, size, interpolation=cv2.INTER_AREA)
                    _frame(image_frame, image_width, image_height, 3)
                    encoded_ok, encoded = cv2.imencode(
                        ".jpg", image_frame, [cv2.IMWRITE_JPEG_QUALITY, quality])
                    if (type(encoded_ok) is not bool or not encoded_ok
                            or type(getattr(encoded, "nbytes", None)) is not int
                            or not 4 <= encoded.nbytes <= image_bytes):
                        raise ValueError("camera vision image budget")
                    raw = encoded.tobytes()
                    if (type(raw) is not bytes or len(raw) != encoded.nbytes
                            or not raw.startswith(b"\xff\xd8") or not raw.endswith(b"\xff\xd9")):
                        raise ValueError("camera vision encoding")
                    image = _vision_base64.b64encode(raw).decode("ascii")
                    state, images, last_image_at = "frame", images + 1, time.monotonic()
                    del image_frame, encoded, raw
            del frame, small, gray, pixels
            check_time()
            yield {"source_frames": index + 1, "active": active, "level": level,
                   "discontinuity": gap, "analysis_state": state, "image_base64": image}
            # The private host resumes only after the local consumer acknowledges.
            # Start a fresh sampling interval then, never a catch-up burst.
            image = None
            next_read = time.monotonic() + interval
    finally:
        if capture is not None:
            capture.release()
