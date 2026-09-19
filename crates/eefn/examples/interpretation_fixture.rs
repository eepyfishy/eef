//! Test-only bounded stdin adapter for evaluating the shared Rust contract.
//! Never packaged; performs no inference, downloads or requested user actions.
use anyhow::{Result, bail};
use serde::Deserialize;
use serde_json::json;
use std::io::Read;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    mode: String,
    input: String,
    #[serde(default)]
    output: String,
    allowed_capabilities: Vec<String>,
}

fn run() -> Result<serde_json::Value> {
    if std::env::var("EEF_INTERPRETATION_FIXTURE").as_deref() != Ok("1") {
        bail!("test fixture only");
    }
    let mut bytes = vec![];
    std::io::stdin().take(65537).read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        bail!("fixture input exceeds limit");
    }
    let request: Request = serde_json::from_slice(&bytes)?;
    match request.mode.as_str() {
        "prompt" => Ok(
            json!({"success":true,"messages":eefn::interpretation::messages(
            &request.input, &request.allowed_capabilities)?}),
        ),
        "validate" => Ok(
            json!({"success":true,"interpretation":eefn::interpretation::validate(
            &request.input, request.output.as_bytes(), &request.allowed_capabilities)?}),
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
