//! Extracted opt-in control request: original job, fixed-weight actor,
//! frozen task and owned source.  No operator-supplied control commit is used.
use crate::{state::AppState,engine};
use serde_json::{json,Value};
use std::path::Path;

pub(super) fn prepare(state:&AppState,mut body:Value)->Result<(Value,Value,Value),String>{
    let mut controlled=Value::Null;
    let mut treated_input=Value::Null;
    if let Some(id)=body["controlForGenerationId"].as_str(){
        if body["mode"]!="continuous"
            ||body["confirmation"]!="START_RSI_EVOLUTION"{
            return Err("대조 실행은 명시적으로 승인한 연속 실험이어야 합니다.".into());
        }
        let Some(g)=state.store.get("rsi_generation",id) else {
            return Err("대조할 실제 후손 세대가 없습니다.".into());
        };
        let Some(anchor)=crate::engine::rsi_closure::control_anchor(&state,&g) else{
            return Err("채택된 방법 A→실제 후손 능력 B의 원본 영수증을 찾지 못했습니다.".into());
        };
        let Some(treated)=anchor["treatedJobId"].as_str()
            .and_then(|id|state.store.get("job",id)) else{
            return Err("후손 B의 실제 모델 작업을 찾지 못했습니다.".into());
        };
        if treated["input"]["rsiPromptBase"].as_str()
            .is_none_or(|s|s.trim().is_empty())
            ||treated["input"]["rsiFrozenCases"].as_array().is_none_or(Vec::is_empty) {
            return Err("동일 작업·동결 사례로 재실행할 증거가 부족합니다.".into());
        }
        body["providerId"]=treated["input"]["providerId"].clone();
        body["model"]=treated["input"]["model"].clone();
        body["request"]=treated["input"]["rsiObjective"].clone();
        body["reasoningEffort"]=treated["input"]["reasoningEffort"].clone();
        body["focusFiles"]=json!([]);
        body["maxAttempts"]=json!(1);
        // The control receives at least B's available model budget, without
        // silently running unlimited paid tokens when B was bounded.
        let treatment_remaining=treated["input"]["rsiRemainingCampaignTokens"].as_u64();
        body["tokenBudget"]=treatment_remaining.map_or(json!(0),|remaining|
            json!(remaining.max(1000).min(2_000_000)));
        body["timeBudgetSeconds"]=json!(1800);
        body["selectionPolicy"]=json!("automatic");
        body["expandAfterGain"]=json!(false);
        body["nativeInduction"]=json!(false);
        body["modelEvolution"]=json!(false);
        body["stopAfterProof"]=json!(false);
        controlled=anchor;
        treated_input=treated["input"].clone();
    }
    Ok((body,controlled,treated_input))
}

pub(super) fn verify_source(state:&AppState,source:&Path,status:&Value,controlled:&Value)->Result<(),String>{
    if controlled.is_object(){
        let treated_campaign=state.store.get("rsi_campaign",
            controlled["treatedCampaignId"].as_str().unwrap_or(""));
        if treated_campaign.as_ref().is_none_or(|g|
            g["sourceDir"]!=json!(source)
                ||g["sourceBranch"]!=status["branch"])
            ||status["dirty"].as_u64().unwrap_or(u64::MAX)!=0 {
            return Err("대조 실험은 동일한 깨끗한 DOMINION 소스·브랜치에서만 실행합니다.".into());
        }
    }
    Ok(())
}

pub(super) fn apply_input(input:&mut Value,control:&Value,treated:&Value){
    if control.is_object(){
        input["prompt"]=treated["rsiPromptBase"].clone();
        input["rsiObjective"]=treated["rsiObjective"].clone();
        input["rsiFrozenCases"]=treated["rsiFrozenCases"].clone();
        input["rsiCausalControlOf"]=control["treatedJobId"].clone();
        input["rsiCausalControlGeneration"]=control["treatedGenerationId"].clone();
        input["rsiCausalControlCampaign"]=control["treatedCampaignId"].clone();
        input["rsiCausalControlCommit"]=control["methodOffGeneratorCommit"].clone();
        input["rsiSystemEvolution"]=json!(true);
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/api_rsi_control_request_tests.rs"]
mod tests;
