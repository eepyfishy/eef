//! Lifetime containment for bounded process.exec, not an application sandbox.
//! Windows children start suspended and join an unnamed non-inheritable job
//! before resuming. No uncontained fallback, breakaway or admin elevation.
use anyhow::Result;
use tokio::process::{Child, Command};

#[cfg(windows)]
pub(crate) use windows::ProcessTree;

#[cfg(not(windows))]
pub(crate) struct ProcessTree;

pub(crate) fn spawn(command: &mut Command) -> Result<(Child, ProcessTree)> {
    command.kill_on_drop(true);
    #[cfg(windows)]
    {
        windows::spawn(command)
    }
    #[cfg(not(windows))]
    {
        // Existing root-child cleanup only; no non-Windows tree guarantee.
        Ok((command.spawn()?, ProcessTree))
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use anyhow::{Context, bail};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::{ERROR_NO_MORE_FILES, GetLastError, HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
                QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
            },
            Threading::{
                CREATE_NO_WINDOW, CREATE_SUSPENDED, GetProcessIdOfThread, OpenThread, ResumeThread,
                THREAD_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
            },
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
        /// Only the owned unnamed job; no PID-based termination or shared service.
        /// Caller supplies its existing deadline, never a new cleanup allowance.
        pub(crate) async fn terminate_and_observe_until(
            &self,
            deadline: tokio::time::Instant,
        ) -> Result<()> {
            if tokio::time::Instant::now() >= deadline {
                bail!("Process tree observation deadline expired");
            }
            // SAFETY: this live handle belongs exclusively to our contained tree.
            if unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) } == 0 {
                return Err(std::io::Error::last_os_error())
                    .context("Terminate owned process tree");
            }
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
        fn new() -> Result<Self> {
            // SAFETY: null attributes/name create an unnamed, non-inheritable job.
            let job = owned(
                unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) },
                "Create bounded process job",
            )?;
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
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
            Ok(Self(job))
        }
        fn attach_and_resume(&self, child: &Child) -> Result<()> {
            let pid = child.id().context("Suspended child identity unavailable")?;
            let process = child
                .raw_handle()
                .context("Suspended child handle unavailable")?;
            // SAFETY: the live Child retains this process handle throughout this
            // synchronous method. Assignment happens before any user code runs.
            if unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), process) } == 0 {
                return Err(std::io::Error::last_os_error()).context("Contain suspended child");
            }
            // Stable Rust/Tokio do not expose the primary-thread handle. The
            // suspended child cannot spawn more threads or release/reuse its PID.
            let snapshot = owned(
                unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) },
                "Inspect only newly suspended child threads",
            )?;
            let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
            entry.dwSize = std::mem::size_of_val(&entry) as u32;
            let mut thread_id = None;
            let mut more = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) };
            while more != 0 {
                if entry.th32OwnerProcessID == pid {
                    if thread_id.replace(entry.th32ThreadID).is_some() {
                        bail!("Suspended child has ambiguous thread ownership; no execution");
                    }
                }
                entry.dwSize = std::mem::size_of_val(&entry) as u32;
                more = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) };
            }
            if unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
                bail!("Suspended child thread snapshot incomplete; no execution");
            }
            let id = thread_id.context("Suspended child's primary thread unavailable")?;
            let thread = owned(
                unsafe {
                    OpenThread(
                        THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
                        0,
                        id,
                    )
                },
                "Open newly suspended child's thread",
            )?;
            if unsafe { GetProcessIdOfThread(thread.as_raw_handle()) } != pid {
                bail!("Suspended child thread identity changed; no execution");
            }
            // SAFETY: verified thread belongs to our still-live suspended child,
            // already contained in the kill-on-close job. Never resume peers.
            if unsafe { ResumeThread(thread.as_raw_handle()) } != 1 {
                bail!("Suspended child resume count changed; terminate contained work");
            }
            Ok(())
        }
    }
    pub(super) fn spawn(command: &mut Command) -> Result<(Child, ProcessTree)> {
        spawn_checked(command, ProcessTree::attach_and_resume)
    }
    fn spawn_checked(
        command: &mut Command,
        contain: impl FnOnce(&ProcessTree, &Child) -> Result<()>,
    ) -> Result<(Child, ProcessTree)> {
        let tree = ProcessTree::new()?;
        command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
        let mut child = command
            .spawn()
            .context("Create suspended bounded command")?;
        if let Err(error) = contain(&tree, &child) {
            // No downgrade. Closing tree covers assigned work; root kill also
            // covers failed assignment while its primary thread is suspended.
            let _ = child.start_kill();
            return Err(error);
        }
        Ok((child, tree))
    }
    #[cfg(test)]
    pub(super) fn refuse_before_resume(command: &mut Command) -> Result<(Child, ProcessTree)> {
        spawn_checked(command, |_, _| {
            bail!("isolated pre-resume containment refusal")
        })
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::{
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        path::{Path, PathBuf},
        process::Stdio,
        time::Duration,
    };
    use windows_sys::Win32::{
        Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            WaitForSingleObject,
        },
    };
    const HELPER: &str = "process_lifetime::tests::process_tree_child_helper";
    const PREFIX: &str = "eef-contained-fixture:";

    fn args(mode: &str, root: &Path) -> Vec<String> {
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
    struct Processes(Vec<OwnedHandle>);
    impl Processes {
        async fn observe(root: &Path) -> Self {
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
        fn alive(&self) {
            for h in &self.0 {
                assert_eq!(
                    unsafe { WaitForSingleObject(h.as_raw_handle(), 0) },
                    WAIT_TIMEOUT
                );
            }
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
    async fn failed_containment_never_resumes_or_falls_back_to_uncontained_execution() {
        let dir = tempfile::tempdir().unwrap();
        assert!(windows::refuse_before_resume(&mut command("loop", dir.path())).is_err());
        tokio::time::sleep(Duration::from_millis(50)).await;
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
