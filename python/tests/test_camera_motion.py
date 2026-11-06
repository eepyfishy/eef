"""Source behavior only: no camera, network, OpenCV install, Python download or model."""
import importlib.util
import pathlib
import sys
import unittest
from unittest.mock import patch

ROOT = pathlib.Path(__file__).resolve().parents[1]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


module = load("motion_under_test", ROOT / "streams" / "camera_motion.py")
fake = load("camera_fakes", ROOT / "tests" / "camera_motion_fakes.py")


class MotionTests(unittest.TestCase):
    def source(self, camera, **params):
        self.addCleanup(patch.stopall)
        self.clock = fake.Clock()
        self.cv = fake.cv2(camera)
        patch.dict(sys.modules, {"cv2": self.cv}).start()
        patch.object(module, "time", self.clock).start()
        return module.stream("motion", {"max_frames": 3, **params})

    def test_lazy_single_open_small_metadata_and_hysteresis(self):
        camera = fake.Camera([fake.Frame(n) for n in [0, 20, 28, 28]])
        source = self.source(camera, max_frames=4)
        self.assertEqual(camera.creations, 0)
        values = list(source)
        self.assertEqual([v["active"] for v in values], [None, True, True, False])
        self.assertEqual([v["level"] for v in values], [None, 20, 8, 0])
        self.assertEqual([v["source_frames"] for v in values], [1, 2, 3, 4])
        self.assertEqual((camera.creations, camera.closed, camera.reads), (1, 1, 4))
        self.assertEqual(camera.calls, [(0,)])
        self.assertEqual(camera.threads, [1])
        self.assertTrue(all(set(v) == {"source_frames", "active", "level", "discontinuity"} for v in values))

    def test_first_frame_is_unknown_not_a_fabricated_quiet_scene(self):
        camera = fake.Camera([fake.Frame(255)])
        value = list(self.source(camera, max_frames=1))[0]
        self.assertEqual(value, {"source_frames": 1, "active": None, "level": None, "discontinuity": False})

    def test_full_scale_and_threshold_edges(self):
        camera = fake.Camera([fake.Frame(n) for n in [0, 255, 240, 235]])
        values = list(self.source(camera, max_frames=4))
        self.assertEqual([v["level"] for v in values], [None, 255, 15, 5])
        self.assertEqual([v["active"] for v in values], [None, True, True, False])

    def test_difference_uses_all_sample_pixels_and_keeps_initial_middle_unknown(self):
        camera = fake.Camera([fake.Frame(n) for n in [0, 1, 50]])
        source = self.source(camera, sample_width=8, sample_height=8)
        def gray(frame, code):
            value = fake.Frame(width=8, height=8, channels=1)
            value.tobytes = lambda: bytes([0, 20]) * 32 if frame.value == 1 else bytes([frame.value]) * 64
            return value
        self.cv.cvtColor = gray
        values = list(source)
        self.assertEqual([v["level"] for v in values], [None, 10, 40])
        self.assertEqual([v["active"] for v in values], [None, None, True])

    def test_explicit_close_releases_without_another_read(self):
        camera = fake.Camera([fake.Frame(0), fake.Frame(20)])
        source = self.source(camera)
        next(source)
        source.close()
        self.assertEqual((camera.creations, camera.closed, camera.reads), (1, 1, 1))

    def test_missing_frame_fails_without_reconnect_or_inactivity(self):
        camera = fake.Camera([fake.Frame(0), False, fake.Frame(20)])
        source = self.source(camera)
        next(source)
        with self.assertRaisesRegex(RuntimeError, "camera motion read"):
            next(source)
        self.assertEqual((camera.creations, camera.closed, camera.reads), (1, 1, 2))

    def test_read_exception_releases_source(self):
        camera = fake.Camera([RuntimeError("synthetic private driver failure")])
        with self.assertRaises(RuntimeError):
            next(self.source(camera))
        self.assertEqual(camera.closed, 1)

    def test_actual_frame_limits_apply_even_when_device_ignores_settings(self):
        for frame in [fake.Frame(width=641), fake.Frame(height=481), fake.Frame(channels=4),
                      fake.Frame(dtype="float32"), fake.Frame(width=0)]:
            camera = fake.Camera([frame])
            with self.assertRaises(ValueError):
                next(self.source(camera))
            self.assertEqual(camera.closed, 1)

    def test_wrong_frame_byte_count_is_rejected(self):
        frame = fake.Frame()
        frame.nbytes += 1
        camera = fake.Camera([frame])
        with self.assertRaises(ValueError):
            next(self.source(camera))
        self.assertEqual(camera.closed, 1)

    def test_invalid_resized_sample_releases_without_output(self):
        camera = fake.Camera([fake.Frame()])
        source = self.source(camera)
        self.cv.resize = lambda *args, **kwargs: fake.Frame(width=63, height=48)
        with self.assertRaises(ValueError):
            next(source)
        self.assertEqual(camera.closed, 1)

    def test_invalid_gray_bytes_cannot_be_a_motion_observation(self):
        camera = fake.Camera([fake.Frame()])
        source = self.source(camera)
        gray = fake.Frame(width=64, height=48, channels=1)
        gray.tobytes = lambda: b"short"
        self.cv.cvtColor = lambda *args: gray
        with self.assertRaises(ValueError):
            next(source)
        self.assertEqual(camera.closed, 1)

    def test_rtsp_timeout_hints_and_audio_disable_are_supplied_once_without_fallback(self):
        camera = fake.Camera([], opened=False)
        endpoint = "rtsp://private-user:private-secret@camera.invalid/live"
        source = self.source(camera, device=endpoint)
        with self.assertRaisesRegex(RuntimeError, "camera motion unavailable"):
            next(source)
        self.assertEqual(camera.calls, [(endpoint, 1900, [53, 2000, 54, 2000, 58, -1])])
        self.assertEqual((camera.creations, camera.closed, camera.reads), (1, 1, 0))

    def test_parameters_are_strict_before_capture(self):
        for change in [{"width": True}, {"width": 1921}, {"height": 1081}, {"sample_width": 161},
                       {"sample_height": 7}, {"max_frames": 0}, {"max_frames": 100001},
                       {"interval_ms": 99}, {"activation_level": 256}, {"deactivation_level": 16},
                       {"io_timeout_ms": 10001}, {"max_duration_ms": 99},
                       {"io_timeout_ms": 2000, "max_duration_ms": 1000}, {"record_to": "forbidden"}]:
            camera = fake.Camera([])
            with self.assertRaises(ValueError):
                next(self.source(camera, **change))
            self.assertEqual(camera.creations, 0)

    def test_device_selection_cannot_be_a_file_or_unreviewed_protocol(self):
        for device in [True, -1, 1024, None, "movie.mp4", "file:///secret", "https://camera.invalid/",
                       "rtsp:///missing", "rtsp://camera.invalid:0/live", "rtsp://camera.invalid/live#fragment",
                       "rtsp://camera.invalid/\nprivate", "rtsp://camera.invalid:bad/live"]:
            camera = fake.Camera([])
            with self.assertRaises(ValueError):
                next(self.source(camera, device=device))
            self.assertEqual(camera.creations, 0)

    def test_resolution_change_invalidates_previous_comparison(self):
        camera = fake.Camera([fake.Frame(0), fake.Frame(20),
                              fake.Frame(60, width=320, height=240), fake.Frame(60, width=320, height=240)])
        values = list(self.source(camera, max_frames=4))
        self.assertEqual([v["active"] for v in values], [None, True, None, False])
        self.assertEqual([v["level"] for v in values], [None, 20, None, 0])
        self.assertEqual([v["discontinuity"] for v in values], [False, False, True, False])

    def test_backpressure_does_not_cause_catch_up_reads_or_extra_frames(self):
        camera = fake.Camera([fake.Frame()] * 5)
        source = self.source(camera, max_frames=4)
        next(source)
        next(source)
        self.clock.now += 5
        next(source)
        next(source)
        source.close()
        self.assertEqual(camera.reads, 4)
        self.assertEqual(len(self.clock.sleeps), 2)
        for delay in self.clock.sleeps:
            self.assertAlmostEqual(delay, 0.2)

    def test_open_and_read_are_inside_the_same_local_deadline(self):
        for phase in ["open", "read"]:
            camera = fake.Camera([fake.Frame()])
            source = self.source(camera, max_duration_ms=1000, io_timeout_ms=1000)
            def stall():
                self.clock.now += 2
            if phase == "open":
                camera.on_open = stall
            else:
                camera.on_read = stall
            with self.assertRaises(TimeoutError):
                next(source)
            self.assertEqual(camera.closed, 1)
            self.assertEqual(camera.reads, 0 if phase == "open" else 1)

    def test_expired_initialization_cannot_open_a_camera(self):
        camera = fake.Camera([fake.Frame()])
        source = self.source(camera, max_duration_ms=1000, io_timeout_ms=1000)
        def stall(_):
            self.clock.now += 2
        self.cv.setNumThreads = stall
        with self.assertRaises(TimeoutError):
            next(source)
        self.assertEqual((camera.creations, camera.closed, camera.reads), (0, 0, 0))

    def test_explicit_action_and_finite_frame_budget_are_required(self):
        camera = fake.Camera([fake.Frame()] * 4)
        self.assertEqual(len(list(self.source(camera, max_frames=2))), 2)
        self.assertEqual(camera.reads, 2)
        with self.assertRaises(ValueError):
            next(module.stream("capture", {}))
        self.assertFalse(hasattr(module, "handle"))
        self.assertFalse(hasattr(module, "CAPABILITY"))


if __name__ == "__main__":
    unittest.main()
