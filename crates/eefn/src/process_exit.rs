//! Bounded owned-process handle coverage. Job accounting alone is not a process
//! wait barrier; a witness additionally needs every lifetime member represented
//! by a unique retained handle, and every such handle signalled.
use anyhow::{Context, Result, bail, ensure};
use std::{
    os::windows::io::{AsRawHandle, BorrowedHandle, OwnedHandle},
    sync::Mutex,
};
use windows_sys::Win32::{
    Foundation::{CompareObjectHandles, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        JobObjects::{
            IsProcessInJob, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
            JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList,
            QueryInformationJobObject,
        },
        Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            WaitForSingleObject,
        },
    },
};

const MAX_TRACKED: usize = 128;
#[derive(Default)]
pub(super) struct Tracker(Mutex<Vec<OwnedHandle>>);
#[repr(C)]
struct Members {
    assigned: u32,
    listed: u32,
    ids: [usize; MAX_TRACKED],
}
impl Tracker {
    fn hold(&self, job: HANDLE, process: OwnedHandle) -> Result<()> {
        let mut belongs = 0;
        // SAFETY: both handles are retained by their owners throughout this call.
        ensure!(
            unsafe { IsProcessInJob(process.as_raw_handle(), job, &mut belongs) } != 0
                && belongs != 0,
            "Observed process membership in the owned job is unconfirmed"
        );
        let mut held = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Owned process coverage unavailable"))?;
        if held.iter().any(|old| unsafe { CompareObjectHandles(old.as_raw_handle(), process.as_raw_handle()) } != 0) {
            return Ok(());
        }
        ensure!(
            held.len() < MAX_TRACKED,
            "Owned process handle coverage limit exceeded"
        );
        held.push(process);
        Ok(())
    }
    pub(super) fn hold_root(&self, job: HANDLE, process: HANDLE) -> Result<()> {
        // SAFETY: caller holds the suspended, already assigned Child. Duplicate
        // that exact handle before user code resumes; never reopen the root PID.
        let process = unsafe { BorrowedHandle::borrow_raw(process) }
            .try_clone_to_owned()
            .context("Retain owned root process identity")?;
        self.hold(job, process)
    }
    pub(super) fn capture_members(&self, job: HANDLE) -> Result<()> {
        let mut members = Members {
            assigned: 0,
            listed: 0,
            ids: [0; MAX_TRACKED],
        };
        let mut bytes = 0;
        // SAFETY: documented variable-tail layout with fixed bounded storage.
        ensure!(
            unsafe {
                QueryInformationJobObject(
                    job,
                    JobObjectBasicProcessIdList,
                    (&mut members as *mut Members).cast(),
                    std::mem::size_of_val(&members) as u32,
                    &mut bytes,
                )
            } != 0,
            "Owned job member list unavailable or exceeds the handle coverage bound"
        );
        ensure!(
            members.assigned == members.listed
                && members.listed as usize <= MAX_TRACKED
                && bytes as usize <= std::mem::size_of_val(&members)
                && bytes as usize >= 8 + members.listed as usize * std::mem::size_of::<usize>(),
            "Incomplete owned job member list"
        );
        for pid in &members.ids[..members.listed as usize] {
            let pid = u32::try_from(*pid)?;
            // This list belongs only to our exact unnamed job. A PID is only a
            // lookup hint: membership and kernel-object identity are checked
            // before retaining/waiting, never PID-based termination.
            let raw = unsafe {
                OpenProcess(
                    PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                    0,
                    pid,
                )
            };
            ensure!(!raw.is_null(), "Owned process handle could not be retained");
            // SAFETY: OpenProcess returned a new owned handle, wrapped once.
            let process = unsafe {
                <OwnedHandle as std::os::windows::io::FromRawHandle>::from_raw_handle(raw)
            };
            self.hold(job, process)?;
        }
        Ok(())
    }
    pub(super) fn observed_all(&self, job: HANDLE) -> Result<bool> {
        // A raced/missed lookup cannot be promoted into proof. The independent
        // lifetime count below must exactly match unique held identities.
        let _ = self.capture_members(job);
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        let mut bytes = 0;
        ensure!(
            unsafe {
                QueryInformationJobObject(
                    job,
                    JobObjectBasicAccountingInformation,
                    (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    std::mem::size_of_val(&accounting) as u32,
                    &mut bytes,
                )
            } != 0
                && bytes as usize == std::mem::size_of_val(&accounting),
            "Owned process lifetime accounting unavailable"
        );
        let held = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Owned process coverage unavailable"))?;
        ensure!(
            accounting.TotalProcesses as usize <= MAX_TRACKED,
            "Owned process lifetime exceeds handle coverage bound"
        );
        if accounting.ActiveProcesses != 0 {
            return Ok(false);
        }
        ensure!(
            accounting.TotalProcesses as usize == held.len() && !held.is_empty(),
            "Owned process lifetime handle coverage is incomplete; exit unconfirmed"
        );
        for process in held.iter() {
            match unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } {
                WAIT_OBJECT_0 => {}
                WAIT_TIMEOUT => return Ok(false),
                _ => bail!("Owned process exit handle observation unavailable"),
            }
        }
        Ok(true)
    }
}
