"""Bounded metadata stream transport; no workload or listening authorization.

Trusted plugins implement stream(action, params) as an iterator. Device imports
and capture belong in that iterator, never in the normal one-shot host. Ready is
protocol readiness only. This host is not called by node startup or ordinary RPC.
"""
import contextlib
import importlib.util
import hashlib
import json
import pathlib
import sys
import types
import uuid


def main():
    request = json.load(sys.stdin)
    pinned = sys.argv[1:] == ["--pinned-source-v1", "--reviewed-bytes"]
    fields = {"schema_version", "stream_id", "action", "params", "bounds"}
    if pinned:
        fields |= {"source_utf8", "source_sha256"}
    if (not isinstance(request, dict)
            or set(request) != fields
            or type(request["schema_version"]) is not int
            or request["schema_version"] != (2 if pinned else 1)
            or not isinstance(request["params"], dict)
            or not isinstance(request["action"], str)):
        raise ValueError("stream request")
    if pinned:
        source, digest = request["source_utf8"], request["source_sha256"]
        if (not isinstance(source, str) or not source or "\0" in source
                or len(source.encode("utf8")) > 48 * 1024
                or not isinstance(digest, str) or len(digest) != 64
                or any(c not in "0123456789abcdef" for c in digest)
                or hashlib.sha256(source.encode("utf8")).hexdigest() != digest):
            raise ValueError("stream source")
    stream_id = request["stream_id"]
    parsed = uuid.UUID(stream_id)
    if str(parsed) != stream_id or not parsed.int:
        raise ValueError("stream identity")
    bounds = request["bounds"]
    if not isinstance(bounds, dict) or set(bounds) != {
            "max_frame_bytes", "max_stdout_bytes", "max_observations", "max_source_frames"}:
        raise ValueError("stream bounds")
    for name, lower, upper in (
            ("max_frame_bytes", 128, 16384), ("max_stdout_bytes", 1024, 4 * 1024 * 1024),
            ("max_observations", 1, 100000), ("max_source_frames", 1, 1000000000000)):
        if type(bounds[name]) is not int or not lower <= bounds[name] <= upper:
            raise ValueError("stream bound")
    if bounds["max_frame_bytes"] > bounds["max_stdout_bytes"]:
        raise ValueError("stream frame budget")
    wire, total = sys.stdout.buffer, 0

    def emit(value):
        nonlocal total
        encoded = (json.dumps(value, ensure_ascii=True, separators=(",", ":")) + "\n").encode("utf8")
        total += len(encoded)
        if len(encoded) > bounds["max_frame_bytes"] or total > bounds["max_stdout_bytes"]:
            raise ValueError("stream output budget")
        wire.write(encoded)
        wire.flush()

    if pinned:
        module = types.ModuleType("eef_stream_plugin")
        module.__file__ = "<eef-pinned-stream>"
    else:
        spec = importlib.util.spec_from_file_location("eef_stream_plugin", pathlib.Path(sys.argv[1]).resolve())
        if spec is None or spec.loader is None:
            raise ValueError("stream module")
        module = importlib.util.module_from_spec(spec)
    iterator = None
    try:
        with contextlib.redirect_stdout(sys.stderr):
            if pinned:
                exec(compile(source, "<eef-pinned-stream>", "exec"), module.__dict__)
            else:
                spec.loader.exec_module(module)
            handler = getattr(module, "stream", None)
            if not callable(handler):
                raise ValueError("stream handler")
            iterator = handler(request["action"], request["params"])
            if iter(iterator) is not iterator:
                raise ValueError("stream iterator required")
        emit({"type": "ready", "schema_version": 1, "stream_id": stream_id})
        sequence, frames = 0, 0
        while True:
            with contextlib.redirect_stdout(sys.stderr):
                try:
                    record = next(iterator)
                except StopIteration:
                    break
            if not isinstance(record, dict) or set(record) != {
                    "source_frames", "active", "level", "discontinuity"}:
                raise ValueError("stream record")
            count, active, level, gap = (record[name] for name in (
                "source_frames", "active", "level", "discontinuity"))
            if (type(count) is not int or not frames < count <= bounds["max_source_frames"]
                    or active is not None and type(active) is not bool
                    or level is not None and (type(level) is not int or not 0 <= level <= 32768)
                    or type(gap) is not bool or gap and active is not None
                    or active is not None and level is None):
                raise ValueError("stream observation")
            sequence += 1
            if sequence > bounds["max_observations"]:
                raise ValueError("stream observation budget")
            frames = count
            emit({"type": "observation", "stream_id": stream_id, "sequence": sequence, **record})
        if not sequence:
            raise ValueError("stream ended without observations")
        emit({"type": "end", "stream_id": stream_id, "observations": sequence, "source_frames": frames})
    finally:
        if iterator is not None:
            with contextlib.redirect_stdout(sys.stderr):
                close = getattr(iterator, "close", None)
                if callable(close):
                    close()


if __name__ == "__main__":
    try:
        main()
    except Exception:
        # Plugin text, paths, tracebacks and media never become protocol errors.
        sys.stderr.write("stream_failed\n")
        raise SystemExit(1)
