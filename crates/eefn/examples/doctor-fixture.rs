//! Owned native probe-fault fixture; never packaged or used as a real runtime.
use anyhow::{Context, Result, bail};
use serde_json::json;
use std::{
    io::Write,
    process::{Command, Stdio},
    time::Duration,
};
fn main() -> Result<()> {
    if std::env::var_os("EEF_DOCTOR_PRIVATE_ENV_SENTINEL").is_some() {
        bail!("unexpected inherited fixture credential")
    }
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|a| a == "child") {
        std::thread::sleep(Duration::from_secs(30));
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    let dir = exe.parent().context("fixture directory")?;
    let mode = std::fs::read_to_string(dir.join("doctor-fixture-mode"))?;
    match mode.as_str() {
        "malformed" => {
            println!("PRIVATE_PROBE_TRACE not a protocol reply");
            eprintln!("PRIVATE_PROBE_TRACE hidden stderr");
        }
        "large" => {
            let data = vec![b'x'; 131072];
            std::io::stdout().write_all(&data)?;
        }
        "hang" | "inherited" => {
            let child = Command::new(&exe)
                .arg("child")
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()?;
            std::fs::write(
                dir.join("doctor-fixture-pids.json"),
                serde_json::to_vec(&json!({"root":std::process::id(),"child":child.id()}))?,
            )?;
            if mode == "hang" {
                std::thread::sleep(Duration::from_secs(30));
            } else {
                let modules: Vec<String> =
                    serde_json::from_str(args.last().context("fixture modules")?)?;
                println!(
                    "{}",
                    json!({"schema_version":1,"version":[3,11,9],"bits":64,"modules":modules.iter().map(|name|json!({"name":name,"imported":true})).collect::<Vec<_>>()})
                );
            }
        }
        _ => bail!("unsupported isolated fixture mode"),
    }
    Ok(())
}
