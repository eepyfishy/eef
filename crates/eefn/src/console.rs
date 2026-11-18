//! Console ownership and interruption for the native service entrypoints.
//! Never detach an inherited terminal or discard redirected command/log handles.

/// Complete only for an observed console interrupt. Listener failure must not
/// become an exit request, spin on a ready error or disable management controls.
pub async fn interruption() {
    interruption_from(tokio::signal::ctrl_c()).await
}

async fn interruption_from(signal: impl std::future::Future<Output = std::io::Result<()>>) {
    if signal.await.is_err() {
        tracing::warn!("Console interrupt listener unavailable; service controls remain active");
        std::future::pending::<()>().await;
    }
}

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
    use futures_util::FutureExt;

    #[test]
    fn observed_interrupt_still_completes_the_shutdown_signal() {
        assert!(
            interruption_from(std::future::ready(Ok(())))
                .now_or_never()
                .is_some()
        );
    }

    #[test]
    fn failed_listener_does_not_request_exit_retry_or_block_explicit_control() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let attempts = AtomicUsize::new(0);
        let control = tokio::sync::Notify::new();
        let signal = async {
            attempts.fetch_add(1, Ordering::SeqCst);
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        };
        let mut lifecycle = Box::pin(async {
            tokio::select! {
                biased;
                _ = interruption_from(signal) => panic!("Listener failure requested exit"),
                _ = control.notified() => {},
            }
        });
        assert!((&mut lifecycle).now_or_never().is_none());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert!((&mut lifecycle).now_or_never().is_none());
        assert_eq!(attempts.load(Ordering::SeqCst), 1, "No listener retry loop");
        control.notify_one();
        assert!((&mut lifecycle).now_or_never().is_some());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }
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
