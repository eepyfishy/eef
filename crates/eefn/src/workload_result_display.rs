//! Human output for explicit result reads. JSON reports and authority stay unchanged.
use crate::{grants::workload::VisionRecord, request_result::ResultCommand};
use anyhow::{Context, Result, ensure};
use serde_json::Value;

fn supported(value: &Value) -> bool {
    matches!(
        value["report_type"].as_str(),
        Some("coordinator_workload_result" | "coordinator_workload_uncertain_result")
    )
}
/// Render only validated workload reports. Other existing result types retain
/// their own output, and malformed workload evidence cannot produce partial text.
pub fn render(value: &Value) -> Result<Option<String>> {
    let wrapped = value["report_type"] == "request_result";
    let report = if wrapped { &value["result"] } else { value };
    if !supported(report) {
        return Ok(None);
    }
    let command = ResultCommand {
        id: value["id"]
            .as_str()
            .context("Result job identity missing")?
            .into(),
        grant_id: if wrapped {
            &value["grant_id"]
        } else {
            &report["definition"]["grant_id"]
        }
        .as_str()
        .context("Result grant identity missing")?
        .into(),
    };
    if wrapped {
        ensure!(
            crate::request_result::confirmed_node(
                value,
                &command,
                value["node_id"].as_str().context("Result node missing")?,
                value["request_id"]
                    .as_str()
                    .context("Result request missing")?
            ),
            "Unconfirmed workload result wrapper"
        );
    }
    ensure!(
        crate::request_result::confirmed(report, &command, None, None),
        "Unconfirmed workload result"
    );
    let mut output = format!(
        "Result for job {} / grant {}\n",
        command.id, command.grant_id
    );
    if report["report_type"] == "coordinator_workload_result" {
        let report: crate::workload_result::Report = serde_json::from_value(report.clone())?;
        output.push_str(&format!(
            "Status: completed (coordinator checkpoint)\nNode: {}\n",
            report.definition.executor_node
        ));
        if let Some(events) = &report.metadata {
            output.push_str(&format!("Metadata events: {}\n", events.count));
        } else {
            output.push_str("Metadata summary: unavailable in this legacy checkpoint\n");
        }
        if let Some(vision) = report.vision {
            output.push_str(&format!(
                "Analyses: {} completed of {} recorded\n",
                vision.summary.completed, vision.summary.calls
            ));
            records(&mut output, &vision.analyses);
        }
    } else {
        use crate::workload_uncertain_result::AnalysisAvailability;
        let report: crate::workload_uncertain_result::Report =
            serde_json::from_value(report.clone())?;
        output.push_str(&format!(
            "Status: interrupted; completion unconfirmed\nNode: {}\nMetadata events: {}\n",
            report.definition.executor_node, report.metadata.count
        ));
        match report.analysis_availability {
            AnalysisAvailability::NotApplicable => output.push_str("No model analysis for this workload.\n"),
            AnalysisAvailability::UnavailableOrUnconfirmed => output.push_str("Retained analysis unavailable or unconfirmed. Missing data does not prove no effects.\n"),
            AnalysisAvailability::RetainedSnapshot => {
                let vision = report.vision.context("Validated analysis snapshot missing")?;
                output.push_str(&format!("Node evidence: {} (does not settle this job)\nAnalyses: {} completed of {} recorded\n", vision.receipt.state, vision.summary.completed, vision.summary.calls));
                records(&mut output, &vision.analyses);
            }
        }
    }
    output.push_str("Read only. No work was resumed, retried or reconciled. External effects and model quality are not proven.\n");
    Ok(Some(output))
}
fn records(output: &mut String, records: &[VisionRecord]) {
    for record in records {
        if let Some(text) = record.text() {
            output.push_str(&format!("Analysis {}:\n  ", record.sequence()));
            text_for_terminal(output, text);
            output.push('\n');
        } else {
            output.push_str(&format!(
                "Analysis {}: pending; outcome unconfirmed\n",
                record.sequence()
            ));
        }
    }
}
fn text_for_terminal(output: &mut String, text: &str) {
    for c in text.chars() {
        if c == '\n' {
            output.push_str("\n  ");
        } else if c.is_control()
            || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        {
            output.extend(c.escape_default());
        } else {
            output.push(c);
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn model_text_cannot_emit_terminal_controls_or_spoof_unindented_status_lines() {
        let mut text = String::new();
        super::text_for_terminal(
            &mut text,
            "สวัสดี 🐟\nStatus: completed\r\u{1b}[2J\u{202e}hidden\u{2028}Status: done",
        );
        assert!(text.contains("สวัสดี 🐟\n  Status: completed"));
        assert!(!text.contains(['\r', '\u{1b}', '\u{202e}', '\u{2028}']));
        assert!(text.chars().filter(|c| c.is_control()).all(|c| c == '\n'));
    }
}
