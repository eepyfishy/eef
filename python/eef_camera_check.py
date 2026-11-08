"""Fixed dependency check: no camera, endpoint, model or user plugin is opened.

Runs inside the selected interpreter and the Rust owner's existing process
deadline, output limits and Windows job memory ceiling. Imports are trusted
installed code, not a sandbox. All failures use closed categories.
"""
import contextlib
import json
import os
import sys

# A diagnostic needs no parallel image/math workers. Bound dependency-created
# workers before import, in addition to the owner's aggregate process limits.
for variable in ("OPENBLAS_NUM_THREADS", "OMP_NUM_THREADS", "OPENCV_FOR_THREADS_NUM"):
    os.environ[variable] = "1"


def check():
    try:
        with contextlib.redirect_stdout(sys.stderr):
            import cv2
            import numpy as np
    except Exception:
        return "dependency_unavailable"
    try:
        with contextlib.redirect_stdout(sys.stderr):
            cv2.setNumThreads(1)
            frame = np.zeros((16, 16, 3), dtype=np.uint8)
            gray = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)
            small = cv2.resize(gray, (8, 8), interpolation=cv2.INTER_AREA)
            if small.shape != (8, 8) or small.dtype != np.uint8 or small.nbytes != 64:
                return "image_operation_failed"
        return "ready"
    except Exception:
        return "image_operation_failed"


json.dump({"ok": True, "data": {"status": check()}}, sys.stdout,
          ensure_ascii=True, separators=(",", ":"))
