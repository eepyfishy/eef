"""Bundle-local compatibility for pywin32's two system-DLL loader functions.

The pinned native helper truncates module filenames at MAX_PATH. Preserve the
already loaded DLL's identity using a bounded growing Windows API buffer.
Loaded paths may need the extended form when Python imports that same image.
This neither installs DLLs globally nor changes the DLL search policy.
"""

import ctypes
from ctypes import wintypes
import ntpath

_kernel = ctypes.WinDLL("kernel32", use_last_error=True)
_handle = _kernel.GetModuleHandleW
_handle.argtypes = [wintypes.LPCWSTR]
_handle.restype = wintypes.HMODULE
_filename = _kernel.GetModuleFileNameW
_filename.argtypes = [wintypes.HMODULE, wintypes.LPWSTR, wintypes.DWORD]
_filename.restype = wintypes.DWORD
_load = _kernel.LoadLibraryExW
_load.argtypes = [wintypes.LPCWSTR, wintypes.HANDLE, wintypes.DWORD]
_load.restype = wintypes.HMODULE
_MAX_PATH_CHARS = 32768


def _module_filename(handle):
    if not handle:
        return None
    capacity = 256
    while capacity <= _MAX_PATH_CHARS:
        buffer = ctypes.create_unicode_buffer(capacity)
        count = _filename(handle, buffer, capacity)
        if count == 0:
            return None
        if count < capacity:
            path = buffer.value
            if count >= 260 and not path.startswith("\\\\?\\"):
                path = ntpath.normpath(path)
                if not ntpath.isabs(path) or not ntpath.splitdrive(path)[0]:
                    raise OSError("Loaded module did not supply an absolute filename")
                if path.startswith("\\\\"):
                    path = "\\\\?\\UNC\\" + path[2:]
                else:
                    path = "\\\\?\\" + path
            return path
        capacity *= 2
    raise OSError("Loaded module path exceeds the observation limit")


def _name(name):
    if not isinstance(name, str):
        raise TypeError("module name must be text")
    return name


def GetModuleFilename(name):
    return _module_filename(_handle(_name(name)))


def LoadModule(name):
    # Match the pinned helper's flags and lifetime: no fallback directory and no
    # FreeLibrary that could invalidate the image Python is about to import.
    return _module_filename(_load(_name(name), None, 0x1000 | 0x0100))
