"""No hardware, device package, network, interpreter download or model is used."""
import array
import importlib.util
import pathlib
import sys
import types
import unittest
from unittest.mock import patch

SOURCE = pathlib.Path(__file__).resolve().parents[1] / "streams" / "microphone_energy.py"
spec = importlib.util.spec_from_file_location("energy_under_test", SOURCE)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class Device:
    def __init__(self, blocks):
        self.blocks = iter(blocks)
        self.opened = self.closed = self.reads = 0
        self.options = None

    def factory(self, **options):
        self.options = options
        return self

    def __enter__(self):
        self.opened += 1
        return self

    def __exit__(self, *args):
        self.closed += 1

    def read(self, count):
        self.reads += 1
        self.requested = count
        value = next(self.blocks)
        if isinstance(value, Exception):
            raise value
        return value


def block(level, gap=False, channels=1):
    return array.array("h", [level] * (80 * channels)).tobytes(), gap


class EnergyTests(unittest.TestCase):
    def source(self, fixture, **params):
        self.addCleanup(patch.stopall)
        patch.dict(sys.modules, {"sounddevice": types.SimpleNamespace(RawInputStream=fixture.factory)}).start()
        return module.stream("energy", {"block_frames": 80, "max_blocks": 3, **params})

    def test_lazy_single_open_and_normal_close(self):
        device = Device([block(0), block(2000), block(100)])
        source = self.source(device)
        self.assertEqual(device.opened, 0)
        result = list(source)
        self.assertEqual([r["active"] for r in result], [False, True, False])
        self.assertEqual([r["source_frames"] for r in result], [80, 160, 240])
        self.assertEqual((device.opened, device.closed, device.reads), (1, 1, 3))
        self.assertEqual(device.options, {"samplerate": 16000, "channels": 1, "dtype": "int16", "blocksize": 80, "device": None})
        self.assertTrue(all(set(r) == {"source_frames", "active", "level", "discontinuity"} for r in result))

    def test_hysteresis_is_energy_not_speech_and_initial_middle_is_unknown(self):
        device = Device([block(750), block(2000), block(750)])
        result = list(self.source(device))
        self.assertEqual([r["active"] for r in result], [None, True, True])
        self.assertEqual([r["level"] for r in result], [750, 2000, 750])

    def test_overflow_resets_decision_instead_of_fabricating_quiet(self):
        device = Device([block(2000), block(0, gap=True), block(750)])
        result = list(self.source(device))
        self.assertEqual([r["active"] for r in result], [True, None, None])
        self.assertEqual(result[1], {"source_frames": 160, "active": None, "level": None, "discontinuity": True})

    def test_close_releases_context_without_reading_next_block(self):
        device = Device([block(100), block(200)])
        source = self.source(device)
        next(source)
        source.close()
        self.assertEqual((device.opened, device.closed, device.reads), (1, 1, 1))

    def test_read_failure_closes_context_and_preserves_failure(self):
        device = Device([block(0), RuntimeError("synthetic read failure")])
        source = self.source(device)
        next(source)
        with self.assertRaises(RuntimeError):
            next(source)
        self.assertEqual(device.closed, 1)

    def test_invalid_block_cannot_report_success_or_activity(self):
        for value in [(b"", False), (b"\x00" * 159, False), (b"\x00" * 162, False), (b"\x00" * 160, 1)]:
            device = Device([value])
            with self.assertRaises(ValueError):
                next(self.source(device))
            self.assertEqual(device.closed, 1)

    def test_signed_full_scale_and_stereo_rms_are_bounded(self):
        device = Device([block(-32768, channels=2)])
        value = list(self.source(device, channels=2, max_blocks=1))[0]
        self.assertEqual(value["level"], 32768)
        self.assertTrue(value["active"])
        self.assertEqual(value["source_frames"], 80)

    def test_level_accounts_for_both_interleaved_channels(self):
        samples = array.array("h", [3000, 4000] * 80).tobytes()
        device = Device([(samples, False)])
        value = list(self.source(device, channels=2, max_blocks=1))[0]
        self.assertEqual(value["level"], 3535)

    def test_parameters_reject_widening_and_wrong_types_before_device_open(self):
        for change in [{"sample_rate": True}, {"sample_rate": 48001}, {"channels": 3},
                       {"block_frames": 4097}, {"block_frames": 79}, {"max_blocks": 0},
                       {"max_blocks": 100001}, {"activation_level": 32769},
                       {"deactivation_level": 1001}, {"device": True},
                       {"device": "bad\nname"}, {"device": -1}, {"data_base64": "forbidden"}]:
            device = Device([])
            with self.assertRaises(ValueError):
                next(self.source(device, **change))
            self.assertEqual(device.opened, 0)

    def test_resource_device_selection_is_forwarded_once(self):
        device = Device([block(0)])
        list(self.source(device, device="owner-selected microphone", max_blocks=1))
        self.assertEqual(device.options["device"], "owner-selected microphone")
        self.assertEqual(device.opened, 1)

    def test_no_source_opens_without_the_explicit_action(self):
        with self.assertRaises(ValueError):
            next(module.stream("record", {}))
        self.assertFalse(hasattr(module, "handle"))
        self.assertFalse(hasattr(module, "CAPABILITY"))


if __name__ == "__main__":
    unittest.main()
