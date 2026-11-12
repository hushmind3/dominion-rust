//! Operator-owned evaluation registry. The task suite is pinned before model
//! execution and never loaded from a candidate's modified source tree.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::BTreeSet,path::Path};

fn identifier(value:&str)->bool {
    !value.is_empty() && value.len()<=64 && value.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-')
}
fn fingerprint(value:&Value)->String {
    let mut copy=value.clone();copy.as_object_mut().unwrap().remove("sha256");
    format!("{:x}",Sha256::digest(serde_json::to_vec(&copy).unwrap()))
}
fn validate(profile:&Value)->Result<(),String> {
    if !profile["id"].as_str().is_some_and(identifier) || !profile["name"].as_str().is_some_and(|v|!v.is_empty()&&v.len()<=200) {
        return Err("평가 프로필 id/name 오류".into());
    }
    let roles=profile["requiredComponents"].as_array().filter(|roles|!roles.is_empty()&&roles.len()<=32).ok_or("필수 구성요소 1~32개가 필요합니다.")?;
    let mut unique=BTreeSet::new();
    for role in roles {
        let name=role.as_str().filter(|name|identifier(name)).ok_or("구성요소 이름 오류")?;
        if !unique.insert(name){return Err("중복 필수 구성요소".into());}
    }
    let tasks=profile["tasks"].as_array().filter(|tasks|!tasks.is_empty()&&tasks.len()<=16).ok_or("평가 과제 1~16개가 필요합니다.")?;
    let mut ids=BTreeSet::new();
    for task in tasks {
        let id=task["id"].as_str().filter(|id|identifier(id)).ok_or("평가 과제 id 오류")?;
        if !ids.insert(id) || !task["domain"].as_str().is_some_and(|d|super::rsi_evolve::DOMAINS.contains(&d))
            || !task["file"].as_str().is_some_and(super::rsi::safe_path)
            || !task["request"].as_str().is_some_and(|r|r.len()>=20&&r.len()<=6000)
            || !task["metric"].as_str().is_some_and(|m|matches!(m,"quality"|"elapsedMs"|"contextChars"|"tokens")) {
            return Err("평가 과제의 분야·소스·목표·지표 오류".into());
        }
        for key in ["oracle","guard"] {
            if !task[key].as_str().is_some_and(|filter|!filter.is_empty()&&filter.len()<=240
                && filter.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b':')) {
                return Err("평가는 셸 명령이 아닌 정확한 Rust 테스트 이름이어야 합니다.".into());
            }
        }
        if !task["oracle"].as_str().unwrap().starts_with("rsi_frozen_oracle::") {
            return Err("주요 평가는 동결된 외부 oracle을 사용해야 합니다.".into());
        }
    }
    if !profile["oracleSource"].as_str().is_some_and(|s|!s.is_empty()&&s.len()<=500_000) {
        return Err("독립 평가 원문이 없거나 너무 큽니다.".into());
    }
    Ok(())
}
fn freeze(mut profile:Value,oracle:&str)->Result<Value,String> {
    profile["oracleSource"]=json!(oracle);validate(&profile)?;
    profile["sha256"]=json!(fingerprint(&profile));Ok(profile)
}
pub fn default_profile()->Value {
    freeze(serde_json::from_str(include_str!("../../rsi/evaluations/default.json")).unwrap(),
        include_str!("rsi_oracle_fixture.rs")).expect("bundled RSI profile")
}
pub fn load(root:&Path,id:&str)->Result<Value,String> {
    if id=="default" {return Ok(default_profile());}
    if !identifier(id){return Err("평가 프로필 이름 오류".into());}
    let directory=root.join("rsi/evaluations").canonicalize().map_err(|e|e.to_string())?;
    let file=directory.join(format!("{id}.json")).canonicalize().map_err(|e|e.to_string())?;
    if !file.starts_with(&directory){return Err("평가 프로필 경로 이탈".into());}
    let profile:Value=serde_json::from_slice(&std::fs::read(file).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    if profile["id"]!=id {return Err("프로필 파일명과 id가 다릅니다.".into());}
    let fixture=directory.join(profile["oracleFixture"].as_str().ok_or("oracleFixture 경로 없음")?)
        .canonicalize().map_err(|e|e.to_string())?;
    if !fixture.starts_with(&directory) || fixture.extension().and_then(|e|e.to_str())!=Some("rs") {
        return Err("독립 oracle은 rsi/evaluations 아래 Rust 파일이어야 합니다.".into());
    }
    freeze(profile,&std::fs::read_to_string(fixture).map_err(|e|e.to_string())?)
}
pub fn from_input(input:&Value)->Result<Value,String> {
    let profile=&input["rsiEvaluationProfile"];
    if profile.is_null(){return Ok(default_profile());}
    validate(profile)?;
    if profile["sha256"]!=fingerprint(profile){return Err("시작할 때 동결한 평가 프로필 해시가 다릅니다.".into());}
    Ok(profile.clone())
}
pub fn summary(profile:&Value)->Value {
    json!({"id":profile["id"],"name":profile["name"],"sha256":profile["sha256"],
        "taskCount":profile["tasks"].as_array().map(Vec::len),"requiredComponents":profile["requiredComponents"]})
}
pub fn catalogue(root:&Path)->Value {
    let mut profiles=vec![summary(&default_profile())];let mut errors=Vec::new();
    if let Ok(entries)=std::fs::read_dir(root.join("rsi/evaluations")) {
        for entry in entries.flatten() {
            if entry.path().extension().and_then(|e|e.to_str())!=Some("json"){continue;}
            let id=entry.path().file_stem().and_then(|v|v.to_str()).unwrap_or("").to_string();
            if id=="default"{continue;}
            match load(root,&id) {Ok(profile)=>profiles.push(summary(&profile)),Err(error)=>errors.push(json!({"id":id,"error":error}))}
        }
    }
    profiles.sort_by(|a,b|a["id"].as_str().cmp(&b["id"].as_str()));
    json!({"profiles":profiles,"errors":errors})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_can_register_new_tasks_and_roles_but_frozen_bytes_cannot_change() {
        let mut profile=default_profile();profile["requiredComponents"]=json!(["novel_reasoner"]);
        profile["tasks"][0]["id"]=json!("new-reasoning-task");
        assert!(from_input(&json!({"rsiEvaluationProfile":profile})).is_err());
        profile["sha256"]=json!(fingerprint(&profile));
        let frozen=from_input(&json!({"rsiEvaluationProfile":profile})).unwrap();
        assert_eq!(frozen["requiredComponents"],json!(["novel_reasoner"]));
        assert_eq!(super::super::rsi_meta::cases_for(&frozen)[0]["id"],"meta-downstream-new-reasoning-task");
        profile["tasks"][0]["oracle"]=json!("sh; echo success");
        assert!(freeze(profile,"fixture").is_err());
    }
    #[test]
    fn custom_oracle_is_pinned_and_cannot_escape_its_registry() {
        let root=std::env::temp_dir().join(format!("rsi-profiles-{}",uuid::Uuid::new_v4()));
        let directory=root.join("rsi/evaluations");std::fs::create_dir_all(&directory).unwrap();
        let mut profile=default_profile();profile["id"]=json!("custom");profile["oracleFixture"]=json!("custom.rs");
        std::fs::write(directory.join("custom.json"),profile.to_string()).unwrap();
        std::fs::write(directory.join("custom.rs"),"fn first() {}").unwrap();
        let first=load(&root,"custom").unwrap();
        std::fs::write(directory.join("custom.rs"),"fn second() {}").unwrap();
        assert_ne!(first["sha256"],load(&root,"custom").unwrap()["sha256"]);
        assert_eq!(from_input(&json!({"rsiEvaluationProfile":first})).unwrap()["oracleSource"],"fn first() {}");
        profile["oracleFixture"]=json!("../../../outside.rs");
        std::fs::write(directory.join("custom.json"),profile.to_string()).unwrap();
        assert!(load(&root,"custom").is_err());assert!(load(&root,"../escape").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
