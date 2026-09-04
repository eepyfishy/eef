"""Standard-library example proving that plugins run in bundled CPython."""

from datetime import datetime, timezone

CAPABILITY = "system.clock"
ACTIONS = ["now"]
PARAMS_SCHEMA = {}


def handle(action, params):
    if action != "now":
        raise ValueError(f"unknown clock action: {action}")
    now = datetime.now(timezone.utc)
    return {"utc": now.isoformat(), "timestamp": now.timestamp(), "runtime": "python"}
