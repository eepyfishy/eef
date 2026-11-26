"""Pure path tests; no package, extension, model, device or network is used."""

import importlib.util
import pathlib
import unittest
from unittest.mock import Mock, patch
import ctypes
import os
import sys

spec = importlib.util.spec_from_file_location(
    "bundled_paths_under_test", pathlib.Path(__file__).with_name("bundled_sitecustomize.py")
)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
extended = module._extended_path


class BundledPaths(unittest.TestCase):
    root = r"C:\application\python"

    def test_owned_paths_keep_their_targets_and_order(self):
        paths = [self.root + suffix for suffix in [r"\python311.zip", "", r"\Lib\site-packages", r"\Lib\site-packages\win32"]]
        self.assertEqual([extended(path, self.root) for path in paths], ["\\\\?\\" + path for path in paths])

    def test_existing_verbatim_paths_are_unchanged_and_idempotent(self):
        for path in [self.root, self.root + r"\Lib\site-packages"]:
            result = extended(path, self.root)
            self.assertEqual(extended(result, self.root), result)
        foreign = r"\\?\D:\other\python"
        self.assertIs(extended(foreign, self.root), foreign)

    def test_custom_and_sibling_paths_are_not_rewritten(self):
        for path in [r"D:\python", r"C:\application\python-other", r"C:\application\outside", r"C:\application\python\..\outside"]:
            self.assertIs(extended(path, self.root), path)

    def test_relative_and_nonstring_entries_are_not_added_or_reinterpreted(self):
        for path in ["", "relative", r"C:relative", r"\rooted-relative", None, b"bytes"]:
            self.assertIs(extended(path, self.root), path)

    def test_dot_segments_inside_runtime_are_resolved_before_verbatim_prefix(self):
        self.assertEqual(extended(self.root + r"\Lib\..\python311.zip", self.root), r"\\?\C:\application\python\python311.zip")

    def test_extended_runtime_root_and_case_comparison(self):
        self.assertEqual(extended(r"c:\APPLICATION\python\Lib", "\\\\?\\" + self.root), r"\\?\c:\APPLICATION\python\Lib")

    def test_unc_targets_keep_the_same_server_and_share(self):
        root = r"\\server\share\python"
        path = root + r"\Lib\site-packages"
        expected = r"\\?\UNC\server\share\python\Lib\site-packages"
        self.assertEqual(extended(path, root), expected)
        self.assertEqual(extended(path, r"\\?\UNC\server\share\python"), expected)
        foreign = r"\\server\other-share\python"
        self.assertIs(extended(foreign, root), foreign)

    def test_device_and_nonabsolute_roots_are_not_transformed(self):
        for path, root in [(r"\\.\PIPE\pipe", self.root), (self.root, r"\\.\C:\application\python"), (self.root, "relative"), (self.root, r"C:relative")]:
            self.assertIs(extended(path, root), path)


class Win32ModulePaths(unittest.TestCase):
    def setUp(self):
        self.kernel = Mock()
        self.kernel.GetModuleHandleW.return_value = 123
        self.kernel.LoadLibraryExW.return_value = 456
        source = pathlib.Path(__file__).with_name("bundled_win32sysloader.py")
        loader_spec = importlib.util.spec_from_file_location("win32_loader_under_test", source)
        self.loader = importlib.util.module_from_spec(loader_spec)
        with patch.object(ctypes, "WinDLL", return_value=self.kernel, create=True):
            loader_spec.loader.exec_module(self.loader)

    def filename(self, path):
        def query(handle, buffer, capacity):
            buffer.value = path[:capacity - 1]
            return capacity if len(path) >= capacity else len(path)
        self.kernel.GetModuleFileNameW.side_effect = query

    def test_full_loaded_module_name_is_returned_after_multiple_buffer_expansions(self):
        path = "C:\\" + "long\\" * 200 + "pywintypes311.dll"
        self.filename(path)
        self.assertEqual(self.loader.GetModuleFilename("pywintypes311.dll"), "\\\\?\\" + path)
        self.assertGreater(self.kernel.GetModuleFileNameW.call_count, 2)
        self.kernel.LoadLibraryExW.assert_not_called()

    def test_short_and_already_extended_names_are_preserved(self):
        for path in [r"C:\Windows\System32\example.dll", "\\\\?\\C:\\" + "long\\" * 100 + "example.dll"]:
            self.filename(path)
            self.assertEqual(self.loader.GetModuleFilename("example.dll"), path)

    def test_long_unc_name_preserves_server_share_and_loaded_image(self):
        path = "\\\\server\\share\\" + "long\\" * 100 + "example.dll"
        self.filename(path)
        self.assertEqual(self.loader.GetModuleFilename("example.dll"), "\\\\?\\UNC\\" + path[2:])

    def test_missing_or_unobservable_module_retains_none_result(self):
        self.kernel.GetModuleHandleW.return_value = 0
        self.assertIsNone(self.loader.GetModuleFilename("absent.dll"))
        self.kernel.GetModuleFileNameW.assert_not_called()
        self.kernel.GetModuleHandleW.return_value = 123
        self.kernel.GetModuleFileNameW.return_value = 0
        self.assertIsNone(self.loader.GetModuleFilename("unobserved.dll"))

    def test_excessive_name_fails_without_returning_a_truncated_path(self):
        self.kernel.GetModuleFileNameW.side_effect = lambda handle, buffer, capacity: capacity
        with self.assertRaisesRegex(OSError, "observation limit"):
            self.loader.GetModuleFilename("too-long.dll")
        self.assertEqual(self.kernel.GetModuleFileNameW.call_args.args[-1], 32768)

    def test_load_uses_original_flags_without_search_fallback_or_handle_release(self):
        self.filename(r"C:\application\python\pywintypes311.dll")
        self.assertEqual(self.loader.LoadModule("pywintypes311.dll"), r"C:\application\python\pywintypes311.dll")
        self.kernel.LoadLibraryExW.assert_called_once_with("pywintypes311.dll", None, 0x1100)
        self.kernel.GetModuleHandleW.assert_not_called()
        self.kernel.FreeLibrary.assert_not_called()

    def test_module_names_remain_unicode_only(self):
        for name in [None, b"example.dll", 1]:
            with self.assertRaises(TypeError):
                self.loader.GetModuleFilename(name)
            with self.assertRaises(TypeError):
                self.loader.LoadModule(name)
        self.kernel.LoadLibraryExW.assert_not_called()


@unittest.skipUnless(os.name == "nt", "native Windows API observation")
class NativeWin32ModulePaths(unittest.TestCase):
    def test_already_loaded_python_image_uses_the_real_windows_api(self):
        source = pathlib.Path(__file__).with_name("bundled_win32sysloader.py")
        loader_spec = importlib.util.spec_from_file_location("real_win32_loader_under_test", source)
        loader = importlib.util.module_from_spec(loader_spec)
        loader_spec.loader.exec_module(loader)
        name = f"python{sys.version_info.major}{sys.version_info.minor}.dll"
        path = loader.GetModuleFilename(name)
        self.assertTrue(path and os.path.isfile(path))
        # Like the original helper, this retains its one load reference until
        # this short-lived test process exits; it must not unload the interpreter.
        self.assertEqual(loader.LoadModule(path), path)
        self.assertEqual(loader.GetModuleFilename(name), path)


if __name__ == "__main__":
    unittest.main()
