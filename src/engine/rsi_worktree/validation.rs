//! validation: isolated RSI worktree responsibility; no automatic production deployment.
use super::*;

pub(super) fn classify(patch:&str)->&'static str {
    if patch.lines().any(|line|line.starts_with("diff --git ")
        && (line.contains("src/engine/rsi.rs ")
            || line.contains("src/engine/rsi_worktree.rs ")
            || line.contains("src/engine/rsi_evolve.rs ")
            || line.contains("src/engine/rsi_metrics.rs ")
            || line.contains("src/engine/algorithms.rs ")
            || line.contains("src/engine/algorithm_eval.rs ")
            || line.contains("src/engine/policy_patch.rs ")
            || line.contains("src/api/rsi.rs "))) {
        "improver"
    } else if patch.lines().filter(|line|line.starts_with("diff --git "))
        .all(|line|line.contains("src/engine/memory")
            || line.contains("src/engine/algorithms.rs")
            || line.contains("src/engine/room_state.rs")) {
        "algorithm"
    } else {"harness"}
}
/// The last bytes of Cargo stderr usually contain only "could not compile".
/// Keep the *first actual* compiler error and its source location separately
/// so the next real model edit targets the failing file/line instead of the
/// unrelated end of a long build log.
pub(super) fn primary_compiler_diagnostic(stderr:&str)->String {
    let lines=stderr.lines().collect::<Vec<_>>();
    let start=lines.iter().position(|line|{
        let text=line.trim_start();
        text.starts_with("error[E") || text.starts_with("error:")
    });
    let Some(start)=start else {
        return stderr.chars().rev().take(1200)
            .collect::<String>().chars().rev().collect()
    };
    lines.iter().skip(start).take(20)
        .take_while(|line|!line.trim_start().starts_with("error: could not compile"))
        .copied().collect::<Vec<_>>().join("\n")
        .chars().take(2000).collect()
}
pub(super) fn check_result(label:&str, result:Result<std::process::Output,String>)->Value {
    match result {
        Ok(output)=>{
            let stderr=String::from_utf8_lossy(&output.stderr);
            json!({
                "label":label,"ran":true,"passed":output.status.success(),
                "exitCode":output.status.code(),
                "stdout":String::from_utf8_lossy(&output.stdout).chars().rev().take(2600).collect::<String>().chars().rev().collect::<String>(),
                "stderr":stderr.chars().rev().take(2600).collect::<String>().chars().rev().collect::<String>(),
                "primaryDiagnostic":if output.status.success() {String::new()}
                    else {primary_compiler_diagnostic(&stderr)}
            })
        },
        Err(error)=>json!({"label":label,"ran":false,"passed":false,"error":error}),
    }
}
pub(super) async fn validate(worktree:&Worktree,patch:&str)->Vec<Value> {
    let mut checks=Vec::new();
    let rust_change=patch.lines().any(|line|line.starts_with("diff --git a/src/")||line.starts_with("diff --git a/tests/"));
    let web_change=patch.lines().any(|line|line.starts_with("diff --git a/web/"));
    if rust_change {
        let mut command=Command::new(cargo_executable());
        command.current_dir(&worktree.path).args(["check","--all-targets","--color","never"]);
        checks.push(check_result("cargo check --all-targets",run(command,CHECK_LIMIT).await));
    }
    if web_change {
        let installed=worktree.source.join("web/node_modules");
        let deps=worktree.path.join("web/node_modules");
        #[cfg(unix)]
        if installed.is_dir() && !deps.exists() {
            let _=std::os::unix::fs::symlink(&installed,&deps);
        }
        if deps.exists() {
            let mut command=Command::new(npm_executable());
            command.current_dir(worktree.path.join("web")).args(["run","typecheck"]);
            checks.push(check_result("npm run typecheck",run(command,Duration::from_secs(120)).await));
        } else {
            checks.push(json!({"label":"npm run typecheck","ran":false,"passed":false,
                "error":"원본 web/node_modules가 없어 독립 타입 검사를 실행하지 않았습니다."}));
        }
    }
    checks
}

/// Runs the shared audit only when the patch adds an unsafe construct. The
/// automatic check is scoped to files the patch changed, so a pre-existing
/// issue in an unrelated file cannot block the candidate.
pub(crate) fn unsafe_audit_for_patch(workspace:&Path,patch:&str)->Option<Value>{
    if !crate::engine::unsafe_audit::needs_audit(patch){return None;}
    let mut paths=Vec::new();
    for line in patch.lines(){
        let Some(path)=line.strip_prefix("+++ b/") else{continue};
        if path=="dev/null"||!crate::engine::rsi::safe_path(path)||paths.iter().any(|item|item==path){continue;}
        paths.push(path.to_string());
    }
    if paths.is_empty(){
        return Some(crate::engine::unsafe_audit::audit_workspace(workspace));
    }
    let mut scanned=0u64;
    let mut findings=Vec::new();
    for relative in paths{
        let path=workspace.join(&relative);
        if !path.is_file(){continue;}
        let report=crate::engine::unsafe_audit::audit_path(&path);
        scanned+=report["scanned"].as_u64().unwrap_or(0);
        if let Some(rows)=report["findings"].as_array(){
            findings.extend(rows.iter().cloned());
        }
    }
    findings.sort_by(|a,b|a.as_str().cmp(&b.as_str()));
    Some(json!({
        "scanned":scanned,"findings":findings,"passed":findings.is_empty(),
        "scope":"changed_files",
    }))
}

/// Converts the audit report into the existing verification-check shape so a
/// missing SAFETY reason blocks the candidate through the normal repair loop.
pub(crate) fn unsafe_audit_check(audit:&Value)->Value{
    let passed=audit["passed"]==true;
    json!({
        "label":"unsafe_safety","passed":passed,
        "error":if passed {""} else {"unsafe 블록에 SAFETY 근거가 없습니다."},
        "findings":audit["findings"],
    })
}

/// Compilation and typecheck are only candidate-integrity evidence.
/// A completed run means the model produced a patch and verification ran
/// without error; it does NOT mean the patch improves runtime behavior.
pub(super) fn run_status(
    has_candidate:bool,checks_passed:bool,audit_failed:bool,source_clean:bool,
    model_error:&str,capture_error:&str,
)->(&'static str,&'static str){
    if !source_clean {return ("needs_review","original_source_changed");}
    if !model_error.is_empty(){return ("needs_review","model_error");}
    if !capture_error.is_empty()||!has_candidate {
        return ("needs_review","no_valid_candidate");
    }
    if audit_failed{return ("needs_review","unsafe_safety_failed");}
    if !checks_passed{return ("needs_review","build_or_typecheck_failed");}
    ("completed","candidate_compiles_not_performance_verified")
}

