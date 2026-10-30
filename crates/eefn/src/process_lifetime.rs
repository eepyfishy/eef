//! Lifetime containment for bounded process.exec, not an application sandbox.
//! Windows children start suspended and join an unnamed non-inheritable job
//! before resuming. No uncontained fallback, breakaway or admin elevation.
use anyhow::Result;
use tokio::process::{Child, Command};

#[cfg(windows)]
#[path = "process_thread.rs"]
mod process_thread;

#[cfg(windows)]
pub(crate) use windows::ProcessTree;

#[cfg(not(windows))]
pub(crate) struct ProcessTree;

#[cfg(not(windows))]
impl ProcessTree {
    pub(crate) fn accounting(&self) -> crate::job_accounting::JobAccounting {
        crate::job_accounting::JobAccounting::Unsupported {}
    }
}

#[cfg(test)]
pub(crate) fn spawn(command: &mut Command) -> Result<(Child, ProcessTree)> {
    spawn_with_job_memory_limit(command, None)
}

pub(crate) fn spawn_with_job_memory_limit(
    command: &mut Command,
    limit_mib: Option<u64>,
) -> Result<(Child, ProcessTree)> {
    spawn_limited(command, limit_mib, None)
}

#[derive(Debug, thiserror::Error)]
#[error("owned process start deadline elapsed")]
pub(crate) struct StartDeadlineElapsed;

pub(crate) fn intersect_deadline(
    local: tokio::time::Instant,
    parent: Option<tokio::time::Instant>,
) -> tokio::time::Instant {
    parent.map_or(local, |parent| parent.min(local))
}

fn check_start_deadline(deadline: Option<tokio::time::Instant>) -> Result<()> {
    if deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
        return Err(StartDeadlineElapsed.into());
    }
    Ok(())
}

/// Intersect with an existing absolute budget; never start a fresh time window.
pub(crate) fn spawn_with_job_memory_limit_before(
    command: &mut Command,
    limit_mib: Option<u64>,
    deadline: tokio::time::Instant,
) -> Result<(Child, ProcessTree)> {
    spawn_limited(command, limit_mib, Some(deadline))
}

fn spawn_limited(
    command: &mut Command,
    limit_mib: Option<u64>,
    deadline: Option<tokio::time::Instant>,
) -> Result<(Child, ProcessTree)> {
    let limit = crate::model_memory::bytes(limit_mib)?;
    check_start_deadline(deadline)?;
    command.kill_on_drop(true);
    #[cfg(windows)]
    {
        windows::spawn(command, limit, deadline)
    }
    #[cfg(not(windows))]
    {
        let _ = limit;
        // A requested limit was rejected before spawn. No non-Windows tree guarantee.
        check_start_deadline(deadline)?;
        Ok((command.spawn()?, ProcessTree))
    }
}

#[cfg(test)]
mod start_deadline_tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn deadline_intersection_never_extends_either_bound() {
        let local = tokio::time::Instant::now() + Duration::from_secs(60);
        let earlier = local - Duration::from_secs(10);
        let later = local + Duration::from_secs(10);
        assert_eq!(intersect_deadline(local, None), local);
        assert_eq!(intersect_deadline(local, Some(earlier)), earlier);
        assert_eq!(intersect_deadline(local, Some(later)), local);
    }
    #[tokio::test]
    async fn expired_start_refuses_before_even_spawning_a_missing_program() {
        let mut command = Command::new("PRIVATE-program-must-never-start");
        let error = match spawn_with_job_memory_limit_before(
            &mut command,
            None,
            tokio::time::Instant::now(),
        ) {
            Ok(_) => panic!("Expired start must refuse"),
            Err(error) => error,
        };
        assert!(error.is::<StartDeadlineElapsed>());
        assert_eq!(error.to_string(), "owned process start deadline elapsed");
        assert!(!error.to_string().contains("PRIVATE"));
        assert!(check_start_deadline(None).is_ok());
        assert!(
            check_start_deadline(Some(tokio::time::Instant::now() + Duration::from_secs(60)))
                .is_ok()
        );
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use anyhow::{Context, bail};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::{HANDLE, INVALID_HANDLE_VALUE},
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_JOB_MEMORY,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
                JobObjectExtendedLimitInformation, QueryInformationJobObject,
                SetInformationJobObject, TerminateJobObject,
            },
            Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED},
        },
    };

    pub(crate) struct ProcessTree(OwnedHandle);
    fn owned(raw: HANDLE, operation: &str) -> Result<OwnedHandle> {
        if raw.is_null() || raw == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error()).context(operation.to_owned());
        }
        // SAFETY: each caller supplies a newly returned owned Win32 handle.
        Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
    }
    impl ProcessTree {
        pub(crate) fn request_termination(&self) -> Result<()> {
            // SAFETY: this live handle belongs exclusively to our contained tree.
            if unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) } == 0 {
                return Err(std::io::Error::last_os_error())
                    .context("Terminate owned process tree");
            }
            Ok(())
        }
        /// Query only this owned handle. These two observations are not atomic,
        /// a process-liveness guarantee, working-set usage or a permission.
        pub(crate) fn accounting(&self) -> crate::job_accounting::JobAccounting {
            self.accounting_checked()
                .unwrap_or(crate::job_accounting::JobAccounting::Unavailable {})
        }
        fn accounting_checked(&self) -> Result<crate::job_accounting::JobAccounting> {
            let memory = self.memory_information()?;
            let mut cpu = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            let mut bytes = 0;
            // SAFETY: owned live job and correctly sized initialized output buffer.
            if unsafe {
                QueryInformationJobObject(
                    self.0.as_raw_handle(),
                    JobObjectBasicAccountingInformation,
                    (&mut cpu as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    std::mem::size_of_val(&cpu) as u32,
                    &mut bytes,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error())
                    .context("Observe owned job CPU/process accounting");
            }
            if bytes as usize != std::mem::size_of_val(&cpu) {
                bail!("Owned job accounting response length changed")
            }
            let sample = crate::job_accounting::JobAccounting::Available {
                job_commit_limit_bytes: (memory.BasicLimitInformation.LimitFlags
                    & JOB_OBJECT_LIMIT_JOB_MEMORY
                    != 0)
                    .then_some(memory.JobMemoryLimit as u64),
                peak_job_commit_bytes: memory.PeakJobMemoryUsed as u64,
                active_processes: cpu.ActiveProcesses,
                total_user_cpu_100ns: u64::try_from(cpu.TotalUserTime)
                    .context("Invalid owned-job user CPU time")?,
                total_kernel_cpu_100ns: u64::try_from(cpu.TotalKernelTime)
                    .context("Invalid owned-job kernel CPU time")?,
            };
            if !sample.validate() {
                bail!("Invalid owned job accounting limit")
            }
            Ok(sample)
        }
        /// Only the owned unnamed job; no PID-based termination or shared service.
        /// Caller supplies its existing deadline, never a new cleanup allowance.
        pub(crate) async fn terminate_and_observe_until(
            &self,
            deadline: tokio::time::Instant,
        ) -> Result<()> {
            if tokio::time::Instant::now() >= deadline {
                bail!("Process tree observation deadline expired");
            }
            self.request_termination()?;
            self.observe_empty_until(deadline).await
        }
        /// Read-only cleanup observation of this owned job. It issues no new
        /// execution/termination request and grants no additional source time.
        pub(crate) async fn observe_empty_until(
            &self,
            deadline: tokio::time::Instant,
        ) -> Result<()> {
            loop {
                if tokio::time::Instant::now() >= deadline {
                    bail!("Owned process tree exit unconfirmed before deadline");
                }
                let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
                let mut bytes = 0u32;
                // SAFETY: exact owned job, initialized correctly sized output buffer.
                if unsafe {
                    QueryInformationJobObject(
                        self.0.as_raw_handle(),
                        JobObjectBasicAccountingInformation,
                        (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                        std::mem::size_of_val(&accounting) as u32,
                        &mut bytes,
                    )
                } == 0
                {
                    return Err(std::io::Error::last_os_error())
                        .context("Observe owned process tree accounting");
                }
                if bytes as usize != std::mem::size_of_val(&accounting) {
                    bail!("Owned process tree accounting length changed");
                }
                if tokio::time::Instant::now() >= deadline {
                    bail!("Owned process tree observation completed after deadline");
                }
                if accounting.ActiveProcesses == 0 {
                    return Ok(());
                }
                tokio::time::sleep_until(
                    deadline
                        .min(tokio::time::Instant::now() + std::time::Duration::from_millis(10)),
                )
                .await;
            }
        }
        fn new(memory_bytes: Option<usize>) -> Result<Self> {
            // SAFETY: null attributes/name create an unnamed, non-inheritable job.
            let job = owned(
                unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) },
                "Create bounded process job",
            )?;
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if let Some(bytes) = memory_bytes {
                limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
                limits.JobMemoryLimit = bytes;
            }
            // SAFETY: live owned handle and correctly sized initialized limits.
            if unsafe {
                SetInformationJobObject(
                    job.as_raw_handle(),
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&limits) as u32,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error()).context("Set process tree lifetime");
            }
            let tree = Self(job);
            let actual = tree.memory_information()?;
            if actual.BasicLimitInformation.LimitFlags != limits.BasicLimitInformation.LimitFlags
                || actual.JobMemoryLimit != limits.JobMemoryLimit
            {
                bail!("Owned job limits not confirmed before spawn; no unlimited fallback")
            }
            Ok(tree)
        }
        pub(super) fn memory_information(&self) -> Result<JOBOBJECT_EXTENDED_LIMIT_INFORMATION> {
            let mut information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            let mut bytes = 0;
            // SAFETY: owned live job; correctly sized initialized output buffer.
            if unsafe {
                QueryInformationJobObject(
                    self.0.as_raw_handle(),
                    JobObjectExtendedLimitInformation,
                    (&mut information as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&information) as u32,
                    &mut bytes,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error()).context("Read back owned job limits");
            }
            if bytes as usize != std::mem::size_of_val(&information) {
                bail!("Owned job limits response length changed")
            }
            Ok(information)
        }
        fn attach_and_resume(
            &self,
            child: &Child,
            before_resume: impl FnOnce(&OwnedHandle) -> Result<()>,
        ) -> Result<()> {
            let pid = child.id().context("Suspended child identity unavailable")?;
            let process = child
                .raw_handle()
                .context("Suspended child handle unavailable")?;
            // SAFETY: the live Child retains this process handle throughout this
            // synchronous method. Assignment happens before any user code runs.
            if unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), process) } == 0 {
                return Err(std::io::Error::last_os_error()).context("Contain suspended child");
            }
            // The live, suspended child is already contained. Enumerate only
            // its thread IDs, not all threads on the PC. No fallback on refusal.
            super::process_thread::resume(child, pid, before_resume)
        }
    }
    pub(super) fn spawn(
        command: &mut Command,
        memory_bytes: Option<usize>,
        deadline: Option<tokio::time::Instant>,
    ) -> Result<(Child, ProcessTree)> {
        spawn_checked(command, memory_bytes, |tree, child| {
            tree.attach_and_resume(child, |_| check_start_deadline(deadline))
        })
    }
    fn spawn_checked(
        command: &mut Command,
        memory_bytes: Option<usize>,
        contain: impl FnOnce(&ProcessTree, &Child) -> Result<()>,
    ) -> Result<(Child, ProcessTree)> {
        let started = std::time::Instant::now();
        let tree = ProcessTree::new(memory_bytes)?;
        let prepared = std::time::Instant::now();
        command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
        let mut child = command
            .spawn()
            .context("Create suspended bounded command")?;
        let created = std::time::Instant::now();
        let contained = contain(&tree, &child);
        // Optional bounded diagnostics, never command/handle/PID/error contents.
        tracing::debug!(
            prepare_ms = prepared.duration_since(started).as_millis(),
            create_ms = created.duration_since(prepared).as_millis(),
            contain_resume_ms = created.elapsed().as_millis(),
            containment_confirmed = contained.is_ok(),
            "Owned Windows process startup timing"
        );
        if let Err(error) = contained {
            // No downgrade. Closing tree covers assigned work; root kill also
            // covers failed assignment while its primary thread is suspended.
            let _ = child.start_kill();
            return Err(error);
        }
        Ok((child, tree))
    }
    #[cfg(test)]
    pub(super) fn refuse_before_resume(command: &mut Command) -> Result<(Child, ProcessTree)> {
        spawn_checked(command, None, |_, _| {
            bail!("isolated pre-resume containment refusal")
        })
    }
    #[cfg(test)]
    pub(super) fn inspect_final_gate(
        command: &mut Command,
        inspect: impl FnOnce(&Child, &OwnedHandle) -> Result<()>,
    ) -> Result<(Child, ProcessTree)> {
        spawn_checked(command, None, |tree, child| {
            tree.attach_and_resume(child, |thread| inspect(child, thread))
        })
    }
}

#[cfg(all(test, windows))]
pub(crate) mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::{
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        path::{Path, PathBuf},
        process::Stdio,
        time::Duration,
    };
    use windows_sys::Win32::{
        Foundation::{GetLastError, WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc, VirtualFree,
        },
        System::Threading::{
            GetProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            ResumeThread, SuspendThread, WaitForSingleObject,
        },
    };
    const HELPER: &str = "process_lifetime::tests::process_tree_child_helper";
    const PREFIX: &str = "eef-contained-fixture:";

    pub(crate) fn args(mode: &str, root: &Path) -> Vec<String> {
        // A recognized libtest skip filter carries test-only data without global
        // environment edits, shell quoting, or a shipping executable backdoor.
        vec![
            "--exact".into(),
            HELPER.into(),
            "--nocapture".into(),
            "--test-threads=1".into(),
            "--skip".into(),
            format!("{PREFIX}{}", json!([mode, root])),
        ]
    }
    #[test]
    fn process_tree_child_helper() {
        let Some(raw) =
            std::env::args().find_map(|arg| arg.strip_prefix(PREFIX).map(str::to_owned))
        else {
            return;
        };
        let (mode, root): (String, PathBuf) = serde_json::from_str(&raw).unwrap();
        if mode == "memory-root" {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(args("memory-leaf", &root))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        if mode == "memory-leaf" {
            // Controlled fixture allocation only; never touch all reserved pages.
            let bytes = 256 * 1024 * 1024;
            let allocation = unsafe {
                VirtualAlloc(
                    std::ptr::null(),
                    bytes,
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_READWRITE,
                )
            };
            let granted = !allocation.is_null();
            let error = if granted {
                0
            } else {
                unsafe { GetLastError() }
            };
            if granted {
                assert_ne!(unsafe { VirtualFree(allocation, 0, MEM_RELEASE) }, 0);
            }
            std::fs::write(
                root.join("memory.json"),
                serde_json::to_vec(
                    &json!({"granted":granted,"requested_bytes":bytes,"error":error}),
                )
                .unwrap(),
            )
            .unwrap();
            return;
        }
        if mode != "child" {
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(args("child", &root))
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            std::fs::write(
                root.join("pids.json"),
                serde_json::to_vec(&json!({
                "root":std::process::id(), "child":child.id()}))
                .unwrap(),
            )
            .unwrap();
        }
        // Finite fallback even if a test fails; these are only our own children.
        let end = std::time::Instant::now() + Duration::from_secs(30);
        while std::time::Instant::now() < end {
            if (mode == "exit" || mode == "fail") && root.join("release").is_file() {
                if mode == "fail" {
                    std::process::exit(17);
                }
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn command(mode: &str, root: &Path) -> Command {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(args(mode, root))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd
    }
    pub(crate) struct Processes(Vec<OwnedHandle>);
    impl Processes {
        pub(crate) async fn observe(root: &Path) -> Self {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if let Ok(bytes) = std::fs::read(root.join("pids.json")) {
                        if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                            let mut handles = Vec::new();
                            for key in ["root", "child"] {
                                let pid: u32 = value[key].as_u64().unwrap().try_into().unwrap();
                                // Keep real handles, never later kill/wait a reused PID.
                                let raw = unsafe {
                                    OpenProcess(
                                        PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                                        0,
                                        pid,
                                    )
                                };
                                assert!(
                                    !raw.is_null(),
                                    "own helper process {key} disappeared before observation"
                                );
                                handles.push(unsafe { OwnedHandle::from_raw_handle(raw) });
                            }
                            return Self(handles);
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("own root/child helpers must start")
        }
        pub(crate) fn alive(&self) {
            for h in &self.0 {
                assert_eq!(
                    unsafe { WaitForSingleObject(h.as_raw_handle(), 0) },
                    WAIT_TIMEOUT
                );
            }
        }
        pub(crate) fn exited_now(&self) -> bool {
            self.0
                .iter()
                .all(|h| unsafe { WaitForSingleObject(h.as_raw_handle(), 0) } == WAIT_OBJECT_0)
        }
        async fn exited(&self) {
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    if self.0.iter().all(
                        |h| unsafe { WaitForSingleObject(h.as_raw_handle(), 0) } == WAIT_OBJECT_0,
                    ) {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("own held root AND descendant handles must observe exit");
        }
    }
    fn engine(root: &Path, seconds: u64) -> crate::NodeEngine {
        let mut policy = crate::NodePolicy::default();
        policy.process.enabled = true;
        policy.process.allowed_executables = vec![
            std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        ];
        policy.process.max_seconds = seconds;
        crate::NodeEngine::new(false, vec![], root.into())
            .unwrap()
            .with_policy(policy)
            .unwrap()
    }
    fn params(mode: &str, root: &Path) -> Value {
        json!({"program":std::env::current_exe().unwrap(), "args":args(mode, root)})
    }
    #[tokio::test]
    async fn accounting_observes_only_the_owned_job_without_retiring_it() {
        let dir = tempfile::tempdir().unwrap();
        let (mut child, tree) =
            spawn_with_job_memory_limit(&mut command("loop", dir.path()), Some(128)).unwrap();
        let processes = Processes::observe(dir.path()).await;
        processes.alive();
        for _ in 0..2 {
            let crate::job_accounting::JobAccounting::Available {
                job_commit_limit_bytes,
                peak_job_commit_bytes,
                active_processes,
                ..
            } = tree.accounting()
            else {
                panic!("Owned job accounting unavailable")
            };
            assert_eq!(job_commit_limit_bytes, Some(128 * 1024 * 1024));
            assert!(peak_job_commit_bytes > 0);
            assert!(
                active_processes >= 2,
                "Job accounting includes at least the two held live fixture processes"
            );
            processes.alive();
        }
        drop(tree);
        let _ = child.wait().await;
        processes.exited().await;
    }
    #[tokio::test]
    async fn job_commit_ceiling_is_read_back_and_enforced_on_descendants() {
        for (limit, expected) in [(None, true), (Some(128), false)] {
            let dir = tempfile::tempdir().unwrap();
            let (mut child, tree) =
                spawn_with_job_memory_limit(&mut command("memory-root", dir.path()), limit)
                    .unwrap();
            let information = tree.memory_information().unwrap();
            assert_eq!(
                information.JobMemoryLimit,
                limit.map(|mib| mib as usize * 1024 * 1024).unwrap_or(0)
            );
            assert!(
                tokio::time::timeout(Duration::from_secs(10), child.wait())
                    .await
                    .unwrap()
                    .unwrap()
                    .success()
            );
            let result: Value =
                serde_json::from_slice(&std::fs::read(dir.path().join("memory.json")).unwrap())
                    .unwrap();
            assert_eq!(result["granted"], expected);
            if !expected {
                assert_ne!(result["error"], 0);
            }
        }
    }
    #[tokio::test]
    async fn invalid_job_commit_ceiling_never_starts_the_root() {
        let dir = tempfile::tempdir().unwrap();
        for limit in [0, 63, 1_048_577] {
            assert!(
                spawn_with_job_memory_limit(&mut command("memory-root", dir.path()), Some(limit))
                    .is_err()
            );
            assert!(!dir.path().join("memory.json").exists());
        }
    }
    #[tokio::test]
    async fn failed_containment_never_resumes_or_falls_back_to_uncontained_execution() {
        let dir = tempfile::tempdir().unwrap();
        assert!(windows::refuse_before_resume(&mut command("loop", dir.path())).is_err());
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!dir.path().join("pids.json").exists());
    }
    fn still_initially_suspended(thread: &OwnedHandle) {
        // Only the verified primary thread of our owned, not-yet-started child.
        // Restore its original count before either allowing or refusing resume.
        let count = unsafe { SuspendThread(thread.as_raw_handle()) };
        assert_ne!(count, u32::MAX);
        let restored = unsafe { ResumeThread(thread.as_raw_handle()) };
        assert_eq!(restored, count + 1);
        assert_eq!(count, 1, "Final gate must precede the original resume");
    }
    #[tokio::test]
    async fn expired_final_gate_keeps_contained_root_suspended_and_observes_its_exit() {
        let dir = tempfile::tempdir().unwrap();
        let mut held = None;
        let mut checks = 0;
        let result =
            windows::inspect_final_gate(&mut command("loop", dir.path()), |child, thread| {
                checks += 1;
                still_initially_suspended(thread);
                let pid = child.id().unwrap();
                let raw = unsafe {
                    OpenProcess(
                        PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                        0,
                        pid,
                    )
                };
                assert!(!raw.is_null());
                let observer = unsafe { OwnedHandle::from_raw_handle(raw) };
                assert_eq!(unsafe { GetProcessId(observer.as_raw_handle()) }, pid);
                held = Some(observer);
                // Deterministic final-gate expiry, not a sleep-dependent start race.
                check_start_deadline(Some(tokio::time::Instant::now()))
            });
        let error = match result {
            Ok(_) => panic!("Expired final gate resumed work"),
            Err(error) => error,
        };
        assert!(error.is::<StartDeadlineElapsed>());
        assert_eq!(checks, 1);
        let held = held.unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if unsafe { WaitForSingleObject(held.as_raw_handle(), 0) } == WAIT_OBJECT_0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Our refused root must exit through its retained handle");
        assert!(!dir.path().join("pids.json").exists());
    }
    #[tokio::test]
    async fn final_gate_runs_once_before_the_allowed_primary_thread_is_resumed() {
        let dir = tempfile::tempdir().unwrap();
        let mut checks = 0;
        let (mut child, tree) =
            windows::inspect_final_gate(&mut command("loop", dir.path()), |_, thread| {
                checks += 1;
                still_initially_suspended(thread);
                check_start_deadline(None)
            })
            .unwrap();
        assert_eq!(checks, 1);
        let processes = Processes::observe(dir.path()).await;
        processes.alive();
        drop(tree);
        processes.exited().await;
        child.wait().await.unwrap();
    }
    #[tokio::test]
    async fn engine_parent_deadline_is_not_replaced_by_a_fresh_local_process_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let runner = engine(dir.path(), 30);
        let error = runner
            .execute_with_python_output_limit(
                "process.exec",
                "run",
                params("loop", dir.path()),
                None,
                Some(tokio::time::Instant::now()),
            )
            .await
            .unwrap_err();
        assert!(error.is::<StartDeadlineElapsed>());
        assert!(!dir.path().join("pids.json").exists());
    }
    #[tokio::test]
    async fn explicit_owned_tree_termination_observes_empty_job_with_live_observer_handles() {
        let one = tempfile::tempdir().unwrap();
        let two = tempfile::tempdir().unwrap();
        let (mut root1, tree1) = spawn(&mut command("loop", one.path())).unwrap();
        let (mut root2, tree2) = spawn(&mut command("loop", two.path())).unwrap();
        let p1 = Processes::observe(one.path()).await;
        let p2 = Processes::observe(two.path()).await;
        p1.alive();
        p2.alive();
        tree1
            .terminate_and_observe_until(tokio::time::Instant::now() + Duration::from_secs(3))
            .await
            .unwrap();
        p1.exited().await;
        p2.alive();
        root1.wait().await.unwrap();
        drop(tree2);
        p2.exited().await;
        root2.wait().await.unwrap();
    }

    #[tokio::test]
    async fn expired_observation_deadline_does_not_terminate_a_live_owned_tree() {
        let dir = tempfile::tempdir().unwrap();
        let (mut child, tree) = spawn(&mut command("loop", dir.path())).unwrap();
        let processes = Processes::observe(dir.path()).await;
        assert!(
            tree.terminate_and_observe_until(tokio::time::Instant::now())
                .await
                .is_err()
        );
        processes.alive();
        drop(tree);
        processes.exited().await;
        child.wait().await.unwrap();
    }

    #[tokio::test]
    async fn dropping_one_tree_terminates_its_children_without_touching_another_tree() {
        let one = tempfile::tempdir().unwrap();
        let two = tempfile::tempdir().unwrap();
        let (mut root1, tree1) = spawn(&mut command("loop", one.path())).unwrap();
        let (mut root2, tree2) = spawn(&mut command("loop", two.path())).unwrap();
        let p1 = Processes::observe(one.path()).await;
        let p2 = Processes::observe(two.path()).await;
        p1.alive();
        p2.alive();
        drop(tree1);
        p1.exited().await;
        p2.alive();
        drop(tree2);
        p2.exited().await;
        root1.wait().await.unwrap();
        root2.wait().await.unwrap();
    }
    #[tokio::test]
    async fn completed_command_closes_descendant_pipes_without_detached_readers() {
        let dir = tempfile::tempdir().unwrap();
        let runner = engine(dir.path(), 10);
        let input = params("exit", dir.path());
        let task = tokio::spawn(async move { runner.execute("process.exec", "run", input).await });
        let processes = Processes::observe(dir.path()).await;
        processes.alive();
        std::fs::write(dir.path().join("release"), b"own helper may exit").unwrap();
        let value = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(value["success"], true);
        assert_eq!(value["process_tree"]["empty_observed"], true);
        assert_eq!(value["process_tree"]["mode"], "windows_owned_job");
        assert_eq!(
            value["process_tree"]["external_effects_stop_confirmed"],
            false
        );
        processes.exited().await;
    }
    #[tokio::test]
    async fn process_timeout_terminates_its_root_and_descendant() {
        let dir = tempfile::tempdir().unwrap();
        let runner = engine(dir.path(), 2);
        let input = params("loop", dir.path());
        let task = tokio::spawn(async move { runner.execute("process.exec", "run", input).await });
        let processes = Processes::observe(dir.path()).await;
        let error = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(error.to_string().contains("timeout"));
        processes.exited().await;
    }
    #[tokio::test]
    async fn cancelled_operation_drops_readers_and_terminates_its_root_and_descendant() {
        let dir = tempfile::tempdir().unwrap();
        let runner = engine(dir.path(), 30);
        let input = params("loop", dir.path());
        let task = tokio::spawn(async move { runner.execute("process.exec", "run", input).await });
        let processes = Processes::observe(dir.path()).await;
        processes.alive();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        processes.exited().await;
    }
    #[tokio::test]
    async fn completed_failed_process_retains_unsuccessful_report_and_denies_replay() {
        let dir = tempfile::tempdir().unwrap();
        let runner = engine(dir.path(), 30);
        let mut grant = crate::grants::tests::grant();
        grant.capability = "process.exec".into();
        grant.action = "run".into();
        grant.params = params("fail", dir.path());
        let ledger = crate::grants::GrantLedger::open(
            &dir.path().join("receipts.db"),
            Default::default(),
            "owner",
        )
        .unwrap();
        let accepted = crate::grants::AcceptedGrant::prepare(
            grant.clone(),
            ledger.clone(),
            &runner,
            &grant.issuer_id,
            &grant.session_id,
            "owner",
        )
        .unwrap();
        let task = tokio::spawn(async move { accepted.run(&runner).await });
        let processes = Processes::observe(dir.path()).await;
        processes.alive();
        std::fs::write(dir.path().join("release"), b"own failed helper may exit").unwrap();
        let report = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(report.state, crate::grants::ReceiptState::Completed);
        assert!(!report.success);
        assert!(report.persisted);
        assert!(report.confirms(&grant));
        assert_eq!(report.data["success"], false);
        assert_eq!(report.data["exit_code"], 17);
        assert_eq!(report.data["process_tree"]["empty_observed"], true);
        assert_eq!(
            report.error_code.as_deref(),
            Some("process_exit_unsuccessful")
        );
        processes.exited().await;
        let saved = ledger
            .result(
                &grant.issuer_id,
                &grant.grant_id,
                &crate::grants::definition_digest(&json!(grant)).unwrap(),
            )
            .unwrap();
        assert_eq!(saved["report"], json!(report));
        let runner = engine(dir.path(), 30);
        assert!(
            crate::grants::AcceptedGrant::prepare(
                grant.clone(),
                ledger,
                &runner,
                &grant.issuer_id,
                &grant.session_id,
                "owner"
            )
            .is_err()
        );
    }
    #[tokio::test]
    async fn granted_deadline_terminates_tree_but_retains_unknown_effects_and_denies_replay() {
        let dir = tempfile::tempdir().unwrap();
        let runner = engine(dir.path(), 30);
        let mut grant = crate::grants::tests::grant();
        grant.capability = "process.exec".into();
        grant.action = "run".into();
        grant.params = params("loop", dir.path());
        grant.limits.max_execution_ms = 2000;
        let ledger = crate::grants::GrantLedger::open(
            &dir.path().join("receipts.db"),
            Default::default(),
            "owner",
        )
        .unwrap();
        let accepted = crate::grants::AcceptedGrant::prepare(
            grant.clone(),
            ledger.clone(),
            &runner,
            &grant.issuer_id,
            &grant.session_id,
            "owner",
        )
        .unwrap();
        let task = tokio::spawn(async move { accepted.run(&runner).await });
        let processes = Processes::observe(dir.path()).await;
        let report = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(report.state, crate::grants::ReceiptState::Unknown);
        assert!(!report.success);
        assert!(report.persisted);
        assert!(report.confirms(&grant));
        processes.exited().await;
        let fresh_runner = engine(dir.path(), 30);
        assert!(
            crate::grants::AcceptedGrant::prepare(
                grant.clone(),
                ledger,
                &fresh_runner,
                &grant.issuer_id,
                &grant.session_id,
                "owner"
            )
            .is_err()
        );
    }
}
