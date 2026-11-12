"""Native test prelude: real encoded frames/decoder; substitute only acquisition.

The reviewed producer still receives its bound RTSP resource. This test-only
wrapper redirects that one acquisition to a generated AVI. Nothing here is a
shipping source, a physical-camera test or an RTSP transport test.
"""
import json as _eef_decode_json
import os as _eef_decode_os
import pathlib as _eef_decode_pathlib
import time as _eef_decode_time

assert _eef_decode_frames in (3, 5)
assert _eef_decode_size in ((160, 120), (640, 480))
_eef_decode_root = _eef_decode_pathlib.Path(_eef_decode_root)
_eef_decode_video = _eef_decode_root / "generated.avi"
_eef_decode_fps = 5


def _eef_decode_log(kind):
    with (_eef_decode_root / "trace.jsonl").open("a", encoding="utf8") as output:
        output.write(_eef_decode_json.dumps({"kind": kind}) + "\n")


_eef_decode_log("starting")
for _eef_decode_variable in ("OPENBLAS_NUM_THREADS", "OMP_NUM_THREADS", "OPENCV_FOR_THREADS_NUM"):
    _eef_decode_os.environ[_eef_decode_variable] = "1"
import cv2 as _eef_decode_cv
import numpy as _eef_decode_np

_eef_decode_cv.setNumThreads(1)
_eef_decode_log("imported")
_eef_decode_writer = _eef_decode_cv.VideoWriter(
    str(_eef_decode_video), _eef_decode_cv.VideoWriter_fourcc(*"MJPG"), _eef_decode_fps, _eef_decode_size)
assert _eef_decode_writer.isOpened(), "Synthetic encoded-video writer unavailable"
try:
    for _eef_decode_value in (0, 20, 20, 255, 255)[:_eef_decode_frames]:
        _eef_decode_writer.write(_eef_decode_np.full((_eef_decode_size[1], _eef_decode_size[0], 3), _eef_decode_value,
                                                   dtype=_eef_decode_np.uint8))
finally:
    _eef_decode_writer.release()
_eef_decode_log("encoded")
_eef_decode_capture = _eef_decode_cv.VideoCapture

# Reproduce the original failure on the same pinned backend and generated file.
# This is a test control, not a source fallback or a second camera acquisition.
_eef_decode_rejected = _eef_decode_capture(str(_eef_decode_video), _eef_decode_cv.CAP_FFMPEG, [
    _eef_decode_cv.CAP_PROP_OPEN_TIMEOUT_MSEC, 2000,
    _eef_decode_cv.CAP_PROP_READ_TIMEOUT_MSEC, 2000,
    _eef_decode_cv.CAP_PROP_AUDIO_STREAM, -1])
try:
    assert not _eef_decode_rejected.isOpened(), "Review changed FFmpeg audio-option support"
finally:
    _eef_decode_rejected.release()
_eef_decode_log("unsupported_audio_rejected")


class _eef_decode_Device:
    def __init__(self, device, backend, parameters):
        assert device == "rtsp://owner:PRIVATE_CAMERA_PASSWORD@camera.invalid:554/live"
        assert backend == _eef_decode_cv.CAP_FFMPEG
        assert parameters == [_eef_decode_cv.CAP_PROP_OPEN_TIMEOUT_MSEC, 2000,
                              _eef_decode_cv.CAP_PROP_READ_TIMEOUT_MSEC, 2000]
        _eef_decode_log("parameters_validated")
        self.capture = _eef_decode_capture(str(_eef_decode_video), backend, parameters)
        assert self.capture.isOpened(), "Synthetic encoded-video decoder unavailable"
        self.reads = 0
        _eef_decode_log("opened")

    def isOpened(self):
        return self.capture.isOpened()

    def set(self, name, value):
        return self.capture.set(name, value)

    def read(self):
        if self.reads:
            end = _eef_decode_time.monotonic() + 8
            while not (_eef_decode_root / ("ack-" + str(self.reads))).is_file():
                if _eef_decode_time.monotonic() >= end:
                    raise RuntimeError("Encoded camera acknowledgement missing")
                _eef_decode_time.sleep(0.01)
            _eef_decode_log("acknowledged")
            # File decoding has no live-camera cadence. Pace AFTER the durable
            # callback, so a slow journal cannot create a buffered catch-up burst.
            _eef_decode_time.sleep(1 / _eef_decode_fps)
        ok, frame = self.capture.read()
        if ok:
            self.reads += 1
        _eef_decode_log("read" if ok else "eof")
        return ok, frame

    def release(self):
        self.capture.release()
        _eef_decode_log("closed")


_eef_decode_cv.VideoCapture = _eef_decode_Device
