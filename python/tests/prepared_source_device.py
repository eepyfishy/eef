"""Prelude for explicit Rust native unit tests; never a shipping host selector."""
import json as _eef_fixture_json
import os as _eef_fixture_os
import pathlib as _eef_fixture_pathlib
import struct as _eef_fixture_struct
import subprocess as _eef_fixture_subprocess
import sys as _eef_fixture_sys
import time as _eef_fixture_time
import types as _eef_fixture_types

# The including test supplies an isolated directory and closed fixture mode.
_eef_fixture_root = _eef_fixture_pathlib.Path(_eef_fixture_root)
assert _eef_fixture_mode in ("finite", "blocked")
assert _eef_fixture_sys.version_info[:3] == (3, 11, 9)


def _eef_fixture_log(kind):
    with (_eef_fixture_root / "trace.jsonl").open("a", encoding="utf8") as output:
        output.write(_eef_fixture_json.dumps({"kind": kind}) + "\n")


def _eef_fixture_wait(name):
    end = _eef_fixture_time.monotonic() + 8
    while not (_eef_fixture_root / name).is_file():
        if _eef_fixture_time.monotonic() >= end:
            raise RuntimeError("synthetic observation acknowledgement missing")
        _eef_fixture_time.sleep(0.01)


class _eef_fixture_Input:
    def __init__(self, **settings):
        assert settings == {"samplerate": 16000, "channels": 1, "dtype": "int16",
                            "blocksize": 80, "device": "PRIVATE_DEVICE"}
        self.index = 0
        # Deliberately retain a real descendant through normal root completion.
        # The production owned-job path must observe both exits before reuse.
        self.child = _eef_fixture_subprocess.Popen(
            [_eef_fixture_sys.executable, "-I", "-B", "-c", "import time; time.sleep(30)"],
            stdin=_eef_fixture_subprocess.DEVNULL,
            stdout=_eef_fixture_subprocess.DEVNULL,
            stderr=_eef_fixture_subprocess.DEVNULL)
        (_eef_fixture_root / "pids.json").write_text(_eef_fixture_json.dumps(
            {"root": _eef_fixture_os.getpid(), "child": self.child.pid}), encoding="utf8")
        _eef_fixture_log("opened")

    def __enter__(self):
        return self

    def __exit__(self, *unused):
        _eef_fixture_log("closed")

    def read(self, frames):
        assert frames == 80
        if self.index == 0:
            _eef_fixture_wait("observed")
            if _eef_fixture_mode == "blocked":
                _eef_fixture_log("blocked")
                _eef_fixture_time.sleep(30)
                raise RuntimeError("blocked fixture was not cancelled")
        # Pace required transitions from the Rust sink acknowledgement, avoiding
        # a scheduler backlog that would fabricate an event-rate overflow. The
        # real 100ms reporting limit is retained, not bypassed in the source.
        previous = {1: 1, 2: 1, 3: 3, 4: 4}.get(self.index)
        if previous is not None:
            _eef_fixture_wait("ack-" + str(previous))
        _eef_fixture_time.sleep(0.15)
        level, gap = [(0, False), (0, False), (1200, False), (0, True), (0, False)][self.index]
        self.index += 1
        return _eef_fixture_struct.pack("<h", level) * frames, gap


_eef_fixture_sd = _eef_fixture_types.ModuleType("sounddevice")
_eef_fixture_sd.RawInputStream = _eef_fixture_Input
_eef_fixture_sys.modules["sounddevice"] = _eef_fixture_sd
