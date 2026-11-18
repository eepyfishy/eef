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
    detach_owned_service_console_checked().unwrap_or(false)
}

#[cfg(any(windows, test))]
fn clear_then_detach<H: Copy>(
    original: [H; 3],
    mut replace: impl FnMut(usize, Option<H>) -> std::io::Result<()>,
    detach: impl FnOnce() -> std::io::Result<()>,
) -> std::io::Result<()> {
    for index in 0..original.len() {
        if let Err(error) = replace(index, None) {
            for (slot, handle) in original.iter().copied().enumerate().take(index + 1) {
                let _ = replace(slot, Some(handle));
            }
            return Err(error);
        }
    }
    if let Err(error) = detach() {
        for (slot, handle) in original.into_iter().enumerate() {
            let _ = replace(slot, Some(handle));
        }
        return Err(error);
    }
    Ok(())
}

/// Checked service entrypoint. Clear only exclusively owned console handles
/// before detaching, so later files cannot inherit stale standard-handle slots.
/// Inherited terminals and redirected input/output are left untouched. No handle
/// is manually closed; FreeConsole owns the old console handles' lifetime.
pub fn detach_owned_service_console_checked() -> std::io::Result<bool> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::INVALID_HANDLE_VALUE,
            System::Console::{
                FreeConsole, GetConsoleMode, GetConsoleProcessList, GetStdHandle, STD_ERROR_HANDLE,
                STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
            },
        };
        let mut processes = [0u32; 2];
        // SAFETY: valid buffers and borrowed handles. We replace only this
        // process's slots after proving exclusive console ownership/no redirects.
        // No OS/file handles are allocated between clearing and FreeConsole.
        unsafe {
            let count = GetConsoleProcessList(processes.as_mut_ptr(), processes.len() as u32);
            let kinds = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE];
            let original = kinds.map(|kind| GetStdHandle(kind));
            let redirected = original.iter().any(|handle| {
                if handle.is_null() || *handle == INVALID_HANDLE_VALUE {
                    return false;
                }
                let mut mode = 0;
                GetConsoleMode(*handle, &mut mode) == 0
            });
            if !may_detach(count, processes[0], std::process::id(), redirected) {
                return Ok(false);
            }
            clear_then_detach(
                original,
                |index, handle| {
                    if SetStdHandle(kinds[index], handle.unwrap_or(std::ptr::null_mut())) == 0 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                },
                || {
                    if FreeConsole() == 0 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                },
            )?;
            Ok(true)
        }
    }
    #[cfg(not(windows))]
    {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;

    #[test]
    fn standard_slots_are_cleared_before_console_handles_can_be_reused() {
        let slots = std::cell::RefCell::new([Some(10), Some(20), Some(20)]);
        let detached = std::cell::Cell::new(false);
        clear_then_detach(
            [10, 20, 20],
            |index, handle| {
                assert!(!detached.get());
                slots.borrow_mut()[index] = handle;
                Ok(())
            },
            || {
                assert_eq!(*slots.borrow(), [None, None, None]);
                detached.set(true);
                Ok(())
            },
        )
        .unwrap();
        assert!(detached.get());
        assert_eq!(*slots.borrow(), [None, None, None]);
    }

    #[test]
    fn failed_slot_clear_restores_prior_slots_without_detaching() {
        for fail in 0..3 {
            let original = [10, 20, 20];
            let slots = std::cell::RefCell::new(original.map(Some));
            let result = clear_then_detach(
                original,
                |index, handle| {
                    if index == fail && handle.is_none() {
                        return Err(std::io::ErrorKind::PermissionDenied.into());
                    }
                    slots.borrow_mut()[index] = handle;
                    Ok(())
                },
                || panic!("Cannot detach after failed standard-slot clearing"),
            );
            assert_eq!(
                result.unwrap_err().kind(),
                std::io::ErrorKind::PermissionDenied
            );
            assert_eq!(*slots.borrow(), original.map(Some));
        }
    }

    #[test]
    fn failed_console_detach_restores_still_attached_standard_slots() {
        let original = [10, 20, 20];
        let slots = std::cell::RefCell::new(original.map(Some));
        let result = clear_then_detach(
            original,
            |index, handle| {
                slots.borrow_mut()[index] = handle;
                Ok(())
            },
            || {
                assert_eq!(*slots.borrow(), [None, None, None]);
                Err(std::io::ErrorKind::Other.into())
            },
        );
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::Other);
        assert_eq!(*slots.borrow(), original.map(Some));
    }

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
