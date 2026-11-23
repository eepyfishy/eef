//! Test-only bounded method prompt/validation adapter. No model or workload runs.
use anyhow::{Result, bail};
use serde::Deserialize;
use serde_json::json;
use std::io::Read;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    mode: String,
    draft: eefn::request_drafts::DraftRecord,
    allowed_capabilities: Vec<String>,
    #[serde(default)]
    output: String,
}

fn run() -> Result<serde_json::Value> {
    if std::env::var("EEF_METHOD_FIXTURE").as_deref() != Ok("1") {
        bail!("test fixture only");
    }
    let mut bytes = vec![];
    std::io::stdin().take(65537).read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        bail!("fixture input exceeds limit");
    }
    let request: Request = serde_json::from_slice(&bytes)?;
    match request.mode.as_str() {
        "validate_task" => {
            let task: eefn::interpreter_task::Task = serde_json::from_str(&request.output)?;
            Ok(json!({"success":true,"spec":task.validate()?}))
        }
        "generation_reference" => Ok(json!({"success":true,
            "messages":eefn::request_preparation::messages(&request.draft, &request.allowed_capabilities)?,
            "schema":eefn::request_preparation::output_schema(&request.draft, &request.allowed_capabilities)?})),
        "prompt" => Ok(
            json!({"success":true,"messages":eefn::request_preparation::messages(
            &request.draft, &request.allowed_capabilities)?}),
        ),
        "validate" => Ok(
            json!({"success":true,"proposal":eefn::request_preparation::validate(
            &request.draft, &request.allowed_capabilities, request.output.as_bytes())?}),
        ),
        _ => bail!("unsupported fixture mode"),
    }
}

fn main() {
    let report = match run() {
        Ok(value) => value,
        Err(error) => json!({"success":false,"error":error.to_string()}),
    };
    println!("{report}");
    if report["success"] != true {
        std::process::exit(1);
    }
}
