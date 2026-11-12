//! Open-ended role admission for independently adopted intelligence modules.
//!
//! Source creation is not a capability. A new subsystem is inherited only
//! when the SAME adopted patch changes its implementation AND a distinct
//! executable integration point. A fresh compiled descendant rehashes both.
//! The immutable evaluator, not this editable descendant, decides adoption.
use crate::engine::{rsi,rsi_mutual_program,rsi_evolve};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::{BTreeMap,BTreeSet},fs,path::Path};

pub(crate) fn valid_role(s:&str)->bool {
    (3..=64).contains(&s.len()) && s!="system"
        && s.as_bytes().first().is_some_and(|c|c.is_ascii_alphabetic())
        && s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-')
}
fn sha_ok(s:&str)->bool {
    s.len()==64 && s.bytes().all(|c|c.is_ascii_hexdigit())
}
fn paths(value:&Value)->Option<Vec<String>> {
    let a=value.as_array().filter(|a|!a.is_empty()&&a.len()<=8)?;
    let mut out=BTreeSet::new();
    for v in a {
        let p=v.as_str().filter(|p|rsi::safe_path(p))?;
        if !out.insert(p.to_owned()){return None;}
    }
    Some(out.into_iter().collect())
}
/// Validate model-authored role declarations at submission. Every module
/// must belong to its role, while a separate existing infrastructure source
/// must integrate it into the real runtime.
pub(crate) fn validate_declarations(design:&Value,assignments:&Value,
    assigned_roles:&[Value],targets:&[Value])->Result<Vec<Value>,String> {
    let mut result=Vec::new();
    let mut names=BTreeSet::new();
    let known:&[&str]=&[
        "algorithms","harness","memory","learning","tools",
        "improvement_methods","models","frontend"
    ];
    let Some(rows)=design["newComponents"].as_array() else {
        if design["newComponents"].is_null(){return Ok(result);}
        return Err("새 구성요소 선언은 배열이어야 합니다.".into());
    };
    if rows.len()>8 {return Err("한 설계에 새로운 구성요소는 최대 8개입니다.".into());}
    for item in rows {
        let role=item["role"].as_str().filter(|r|valid_role(r))
            .ok_or("신규 지능 역할 이름 오류")?;
        if known.contains(&role) || !names.insert(role.to_owned())
            || !assigned_roles.iter().any(|v|v==role) {
            return Err("새 구성요소가 기존 역할과 충돌하거나 설계 역할에서 누락됐습니다.".into());
        }
        let domain=item["domain"].as_str()
            .filter(|d|rsi_evolve::DOMAINS.contains(d))
            .ok_or("새 구성요소의 개선 분야 오류")?;
        let purpose=item["purpose"].as_str()
            .filter(|s|(24..=1000).contains(&s.chars().count()))
            .ok_or("새 구성요소의 목적은 24~1000자여야 합니다.")?;
        let modules=paths(&item["sourcePaths"])
            .ok_or("새 구성요소의 실제 소스가 없습니다.")?;
        let links=paths(&item["integrationPaths"])
            .ok_or("새 구성요소의 기존 실행 경로 연결이 없습니다.")?;
        if modules.iter().any(|m|!assignments[role].as_array().is_some_and(|rows|
            rows.iter().any(|x|x==m)))
            || links.iter().any(|path|modules.contains(path)
                || !targets.iter().any(|t|t==path)
                || !assigned_roles.iter().filter_map(Value::as_str)
                    .filter(|other|*other!=role).any(|other|
                        assignments[other].as_array().is_some_and(|rows|
                            rows.iter().any(|v|v==path)))) {
            return Err("신규 지능 모듈과 별도 실행 경로의 소스 연결이 잘못됐습니다.".into());
        }
        result.push(json!({"role":role,"domain":domain,"purpose":purpose,
            "sourcePaths":modules,"integrationPaths":links,
            "integrationRequired":true,"qualityProven":false}));
    }
    Ok(result)
}
fn actual_changed(g:&Value,paths:&[String])->bool {
    let Some(changes)=g["changedSourcePaths"].as_array() else{return false};
    let Some(targets)=g["systemDesign"]["targetFiles"].as_array() else{return false};
    paths.iter().all(|path|changes.iter().any(|s|s==path)
        && targets.iter().any(|s|s==path)
        && g["acceptedSourceHashes"][path].as_str().is_some_and(sha_ok))
}
fn current_sources(root:&Path,paths:&[String])->Option<Vec<Value>> {
    let root=fs::canonicalize(root).ok()?;
    let mut result=Vec::new();
    for path in paths {
        let file=fs::canonicalize(root.join(path)).ok()?;
        if !file.starts_with(&root)||!file.is_file() {return None;}
        let bytes=fs::read(file).ok()?;
        if bytes.len()>2_000_000 {return None;}
        result.push(json!({"path":path,"sha256":format!("{:x}",Sha256::digest(&bytes)),
            "bytes":bytes.len()}));
    }
    Some(result)
}
/// Returned rows use CURRENT source hashes, so a newly recompiled successor
/// can re-design even an invented module without trusting old postimage SHA.
pub(crate) fn admitted(root:&Path,history:&[Value])->Vec<Value> {
    let mut result=BTreeMap::<String,Value>::new();
    for g in history {
        if !rsi_mutual_program::adopted_source(g)
            || g["candidateOrigin"]!="model_driven_detached_worktree" {
            continue;
        }
        let design=&g["systemDesign"];
        let declared=design["components"].as_array()
            .cloned().unwrap_or_else(||vec![design["component"].clone()]);
        let mut assignments=serde_json::Map::new();
        for role in declared.iter().filter_map(Value::as_str){
            if let Some(paths)=design["componentFiles"][role].as_array() {
                assignments.insert(role.to_string(),json!(paths));
            }
        }
        let Ok(rows)=validate_declarations(design,&Value::Object(assignments),
            &declared,design["targetFiles"].as_array().map(Vec::as_slice).unwrap_or(&[]))
            else{continue};
        for row in rows {
            let Some(role)=row["role"].as_str() else{continue};
            let Some(modules)=paths(&row["sourcePaths"]) else{continue};
            let Some(wires)=paths(&row["integrationPaths"]) else{continue};
            if !actual_changed(g,&modules)||!actual_changed(g,&wires){continue;}
            let Some(sources)=current_sources(root,&modules) else{continue};
            let Some(integrations)=current_sources(root,&wires) else{continue};
            if sources.is_empty()||integrations.is_empty(){continue;}
            result.insert(role.to_string(),json!({
                "role":role,"domain":row["domain"],"purpose":row["purpose"],
                "representativeSources":sources,"integrationSources":integrations,
                "source":"verified_source_and_runtime_integration_v1",
                "bornAtCommit":g["acceptedCommit"],
                "bornAtGeneration":g["id"],
                "independentAdoption":true,
                "currentSourceRebound":true,
                "qualityProvenForCurrentRevision":false
            }));
        }
    }
    result.into_values().take(24).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unintegrated_or_unaccepted_new_module_is_not_a_capability(){
        let root=std::env::temp_dir().join(format!("sub3-birth-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src/engine")).unwrap();
        fs::write(root.join("src/engine/novel_planner.rs"),"pub fn think() {}").unwrap();
        fs::write(root.join("src/engine/mod.rs"),"pub mod novel_planner;").unwrap();
        let design=json!({"source":"model_authored_system_design",
            "generatorCommit":"a".repeat(40),"generationJobId":"job",
            "component":"system","components":["new_planner","harness"],
            "componentFiles":{"new_planner":["src/engine/novel_planner.rs"],
                "harness":["src/engine/mod.rs"]},
            "targetFiles":["src/engine/novel_planner.rs","src/engine/mod.rs"],
            "newComponents":[{"role":"new_planner","domain":"improver",
                "purpose":"Introduce a reusable source-grounded new planning component",
                "sourcePaths":["src/engine/novel_planner.rs"],
                "integrationPaths":["src/engine/mod.rs"]}]});
        let mut gen=json!({"id":"birth","jobId":"job",
            "candidateId":"c","candidateOrigin":"model_driven_detached_worktree",
            "generatorCommit":"a".repeat(40),"acceptedCommit":"b".repeat(40),
            "accepted":true,"pairedRevalidated":true,
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary",
                "generatorCommit":"a".repeat(40)},
            "evaluation":{"metrics":{"eligible":true}},
            "changedSourcePaths":["src/engine/novel_planner.rs"],
            "acceptedSourceHashes":{"src/engine/novel_planner.rs":"c".repeat(64),
                "src/engine/mod.rs":"d".repeat(64)},
            "systemDesign":design});
        assert!(admitted(&root,&[gen.clone()]).is_empty());
        gen["changedSourcePaths"]=json!(["src/engine/novel_planner.rs","src/engine/mod.rs"]);
        assert_eq!(admitted(&root,&[gen.clone()])[0]["role"],"new_planner");
        gen["pairedRevalidated"]=json!(false);
        assert!(admitted(&root,&[gen]).is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
