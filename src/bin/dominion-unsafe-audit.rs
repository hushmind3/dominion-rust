//! CLI for the shared undocumented-unsafe audit.
//!
//! Exit status is 0 by default even when findings exist. Pass `--strict` to
//! fail a pipeline on the first unjustified construct. Paths default to `.`.
#[path = "../engine/unsafe_audit.rs"]
#[allow(dead_code)]
mod unsafe_audit;

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut strict = false;
    let mut roots: Vec<PathBuf> = Vec::new();
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--strict" => strict = true,
            other if other.starts_with('-') => {
                eprintln!("알 수 없는 옵션: {other}");
                return ExitCode::from(2);
            }
            other => roots.push(PathBuf::from(other)),
        }
    }
    if roots.is_empty() {
        roots.push(PathBuf::from("."));
    }
    let mut scanned = 0u64;
    let mut findings = Vec::new();
    for root in &roots {
        let report = unsafe_audit::audit_path(root);
        scanned += report["scanned"].as_u64().unwrap_or(0);
        if let Some(rows) = report["findings"].as_array() {
            findings.extend(rows.iter().filter_map(|row| row.as_str().map(str::to_string)));
        }
    }
    findings.sort();
    for finding in &findings {
        println!("{finding}");
    }
    println!(
        "검사한 Rust 파일 {scanned}개, 근거 없는 unsafe {}건",
        findings.len()
    );
    if strict && !findings.is_empty() {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
