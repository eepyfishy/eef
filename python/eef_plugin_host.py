"""Tiny isolated JSON bridge used by the Rust PythonRuntime.

A plugin is a .py file defining:
  CAPABILITY = "sensor_temperature"
  ACTIONS = ["read"]
  PARAMS_SCHEMA = {...}  # optional
  def handle(action, params): ...  # may also be async
"""

import asyncio
import contextlib
import importlib.util
import inspect
import json
import pathlib
import sys
import traceback


def load(path):
    spec = importlib.util.spec_from_file_location("eef_user_plugin", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load plugin: {path}")
    module = importlib.util.module_from_spec(spec)
    with contextlib.redirect_stdout(sys.stderr):
        spec.loader.exec_module(module)
    return module


def run_awaitable(value):
    return asyncio.run(value) if inspect.isawaitable(value) else value


def main():
    path = pathlib.Path(sys.argv[1]).resolve()
    request = json.load(sys.stdin)
    module = load(path)
    if request.get("op") == "inspect":
        data = {
            "path": str(path),
            "capability": str(getattr(module, "CAPABILITY", "")),
            "actions": list(getattr(module, "ACTIONS", ["run"])),
            "params": getattr(module, "PARAMS_SCHEMA", {}),
        }
    elif request.get("op") == "invoke":
        handler = getattr(module, "handle", None)
        if not callable(handler):
            raise RuntimeError("plugin must define handle(action, params)")
        with contextlib.redirect_stdout(sys.stderr):
            data = run_awaitable(handler(request.get("action", "run"), request.get("params", {})))
    else:
        raise RuntimeError("unknown plugin host operation")
    json.dump({"ok": True, "data": data}, sys.stdout, default=str)


try:
    main()
except Exception as exc:
    json.dump({"ok": False, "error": str(exc), "trace": traceback.format_exc(limit=5)}, sys.stdout)
    raise SystemExit(1)
