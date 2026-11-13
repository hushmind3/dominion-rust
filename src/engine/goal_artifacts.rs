//! Workspace output receipts derived only from successful file tool evidence.
//! SHA-256 is an artifact integrity marker, not a quality or test verdict.
use crate::{engine::verification,state::AppState};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeMap;
pub fn collect(state:&AppState,observations:&[Value])->Vec<Value> {
    let Ok(root)=state.config.workspace.canonicalize() else{return Vec::new()};
    let mut paths=BTreeMap::new();
    for item in observations.iter().rev() {
        if paths.len()>=32{break;}
        if !verification::successful_evidence(item){continue;}
        let tool=verification::tool_name(item);
        if !matches!(tool,"workspace_write"|"workspace_replace"|
            "workspace_replace_range"|"file_write"){continue;}
        let args=item.get("arguments").or_else(||item.get("args")).unwrap_or(&Value::Null);
        let Some(raw)=args["path"].as_str().filter(|s|!s.is_empty()&&s.len()<=4096) else{continue};
        let Ok(path)=verification::resolve_path(state,raw) else{continue};
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
