//! Self-rewritable causal-experiment curriculum for open-ended RSI.
//! Distinguishes A-only, B-only and A+B source interventions on identical
//! frozen baseline/profile/cases. A joint gain is not assigned to either role.
//! All executable intelligence here belongs to the rebuilt descendant.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};
fn sha(value:&Value)->String{
    format!("{:x}",Sha256::digest(serde_json::to_vec(value).unwrap_or_default()))
}
fn live(row:&Value)->bool{
    row["path"].as_str().is_some_and(crate::engine::rsi::safe_path)
        && row["role"].as_str().is_some_and(|r|!r.is_empty()&&r.len()<=64)
        && row["sourceSha256"].as_str().is_some_and(|s|
            s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit()))
}
fn observed_arm(g:&Value,left:&str,right:&str)->Option<usize>{
    if g["pairedRevalidated"]!=true
        || !g["candidateId"].is_string()
        || !g["evaluation"]["verdict"].is_string()
        || !g["evaluation"]["metrics"]["holdoutEvidence"].is_array()
        || !g["generatorCommit"].as_str().is_some_and(|s|s.len()==40)
        || !g["comparisonProfileSha256"].as_str().is_some_and(|s|s.len()==64)
        || g["systemDesign"]["source"]!="model_authored_system_design"
        || g["systemDesign"]["generatorCommit"]!=g["generatorCommit"]
        || g["systemDesign"]["generationJobId"]!=g["jobId"]{return None;}
    let changed=g["changedSourcePaths"].as_array().filter(|v|!v.is_empty())?;
    let mapping=&g["systemDesign"]["componentFiles"];
    let mut roles=BTreeSet::new();
    for file in changed {
        let path=file.as_str()?;
        if !crate::engine::rsi::safe_path(path){return None;}
        let a=mapping[left].as_array().is_some_and(|files|
            files.iter().any(|f|f.as_str()==Some(path)));
        let b=mapping[right].as_array().is_some_and(|files|
            files.iter().any(|f|f.as_str()==Some(path)));
        if a==b{return None;}
        roles.insert(if a{left}else{right});
    }
    if roles.len()==1{Some(if roles.contains(left){0}else{1})}
    else if roles.len()==2{Some(2)}else{None}
}
type Block=(String,String,String,bool);
type Arms=[Vec<f64>;3];
/// Factorial contrasts are observational until a separately controlled
/// experiment identifies an interaction. Never mix unrelated baselines.
fn evidence(history:&[Value],left:&str,right:&str)->(Value,[usize;3]){
    let mut blocks=BTreeMap::<Block,Arms>::new();
    let mut counts=[0usize;3];
    let mut seen=BTreeSet::new();
    for g in history {
        let Some(arm)=observed_arm(g,left,right) else{continue};
        let base=g["generatorCommit"].as_str().unwrap_or("");
        let profile=g["comparisonProfileSha256"].as_str().unwrap_or("");
        let mut measured=false;
        for case in g["evaluation"]["metrics"]["holdoutEvidence"]
            .as_array().into_iter().flatten(){
            let (Some(id),Some(before),Some(after))=(
                case["id"].as_str(),case["baselinePass"].as_bool(),
                case["candidatePass"].as_bool())else{continue};
            if id.is_empty(){continue;}
            let gains=case["metricGains"].as_u64().unwrap_or(0) as f64;
            let losses=case["metricRegressions"].as_u64().unwrap_or(0) as f64;
            let delta=(i32::from(after)-i32::from(before)) as f64
                +0.25*(gains-losses).clamp(-1.0,1.0);
            blocks.entry((base.into(),profile.into(),id.into(),before))
                .or_insert_with(||[Vec::new(),Vec::new(),Vec::new()])[arm].push(delta);
            measured=true;
        }
        let key=(g["candidateId"].as_str().unwrap_or("").to_string(),arm);
        if measured&&seen.insert(key){counts[arm]+=1;}
    }
    let mut contrasts=Vec::new();
    for ((base,profile,id,before),arms) in blocks {
        if arms.iter().any(Vec::is_empty){continue;}
        let mean=|v:&Vec<f64>|v.iter().sum::<f64>()/v.len() as f64;
        let a=mean(&arms[0]);let b=mean(&arms[1]);let joint=mean(&arms[2]);
        contrasts.push(json!({"generatorCommit":base,"profileSha256":profile,
            "holdoutId":id,"baselinePass":before,
            "isolatedLeft":a,"isolatedRight":b,"joint":joint,
            "interactionContrast":joint-a-b,
            "causalityProven":false}));
    }
    let mut report=json!({"source":"matched_frozen_factorial_observation_v1",
        "variantGenerations":{"leftOnly":counts[0],"rightOnly":counts[1],
            "joint":counts[2]},
        "matchedHoldouts":contrasts.len(),
        "contrasts":contrasts.into_iter().take(12).collect::<Vec<_>>(),
        "causalityProven":false});
    report["sha256"]=json!(sha(&report));
    (report,counts)
}
/// Inherit the next experimental arm from an independently adopted SOURCE
/// mutation compiled as the current descendant. A failed attempt does not
/// advance the cursor. The actual candidate diff must match the prior arm.
fn inherited_curriculum(history:&[Value],frontier:&[Value],commit:&str)
    ->Option<(Value,Value,usize,Value)>{
    let ancestor=history.iter().rev().find(|g|
        g["accepted"]==true && g["acceptedCommit"]==commit
            && g["pairedRevalidated"]==true
            && g["selfControllerVerified"]==true
            && g["evaluation"]["metrics"]["eligible"]==true
            && g["candidateOrigin"]=="model_driven_detached_worktree")?;
    let program=&ancestor["systemAgenda"]["factorialExperiment"];
    if program["source"]!="compiled_evolvable_causal_ecology_v1"
        || program["generatorCommit"]!=ancestor["generatorCommit"]
        || program["evaluationOwner"]=="model_self_report" {return None;}
    let a=program["left"]["role"].as_str()?;
    let b=program["right"]["role"].as_str()?;
    let required=match program["phase"].as_str()?{
        "isolate_left"=>0,"isolate_right"=>1,"probe_joint"=>2,_=>return None
    };
    if observed_arm(ancestor,a,b)!=Some(required){return None;}
    let next=(required+1).min(3);
    let rebind=|row:&Value|frontier.iter().find(|current|
        current["role"]==row["role"] && current["path"]==row["path"]
            && live(current)).cloned();
    let left=rebind(&program["left"])?;
    let right=rebind(&program["right"])?;
    if left["role"]==right["role"]{return None;}
    let root=if program["lineageRoot"].is_string(){
        program["lineageRoot"].clone()
    }else{ancestor["jobId"].clone()};
    Some((left,right,next,root))
}

pub(crate) fn design(history:&[Value],frontier:&[Value],pair:&Value,commit:&str)->Value{
    let inherited=inherited_curriculum(history,frontier,commit);
    let (a,b,forced_arm,lineage_root)=if let Some((a,b,next,root))=inherited{
        (a,b,Some(next),root)
    }else{
        let a=&pair["primary"];let b=&pair["partner"];
        if pair["source"]!="compiled_descendant_reciprocal_evolution_v1"
            || !live(a)||!live(b)||a["role"]==b["role"]
            || a["path"]==b["path"]||!frontier.contains(a)||!frontier.contains(b){
            return Value::Null;
        }
        (a.clone(),b.clone(),None,Value::Null)
    };
    let (observations,counts)=evidence(history,
        a["role"].as_str().unwrap_or(""),b["role"].as_str().unwrap_or(""));
    let missing=forced_arm.or_else(||counts.iter().position(|v|*v==0));
    let phase=match missing{
        Some(0)=>"isolate_left",Some(1)=>"isolate_right",
        Some(2)=>"probe_joint",_=>"invent_new_interaction"
    };
    let target=if missing==Some(1){b.clone()}else{a.clone()};
    let focus=match missing{Some(0)=>vec![a.clone()],
        Some(1)=>vec![b.clone()],_=>vec![a.clone(),b.clone()]};
    let mut result=json!({
        "source":"compiled_evolvable_causal_ecology_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "left":a,"right":b,"target":target,
        "focus":focus,"phase":phase,
        "lineageRoot":lineage_root,
        "descendantContinuation":forced_arm.is_some(),
        "observations":observations,"externalEvaluatorUnchanged":true,
        "selfRewriteAllowed":true,"causalityProven":false,"qualityProven":false
    });
    result["sha256"]=json!(sha(&result));
    result
}
pub(crate) fn apply(agenda:&mut Value,plan:&Value){
    if plan["source"]!="compiled_evolvable_causal_ecology_v1"
        || agenda["inheritedStage"].is_object()
        || agenda["target"]!=plan["left"]{return;}
    let Some(frontier)=agenda["frontier"].as_array()else{return};
    let Some(focus)=plan["focus"].as_array()else{return};
    if focus.is_empty()||focus.len()>2||focus.iter().any(|r|
        !live(r)||!frontier.contains(r))||!frontier.contains(&plan["target"]){
        return;
    }
    agenda["target"]=plan["target"].clone();
    agenda["focus"]=plan["focus"].clone();
    agenda["factorialExperiment"]=plan.clone();
}
pub(crate) fn instruction(plan:&Value)->String{
    if plan["source"]!="compiled_evolvable_causal_ecology_v1"{
        return String::new();
    }
    let directive=match plan["phase"].as_str().unwrap_or(""){
        "isolate_left"|"isolate_right"=>
            "Edit ONLY the selected source role; inspect the other without changing it.",
        "probe_joint"=>
            "Invent a joint source rewrite and compare against independent A and B source variants on identical frozen cases.",
        _=>
            "Invent a new interaction mechanism from matched observational contrasts, not a recycled patch."
    };
    format!("[EDITABLE FACTORIAL INTELLIGENCE EXPERIMENT] \
        phase={} sha={} A={} source={} B={} source={} \
        matched holdouts={} observations={}. {} \
        Independently verified source edits become the next compiled agent. \
        This experiment designer is itself editable; factorial contrasts \
        do not imply causal proof.",
        plan["phase"],plan["sha256"],plan["left"]["role"],
        plan["left"]["path"],plan["right"]["role"],plan["right"]["path"],
        plan["observations"]["matchedHoldouts"],
        plan["observations"]["variantGenerations"],directive)
}
#[cfg(test)]
mod tests{
    use super::*;
    fn role(name:&str,path:&str)->Value{json!({"role":name,
        "path":path,"sourceSha256":"a".repeat(64),"domain":"improver"})}
    fn generation(id:&str,paths:&[&str])->Value{json!({
        "jobId":id,"generatorCommit":"b".repeat(40),
        "candidateId":id,"pairedRevalidated":true,
        "comparisonProfileSha256":"c".repeat(64),
        "changedSourcePaths":paths,
        "systemDesign":{"source":"model_authored_system_design",
            "generatorCommit":"b".repeat(40),"generationJobId":id,
            "componentFiles":{"memory":["src/engine/memory.rs"],
                "tools":["src/engine/tools.rs"]}},
        "evaluation":{"verdict":"improved","metrics":{"holdoutEvidence":[
            {"id":"case","baselinePass":false,"candidatePass":true,
                "metricGains":0,"metricRegressions":0}]}}
    })}
    #[test]
    fn source_realization_controls_factorial_phase_and_cross_generation_focus(){
        let left=role("memory","src/engine/memory.rs");
        let right=role("tools","src/engine/tools.rs");
        let frontier=vec![left.clone(),right.clone()];
        let pair=json!({"source":"compiled_descendant_reciprocal_evolution_v1",
            "primary":left,"partner":right});
        assert_eq!(design(&[],&frontier,&pair,"x")["phase"],"isolate_left");
        let one=generation("a",&["src/engine/memory.rs"]);
        assert_eq!(design(&[one.clone()],&frontier,&pair,"x")["phase"],
            "isolate_right");
        let two=generation("b",&["src/engine/tools.rs"]);
        assert_eq!(design(&[one.clone(),two.clone()],&frontier,&pair,"x")["phase"],
            "probe_joint");
        let three=generation("c",&["src/engine/memory.rs","src/engine/tools.rs"]);
        let final_plan=design(&[one,two,three],&frontier,&pair,"x");
        assert_eq!(final_plan["observations"]["matchedHoldouts"],1);
        let mut agenda=json!({"inheritedStage":null,"target":pair["primary"],
            "frontier":frontier,"focus":[pair["primary"]]});
        apply(&mut agenda,&final_plan);
        assert_eq!(agenda["focus"].as_array().unwrap().len(),2);
    }
    #[test]
    fn adopted_descendant_advances_to_complement_and_rebinds_source_hash(){
        let first=role("memory","src/engine/memory.rs");
        let second=role("tools","src/engine/tools.rs");
        let pair=json!({"source":"compiled_descendant_reciprocal_evolution_v1",
            "primary":first,"partner":second});
        let old=design(&[],&[first.clone(),second.clone()],&pair,"a");
        assert_eq!(old["phase"],"isolate_left");
        let mut parent=generation("child",&["src/engine/memory.rs"]);
        parent["accepted"]=json!(true);
        parent["acceptedCommit"]=json!("b".repeat(40));
        parent["candidateOrigin"]=json!("model_driven_detached_worktree");
        parent["selfControllerVerified"]=json!(true);
        parent["evaluation"]["metrics"]["eligible"]=json!(true);
        parent["systemAgenda"]["factorialExperiment"]=old;
        parent["systemDesign"]["generatorCommit"]=json!("a");
        parent["generatorCommit"]=json!("a");
        // Recompiled roles can now have different source revisions.
        let mut next_memory=first;
        next_memory["sourceSha256"]=json!("e".repeat(64));
        let frontier=vec![next_memory,second];
        let next=design(&[parent.clone()],&frontier,&Value::Null,
            &"b".repeat(40));
        assert_eq!(next["phase"],"isolate_right");
        assert_eq!(next["target"]["role"],"tools");
        assert_eq!(next["descendantContinuation"],true);
        let mut invalid=parent;
        invalid["pairedRevalidated"]=json!(false);
        assert!(inherited_curriculum(&[invalid],&frontier,&"b".repeat(40)).is_none());
    }
    #[test]
    fn claimed_roles_without_changed_sources_never_count(){
        let mut forged=generation("wrong",&["src/engine/jobs.rs"]);
        assert!(observed_arm(&forged,"memory","tools").is_none());
        forged["changedSourcePaths"]=json!(["src/engine/memory.rs"]);
        forged["pairedRevalidated"]=json!(false);
        assert!(observed_arm(&forged,"memory","tools").is_none());
    }
}
