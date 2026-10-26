//! Primary-thread lookup for a newly suspended, already job-contained child.
//! Capture only that child's thread IDs, never memory, contexts or other targets.
use anyhow::{Context, Result, bail};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use tokio::process::Child;
use windows_sys::Win32::{
    Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS},
    System::{
        Diagnostics::ProcessSnapshotting::{
            HPSS, HPSSWALK, PSS_CAPTURE_THREADS, PSS_THREAD_ENTRY, PSS_WALK_THREADS,
            PssCaptureSnapshot, PssFreeSnapshot, PssWalkMarkerCreate, PssWalkMarkerFree,
            PssWalkSnapshot,
        },
        Threading::{
            GetCurrentProcess, GetProcessIdOfThread, OpenThread, ResumeThread,
            THREAD_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
        },
    },
};

// These APIs return Win32 error codes directly, not via GetLastError.
fn status(code: u32, message: &'static str) -> Result<()> {
    if code != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(code as i32)).context(message);
    }
    Ok(())
}

struct Snapshot(HPSS);
impl Snapshot {
    fn close(mut self) -> Result<()> {
        let handle = std::mem::replace(&mut self.0, std::ptr::null_mut());
        // Captured locally: the containing process is ours, not the target child.
        status(
            unsafe { PssFreeSnapshot(GetCurrentProcess(), handle) },
            "Free owned child thread snapshot",
        )
    }
}
impl Drop for Snapshot {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // Best effort on a prior refusal/unwind, never CloseHandle(HPSS).
            unsafe { PssFreeSnapshot(GetCurrentProcess(), self.0) };
        }
    }
}
struct Marker(HPSSWALK);
impl Marker {
    fn close(mut self) -> Result<()> {
        let handle = std::mem::replace(&mut self.0, std::ptr::null_mut());
        status(
            unsafe { PssWalkMarkerFree(handle) },
            "Free owned child thread walk marker",
        )
    }
}
impl Drop for Marker {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { PssWalkMarkerFree(self.0) };
        }
    }
}

fn unique_thread(pid: u32, mut next: impl FnMut() -> Result<Option<(u32, u32)>>) -> Result<u32> {
    let (owner, id) = next()?.context("Suspended child's primary thread unavailable")?;
    if owner != pid || id == 0 {
        bail!("Suspended child thread snapshot has invalid ownership; no execution");
    }
    // A successful second read must confirm enumeration is complete. A partial
    // snapshot/error is not evidence that the first thread was the only one.
    if next()?.is_some() {
        bail!("Suspended child has ambiguous thread ownership; no execution");
    }
    Ok(id)
}

pub(super) fn resume(child: &Child, pid: u32) -> Result<()> {
    if child.id() != Some(pid) {
        bail!("Suspended child identity changed; no execution");
    }
    let process = child
        .raw_handle()
        .context("Suspended child handle unavailable")?;
    let mut raw = std::ptr::null_mut();
    // SAFETY: Child owns a live process handle. Only thread IDs are captured;
    // no address-space clone, memory, handles, contexts or remote duplication.
    status(
        unsafe { PssCaptureSnapshot(process, PSS_CAPTURE_THREADS, 0, &mut raw) },
        "Capture only newly suspended child threads",
    )?;
    if raw.is_null() {
        bail!("Owned child snapshot returned no handle; no execution");
    }
    let snapshot = Snapshot(raw);
    let mut raw_marker = std::ptr::null_mut();
    status(
        unsafe { PssWalkMarkerCreate(std::ptr::null(), &mut raw_marker) },
        "Create owned child thread walk marker",
    )?;
    if raw_marker.is_null() {
        bail!("Owned child walk returned no marker; no execution");
    }
    let marker = Marker(raw_marker);
    let id = unique_thread(pid, || {
        let mut entry = PSS_THREAD_ENTRY::default();
        let code = unsafe {
            PssWalkSnapshot(
                snapshot.0,
                PSS_WALK_THREADS,
                marker.0,
                (&mut entry as *mut PSS_THREAD_ENTRY).cast(),
                std::mem::size_of_val(&entry) as u32,
            )
        };
        if code == ERROR_NO_MORE_ITEMS {
            return Ok(None);
        }
        status(code, "Read complete owned child thread snapshot")?;
        Ok(Some((entry.ProcessId, entry.ThreadId)))
    })?;
    // Verify normal cleanup before any execution. Prior errors drop both owners.
    marker.close()?;
    snapshot.close()?;
    let raw_thread = unsafe {
        OpenThread(
            THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
            0,
            id,
        )
    };
    if raw_thread.is_null() {
        return Err(std::io::Error::last_os_error()).context("Open newly suspended child's thread");
    }
    // SAFETY: OpenThread returned this newly owned handle.
    let thread = unsafe { OwnedHandle::from_raw_handle(raw_thread) };
    if unsafe { GetProcessIdOfThread(thread.as_raw_handle()) } != pid {
        bail!("Suspended child thread identity changed; no execution");
    }
    // SAFETY: verified thread of our still-live, suspended, job-contained child.
    // No other process is resumed. A changed count is a refusal, never a retry.
    if unsafe { ResumeThread(thread.as_raw_handle()) } != 1 {
        bail!("Suspended child resume count changed; terminate contained work");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn select(items: Vec<(u32, u32)>) -> Result<u32> {
        let mut items = items.into_iter();
        unique_thread(10, || Ok(items.next()))
    }
    #[test]
    fn accepts_only_one_complete_owned_thread() {
        assert_eq!(select(vec![(10, 20)]).unwrap(), 20);
    }
    #[test]
    fn rejects_empty_or_zero_thread() {
        assert!(select(vec![]).is_err());
        assert!(select(vec![(10, 0)]).is_err());
    }
    #[test]
    fn rejects_foreign_or_ambiguous_threads() {
        assert!(select(vec![(11, 20)]).is_err());
        assert!(select(vec![(10, 20), (10, 21)]).is_err());
        assert!(select(vec![(10, 20), (10, 20)]).is_err());
        assert!(select(vec![(10, 20), (11, 21)]).is_err());
    }
    #[test]
    fn rejects_initial_and_terminal_walk_failures() {
        assert!(unique_thread(10, || bail!("initial failure")).is_err());
        let mut first = true;
        assert!(
            unique_thread(10, || {
                if std::mem::replace(&mut first, false) {
                    Ok(Some((10, 20)))
                } else {
                    bail!("partial walk failure")
                }
            })
            .is_err()
        );
    }
}
