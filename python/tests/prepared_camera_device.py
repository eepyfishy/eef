"""Explicit Rust native-test prelude; simulated camera API with real owned processes."""
import json as _eef_camera_json
import os as _eef_camera_os
import pathlib as _eef_camera_pathlib
import subprocess as _eef_camera_subprocess
import sys as _eef_camera_sys
import time as _eef_camera_time
import types as _eef_camera_types

assert _eef_camera_mode in ("finite", "blocked")
assert _eef_camera_sys.version_info[:3] == (3, 11, 9)
_eef_camera_root = _eef_camera_pathlib.Path(_eef_camera_root)
_eef_camera_fake = _eef_camera_types.ModuleType("camera_motion_fakes")
exec(compile(_eef_camera_fake_source, "<eef-camera-fakes>", "exec"), _eef_camera_fake.__dict__)


def _eef_camera_log(kind):
    with (_eef_camera_root / "trace.jsonl").open("a", encoding="utf8") as output:
        output.write(_eef_camera_json.dumps({"kind": kind}) + "\n")


def _eef_camera_wait(name):
    end = _eef_camera_time.monotonic() + 8
    while not (_eef_camera_root / name).is_file():
        if _eef_camera_time.monotonic() >= end:
            raise RuntimeError("Synthetic camera acknowledgement missing")
        _eef_camera_time.sleep(0.01)


class _eef_camera_Device(_eef_camera_fake.Camera):
    def factory(self, *args):
        assert args[0] == "rtsp://owner:PRIVATE_CAMERA_PASSWORD@camera.invalid:554/live"
        self.child = _eef_camera_subprocess.Popen(
            [_eef_camera_sys.executable, "-I", "-B", "-c", "import time; time.sleep(30)"],
            stdin=_eef_camera_subprocess.DEVNULL,
            stdout=_eef_camera_subprocess.DEVNULL,
            stderr=_eef_camera_subprocess.DEVNULL)
        (_eef_camera_root / "pids.json").write_text(_eef_camera_json.dumps(
            {"root": _eef_camera_os.getpid(), "child": self.child.pid}), encoding="utf8")
        _eef_camera_log("opened")
        return super().factory(*args)

    def read(self):
        if self.reads == 0:
            _eef_camera_wait("observed")
            if _eef_camera_mode == "blocked":
                _eef_camera_log("blocked")
                _eef_camera_time.sleep(30)
                raise RuntimeError("Blocked camera fixture was not cancelled")
        else:
            _eef_camera_wait("ack-" + str(self.reads))
        return super().read()

    def release(self):
        super().release()
        _eef_camera_log("closed")


_eef_camera_device = _eef_camera_Device([
    _eef_camera_fake.Frame(0), _eef_camera_fake.Frame(20), _eef_camera_fake.Frame(20),
    _eef_camera_fake.Frame(80, width=320, height=240), _eef_camera_fake.Frame(80, width=320, height=240)])
_eef_camera_sys.modules["cv2"] = _eef_camera_fake.cv2(_eef_camera_device)
