//! Read-only audit for undocumented `unsafe` constructs.
//!
//! Rust requires `unsafe` but does not require a justification. This module
//! reports every `unsafe` block, function, impl, trait or extern item whose
//! preceding lines do not contain a `SAFETY` marker. It is used by the
//! `dominion-unsafe-audit` binary, by the RSI `unsafe_audit` tool, and by the
//! RSI worktree verifier when a patch introduces an `unsafe` construct.

use serde_json::{json, Value};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

const SAFETY_MARKER: &str = "SAFETY";
const LOOKBACK_LINES: usize = 3;
const SKIP_DIRS: [&str; 4] = ["target", ".git", "node_modules", "dist"];

/// True when a unified diff adds an `unsafe` construct. The worktree verifier
/// uses this to avoid scanning the repository for unrelated patches.
pub fn needs_audit(patch: &str) -> bool {
    patch.lines().any(|line| {
        let Some(added) = line.strip_prefix('+') else {
            return false;
        };
        if added.starts_with("+++") {
            return false;
        }
        let mut in_block_comment = false;
        let code = code_part(added, &mut in_block_comment);
        has_unsafe_construct(&code)
    })
}

/// Audits a workspace and returns a JSON report.
pub fn audit_workspace(root: &Path) -> Value {
    audit_path(root)
}

/// Audits one file or directory. Directory traversal is deterministic.
pub fn audit_path(path: &Path) -> Value {
    let mut scanned = 0usize;
    let mut findings = Vec::new();
    scan(path, &mut scanned, &mut findings);
    findings.sort();
    json!({
        "scanned": scanned,
        "findings": findings,
        "passed": findings.is_empty(),
        "marker": SAFETY_MARKER,
        "lookbackLines": LOOKBACK_LINES,
    })
}

fn scan(path: &Path, scanned: &mut usize, findings: &mut Vec<String>) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    if metadata.is_dir() {
        let name = path.file_name().and_then(OsStr::to_str).unwrap_or("");
        if SKIP_DIRS.iter().any(|skip| *skip == name) {
            return;
        }
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        let mut children: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect();
        children.sort();
        for child in children {
            scan(&child, scanned, findings);
        }
        return;
    }
    if path.extension().and_then(OsStr::to_str) != Some("rs") {
        return;
    }
    let Ok(source) = fs::read_to_string(path) else {
        return;
    };
    *scanned += 1;
    collect(path, &source, findings);
}

fn collect(path: &Path, source: &str, findings: &mut Vec<String>) {
    let lines = source.lines().collect::<Vec<_>>();
    let mut in_block_comment = false;
    for (index, line) in lines.iter().enumerate() {
        let code = code_part(line, &mut in_block_comment);
        if !has_unsafe_construct(&code) {
            continue;
        }
        if justified(&lines, index) {
            continue;
        }
        findings.push(format!("{}:{}", path.display(), index + 1));
    }
}

/// Drops comments and string/char literals from one line. Block-comment state
/// is carried across lines. Raw strings are left untouched; this audit targets
/// hand-written crate sources, not generated data.
fn code_part(line: &str, in_block_comment: &mut bool) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut index = 0usize;
    let mut quote: Option<u8> = None;
    while index < bytes.len() {
        let byte = bytes[index];
        if *in_block_comment {
            if byte == b'*' && bytes.get(index + 1) == Some(&b'/') {
                *in_block_comment = false;
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }
        if let Some(active) = quote {
            if byte == b'\\' {
                index += 2;
                continue;
            }
            if byte == active {
                quote = None;
            }
            index += 1;
            continue;
        }
        match byte {
            b'/' if bytes.get(index + 1) == Some(&b'/') => break,
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                *in_block_comment = true;
                index += 2;
            }
            b'"' | b'\'' => {
                quote = Some(byte);
                index += 1;
            }
            _ if byte.is_ascii() => {
                out.push(byte as char);
                index += 1;
            }
            _ => {
                // Keep byte positions aligned without corrupting UTF-8 text.
                out.push(' ');
                index += 1;
            }
        }
    }
    out
}

/// True when the code opens an `unsafe` block, function, impl, trait or extern
/// item. A bare identifier such as `unsafe_foo` or `unsafe_code` is not one.
fn has_unsafe_construct(code: &str) -> bool {
    let bytes = code.as_bytes();
    let mut search = 0usize;
    while let Some(offset) = code[search..].find("unsafe") {
        let start = search + offset;
        let end = start + "unsafe".len();
        search = end;
        if start > 0 && is_ident_byte(bytes[start - 1]) {
            continue;
        }
        if code
            .as_bytes()
            .get(end)
            .is_some_and(|byte| is_ident_byte(*byte))
        {
            continue;
        }
        let rest = code[end..].trim_start();
        if rest.starts_with('{')
            || rest.starts_with("fn ")
            || rest.starts_with("impl")
            || rest.starts_with("trait ")
            || rest.starts_with("extern ")
            || rest.starts_with("extern\"")
        {
            return true;
        }
    }
    false
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn justified(lines: &[&str], index: usize) -> bool {
    let start = index.saturating_sub(LOOKBACK_LINES);
    lines[start..index]
        .iter()
        .rev()
        .any(|line| line.to_ascii_uppercase().contains(SAFETY_MARKER))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("dominion-unsafe-{label}-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn flags_undocumented_constructs_and_accepts_safety_comments() {
        let dir = temp_dir("scan");
        fs::write(
            dir.join("good.rs"),
            "// SAFETY: private group created by this process\nunsafe {\n    kill(-id, 9);\n}\n",
        )
        .unwrap();
        fs::write(
            dir.join("bad.rs"),
            "unsafe fn f() {}\nunsafe impl Send for X {}\nunsafe { g(); }\n",
        )
        .unwrap();
        fs::write(
            dir.join("prose.rs"),
            "// this comment mentions unsafe { but is not code\nlet unsafe_foo = 1;\nlet s = \"unsafe {}\";\n",
        )
        .unwrap();
        let report = audit_path(&dir);
        let findings = report["findings"].as_array().unwrap();
        assert_eq!(report["scanned"], 3);
        assert_eq!(findings.len(), 3, "{findings:?}");
        assert!(findings.iter().any(|row| row.as_str().unwrap().ends_with("bad.rs:1")));
        assert!(findings.iter().any(|row| row.as_str().unwrap().ends_with("bad.rs:2")));
        assert!(findings.iter().any(|row| row.as_str().unwrap().ends_with("bad.rs:3")));
        assert!(!findings.iter().any(|row| row.as_str().unwrap().contains("good.rs")));
        assert!(!findings.iter().any(|row| row.as_str().unwrap().contains("prose.rs")));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn needs_audit_reads_added_lines_only() {
        assert!(needs_audit("+unsafe {\n+    kill(-id, 9);\n+}\n"));
        assert!(needs_audit("+    unsafe fn helper() {}\n"));
        assert!(!needs_audit("-unsafe {\n"));
        assert!(!needs_audit("+let unsafe_foo = 1;\n"));
        assert!(!needs_audit("+// unsafe { is prose\n"));
        assert!(!needs_audit("+++ b/src/unsafe.rs\n"));
    }

    #[test]
    fn skips_target_and_symlinks() {
        let dir = temp_dir("skip");
        fs::create_dir_all(dir.join("target")).unwrap();
        fs::write(dir.join("target/generated.rs"), "unsafe { x(); }\n").unwrap();
        let report = audit_path(&dir);
        assert_eq!(report["scanned"], 0);
        assert!(report["passed"].as_bool().unwrap());
        let _ = fs::remove_dir_all(dir);
    }
}
