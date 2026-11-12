"""Private, pinned, pull-paced camera vision transport; no network/work authority.

The owner must supervise the original process deadline and camera reservation.
Images go only through this local pipe. This host is not the legacy metadata host.
"""
import base64
import contextlib
import hashlib
import json
import sys
import types
import uuid


def strict_json(line):
    def unique(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError("vision duplicate field")
            result[key] = value
        return result

    def constant(_):
        raise ValueError("vision nonfinite value")

    return json.loads(line, object_pairs_hook=unique, parse_constant=constant)


def control(wire, stream_id, kind, sequence=None):
    line = wire.readline(513)
    if not line or len(line) > 512 or not line.endswith(b"\n"):
        raise ValueError("vision control")
    value = strict_json(line)
    keys = {"type", "schema_version", "stream_id"} | ({"sequence"} if sequence is not None else set())
    if (not isinstance(value, dict) or set(value) != keys or value["type"] != kind
            or type(value["schema_version"]) is not int or value["schema_version"] != 1
            or value["stream_id"] != stream_id
            or sequence is not None and (type(value["sequence"]) is not int or value["sequence"] != sequence)):
        raise ValueError("vision control")


def main():
    if sys.argv[1:] != ["--pinned-camera-vision-v1"]:
        raise ValueError("vision mode")
    incoming = sys.stdin.buffer
    line = incoming.readline(64 * 1024 + 1)
    if len(line) > 64 * 1024 or not line.endswith(b"\n"):
        raise ValueError("vision request")
    request = strict_json(line)
    if (not isinstance(request, dict) or set(request) != {
            "schema_version", "stream_id", "source_utf8", "source_sha256", "params", "bounds"}
            or type(request["schema_version"]) is not int or request["schema_version"] != 1
            or not isinstance(request["params"], dict)):
        raise ValueError("vision request")
    stream_id = request["stream_id"]
    parsed = uuid.UUID(stream_id)
    if str(parsed) != stream_id or not parsed.int:
        raise ValueError("vision identity")
    source, digest = request["source_utf8"], request["source_sha256"]
    if (not isinstance(source, str) or not source or "\0" in source
            or len(source.encode("utf8")) > 48 * 1024
            or not isinstance(digest, str) or len(digest) != 64
            or any(c not in "0123456789abcdef" for c in digest)
            or hashlib.sha256(source.encode("utf8")).hexdigest() != digest):
        raise ValueError("vision source")
    bounds = request["bounds"]
    if not isinstance(bounds, dict) or set(bounds) != {
            "max_frame_bytes", "max_stdout_bytes", "max_observations", "max_source_frames",
            "max_image_bytes", "max_images"}:
        raise ValueError("vision bounds")
    for key, lower, upper in (
            ("max_frame_bytes", 1024, 3 * 1024 * 1024),
            ("max_stdout_bytes", 1024, 4 * 1024 * 1024),
            ("max_observations", 1, 100000), ("max_source_frames", 1, 100000),
            ("max_image_bytes", 128, 2 * 1024 * 1024), ("max_images", 1, 100)):
        if type(bounds[key]) is not int or not lower <= bounds[key] <= upper:
            raise ValueError("vision bound")
    if (bounds["max_frame_bytes"] > bounds["max_stdout_bytes"]
            or 4 * ((bounds["max_image_bytes"] + 2) // 3) + 1024 > bounds["max_frame_bytes"]
            or bounds["max_images"] > bounds["max_source_frames"]):
        raise ValueError("vision image budget")
    analysis = request["params"].get("analysis")
    if (not isinstance(analysis, dict)
            or analysis.get("max_image_bytes") != bounds["max_image_bytes"]
            or analysis.get("max_images") != bounds["max_images"]
            or type(request["params"].get("max_frames")) is not int
            or not 1 <= request["params"]["max_frames"] <= min(bounds["max_observations"], bounds["max_source_frames"])):
        raise ValueError("vision source bounds")
    outgoing, total = sys.stdout.buffer, 0

    def emit(value):
        nonlocal total
        encoded = (json.dumps(value, ensure_ascii=True, separators=(",", ":")) + "\n").encode("utf8")
        total += len(encoded)
        if len(encoded) > bounds["max_frame_bytes"] or total > bounds["max_stdout_bytes"]:
            raise ValueError("vision output budget")
        outgoing.write(encoded)
        outgoing.flush()

    module = types.ModuleType("eef_private_camera_vision")
    module.__file__ = "<eef-pinned-camera-vision>"
    iterator = None
    try:
        with contextlib.redirect_stdout(sys.stderr):
            exec(compile(source, module.__file__, "exec"), module.__dict__)
            handler = getattr(module, "stream", None)
            if not callable(handler):
                raise ValueError("vision handler")
            iterator = handler("vision", request["params"])
            if iter(iterator) is not iterator:
                raise ValueError("vision iterator")
        emit({"type": "ready", "schema_version": 1, "protocol": "camera_vision_v1", "stream_id": stream_id})
        # A legacy reader that closes stdin cannot accidentally start capture.
        control(incoming, stream_id, "start")
        sequence, frames, images = 0, 0, 0
        while True:
            with contextlib.redirect_stdout(sys.stderr):
                try:
                    record = next(iterator)
                except StopIteration:
                    break
            if not isinstance(record, dict) or set(record) != {
                    "source_frames", "active", "level", "discontinuity", "analysis_state", "image_base64"}:
                raise ValueError("vision record")
            count, active, level, gap = (record[name] for name in (
                "source_frames", "active", "level", "discontinuity"))
            state, image = record["analysis_state"], record["image_base64"]
            if (type(count) is not int or count != frames + 1 or count > bounds["max_source_frames"]
                    or active is not None and type(active) is not bool
                    or level is not None and (type(level) is not int or not 0 <= level <= 255)
                    or active is not None and level is None
                    or type(gap) is not bool or gap and (active is not None or level is not None)
                    or state not in ("not_requested", "frame", "budget_exhausted", "rate_limited")):
                raise ValueError("vision observation")
            if state == "frame":
                if (active is not True or gap or not isinstance(image, str)
                        or len(image) > 4 * ((bounds["max_image_bytes"] + 2) // 3)):
                    raise ValueError("vision image")
                raw = base64.b64decode(image, validate=True)
                if (not 4 <= len(raw) <= bounds["max_image_bytes"]
                        or not raw.startswith(b"\xff\xd8") or not raw.endswith(b"\xff\xd9")):
                    raise ValueError("vision image")
                del raw
                images += 1
                if images > bounds["max_images"]:
                    raise ValueError("vision image count")
            elif image is not None:
                raise ValueError("vision unexpected image")
            sequence += 1
            if sequence > bounds["max_observations"]:
                raise ValueError("vision observation count")
            frames = count
            emit({"type": "observation", "stream_id": stream_id, "sequence": sequence, **record})
            control(incoming, stream_id, "ack", sequence)
            del record, image
        if sequence != request["params"]["max_frames"] or frames != sequence:
            raise ValueError("vision incomplete source")
        emit({"type": "end", "stream_id": stream_id, "observations": sequence,
              "source_frames": frames, "images": images})
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
        sys.stderr.write("vision_stream_failed\n")
        raise SystemExit(1)
