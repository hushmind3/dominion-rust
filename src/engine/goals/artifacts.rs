//! Deliverable receipts from actual successful tool evidence, not model prose.
//! Only workspace-scoped write operations yield file manifests.
use crate::{state::AppState,engine::{verification,goal_workspace}};
use serde_json::{json,Value};
use std::{collections::BTreeSet,fs,path::{Path,Component}};
use sha2::{Digest,Sha256};

fn candidate(item:&Value)->Option<&str>{
    if !verification::successful_evidence(item){return None;}
    if !matches!(verification::tool_name(item),
        "workspace_write" | "workspace_replace" | "workspace_replace_function" | "file_write"){return None;}
    let args=item.get("arguments").or_else(||item.get("args"))?;
    args["path"].as_str().or_else(||args["file"].as_str())
        .or_else(||args["filename"].as_str())
}

pub(super) fn collect(state:&AppState,goal:&Value,evidence:&[Value])->Value{
    let scoped=match goal_workspace::workspace_for_goal(state,goal){
        Ok(path)=>path,Err(_)=>return json!({"files":[],"writeEvidence":0,"schema":"goal-artifacts-v1","workspaceUnavailable":true})
    };
    let root=match fs::canonicalize(&scoped){
        Ok(root)=>root,Err(_)=>return json!({"files":[],"writeEvidence":0,"schema":"goal-artifacts-v1"})
    };
    let mut paths=BTreeSet::new();
    let mut supported=0usize;
    for item in evidence{
        let Some(raw)=candidate(item) else {continue;};
        supported+=1;
        let candidate=Path::new(raw);
        if raw.len()>1024 || candidate.components().any(|component|
            matches!(component,Component::ParentDir)) {continue;}
        // Do not allow a tool to publish file contents outside the assigned
        // workspace, including via absolute paths and symlink hops.
        let Ok(resolved)=verification::resolve_in_root(&root,raw) else {continue;};
        let Ok(real)=fs::canonicalize(&resolved) else {continue;};
        if real.starts_with(&root) && real.is_file(){paths.insert(real);}
    }
    let files=paths.into_iter().take(40).filter_map(|path|{
        let meta=fs::metadata(&path).ok()?;
        let relative=path.strip_prefix(&root).ok()?.to_str()?.to_string();
        let hash=if meta.len()<=8*1024*1024{
            fs::read(&path).ok().map(|bytes|format!("{:x}",Sha256::digest(&bytes)))
        }else{None};
        Some(json!({"path":relative,"bytes":meta.len(),
            "sha256":hash,"verifiedAt":chrono::Utc::now().to_rfc3339()}))
    }).collect::<Vec<_>>();
    json!({"schema":"goal-artifacts-v1","files":files,
        "writeEvidence":supported,"source":"successful_tool_results",
        "doesNotVerifyFullTaskQuality":true})
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn only_successful_writes_can_claim_artifacts(){
        assert_eq!(candidate(&json!({"tool":"file_read","arguments":{"path":"x"},
            "result":{"ok":true}})),None);
        assert_eq!(candidate(&json!({"tool":"workspace_write","arguments":{"path":"x"},
            "result":{"success":false}})),None);
        assert_eq!(candidate(&json!({"tool":"workspace_write","arguments":{"path":"x"},
            "result":{"success":true}})),Some("x"));
    }
}
