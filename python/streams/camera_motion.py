"""Single-open motion observations for a future explicitly admitted camera job.

Frame difference is not person/presence recognition. No startup registration or
one-shot handler exists. The caller must own permission, source reservation,
process limits, deadline and cancellation before advancing this iterator.
"""
import time
from urllib.parse import urlsplit


def _integer(params, name, default, lower, upper):
    value = params.get(name, default)
    if type(value) is not int or not lower <= value <= upper:
        raise ValueError("camera motion parameter")
    return value


def _device(value):
    if type(value) is int and 0 <= value <= 1023:
        return value
    # An arbitrary string could make VideoCapture read a local file or select a
    # different protocol. Only an explicitly bound RTSP endpoint is supported.
    if not isinstance(value, str) or not 1 <= len(value) <= 1024 or any(
            c.isspace() or ord(c) < 32 or ord(c) == 127 for c in value):
        raise ValueError("camera motion device")
    try:
        address = urlsplit(value)
        valid = address.scheme in ("rtsp", "rtsps") and address.hostname and not address.fragment
        valid = valid and (address.port is None or 1 <= address.port <= 65535)
    except ValueError:
        valid = False
    if not valid:
        raise ValueError("camera motion device")
    return value


def _frame(frame, width, height, channels):
    shape = getattr(frame, "shape", ())
    if (len(shape) != (3 if channels == 3 else 2)
            or any(type(n) is not int or n <= 0 for n in shape)
            or shape[0] > height or shape[1] > width
            or (channels == 3 and shape[2] != 3)
            or getattr(getattr(frame, "dtype", None), "name", None) != "uint8"
            or getattr(frame, "nbytes", None) != shape[0] * shape[1] * channels):
        raise ValueError("camera motion frame")
    return shape[0], shape[1]


def stream(action, params):
    if action != "motion" or not isinstance(params, dict) or set(params) - {
            "device", "width", "height", "sample_width", "sample_height",
            "max_frames", "interval_ms", "activation_level", "deactivation_level",
            "io_timeout_ms", "max_duration_ms"}:
        raise ValueError("camera motion request")
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
    if io_timeout / 1000 > duration:
        raise ValueError("camera motion parameter")

    deadline = time.monotonic() + duration

    def check_time():
        if time.monotonic() >= deadline:
            raise TimeoutError("camera motion deadline")

    # Import/open lazily. No fallback backend, file, reconnect, scan or retry.
    import cv2
    check_time()
    # This iterator runs in its own admitted source process, never a shared
    # inference host. Limit OpenCV transforms; decoder/driver limits are separate.
    cv2.setNumThreads(1)
    capture = None
    try:
        check_time()
        if isinstance(device, str):
            capture = cv2.VideoCapture(device, cv2.CAP_FFMPEG, [
                cv2.CAP_PROP_OPEN_TIMEOUT_MSEC, io_timeout,
                cv2.CAP_PROP_READ_TIMEOUT_MSEC, io_timeout,
                cv2.CAP_PROP_AUDIO_STREAM, -1])
        else:
            capture = cv2.VideoCapture(device)
        check_time()
        if not capture.isOpened():
            raise RuntimeError("camera motion unavailable")
        # Hardware can ignore these requests; validate every actual frame too.
        capture.set(cv2.CAP_PROP_FRAME_WIDTH, width)
        capture.set(cv2.CAP_PROP_FRAME_HEIGHT, height)
        previous, dimensions, active = None, None, None
        next_read = time.monotonic()
        for index in range(max_frames):
            delay = next_read - time.monotonic()
            if delay > 0:
                time.sleep(min(delay, max(0, deadline - time.monotonic())))
            check_time()
            next_read = time.monotonic() + interval
            ok, frame = capture.read()
            check_time()
            if type(ok) is not bool or not ok or frame is None:
                # Missing input is a failed source, not a quiet scene or an
                # invented frame. Never reopen the device to hide a read loss.
                raise RuntimeError("camera motion read")
            current_dimensions = _frame(frame, width, height, 3)
            small = cv2.resize(frame, (sample_width, sample_height), interpolation=cv2.INTER_AREA)
            if _frame(small, sample_width, sample_height, 3) != (sample_height, sample_width):
                raise ValueError("camera motion sample")
            gray = cv2.cvtColor(small, cv2.COLOR_BGR2GRAY)
            if _frame(gray, sample_width, sample_height, 1) != (sample_height, sample_width):
                raise ValueError("camera motion sample")
            pixels = gray.tobytes()
            if type(pixels) is not bytes or len(pixels) != sample_width * sample_height:
                raise ValueError("camera motion sample")
            gap = dimensions is not None and dimensions != current_dimensions
            if previous is None or gap:
                level, active = None, None
            else:
                level = sum(abs(a - b) for a, b in zip(pixels, previous)) // len(pixels)
                if level >= activate:
                    active = True
                elif level <= deactivate:
                    active = False
            previous, dimensions = pixels, current_dimensions
            # At most two tiny grayscale samples exist during comparison. No
            # image, endpoint, codec output or raw frame crosses the iterator.
            del frame, small, gray, pixels
            check_time()
            yield {"source_frames": index + 1, "active": active,
                   "level": level, "discontinuity": gap}
    finally:
        if capture is not None:
            capture.release()
