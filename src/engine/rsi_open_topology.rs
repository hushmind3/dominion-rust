//! Open-ended, source-attested invention of NEW intelligence components.
//!
//! Existing algorithms, model, memory, harness and tool roles are only seeds.
//! A successor may author an entirely new role and a new implementation,
//! but must connect it to an EXISTING executing subsystem via a real source
//! change. Descendants inherit the role only after paired adoption receipts.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::{BTreeMap,BTreeSet},fs,path::Path};

fn role_name(role:&str)->bool{
    !role.is_empty() && role.len()<=64 && role!="system"
        && role.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-')
}
fn digest(v:&Value)->String{
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn frozen_core(name:&str)->bool{
    matches!(name,"algorithms"|"harness"|"memory"|"learning"|"tools"|
        "models"|"improvement_methods"|"frontend")
}
fn current_file(root:&Path,path:&str)->bool{
    if !crate::engine::rsi::safe_path(path){return false;}
    let (Ok(root),Ok(candidate))=(fs::canonicalize(root),
        fs::canonicalize(root.join(path)))else{return false};
    candidate.starts_with(&root) && candidate.is_file()
}

/// Validate the model's proposed COMPONENT INVENTION before any code write.
/// New modules must have BOTH a new implementation and a source change to
/// a current, independently editable integration point (harness/tool/model).
pub(crate) fn validate_proposal(
    body:&Value,root:&Path,roles:&[Value],targets:&[Value],
    assignments:&serde_json::Map<String,Value>
)->Result<Vec<Value>,String>{
    let Some(rows)=body["moduleIntroductions"].as_array()else{
        return Ok(Vec::new());
    };
    if rows.len()>8{return Err("새 지능 구성요소 계획은 최대 8개입니다.".into());}
    let mut names=BTreeSet::<String>::new();
    let mut introduced=Vec::new();
    for row in rows{
        let role=row["role"].as_str().filter(|r|
            role_name(r) && !frozen_core(r)
                && roles.iter().any(|v|v.as_str()==Some(*r)))
            .ok_or("새 지능 역할은 기존 8개 범주와 구별된 실제 구성요소여야 합니다.")?;
        if !names.insert(role.to_string()){
            return Err("새 지능 역할이 중복됐습니다.".into());
        }
        let files=row["newFiles"].as_array().filter(|fs|
            !fs.is_empty()&&fs.len()<=8)
            .ok_or("새 지능 역할에는 새 구현 파일이 필요합니다.")?;
        let consumers=row["integrationFiles"].as_array().filter(|fs|
            !fs.is_empty()&&fs.len()<=8)
            .ok_or("새 모듈을 기존 실행 시스템에 연결할 파일이 필요합니다.")?;
        let interface=row["interfaceContract"].as_str()
            .filter(|s|(30..=2000).contains(&s.chars().count()))
            .ok_or("새 지능 모듈의 실제 입력·출력 계약이 필요합니다.")?;
        let effect=row["improvementMechanism"].as_str()
            .filter(|s|(30..=2000).contains(&s.chars().count()))
            .ok_or("새 모듈이 다른 구성요소의 자기개선을 돕는 방법이 필요합니다.")?;
        let mut new_paths=BTreeSet::<String>::new();
        for file in files{
            let path=file.as_str().filter(|p|crate::engine::rsi::safe_path(p)
                && targets.iter().any(|v|v.as_str()==Some(*p))
                && assignments.get(role).and_then(Value::as_array).is_some_and(|assigned|
                    assigned.iter().any(|entry|entry.as_str()==Some(*p)))
                && !root.join(p).exists())
                .ok_or("새 지능 파일은 실제로 존재하지 않는 고유한 대상이어야 합니다.")?;
            if !new_paths.insert(path.to_owned()){
                return Err("새 지능 파일이 중복됐습니다.".into());
            }
        }
        let mut consumer_paths=BTreeSet::<String>::new();
        for file in consumers{
            let path=file.as_str().filter(|p|crate::engine::rsi::safe_path(p)
                && targets.iter().any(|v|v.as_str()==Some(*p))
                && current_file(root,p)
                && assignments.iter().any(|(owner,assigned)|
                    owner!=role && assigned.as_array().is_some_and(|items|
                        items.iter().any(|item|item.as_str()==Some(*p)))))
                .ok_or("기존 하네스·모델·도구 등의 실제 통합 소스 변경이 필요합니다.")?;
            if !consumer_paths.insert(path.to_owned()){
                return Err("기존 소비자 소스가 중복됐습니다.".into());
            }
        }
        introduced.push(json!({
            "role":role,"newFiles":new_paths.into_iter().collect::<Vec<_>>(),
            "integrationFiles":consumer_paths.into_iter().collect::<Vec<_>>(),
            "interfaceContract":interface,
            "improvementMechanism":effect,
            "qualityProven":false,
            "productionReady":false
        }));
    }
    Ok(introduced)
}

fn verified_invention(g:&Value,row:&Value,root:&Path)->Option<Value>{
    let d=&g["systemDesign"];
    if g["accepted"]!=true || g["pairedRevalidated"]!=true
        || g["evaluation"]["metrics"]["eligible"]!=true
        || g["candidateOrigin"]!="model_driven_detached_worktree"
        || d["source"]!="model_authored_system_design"
        || d["generatorCommit"]!=g["generatorCommit"]
        || d["generationJobId"]!=g["jobId"]
        || !crate::engine::rsi_system::design_implemented(d,
            &g["acceptedSourceHashes"].as_object()?.keys()
                .map(|k|json!(k)).collect::<Vec<_>>()) {
        return None;
    }
    let role=row["role"].as_str().filter(|r|role_name(r)&&!frozen_core(r))?;
    let new_files=row["newFiles"].as_array()?;
    let integration=row["integrationFiles"].as_array()?;
    if new_files.is_empty()||integration.is_empty(){return None;}
    let entries=new_files.iter().chain(integration.iter());
    let mut hashes=BTreeMap::new();
    for value in entries {
        let path=value.as_str().filter(|p|crate::engine::rsi::safe_path(p)
            && current_file(root,p))?;
        let sha=g["acceptedSourceHashes"][path].as_str().filter(|h|
            h.len()==64&&h.bytes().all(|c|c.is_ascii_hexdigit()))?;
        hashes.insert(path.to_string(),sha.to_string());
    }
    Some(json!({
        "role":role,"newFiles":new_files,
        "integrationFiles":integration,
        "interfaceContract":row["interfaceContract"],
        "improvementMechanism":row["improvementMechanism"],
        "introductionDesignId":d["id"],
        "introductionCommit":g["acceptedCommit"],
        "firstAcceptedSourceHashes":hashes,
        "adoptionVerified":true,
        "newModulesMayBeRedesigned":true,
        "latestIndependentEffectUnproven":true
    }))
}

/// The next compiled descendant rediscovers adopted invented abilities
/// and their integration paths. These may be changed, composed or replaced
/// by another model-authored intelligence architecture in later generations.
pub(crate) fn observed(history:&[Value],root:&Path)->Value{
    let mut roles=BTreeMap::<String,Value>::new();
    for generation in history{
        for entry in generation["systemDesign"]["moduleIntroductions"]
            .as_array().into_iter().flatten(){
            if let Some(row)=verified_invention(generation,entry,root){
                if let Some(role)=row["role"].as_str(){
                    roles.insert(role.to_string(),row.clone());
                }
            }
        }
    }
    let mut report=json!({
        "source":"compiled_source_attested_open_intelligence_topology_v1",
        "inventedComponents":roles.into_values().collect::<Vec<_>>(),
        "seedRolesAreNotAnAllowlist":true,
        "unverifiedModuleClaimsExcluded":true,
        "sourceIdentityReboundInEachDescendant":true,
        "qualityProvenForNewTask":false
    });
    report["sha256"]=json!(digest(&report));
    report
}

/// Exploration bonus for a NEW, independently introduced subsystem that
/// has not yet demonstrated a productive cross-role descendant. This
/// never supplies an improvement label or changes frozen evaluation.
pub(crate) fn exploration_bonus(history:&[Value],role:&str)->f64{
    let mut origins=Vec::new();
    for row in history{
        if row["accepted"]==true
            && row["systemDesign"]["moduleIntroductions"].as_array()
                .is_some_and(|modules|modules.iter().any(|module|module["role"]==role)){
            origins.push(row["acceptedCommit"].clone());
        }
    }
    if origins.is_empty(){return 0.0;}
    let productive=history.iter().any(|row|row["accepted"]==true
        && row["systemDesign"]["component"]==role
        && origins.iter().any(|origin|row["generatorCommit"]==*origin));
    if productive{0.0}else{0.16}
}

pub(crate) fn instruction(topology:&Value)->String{
    let entries=topology["inventedComponents"].as_array();
    if entries.is_none_or(Vec::is_empty){return String::new();}
    let lines=entries.into_iter().flatten().take(6).map(|v|format!(
        "role={} newSource={} integratedInto={} contract={} nextImprovement={} adoptionCommit={}",
        v["role"],v["newFiles"],v["integrationFiles"],
        v["interfaceContract"],v["improvementMechanism"],v["introductionCommit"]
    )).collect::<Vec<_>>();
    format!("[EVOLVABLE INTELLIGENCE TOPOLOGY]\n{}\n\
    You may redesign these introduced components, connect them to new subsystems \
    or invent DIFFERENT roles with moduleIntroductions. A new role is effective \
    only after its implementation AND an existing execution consumer are \
    source-edited, measured and independently adopted.\n",lines.join("\n"))
}
