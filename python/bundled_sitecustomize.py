"""Keep bundled Windows extension/DLL imports usable in deep install folders.

Copied into the embedded runtime as sitecustomize.py. The bundle's existing
``import site`` entry loads it before adapter imports. No paths are added and
custom/system Python installations are not modified. This is not isolation.
"""

import ntpath
import os
import sys


def _ordinary(path):
    if path[:8].lower() == "\\\\?\\unc\\":
        return "\\\\" + path[8:]
    if path.startswith("\\\\?\\"):
        return path[4:]
    return path


def _extended_path(path, runtime_root):
    """Preserve each search target; prefix only absolute paths inside this runtime."""
    if not isinstance(path, str) or not path or path.startswith("\\\\?\\"):
        return path
    normalized = ntpath.normpath(path)
    root = ntpath.normpath(_ordinary(runtime_root))
    if normalized.startswith("\\\\.\\") or root.startswith("\\\\.\\"):
        return path
    if not ntpath.splitdrive(normalized)[0] or not ntpath.splitdrive(root)[0]:
        return path
    if not ntpath.isabs(normalized) or not ntpath.isabs(root):
        return path
    try:
        root_comparison = ntpath.normcase(root)
        if ntpath.commonpath([root_comparison, ntpath.normcase(normalized)]) != root_comparison:
            return path
    except ValueError:  # Different drives/shares are not part of this runtime.
        return path
    if normalized.startswith("\\\\"):
        return "\\\\?\\UNC\\" + normalized[2:]
    return "\\\\?\\" + normalized


if os.name == "nt":
    _runtime = os.path.dirname(os.path.abspath(__file__))
    sys.path[:] = [_extended_path(path, _runtime) for path in sys.path]
