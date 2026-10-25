//! Console ownership for the native command entrypoints, not UI control.
//! Never detach an inherited terminal or discard redirected command/log handles.

#[cfg(any(windows, test))]
fn may_detach(count: u32, first: u32, current: u32, redirected: bool) -> bool {
    count == 1 && first == current && !redirected
}

pub fn attached() -> bool {
    #[cfg(windows)]
    {
        let mut processes = [0u32; 1];
        // SAFETY: the writable buffer has the advertised length.
        unsafe {
            windows_sys::Win32::System::Console::GetConsoleProcessList(
                processes.as_mut_ptr(),
                processes.len() as u32,
            ) != 0
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Called only for a parsed service invocation, never a command/help/error.
/// Explorer/startup may allocate this console; an existing shell remains owned
/// by its other client process and is deliberately left alone.
pub fn detach_owned_service_console() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::INVALID_HANDLE_VALUE,
            System::Console::{
                FreeConsole, GetConsoleMode, GetConsoleProcessList, GetStdHandle, STD_ERROR_HANDLE,
                STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
            },
        };
        let mut processes = [0u32; 2];
        // SAFETY: buffers are valid; handles are borrowed, never closed or
        // replaced. FreeConsole is reached only for our exclusive console.
        unsafe {
            let count = GetConsoleProcessList(processes.as_mut_ptr(), processes.len() as u32);
            let redirected = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE]
                .iter()
                .any(|kind| {
                    let handle = GetStdHandle(*kind);
                    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                        return false;
                    }
                    let mut mode = 0;
                    GetConsoleMode(handle, &mut mode) == 0
                });
            may_detach(count, processes[0], std::process::id(), redirected) && FreeConsole() != 0
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_an_exclusive_owned_service_console_can_be_detached() {
        assert!(may_detach(1, 42, 42, false));
        assert!(!may_detach(0, 0, 42, false));
        assert!(!may_detach(1, 41, 42, false));
        assert!(!may_detach(2, 42, 42, false));
        assert!(!may_detach(10, 42, 42, false));
    }
    #[test]
    fn redirected_service_logging_is_not_detached() {
        assert!(!may_detach(1, 42, 42, true));
    }
}
