use crate::Format;
use errspec::model::Report;
use std::{
    io::{self, Write},
    process::ExitCode,
};

pub(crate) fn write(report: &Report, format: Format) -> ExitCode {
    let output = match format {
        Format::Json => serde_json::to_string_pretty(report)
            .expect("report serialization uses only JSON primitives"),
        Format::Text => {
            let mut lines = vec![format!(
                "{}: {} (analysis complete: {}, tests executed: {})",
                report.mode,
                if report.ok { "pass" } else { "fail" },
                report.analysis_complete,
                report.tests_executed
            )];
            for c in &report.contracts {
                lines.push(format!("{}: {}", c.id, c.tags.join(", ")));
                for case in &c.cases {
                    lines.push(format!(
                        "  {} {}: {}, test={}, observation={:?}",
                        case.tag,
                        case.id.as_deref().unwrap_or(""),
                        case.registration,
                        case.test_status,
                        case.observation_status
                    ));
                }
            }
            for d in &report.diagnostics {
                lines.push(format!(
                    "{} {} {} {}: {}",
                    d.code,
                    d.contract_id.as_deref().unwrap_or(""),
                    d.tag.as_deref().unwrap_or(""),
                    d.scenario_id.as_deref().unwrap_or(""),
                    d.message
                ));
            }
            if let Some(delta) = &report.delta {
                lines.push(format!("delta: {}", serde_json::to_string(delta).unwrap()));
            }
            lines.join("\n")
        }
    };
    if writeln!(io::stdout().lock(), "{output}").is_err() {
        return ExitCode::from(2);
    }
    ExitCode::from(if report.ok {
        0
    } else if report.analysis_complete {
        1
    } else {
        2
    })
}
