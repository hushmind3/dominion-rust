//! Owner-grounded, bounded source coverage for durable autonomous goals.
//! A hint to the executor, NOT an independent success verdict.
use crate::{engine::verification,state::AppState};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::HashSet,fs,path::{Component,Path,PathBuf}};
const MAX_TARGETS:usize=6;
const MAX_INLINE:u64=2000;
fn allowed(path:&str)->bool{
    let p=Path::new(path);
    !path.is_empty()&&path.len()<=240&&!p.is_absolute()
        &&!p.components().any(|c|matches!(c,Component::ParentDir
            |Component::CurDir|Component::RootDir|Component::Prefix(_)))
        &&!path.contains("://")&&!path.contains('\0')
        &&!path.starts_with('-')
        &&path.split('/').all(|part|!part.is_empty()&&!part.starts_with('.'))
}
fn safe_path(state:&AppState,path:&str)->Option<PathBuf>{
    allowed(path).then(||verification::resolve_path(state,path).ok()).flatten()
}
fn digest(path:&Path)->Option<String>{
    let meta=fs::metadata(path).ok()?;
    if !meta.is_file()||meta.len()>8*1024*1024{return None}
    Some(format!("{:x}",Sha256::digest(fs::read(path).ok()?)))
}
/// Include owner-referenced files, even when minimal acceptance checks omit
/// some requested edits. Capture once before tools run; never overwrite on resume.
pub(crate) fn capture(state:&AppState,objective:&str,criteria:&Value)->Value{
    let inferred=verification::infer_criteria(objective);
    let explicit=criteria.as_array().map(Vec::as_slice).unwrap_or(&[]);
    let mut seen=HashSet::new();
    let mut paths=Vec::new();
    for check in inferred.iter().chain(explicit.iter()){
        let Some(path)=check["path"].as_str()else{continue};
        if !seen.insert(path.to_string())||paths.len()>=MAX_TARGETS{continue}
        let Some(full)=safe_path(state,path)else{continue};
        paths.push(json!({"path":path,"initialSha256":digest(&full),
            "initialExists":full.is_file()}));
    }
    json!({"schema":"goal_owner_source_targets_v1","paths":paths})
}
/// Latest real disk contents, not a stale model-reported progress summary.
/// Read only small UTF-8 owner-referenced files; never follow escaping links.
pub(crate) fn view(state:&AppState,baseline:&Value)->Value{
    let mut files=Vec::new();
    let mut outstanding=Vec::new();
    for record in baseline["paths"].as_array().into_iter().flatten().take(MAX_TARGETS){
        let Some(path)=record["path"].as_str()else{continue};
        let Some(full)=safe_path(state,path)else{continue};
        let exists=full.is_file();
        let initial=record["initialSha256"].as_str();
        let observed=digest(&full);
        let status=if !exists&&record["initialExists"]!=true{"not_created"}
            else if !exists{"removed"}
            else if initial.is_none(){"inspect_large_file"}
            else if observed.as_deref()==initial{"unchanged"}
            else{"modified"};
        if matches!(status,"not_created"|"unchanged"|"inspect_large_file"){
            outstanding.push(path);
        }
        let inline=if exists&&fs::metadata(&full).is_ok_and(|m|m.len()<=MAX_INLINE){
            fs::read_to_string(&full).ok().filter(|s|!s.contains('\0'))
        }else{None};
        files.push(json!({"path":path,"status":status,
            "content":inline,"contentTrust":"untrusted_workspace_data"}));
    }
    json!({"source":"fresh_read_only_workspace",
        "files":files,"unmodifiedOwnerReferencedFiles":outstanding,
        "rule":"Finish all requested edits, not only minimal checks; verify with real tools. This is not proof of success."})
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    #[test]
    fn omitted_translation_file_is_tracked_and_refreshes_after_edit(){
        let root=std::env::temp_dir().join(format!("goal-focus-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("trial/i18n")).unwrap();
        fs::write(root.join("trial/settings.json"),"{\"locale\":\"en\"}").unwrap();
        fs::write(root.join("trial/i18n/en.json"),"{\"save\":null}").unwrap();
        let cfg=Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.clone()};
        fs::create_dir_all(&cfg.data_dir).unwrap();
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()));
        let objective="In trial/settings.json change locale to ko. In trial/i18n/en.json add save=Save. Create trial/DONE.md.";
        let checks=json!([{"type":"file_contains","path":"trial/settings.json","contains":"ko"},
            {"type":"file_exists","path":"trial/DONE.md"}]);
        let baseline=capture(&state,objective,&checks);
        let first=view(&state,&baseline);
        assert!(first["unmodifiedOwnerReferencedFiles"].as_array().unwrap()
            .iter().any(|p|p=="trial/i18n/en.json"));
        assert!(first.to_string().contains("save"));
        fs::write(root.join("trial/i18n/en.json"),"{\"save\":\"Save\"}").unwrap();
        let second=view(&state,&baseline);
        assert!(!second["unmodifiedOwnerReferencedFiles"].as_array().unwrap()
            .iter().any(|p|p=="trial/i18n/en.json"));
        assert!(!allowed("../outside.json"));
        assert!(!allowed("/etc/passwd"));
        assert!(!allowed("src/.env"));
        assert!(safe_path(&state,"../outside.json").is_none());
        drop(state);fs::remove_dir_all(root).unwrap();
    }
}
