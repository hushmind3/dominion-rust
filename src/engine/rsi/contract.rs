//! Versioned, source-pinned experiment boundary between a descendant's
//! mutable experimentation recipe and the resident immutable evaluator.
//!
//! A new descendant may rewrite which policy/procedure it runs. The frozen
//! benchmark never trusts its chosen name: the exact source pins, recipe
//! revision and evaluation-suite fingerprint are reconstructed on both sides.
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct SourcePin {
    pub path:String,
    pub source_sha256:String,
}

#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct ExperimentContract {
    pub version:u16,
    pub generator_commit:String,
    pub campaign_id:String,
    pub source_tree_sha256:String,
    pub index_id:String,
    pub history_count:usize,
    pub target_domain:String,
    pub focus:Vec<SourcePin>,
    pub frozen_suite_sha256:String,
    pub recipe_owner:String,
    pub recipe_revision_sha256:String,
    pub recipe_family:String,
    pub evaluator_boundary:String,
}

fn sha(value:&str,n:usize)->bool {
    value.len()==n && value.bytes().all(|b|b.is_ascii_hexdigit())
}

fn digest(bytes:&[u8])->String {
    format!("{:x}",Sha256::digest(bytes))
}

fn recipe_family(plan:&Value)->&str {
    // The names are OPEN and authored in compiled source or inherited
    // model-written programs. A fixed A/B stage enum cannot decide what
    // kinds of experiments future descendants may design.
    for key in [
        &plan["openProgramPlan"]["stage"]["operatorId"],
        &plan["openProgramPlan"]["stage"]["mode"],
        &plan["openProgramPlan"]["program"]["revision"],
        &plan["metaPolicy"]["mode"],
        &plan["recursiveDiscovery"]["phase"],
        &plan["strategy"]["implementation"],
    ] {
        if let Some(value)=key.as_str().filter(|x|!x.is_empty()) {
            return value;
        }
    }
    "source_grounded_general_experiment"
}

/// Re-derive rather than accepting a self-declared "verified" field.
/// The editable descendant chooses the procedure and sources, not the
/// resident evaluator; this contract only pins what will actually run.
pub(crate) fn derive(plan:&Value,input:&Value)->Result<Value,String>{
    let commit=plan["generatorCommit"].as_str()
        .filter(|s|sha(s,40) && input["rsiGeneratorCommit"]==*s)
        .ok_or("rsi_contract: 후손/원본 커밋 불일치")?;
    let index=plan["indexId"].as_str()
        .filter(|id|!id.is_empty() && input["rsiIndexId"]==*id)
        .ok_or("rsi_contract: 소스 인덱스가 다릅니다")?;
    let campaign=input["rsiCampaignId"].as_str()
        .filter(|id|!id.is_empty())
        .ok_or("rsi_contract: 캠페인 계보가 없습니다")?;
    let domain=plan["domain"].as_str()
        .filter(|d|crate::engine::rsi_evolve::DOMAINS.contains(d))
        .ok_or("rsi_contract: 대상 영역이 유효하지 않습니다")?;
    let source_tree=plan["systemBlueprint"]["sourceTreeSha256"].as_str()
        .filter(|sha|sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit()))
        .ok_or("rsi_contract: 소스 트리 지문이 없습니다")?;
    let raw=plan["focusFiles"].as_array()
        .filter(|files|!files.is_empty() && files.len()<=3)
        .ok_or("rsi_contract: 구체적인 코드 편집 대상이 없습니다")?;
    let source=plan["systemBlueprint"]["sourceInventory"].as_array()
        .ok_or("rsi_contract: 실제 소스 인벤토리가 없습니다")?;
    let mut pins=Vec::<SourcePin>::new();
    for item in raw {
        let path=item.as_str()
            .filter(|p|crate::engine::rsi::safe_path(p))
            .ok_or("rsi_contract: 안전하지 않은 편집 대상")?;
        if pins.iter().any(|existing|existing.path==path){
            return Err("rsi_contract: 동일 소스 중복 지정".into());
        }
        let actual=source.iter().find(|row|row["path"]==path)
            .ok_or("rsi_contract: 소스 인벤토리에 없는 편집 대상")?;
        let hash=actual["sha256"].as_str().filter(|sha|sha.len()==64
            && sha.bytes().all(|b|b.is_ascii_hexdigit()))
            .ok_or("rsi_contract: 편집 소스 SHA-256이 없습니다")?;
        pins.push(SourcePin{
            path:path.to_string(),source_sha256:hash.to_string()
        });
    }
    let frozen=serde_json::to_vec(&input["rsiFrozenCases"])
        .map_err(|e|format!("rsi_contract: 고정 평가 지문 오류 {e}"))?;
    if input["rsiFrozenCases"].as_array().is_none_or(|cases|cases.len()<8){
        return Err("rsi_contract: 동결 평가 사례 부족".into());
    }
    // A method can change its algorithms and its experiment procedures.
    // This revision includes ALL executable procedures, not a hardcoded
    // three-generation or four-generation completion recipe.
    let recipe=json!({
        "strategy":plan["strategy"],
        "experimentOperator":plan["experimentOperator"],
        "openProgramPlan":plan["openProgramPlan"],
        "recursiveDiscovery":plan["recursiveDiscovery"],
        "principleProgramme":plan["principleProgramme"],
        "metaPolicy":plan["metaPolicy"],
        "metaActionPolicy":plan["metaActionPolicy"],
        "factorialCurriculum":plan["factorialCurriculum"],
        "mutualCurriculum":plan["mutualCurriculum"],
        "editPolicy":plan["editPolicy"],
        "modelRoute":plan["modelRoute"],
        "methodReturn":plan["downstreamMethodFeedback"],
        "methodReapplication":plan["methodReapplication"],
    });
    let recipe_bytes=serde_json::to_vec(&recipe)
        .map_err(|e|format!("rsi_contract: 개선법 직렬화 오류 {e}"))?;
    // Method designers may invent new names, including languages/symbols
    // not anticipated by this Rust version. Preserve arbitrary semantics in
    // the recipe SHA and mint a stable bounded display name when necessary.
    let raw_family=recipe_family(plan);
    let family=if raw_family.len()<=128 && raw_family.chars().all(|c|
        c.is_ascii_alphanumeric() || matches!(c,'_'|'-'|'.'|'/')) {
        raw_family.to_string()
    }else{
        format!("invented_recipe_{}", &digest(&recipe_bytes)[..20])
    };
    let contract=ExperimentContract{
        version:1,
        generator_commit:commit.to_string(),
        campaign_id:campaign.to_string(),
        source_tree_sha256:source_tree.to_string(),
        index_id:index.to_string(),
        history_count:plan["historyCount"].as_u64()
            .ok_or("rsi_contract: 계보 길이가 없습니다")? as usize,
        target_domain:domain.to_string(),
        focus:pins,
        frozen_suite_sha256:digest(&frozen),
        recipe_owner:"compiled_descendant_binary".into(),
        recipe_revision_sha256:digest(&recipe_bytes),
        recipe_family:family,
        evaluator_boundary:"resident_frozen_paired_worktree_oracle".into(),
    };
    serde_json::to_value(contract).map_err(|e|e.to_string())
}

/// Used by BOTH the resident orchestration process and the rebuilt worker.
/// A dishonest name or a stale source hash cannot satisfy this equality.
pub(crate) fn validate(plan:&Value,input:&Value)->Result<(),String>{
    let stored:ExperimentContract=serde_json::from_value(
        plan["experimentContract"].clone()
    ).map_err(|e|format!("rsi_contract: 유효하지 않은 실험 계약: {e}"))?;
    let expected=derive(plan,input)?;
    let actual=serde_json::to_value(stored).map_err(|e|e.to_string())?;
    if expected!=actual || input["rsiExperimentContract"]!=actual {
        return Err("rsi_contract: 실행 계획과 실제 실험 계약이 다릅니다".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests{
    use super::*;

    #[test]
    fn rejects_forged_source_pins_and_changed_mutable_recipes(){
        let path="src/engine/rsi/planner/compiled.rs";
        let mut input=json!({
            "rsiGeneratorCommit":"a".repeat(40),"rsiIndexId":"idx",
            "rsiCampaignId":"c","rsiFrozenCases":(0..8)
                .map(|i|json!({"id":i})).collect::<Vec<_>>()
        });
        let mut plan=json!({
            "generatorCommit":"a".repeat(40),
            "indexId":"idx","historyCount":1,"domain":"improver",
            "focusFiles":[path],
            "systemBlueprint":{"sourceTreeSha256":"f".repeat(64),
                "sourceInventory":[{"path":path,"sha256":"b".repeat(64)}]},
            "metaPolicy":{"mode":"new_experiment_search"}
        });
        plan["experimentContract"]=derive(&plan,&input).unwrap();
        input["rsiExperimentContract"]=plan["experimentContract"].clone();
        assert!(validate(&plan,&input).is_ok());
        plan["metaPolicy"]["mode"]=json!("different_rewrite");
        assert!(validate(&plan,&input).is_err());
        plan["metaPolicy"]["mode"]=json!("new_experiment_search");
        plan["systemBlueprint"]["sourceInventory"][0]["sha256"]=json!("c".repeat(64));
        assert!(validate(&plan,&input).is_err());
        plan["systemBlueprint"]["sourceInventory"][0]["sha256"]=json!("b".repeat(64));
        input["rsiFrozenCases"][0]["id"]=json!("altered-holdout");
        assert!(validate(&plan,&input).is_err(),
            "a previously pinned frozen suite must not be substituted");
        input["rsiFrozenCases"][0]["id"]=json!(0);
        plan["focusFiles"][0]=json!("src/engine/unknown.rs");
        assert!(validate(&plan,&input).is_err(),
            "an unindexed source edit must not satisfy the original contract");
    }
}
