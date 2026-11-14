//! Bounded read-only Git evidence for long-horizon coding goals.
//! Git results are working-tree observations, not proof that a particular
//! assistant caused the change. The goal's source-edit receipts supply
//! attribution, while the frozen HEAD supplies a reproducible review base.
use serde_json::{json,Value};
use std::path::Path;
use std::process::Command;

fn git(root:&Path,args:&[&str],max_output:usize)->Option<String>{
    let out=Command::new("git").arg("-C").arg(root).arg("--no-optional-locks")
        .args(args).env("GIT_TERMINAL_PROMPT","0").output().ok()?;
    if !out.status.success() || out.stdout.len()>max_output{return None;}
    String::from_utf8(out.stdout).ok().map(|s|s.trim_end().to_string())
}
fn is_root(root:&Path)->bool{
    let Some(result)=git(root,&["rev-parse","--show-toplevel"],8192) else{return false};
    let root=root.canonicalize().ok();
    let top=Path::new(&result).canonicalize().ok();
    root.is_some() && root==top
}
pub(super) fn snapshot(root:&Path)->Value{
    if !is_root(root){return Value::Null;}
    let head=git(root,&["rev-parse","--verify","HEAD"],1024);
    let status=git(root,&["status","--porcelain=v1","--untracked-files=normal"],65536);
    json!({"head":head,"initialStatus":status,"at":chrono::Utc::now().to_rfc3339()})
}
pub(super) fn report(root:&Path,baseline:&Value)->Value{
    if !baseline.is_object() || !is_root(root){return Value::Null;}
    let head=git(root,&["rev-parse","--verify","HEAD"],1024);
    let status=git(root,&["status","--porcelain=v1","--untracked-files=normal"],65536);
    let base=baseline["head"].as_str().filter(|s|s.len()==40 && s.bytes().all(|b|b.is_ascii_hexdigit()));
    let (changed,stat,patch)=if let Some(base)=base{
        (git(root,&["diff","--no-ext-diff","--name-status",base],65536),
         git(root,&["diff","--no-ext-diff","--stat",base],65536),
         git(root,&["diff","--no-ext-diff","--unified=2",base],24576))
    }else{(None,None,None)};
    let files=changed.as_deref().unwrap_or("").lines().take(100)
        .filter_map(|line|line.split_once('\t'))
        .map(|(status,path)|json!({"change":status,"path":path}))
        .collect::<Vec<_>>();
    // A dirty baseline or another active worker makes attribution ambiguous.
    // Keep this separate from the tool receipts used for acceptance.
    let truncated=patch.is_none() && base.is_some();
    json!({"baselineHead":baseline["head"],"currentHead":head,
        "initialStatus":baseline["initialStatus"],"currentStatus":status,
        "changedFiles":files,"diffStat":stat,"diffPreview":patch,
        "diffPreviewTruncated":truncated,
        "attribution":"repository-observation-not-agent-proof"})
}

#[cfg(test)]
#[path = "../../../tests/unit/goal_git_report_tests.rs"]
mod tests;
