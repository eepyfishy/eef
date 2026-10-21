//! Isolated package-setup policy/lifecycle adapter, never shipped. Its synthetic
//! stable context is not a real stable binary, publisher or installer acceptance.
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{io::Read, path::PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema_version: u32,
    program: String,
    executable: PathBuf,
    action: String,
    mode: Option<eefn::package_setup::Mode>,
    source: Option<String>,
    expected_revision: Option<u64>,
    operation_id: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut bytes = vec![];
    std::io::stdin()
        .take(16 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 16 * 1024, "Fixture input bound");
    let input: Input = serde_json::from_slice(&bytes)?;
    ensure!(input.schema_version == 1, "Fixture schema");
    let ctx =
        eefn::package_setup::Context::for_executable(input.executable, &input.program, "1.0.0")?;
    use eefn::package_setup::{Action, Mode};
    let action = match input.action.as_str() {
        "show" => Action::Show,
        "run" => Action::Run,
        "configure" => {
            let mode = input.mode.unwrap_or(Mode::Prompt);
            Action::Configure {
                mode,
                allow_automatic_repair: mode == Mode::Auto,
                manifest: input.source,
                expected_revision: input.expected_revision,
            }
        }
        "approve" => Action::Approve {
            operation_id: input
                .operation_id
                .ok_or_else(|| anyhow::anyhow!("Fixture approval ID"))?,
        },
        _ => anyhow::bail!("Fixture action"),
    };
    let report = eefn::package_setup::execute(&ctx, &action).await?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
