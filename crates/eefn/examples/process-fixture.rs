//! Isolated acceptance helper, never part of installed EEF/EEFN bundles.
//! Creates only its own finite child, or observes held handles without killing.
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use anyhow::{Context, bail};
    use serde::{Deserialize, Serialize};
    use std::{
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        path::PathBuf,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::{FILETIME, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::Threading::{
            GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE, WaitForSingleObject,
        },
    };
    #[derive(Serialize, Deserialize)]
    struct Identity {
        pid: u32,
        created: u64,
    }
    #[derive(Serialize, Deserialize)]
    struct Processes {
        root: Identity,
        child: Identity,
    }
    fn created(handle: HANDLE) -> anyhow::Result<u64> {
        let mut times = [FILETIME::default(); 4];
        // SAFETY: valid live process handle and four separate initialized values.
        if unsafe {
            GetProcessTimes(
                handle,
                &mut times[0],
                &mut times[1],
                &mut times[2],
                &mut times[3],
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error()).context("Read helper creation time");
        }
        Ok((u64::from(times[0].dwHighDateTime) << 32) | u64::from(times[0].dwLowDateTime))
    }
    let mut args = std::env::args_os().skip(1);
    let mode = args.next().context("helper mode required")?;
    let root = PathBuf::from(args.next().context("own fixture directory required")?);
    if !root.is_absolute() || !root.is_dir() || args.next().is_some() {
        bail!("Existing absolute isolated fixture directory required")
    }
    match mode.to_str().context("Invalid mode")? {
        "child" => {
            println!("isolated descendant holding inherited output");
            std::thread::sleep(Duration::from_secs(30));
        }
        "complete" | "fail" | "loop" => {
            let child = Command::new(std::env::current_exe()?)
                .arg("child")
                .arg(&root)
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()?;
            let processes = Processes {
                root: Identity {
                    pid: std::process::id(),
                    created: created(unsafe { GetCurrentProcess() })?,
                },
                child: Identity {
                    pid: child.id(),
                    created: created(child.as_raw_handle())?,
                },
            };
            std::fs::write(root.join("pids.json"), serde_json::to_vec(&processes)?)?;
            println!("isolated root started");
            let end = Instant::now() + Duration::from_secs(30);
            while Instant::now() < end {
                if (mode == "complete" || mode == "fail") && root.join("release").is_file() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            if mode == "fail" {
                std::process::exit(17);
            }
        }
        "observe" => {
            let processes: Processes =
                serde_json::from_slice(&std::fs::read(root.join("pids.json"))?)?;
            if processes.root.pid == processes.child.pid {
                bail!("Distinct helper identities required")
            }
            let mut handles = Vec::new();
            for identity in [&processes.root, &processes.child] {
                // SAFETY: only read/query/synchronize rights. Never terminate by PID.
                let raw = unsafe {
                    OpenProcess(
                        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                        0,
                        identity.pid,
                    )
                };
                if raw.is_null() {
                    return Err(std::io::Error::last_os_error()).context("Open own live helper");
                }
                let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
                if created(handle.as_raw_handle())? != identity.created {
                    bail!("Helper PID reused; no exit inference")
                }
                if unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } != WAIT_TIMEOUT {
                    bail!("Helper exited before observer admission")
                }
                handles.push(handle);
            }
            std::fs::write(root.join("observer-ready"), b"both live identities held")?;
            let admitted = Instant::now();
            let end = admitted + Duration::from_secs(12);
            loop {
                let mut exited = 0;
                for handle in &handles {
                    match unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } {
                        WAIT_OBJECT_0 => exited += 1,
                        WAIT_TIMEOUT => {}
                        _ => bail!("Held helper handle wait failed"),
                    }
                }
                if exited == 2 {
                    println!(
                        "{}",
                        serde_json::json!({"both_exited":true,
                        "held_creation_verified_handles":true,"observed_after_ms":admitted.elapsed().as_millis()})
                    );
                    break;
                }
                if Instant::now() >= end {
                    bail!("Own root/child still alive; no exit proof")
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        _ => bail!("Unknown helper mode"),
    }
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows-only native process acceptance helper");
    std::process::exit(1);
}
