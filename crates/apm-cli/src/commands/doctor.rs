// doctor command - run diagnostic checks and report apm health.

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::{
    config::Config,
    diagnostics::{run_diagnostics, DiagnosticCheck, DiagnosticStatus, DiagnosticsReport},
};

#[derive(Serialize)]
struct DoctorCheckJson {
    name: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Serialize)]
struct DoctorJson {
    checks: Vec<DoctorCheckJson>,
    summary: apm_core::diagnostics::DiagnosticsSummary,
}

pub fn run(config: &Config, json: bool) -> Result<()> {
    let report = run_diagnostics(config);

    if json {
        print_json(&report)?;
        return Ok(());
    }

    print_human(&report);
    Ok(())
}

fn print_json(report: &DiagnosticsReport) -> Result<()> {
    let output = DoctorJson {
        checks: report.checks.iter().map(DoctorCheckJson::from).collect(),
        summary: report.summary.clone(),
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_human(report: &DiagnosticsReport) {
    println!("apm doctor");
    println!("{}", "=".repeat(35));
    println!();

    println!("Checking plugin directories...");
    for check in report.checks.iter().filter(|check| is_plugin_dir(check)) {
        print_check(check);
    }
    println!();

    if let Some(check) = report
        .checks
        .iter()
        .find(|check| check.name == "Quarantine")
    {
        println!("Checking for quarantined plugins...");
        print_check(check);
        println!();
    }

    println!("Checking configuration...");
    for check in report
        .checks
        .iter()
        .filter(|check| !is_plugin_dir(check) && check.name != "Quarantine")
    {
        print_check(check);
    }
    println!();

    let problem_checks: Vec<&DiagnosticCheck> = report
        .checks
        .iter()
        .filter(|check| check.status != DiagnosticStatus::Ok)
        .collect();

    if !problem_checks.is_empty() {
        println!("Remediation hints:");
        for check in problem_checks {
            if let Some(hint) = &check.hint {
                println!("  {}: {}", check.name, hint);
            }
        }
        println!();
    }

    if report.summary.failures == 0 && report.summary.warnings == 0 {
        println!(
            "{}",
            "Summary: All checks passed. apm is ready to use.".green()
        );
    } else if report.summary.failures == 0 {
        println!(
            "{}",
            format!(
                "Summary: {} warning(s) found. apm should work, but review the hints above.",
                report.summary.warnings
            )
            .yellow()
        );
    } else {
        println!(
            "{}",
            format!(
                "Summary: {} failure(s), {} warning(s) found. See hints above to resolve issues.",
                report.summary.failures, report.summary.warnings
            )
            .red()
        );
    }
}

fn print_check(check: &DiagnosticCheck) {
    match check.status {
        DiagnosticStatus::Ok => {
            println!("  {:<45}  {} {}", check.name, "ok".green(), check.detail);
        }
        DiagnosticStatus::Warning => {
            println!(
                "  {:<45}  {} {}",
                check.name,
                "!".yellow(),
                check.detail.yellow()
            );
        }
        DiagnosticStatus::Failure => {
            println!("  {:<45}  {} {}", check.name, "x".red(), check.detail.red());
        }
    }
}

fn is_plugin_dir(check: &DiagnosticCheck) -> bool {
    check.name.contains("Library/Audio/Plug-Ins")
}

impl From<&DiagnosticCheck> for DoctorCheckJson {
    fn from(check: &DiagnosticCheck) -> Self {
        Self {
            name: check.name.clone(),
            status: match check.status {
                DiagnosticStatus::Ok => "ok",
                DiagnosticStatus::Warning => "warning",
                DiagnosticStatus::Failure => "failure",
            }
            .to_string(),
            detail: if check.detail.is_empty() {
                None
            } else {
                Some(check.detail.clone())
            },
        }
    }
}
