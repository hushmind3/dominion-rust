//! Read-only Git patch preview for files proven by the goal's tool receipts.
//! No shell evaluation, no remote execution, and no diff of unrelated files.
use crate::state::AppState;
use serde_json::{json,Value};
use std::{path::{Component,Path},time::Duration};

pub(super) async fn capture(state:&AppState,goal:&Value)->Result<Value,String>{
    let root=state.config.workspace.canonicalize().map_err(|e|e.to_string())?;
    let mut paths=Vec::new();
    for file in goal["deliverables"]["files"].as_array().into_iter().flatten(){
        let Some(relative)=file["path"].as_str() else{continue};
        let name=Path::new(relative);
        if relative.len()>1024 || relative.is_empty() || name.is_absolute()
            || name.components().any(|part|matches!(part,Component::ParentDir)){continue;}
        let actual=root.join(name);
        let Ok(path)=actual.canonicalize() else {continue};
        if !path.starts_with(&root) || !path.is_file(){continue;}
        if std::fs::metadata(path).is_ok_and(|meta|meta.len()<=1024*1024){
            paths.push(relative.to_string());
        }
        if paths.len()>=12{break;}
    }
    if paths.is_empty(){
        return Ok(json!({"diff":"","note":"현재 기록된 파일 수정 산출물이 없어 Git 변경 비교 대상이 없습니다.",
            "files":[],"source":"goal-artifacts-v1"}));
    }
    let mut command=tokio::process::Command::new("git");
    command.current_dir(&root).env("GIT_OPTIONAL_LOCKS","0").kill_on_drop(true);
    command.args(["diff","--no-ext-diff","--no-textconv","--no-color","--unified=3","HEAD","--"]);
    for path in &paths{command.arg(path);}
    let output=tokio::time::timeout(Duration::from_secs(8),command.output()).await
        .map_err(|_|"Git 변경 비교가 제한 시간을 초과했습니다.".to_string())?
        .map_err(|e|format!("Git 변경 비교를 실행할 수 없습니다: {e}"))?;
    if !output.status.success(){
        return Ok(json!({"diff":"","note":"현재 워크스페이스의 Git 변경 비교를 사용할 수 없습니다.",
            "files":paths,"source":"git-diff-head","exitCode":output.status.code()}));
    }
    let source=String::from_utf8_lossy(&output.stdout);
    let shortened=source.chars().take(100_000).collect::<String>();
    Ok(json!({"diff":shortened,"truncated":source.chars().count()>100_000,
        "note":if source.is_empty(){"추적되지 않는 새 파일 또는 HEAD와 차이가 없는 파일일 수 있습니다. 산출물 목록의 해시를 확인하세요."}else{""},
        "files":paths,"source":"git-diff-head","readOnly":true}))
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn only_relative_scoped_paths_are_intended_for_preview(){
        assert!(Path::new("src/main.rs").components()
            .all(|part|!matches!(part,Component::ParentDir)));
        assert!(Path::new("../secret").components()
            .any(|part|matches!(part,Component::ParentDir)));
    }
}
