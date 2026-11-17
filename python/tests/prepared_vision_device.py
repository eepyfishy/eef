"""Explicit native-test prelude only; synthetic camera/encoder, no hardware."""
import json as _v_json
import os as _v_os
import pathlib as _v_path
import subprocess as _v_process
import sys as _v_sys
import types as _v_types

_v_fake = _v_types.ModuleType("eef_vision_fixture")
exec(_eef_vision_fake_source, _v_fake.__dict__)
_v_root = _v_path.Path(_eef_vision_root)


def _v_log(kind):
    with (_v_root / "trace").open("a", encoding="utf8", newline="\n") as out:
        out.write(kind + "\n")


def _v_open():
    _v_log("open")
    child = _v_process.Popen(
        [_v_sys.executable, "-I", "-B", "-c", "import time; time.sleep(30)"],
        stdin=_v_process.DEVNULL, stdout=_v_process.DEVNULL,
        stderr=_v_process.DEVNULL, creationflags=0x08000000)
    (_v_root / "pids.json").write_text(
        _v_json.dumps({"root": _v_os.getpid(), "child": child.pid}), encoding="utf8")


_v_camera = _v_fake.Camera([_v_fake.Frame(n) for n in ((0, 0, 0) if _eef_vision_quiet else (0, 20, 20))])
_v_camera.on_open = _v_open
_v_camera.on_read = lambda: _v_log("read")
_v_release = _v_camera.release


def _v_close():
    _v_release()
    _v_log("close")


_v_camera.release = _v_close
_v_cv = _v_fake.cv2(_v_camera)
_v_cv.IMWRITE_JPEG_QUALITY = 1
_v_jpeg = bytes.fromhex("ffd801020304ffd9")
_v_cv.imencode = lambda *args: (True, _v_types.SimpleNamespace(nbytes=len(_v_jpeg), tobytes=lambda: _v_jpeg))
_v_sys.modules["cv2"] = _v_cv
