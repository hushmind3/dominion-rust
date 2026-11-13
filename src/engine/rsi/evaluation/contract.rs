//! Immutable, typed supervisor-to-descendant experiment boundary.
//! The caller may evolve its strategies and meta-programs, but may not change
//! the source/evaluator/budget identity of an already frozen experiment.
//! This struct is created from the compiled descendant's actual plan and
//! independently reconstructed before a paid worker starts.
use serde::{Serialize,Deserialize};
use serde_json::Value;
use sha2::{Digest,Sha256};

#[derive(Clone,Serialize,Deserialize,Debug,PartialEq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct FrozenExperiment {
    source:String,
    schema_version:u16,
    generator_commit:String,
    history_count:usize,
    source_tree_sha256:String,
    evaluation_profile_sha256:String,
    domain:String,
    focus_paths:Vec<String>,
    meta_policy_revision:String,
    meta_program_revision:Option<String>,
    meta_experiment_dispatch_revision:Option<String>,
    open_program_revision:Option<String>,
    inherited_method_revision:Option<String>,
    edit_policy_revision:Option<String>,
    signature:String,
}
fn sha(s:&str,size:usize)->bool {
    s.len()==size && s.bytes().all(|b|b.is_ascii_hexdigit())
}
fn digest(value:&Value)->String {
    format!("{:x}",Sha256::digest(serde_json::to_vec(value).unwrap_or_default()))
}

/// A source-edit intervention may be redesigned by a recompiled descendant,
/// but its proposed scope must obey the independent frozen boundary. This
/// checker validates observable action effects; it does not recalculate
/// the descendant's mutable experiment ranking or topology.
pub(crate) fn valid_dispatch(plan:&Value,dispatch:&Value)->bool {
    let protocol=&plan["metaEvolutionProgram"];
    if protocol.is_null(){return dispatch.is_null();}
    if dispatch["source"]!="compiled_meta_experiment_dispatch_v1"
        || dispatch["generatorCommit"]!=plan["generatorCommit"]
        || dispatch["protocolRevision"]!=protocol["revision"]
        || dispatch["operatorId"]!=protocol["operatorId"]
        || dispatch["experiment"]!=protocol["operator"]["experiment"]
        || dispatch["targetPath"]!=protocol["target"]["path"]
        || dispatch["revision"].as_str().is_none_or(|s|!sha(s,64)) {
        return false;
    }
    let mut unsigned=dispatch.clone();
    let Some(map)=unsigned.as_object_mut() else{return false;};
    map.remove("revision");
    if digest(&unsigned)!=dispatch["revision"] {return false;}
    let Some(mode)=dispatch["experiment"].as_str() else{return false;};
    let Some(target)=dispatch["targetPath"].as_str() else{return false;};
    let focus=plan["focusFiles"].as_array();
    if !focus.is_some_and(|items|items.iter().any(|p|p.as_str()==Some(target))) {
        return false;
    }
    let paths=dispatch["allowedPaths"].as_array();
    if paths.is_some_and(|paths|paths.is_empty() || paths.len()>8
        || paths.iter().any(|p|p.as_str().is_none_or(|p|
            !crate::engine::rsi::safe_path(p)
            || !focus.is_some_and(|focus|focus.iter().any(|f|f.as_str()==Some(p)))))) {
        return false;
    }
    let isolated=paths.is_some_and(|list|list.len()==1
        && list[0].as_str()==Some(target));
    match mode {
        "paired_source" => paths.is_none()
            && dispatch["needsMethodDisabledControl"]==false
            && dispatch["contrastStatus"]=="normal_independent_frozen_source_pair",
        "isolated_source" => isolated
            && dispatch["needsMethodDisabledControl"]==false
            && dispatch["contrastStatus"]=="one_source_role_with_frozen_pair",
        "factorial_source" => isolated
            && dispatch["needsMethodDisabledControl"]==false
            && dispatch["contrastStatus"]=="isolated_factor_pending_independent_interaction",
        "meta_policy_rewrite" => isolated
            && plan["domain"]=="improver"
            && (target.starts_with("src/engine/rsi/meta/")
                || target=="src/engine/rsi_program_graph.rs"
                || target=="src/engine/rsi_principle_learning.rs")
            && dispatch["needsMethodDisabledControl"]==false
            && dispatch["contrastStatus"]=="isolated_meta_policy_source_revision",
        "method_control" => isolated
            && plan["methodTransfer"]["methodId"].is_string()
            && plan["methodTransfer"]["domain"]==plan["domain"]
            && dispatch["needsMethodDisabledControl"]==true
            && dispatch["contrastStatus"]=="paired_real_method_enabled_disabled_workers",
        "cross_component" => paths.is_some_and(|list|list.len()>=2
                && list.iter().any(|p|p.as_str()==Some(target)))
            && dispatch["needsMethodDisabledControl"]==false
            && dispatch["contrastStatus"]=="joint_source_observation_no_isolated_causal_credit",
        _=>false
    }
}

/// Only boundary metadata is frozen. The selected source patch, new model
/// architecture, newly invented improvement operator and its hypothesis are
/// the editable intelligence produced INSIDE the sealed experiment.
pub(crate) fn bind(plan:&Value,evaluation_profile_sha256:&str)->Result<Value,String>{
    if !valid_dispatch(plan,&plan["metaExperimentDispatch"]){
        return Err("후손의 실험 종류·수정 범위가 동결된 실행 계약을 벗어났습니다.".into());
    }
    let commit=plan["generatorCommit"].as_str()
        .filter(|s|sha(s,40)).ok_or("세대 소스 버전 없음")?;
    let tree=plan["systemBlueprint"]["sourceTreeSha256"].as_str()
        .filter(|s|sha(s,64)).ok_or("후손 소스 트리 해시 없음")?;
    let policy=plan["metaPolicy"]["revision"].as_str()
        .filter(|s|sha(s,64)).ok_or("후손 메타 정책 리비전 없음")?;
    if !sha(evaluation_profile_sha256,64){return Err("동결 평가 프로필 해시 없음".into());}
    let domain=plan["domain"].as_str()
        .filter(|s|crate::engine::rsi_evolve::DOMAINS.contains(s))
        .ok_or("후손 작업 분야가 없습니다")?;
    let focus=plan["focusFiles"].as_array()
        .filter(|paths|paths.len()<=8)
        .ok_or("후손 소스 편집 경계 없음")?;
    let mut paths=Vec::<String>::new();
    for item in focus {
        let path=item.as_str().filter(|path|crate::engine::rsi::safe_path(path))
            .ok_or("후손 편집 대상이 소스 경계 밖입니다")?;
        if paths.iter().any(|old|old==path){return Err("후손 편집 대상 중복".into());}
        paths.push(path.to_owned());
    }
    let optional_sha=|obj:&Value,field:&str|->Result<Option<String>,String>{
        obj[field].as_str().map(|v|{
            if sha(v,64){Ok(v.to_string())}
            else{Err(format!("후손 {field} 버전 해시 오류"))}
        }).transpose()
    };
    let meta=optional_sha(&plan["metaEvolutionProgram"],"revision")?;
    let meta_scope=optional_sha(&plan["metaExperimentDispatch"],"revision")?;
    let open=optional_sha(&plan["openImprovementProgram"],"revision")?;
    let inherited=optional_sha(&plan["methodTransfer"],"revision")?;
    let edit=plan["editPolicy"]["revision"].as_str()
        .filter(|s|!s.trim().is_empty()&&s.len()<=128).map(str::to_owned);
    let mut proof=FrozenExperiment {
        source:"immutable_compiled_rsi_experiment_contract_v1".into(),
        schema_version:1,generator_commit:commit.into(),
        history_count:plan["historyCount"].as_u64().ok_or("후손 역사 길이 없음")? as usize,
        source_tree_sha256:tree.into(),evaluation_profile_sha256:evaluation_profile_sha256.into(),
        domain:domain.into(),focus_paths:paths,
        meta_policy_revision:policy.into(),meta_program_revision:meta,
        meta_experiment_dispatch_revision:meta_scope,
        open_program_revision:open,inherited_method_revision:inherited,
        edit_policy_revision:edit,signature:String::new()
    };
    let mut unsigned=serde_json::to_value(&proof).map_err(|e|e.to_string())?;
    if let Some(fields)=unsigned.as_object_mut(){fields.remove("signature");}
    proof.signature=digest(&unsigned);
    serde_json::to_value(proof).map_err(|e|e.to_string())
}

/// Verify by reconstructing the same contract, not by trusting a model-
/// reported 'verified' flag or a mutable campaign's convenience fields.
pub(crate) fn valid(plan:&Value,evaluation_profile_sha256:&str,observed:&Value)->bool {
    let Ok(actual)=serde_json::from_value::<FrozenExperiment>(observed.clone())
        else{return false;};
    actual.schema_version==1
        && actual.source=="immutable_compiled_rsi_experiment_contract_v1"
        && sha(&actual.signature,64)
        && bind(plan,evaluation_profile_sha256).is_ok_and(|expected|expected==*observed)
}
