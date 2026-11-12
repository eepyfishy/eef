"""Private source tests: synthetic images/camera only, no model or real device."""
import base64
import importlib.util
import os
import pathlib
import sys
import types
import unittest
from unittest.mock import patch

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("vision_fakes", ROOT / "tests" / "camera_motion_fakes.py")
fake = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fake)


class VisionTests(unittest.TestCase):
    def source(self, camera, analysis=None, **params):
        self.addCleanup(patch.stopall)
        self.module = types.ModuleType("vision_under_test")
        source = (ROOT / "streams" / "camera_motion.py").read_text(encoding="utf8") + "\n" + (
            ROOT / "streams" / "camera_vision.py").read_text(encoding="utf8")
        exec(compile(source, "<pinned-vision-test>", "exec"), self.module.__dict__)
        self.clock = fake.Clock()
        self.cv = fake.cv2(camera)
        self.encoded = []
        self.payload = b"\xff\xd8" + bytes(16) + b"\xff\xd9"

        def encode(extension, frame, options):
            self.assertEqual(extension, ".jpg")
            self.encoded.append((frame.shape, options))
            return True, types.SimpleNamespace(nbytes=len(self.payload), tobytes=lambda: self.payload)

        self.cv.IMWRITE_JPEG_QUALITY = 1
        self.cv.imencode = encode
        patch.dict(sys.modules, {"cv2": self.cv}).start()
        patch.dict(os.environ, {}).start()
        patch.object(self.module, "time", self.clock).start()
        settings = {"width": 320, "height": 240, "max_image_bytes": 65536,
                    "jpeg_quality": 70, "max_images": 2, "min_interval_ms": 100,
                    "trigger": "rising", **(analysis or {})}
        return self.module.stream("vision", {"max_frames": 5, "analysis": settings, **params})

    def test_single_open_local_images_only_on_qualifying_transitions(self):
        camera = fake.Camera([fake.Frame(n) for n in (0, 20, 20, 255, 255)])
        source = self.source(camera)
        self.assertEqual(camera.creations, 0)
        records = list(source)
        self.assertEqual([r["active"] for r in records], [None, True, False, True, False])
        self.assertEqual([r["analysis_state"] for r in records],
                         ["not_requested", "frame", "not_requested", "frame", "not_requested"])
        self.assertEqual([r["source_frames"] for r in records], [1, 2, 3, 4, 5])
        self.assertEqual((camera.creations, camera.reads, camera.closed), (1, 5, 1))
        self.assertEqual(camera.threads, [1])
        for record in records:
            if record["analysis_state"] == "frame":
                self.assertEqual(base64.b64decode(record["image_base64"], validate=True), self.payload)
            else:
                self.assertIsNone(record["image_base64"])
        self.assertEqual(self.encoded, [((240, 320, 3), [1, 70])] * 2)

    def test_image_count_budget_does_not_stop_motion_or_invent_an_analysis(self):
        camera = fake.Camera([fake.Frame(n) for n in (0, 20, 20, 255, 255)])
        records = list(self.source(camera, {"max_images": 1}))
        self.assertEqual(records[3]["analysis_state"], "budget_exhausted")
        self.assertIsNone(records[3]["image_base64"])
        self.assertEqual(len(self.encoded), 1)
        self.assertEqual((len(records), camera.closed), (5, 1))

    def test_minimum_analysis_interval_is_separate_from_sampling_interval(self):
        camera = fake.Camera([fake.Frame(n) for n in (0, 20, 20, 255, 255)])
        records = list(self.source(camera, {"min_interval_ms": 1000}))
        self.assertEqual(records[3]["analysis_state"], "rate_limited")
        self.assertIsNone(records[3]["image_base64"])
        self.assertEqual(len(self.encoded), 1)

    def test_active_interval_trigger_is_explicit_not_the_rising_default(self):
        camera = fake.Camera([fake.Frame(n) for n in (0, 20, 40)])
        records = list(self.source(camera, {"trigger": "active_interval", "max_images": 2}, max_frames=3))
        self.assertEqual([r["analysis_state"] for r in records], ["not_requested", "frame", "frame"])

    def test_slow_consumer_gets_a_full_new_interval_after_resume(self):
        camera = fake.Camera([fake.Frame(0), fake.Frame(20)])
        source = self.source(camera, {"max_images": 1}, max_frames=2)
        times = []
        camera.on_read = lambda: times.append(self.clock.now)
        next(source)
        self.clock.now += 3
        next(source)
        source.close()
        self.assertEqual(times, [0, 3.2])
        self.assertEqual(camera.closed, 1)

    def test_dimension_change_is_unknown_and_does_not_encode_a_frame(self):
        camera = fake.Camera([fake.Frame(0), fake.Frame(255, width=320, height=240)])
        records = list(self.source(camera, {"max_images": 1}, max_frames=2))
        self.assertTrue(records[1]["discontinuity"])
        self.assertIsNone(records[1]["active"])
        self.assertEqual(self.encoded, [])

    def test_small_source_is_not_upscaled_for_analysis(self):
        camera = fake.Camera([fake.Frame(0, width=160, height=120), fake.Frame(20, width=160, height=120)])
        list(self.source(camera, {"max_images": 1}, max_frames=2))
        self.assertEqual(self.encoded[0][0], (120, 160, 3))

    def test_image_budget_and_invalid_encoding_fail_without_reopening(self):
        for payload in (bytes(129), b"BAD!", b"\xff\xd8" + bytes(128) + b"\xff\xd9"):
            with self.subTest(length=len(payload)):
                camera = fake.Camera([fake.Frame(0), fake.Frame(20)])
                source = self.source(camera, {"max_images": 1, "max_image_bytes": 128}, max_frames=2)
                self.payload = payload
                next(source)
                with self.assertRaises(ValueError):
                    next(source)
                self.assertEqual((camera.creations, camera.closed), (1, 1))

    def test_close_and_missing_frame_release_once_without_an_extra_acquisition(self):
        camera = fake.Camera([fake.Frame(0), False])
        source = self.source(camera, {"max_images": 1}, max_frames=2)
        next(source)
        with self.assertRaises(RuntimeError):
            next(source)
        self.assertEqual((camera.creations, camera.closed), (1, 1))
        camera = fake.Camera([fake.Frame(0)])
        source = self.source(camera, {"max_images": 1}, max_frames=1)
        next(source)
        source.close()
        self.assertEqual(camera.closed, 1)

    def test_expired_consumer_pause_cannot_restart_capture(self):
        camera = fake.Camera([fake.Frame(0), fake.Frame(20)])
        source = self.source(camera, {"max_images": 1}, max_frames=2, max_duration_ms=2000)
        next(source)
        self.clock.now += 3
        with self.assertRaises(TimeoutError):
            next(source)
        self.assertEqual((camera.reads, camera.closed), (1, 1))

    def test_invalid_scope_rejected_before_device_creation(self):
        for analysis, params in (
                ({"max_images": True}, {}), ({"jpeg_quality": 96}, {}),
                ({"trigger": "anything"}, {}), ({"max_image_bytes": 127}, {}),
                ({"width": 640}, {"width": 320}), ({"max_images": 6}, {}),
                ({}, {"model": "not-a-capture-parameter"}),
                ({}, {"device": "C:/private.jpg"})):
            with self.subTest(analysis=analysis, params=params):
                camera = fake.Camera([])
                with self.assertRaises(ValueError):
                    next(self.source(camera, analysis, **params))
                self.assertEqual(camera.creations, 0)


if __name__ == "__main__":
    unittest.main()
