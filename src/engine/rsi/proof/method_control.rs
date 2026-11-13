//! Independent method-off control for a model-authored descendant.
//! Controls must be genuine pre-A generations evaluated on identical frozen
//! cases and authored by the same *audited* model route. A source-intervention
//! contrast is nonrandomized; it cannot establish universal causal effects.
use crate::{engine::rsi,state::AppState};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn digest(v:&Value)->String {
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}

fn same_task(a:&Value,b:&Value)->bool {
    let a=&a["input"];let b=&b["input"];
    a["rsiFrozenCases"].as_array().is_some_and(|v|!v.is_empty())
        && a["rsiFrozenCases"]==b["rsiFrozenCases"]
        && a["rsiPromptBase"].as_str().is_some_and(|v|!v.trim().is_empty())
        && a["rsiPromptBase"]==b["rsiPromptBase"]
        && a["rsiObjective"]==b["rsiObjective"]
        && a["reasoningEffort"]==b["reasoningEffort"]
        && a["rsiSourceBranch"]==b["rsiSourceBranch"]
        // A weaker budget could make the control fail for unrelated reasons.
        && match (a["rsiRemainingCampaignTokens"].as_u64(),
                   b["rsiRemainingCampaignTokens"].as_u64()){
            (Some(a),Some(b))=>a>=b,
            (None,_)=>true,
            _=>false
        }
}

/// Reopen the original usage ledger; an input provider label alone cannot
/// establish which fixed-weight model actually authored the candidate.
fn audited_actor(state:&AppState,job:&Value)->Option<String>{
    let id=job["id"].as_str()?;
    let session=job["sessionId"].as_str()?;
    if !state.store.get("session",session).is_some_and(|s|
        s["rsiRun"]==true && s["rsiGenerationJobId"]==id){return None;}
    let start=job["rsiAuditStartRowid"].as_i64().filter(|v|*v>=0)?;
    let end=job["rsiAuditEndRowid"].as_i64().filter(|v|*v>start)?;
    let rows=state.store.model_usage_for_session_in_window(session,start,end).ok()?;
    let provider=job["input"]["providerId"].as_str()?.trim();
    let model=job["input"]["model"].as_str()?.trim();
    if provider.is_empty()||model.is_empty(){return None;}
    let mut calls=0usize;
    for row in rows {
        if row["event"]!="model.usage"
            ||row["detail"]["sessionId"]!=session
            ||row["detail"]["purpose"]!="chat"
            ||row["detail"]["totalTokens"].as_u64().unwrap_or(0)==0 {
            continue;
        }
        if row["detail"]["providerId"]!=provider
            ||row["detail"]["model"]!=model {return None;}
        calls+=1;
    }
    (calls>0).then(||format!("{provider}/{model}"))
}

fn verified_control(state:&AppState,g:&Value,pre:&Value)->Option<(Value,Value)>{
    if g["generatorCommit"]!=*pre
        ||g["candidateOrigin"]!="model_driven_detached_worktree"{return None;}
    let candidate=state.store.get("rsi_candidate",g["candidateId"].as_str()?)?;
    if candidate["pairedComparisonComplete"]!=true
        ||candidate["id"]!=g["candidateId"]
        ||candidate["baseCommit"]!=g["generatorCommit"]
        ||candidate["evaluation"]["source"]!="server_invoked_local_paired_git_worktrees"
        ||candidate["evaluation"]["metrics"]!=g["evaluation"]["metrics"]
        ||candidate["evaluation"]["evidenceSha256"]!=g["evaluation"]["evidenceSha256"]
        ||rsi::replayed_pair_metrics(state,&candidate).is_none() {return None;}
    let job=state.store.get("job",g["jobId"].as_str()?)?;
    if job["kind"]!="rsi_generation"
        ||job["id"]!=g["jobId"]
        ||job["input"]["rsiGeneratorCommit"]!=g["generatorCommit"]
        ||job["result"]["rsi"]["candidateId"]!=candidate["id"]
        ||job["rsiCompiledPlanExecuted"]["source"]!="running_compiled_descendant_binary"
        ||job["rsiCompiledPlanExecuted"]["generatorCommit"]!=g["generatorCommit"] {
        return None;
    }
    Some((candidate,job))
}

/// Compare same frozen quality metrics as a second supported capability
/// signal when binary pass/fail accuracy is saturated. Baselines must match
/// in actual floating-point measurements, not only in a summary flag.
fn matched_quality(treated:&Value,control:&Value)->Option<(bool,bool)>{
    let t=treated["metrics"].as_array()?.iter()
        .find(|m|m["key"]=="quality");
    let c=control["metrics"].as_array()?.iter()
        .find(|m|m["key"]=="quality");
    let (Some(t),Some(c))=(t,c) else{
        return (t.is_none()&&c.is_none()).then_some((false,false));
    };
    let (tb,cb,tv,cv)=(t["baseline"].as_f64()?,c["baseline"].as_f64()?,
        t["candidate"].as_f64()?,c["candidate"].as_f64()?);
    if ![tb,cb,tv,cv].iter().all(|v|v.is_finite())
        ||(tb-cb).abs()>1e-9{return None;}
    let favorable=t["improved"]==true &&tv>cv+1e-9;
    let adverse=c["improved"]==true &&cv>tv+1e-9;
    Some((favorable,adverse))
}

/// Compare model-produced candidate outcomes on identical original holdouts,
/// including baseline outcomes. Neither reported gains nor wall-clock savings
/// count as a downstream ability to solve a previously failing task.
fn contrast(treated:&Value,control:&Value)->Option<(usize,usize,usize)>{
    let mut before=BTreeMap::new();
    for row in control["evaluation"]["metrics"]["measurements"].as_array()? {
        let id=row["id"].as_str()?.to_string();
        if before.insert(id,row).is_some(){return None;}
    }
    let mut seen=BTreeSet::new();
    let (mut gains,mut losses,mut count)=(0,0,0);
    for row in treated["evaluation"]["metrics"]["measurements"].as_array()? {
        let id=row["id"].as_str()?;
        if !seen.insert(id){return None;}
        let old=before.get(id)?;
        if row["domain"]!=old["domain"]
            ||row["holdout"]!=old["holdout"]
            ||row["baselinePass"]!=old["baselinePass"] {return None;}
        if row["holdout"]!=true ||row["domain"]=="improver"{continue;}
        let new=row["candidatePass"].as_bool()?;
        let old_pass=old["candidatePass"].as_bool()?;
        let (quality_gain,quality_loss)=if new&&old_pass {
            matched_quality(row,old)?
        }else{(false,false)};
        count+=1;
        if (new&&!old_pass &&row["baselinePass"]==false)||quality_gain {gains+=1;}
        if (!new&&old_pass)||quality_loss {losses+=1;}
    }
    (seen.len()==before.len()&&count>=2).then_some((gains,losses,count))
}

/// Reconstruct a matched source intervention from original jobs and paired
/// receipts. Never invent a control from a forecast, prompt or mutable flag.
pub(super) fn evidence(
    state:&AppState,first:&Value,second:&Value,b_job:&Value,b_candidate:&Value
)->Value{
    let fingerprint=digest(&json!([
        b_job["input"]["rsiFrozenCases"],b_job["input"]["rsiPromptBase"],
        b_job["input"]["rsiObjective"],b_job["input"]["reasoningEffort"]
    ]));
    let pending=json!({
        "source":"source_ablation_matched_control_v1",
        "methodOffGeneratorCommit":first["generatorCommit"],
        "methodOnGeneratorCommit":first["acceptedCommit"],
        "treatedJobId":b_job["id"],"treatedGenerationId":second["id"],"treatedCandidateId":b_candidate["id"],
        "taskSha256":fingerprint,
        "matchedControlFound":false,"measuredAdvantage":false,
        "nonrandomized":true,"causalEffectProven":false
    });
    if first["acceptedCommit"]!=second["generatorCommit"]
        ||second["candidateOrigin"]!="model_driven_detached_worktree"
        ||b_candidate["target"]=="improver"
        ||b_job["result"]["rsi"]["candidateId"]!=b_candidate["id"] {return pending;}
    let Some(actor)=audited_actor(state,b_job) else {return pending};
    let mut matched=Vec::new();
    for row in state.store.all("rsi_generation").into_iter().take(2048) {
        if row["candidateId"]==b_candidate["id"]{continue;}
        let Some((c,j))=verified_control(state,&row,&first["generatorCommit"])
            else{continue};
        if j["input"]["rsiCausalControlOf"]!=b_job["id"]
            ||j["input"]["rsiCausalControlCommit"]!=first["generatorCommit"]{
            continue;
        }
        if c["cases"]!=b_candidate["cases"]
            ||c["oracleVersion"]!=b_candidate["oracleVersion"]
            ||!same_task(&j,b_job)
            ||audited_actor(state,&j).as_deref()!=Some(actor.as_str()) {continue;}
        if c["paths"].as_array().is_none_or(|paths|paths.is_empty()
            ||paths.iter().any(|path|!path.as_str().is_some_and(rsi::safe_path))){
            continue;
        }
        if let Some((wins,losses,count))=contrast(b_candidate,&c){
            matched.push((wins,losses,count,j["id"].clone(),c["id"].clone()));
        }
    }
    if matched.is_empty(){return pending;}
    // Guard against selectively presenting a weak control. A positive
    // result must beat EVERY matching actual control from this source.
    matched.sort_by(|a,b|a.0.cmp(&b.0).then_with(||b.1.cmp(&a.1))
        .then_with(||a.3.as_str().cmp(&b.3.as_str())));
    let gain=matched.iter().all(|v|v.0>0&&v.1==0);
    json!({
        "source":"source_ablation_matched_control_v1",
        "methodOffGeneratorCommit":first["generatorCommit"],
        "methodOnGeneratorCommit":first["acceptedCommit"],
        "treatedJobId":b_job["id"],"treatedGenerationId":second["id"],"treatedCandidateId":b_candidate["id"],
        "taskSha256":fingerprint,"auditedModelActor":actor,
        "matchedControlFound":true,"controlCount":matched.len(),
        "minimumHoldoutPassAdvantage":matched[0].0,
        "maximumHoldoutLoss":matched.iter().map(|v|v.1).max().unwrap_or(0),
        "matchedHoldouts":matched[0].2,
        "controlJobIds":matched.iter().map(|v|v.3.clone()).collect::<Vec<_>>(),
        "controlCandidateIds":matched.iter().map(|v|v.4.clone()).collect::<Vec<_>>(),
        "measuredAdvantage":gain,"nonrandomized":true,"causalEffectProven":false
    })
}
#[cfg(test)]
#[path = "../../../../tests/unit/engine_rsi_method_control_tests.rs"]
mod tests;
