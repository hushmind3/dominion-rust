//! Workspace output receipts derived only from successful file tool evidence.
//! SHA-256 is an artifact integrity marker, not a quality or test verdict.
use crate::{engine::verification,state::AppState};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeMap;
pub fn collect(state:&AppState,observations:&[Value])->Vec<Value>{
    collect_in(&state.config.workspace,observations)
}
pub fn collect_in(workspace:&std::path::Path,observations:&[Value])->Vec<Value>{
    let Ok(root)=workspace.canonicalize() else{return Vec::new()};
    let mut paths=BTreeMap::new();
    for item in observations.iter().rev() {
        if paths.len()>=32{break;}
        if !verification::successful_evidence(item){continue;}
        let tool=verification::tool_name(item);
        if !matches!(tool,"workspace_write"|"workspace_replace"|
            "workspace_replace_range"|"file_write"){continue;}
        let args=item.get("arguments").or_else(||item.get("args")).unwrap_or(&Value::Null);
        let Some(raw)=args["path"].as_str().filter(|s|!s.is_empty()&&s.len()<=4096) else{continue};
        let Ok(path)=verification::resolve_path_in(&root,raw) else{continue};
        let Ok(path)=path.canonicalize() else{continue};
        if !path.starts_with(&root){continue;}
        let Ok(rel)=path.strip_prefix(&root) else{continue};
        let rel=rel.to_string_lossy().replace('\\',"/");
        if rel.is_empty()||paths.contains_key(&rel){continue;}
        let Ok(meta)=path.metadata() else{continue};
        if !meta.is_file()||meta.len()>8*1024*1024{continue;}
        let Ok(data)=std::fs::read(&path) else{continue};
        paths.insert(rel.clone(),json!({"path":rel,"bytes":data.len(),
            "sha256":format!("{:x}",Sha256::digest(&data)),
            "sourceTool":tool,"grade":"observed_file_bytes"}));
    }
    paths.into_values().collect()
}

/// Read-only recheck of an artifact captured from a successful file-writing
/// tool. Never accepts client-supplied paths or reads content into responses.
pub fn verify_current(workspace:&std::path::Path,receipt:&Value)->Value{
    let path=receipt["path"].as_str().unwrap_or("");
    let expected=receipt["sha256"].as_str().unwrap_or("");
    let base=json!({"path":path,"expectedSha256":expected,
        "recordedBytes":receipt["bytes"],"grade":receipt["grade"]});
    if receipt["grade"]!="observed_file_bytes"
        || path.is_empty()||path.len()>4096
        || expected.len()!=64||!expected.bytes().all(|c|c.is_ascii_hexdigit()){
        let mut result=base;result["status"]=json!("invalid_receipt");return result;
    }
    let Ok(root)=workspace.canonicalize() else{
        let mut result=base;result["status"]=json!("workspace_unavailable");return result;
    };
    let Ok(candidate)=verification::resolve_path_in(&root,path) else{
        let mut result=base;result["status"]=json!("unsafe_path");return result;
    };
    let Ok(actual)=candidate.canonicalize() else{
        let mut result=base;result["status"]=json!("missing");return result;
    };
    if !actual.starts_with(&root) {
        let mut result=base;result["status"]=json!("unsafe_path");return result;
    }
    let Ok(meta)=actual.metadata() else{
        let mut result=base;result["status"]=json!("missing");return result;
    };
    if !meta.is_file()||meta.len()>8*1024*1024 {
        let mut result=base;result["status"]=json!("unverifiable");return result;
    }
    let Ok(data)=std::fs::read(&actual) else{
        let mut result=base;result["status"]=json!("unreadable");return result;
    };
    let current=format!("{:x}",Sha256::digest(&data));
    let mut result=base;
    result["currentSha256"]=json!(current);
    result["currentBytes"]=json!(data.len());
    result["status"]=json!(if expected==current {"intact"}else{"changed"});
    result
}
#[cfg(test)]
mod artifact_verification_tests{
    use super::*;
    #[test]
    fn rechecks_actual_file_and_rejects_tampered_receipts(){
        let dir=std::env::temp_dir().join(format!("workflow-proof-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("deliverable.txt"),b"proof").unwrap();
        let digest=format!("{:x}",Sha256::digest(b"proof"));
        let row=json!({"path":"deliverable.txt","sha256":digest,
            "bytes":5,"grade":"observed_file_bytes"});
        assert_eq!(verify_current(&dir,&row)["status"],"intact");
        std::fs::write(dir.join("deliverable.txt"),b"modified").unwrap();
        assert_eq!(verify_current(&dir,&row)["status"],"changed");
        assert_eq!(verify_current(&dir,&json!({"path":"../outside","sha256":digest,
            "grade":"observed_file_bytes"}))["status"],"unsafe_path");
        assert_eq!(verify_current(&dir,&json!({"path":"deliverable.txt",
            "sha256":digest,"grade":"self_reported"}))["status"],"invalid_receipt");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
