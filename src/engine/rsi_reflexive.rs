//! Recompiled reflexive intelligence curriculum: improve the improver,
//! use its *new executable binary* to improve a different subsystem, then
//! let that descendant improve the improver again.  The independent oracle
//! (outside the editable descendant) remains the only adoption authority.
//!
//! This program plans edits and records witnessed lineage transitions.
//! A proposed stage is never a proof of actual learning or capability gain.
use crate::engine::{rsi,rsi_mutual_program};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;

fn digest(v:&Value)->String {
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn actual_source(g:&Value,path:&str)->bool {
    rsi_mutual_program::adopted_source(g)
        && g["changedSourcePaths"].as_array().is_some_and(|paths|
            paths.iter().any(|p|p.as_str()==Some(path)))
        && g["acceptedSourceHashes"][path].as_str().is_some_and(|h|
            h.len()==64 && h.bytes().all(|b|b.is_ascii_hexdigit()))
}
fn meta_source(path:&str)->bool {
    matches!(path,
        "src/engine/rsi_meta_controller.rs"
        |"src/engine/rsi_factorial.rs"
        |"src/engine/rsi_reflexive.rs"
        |"src/engine/rsi_mutual_program.rs"
        |"src/engine/rsi_evolve.rs"
        |"src/engine/rsi_system.rs"
        |"src/engine/rsi_interaction.rs")
}
fn current<'a>(frontier:&'a [Value],path:&str,allowed:&[&str])->Option<&'a Value> {
    frontier.iter().find(|f|f["path"]==path
        && f["domain"].as_str().is_some_and(|d|allowed.contains(&d))
        && f["sourceSha256"].as_str().is_some_and(|sha|sha.len()==64)
        && rsi::safe_path(path))
}
fn seed<'a>(frontier:&'a [Value],allowed:&[&str])->Option<&'a Value> {
    const PREFERRED:&[&str]=&[
        "src/engine/rsi_meta_controller.rs",
        "src/engine/rsi_reflexive.rs",
        "src/engine/rsi_mutual_program.rs",
        "src/engine/rsi_factorial.rs",
        "src/engine/rsi_evolve.rs"];
    PREFERRED.iter().find_map(|path|current(frontier,path,allowed))
        .filter(|row|row["role"]=="improvement_methods")
}
fn applied_meta(g:&Value)->bool {
    // A plan being printed is insufficient: the descendant's binary must
    // have executed the exact meta-policy it is recorded to have chosen.
    g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
        && g["compiledPlanExecuted"]["generatorCommit"]==g["generatorCommit"]
        && g["compiledPlanExecuted"]["metaRevision"]==g["metaPolicy"]["revision"]
        && g["metaPolicy"]["revision"].as_str().is_some_and(|s|s.len()==64)
}
fn stage_executed(g:&Value)->bool {
    let program=&g["reflexiveProgram"];
    let mut unsigned=program.clone();
    let Some(body)=unsigned.as_object_mut() else{return false};
    body.remove("sha256");
    let revised_method_executed=program["phase"]!="execute_revised_experiment"
        || (g["factorialExperiment"]["source"]
                =="compiled_factorial_ablation_curriculum_v1"
            && program["experimentPlanSha256"]==g["factorialExperiment"]["sha256"]
            && g["compiledPlanExecuted"]["factorialSha256"]
                ==g["factorialExperiment"]["sha256"]
            && g["factorialExperiment"]["selected"]["target"]==program["target"]
            && matches!(g["factorialExperiment"]["selected"]["phase"].as_str(),
                Some("isolate_left"|"isolate_right"))
            && g["metaPolicy"]["revisedExperimentPlanSha256"]
                ==program["experimentPlanSha256"]);
    revised_method_executed
        && program["sha256"].as_str()==Some(digest(&unsigned).as_str())
        && applied_meta(g)
        && g["metaPolicy"]["selectionMode"]=="recompiled_reflexive_method_evolution"
        && g["metaPolicy"]["reflexiveProgramSha256"]==program["sha256"]
        && g["compiledPlanExecuted"]["reflexiveSha256"]==program["sha256"]
        && g["compiledPlanExecuted"]["reflexivePhase"]==program["phase"]
}
fn capability_change(g:&Value)->bool {
    rsi_mutual_program::adopted_source(g) && stage_executed(g)
        && rsi_mutual_program::implemented_roles(g).iter().any(|r|
            r!="improvement_methods")
        && g["changedSourcePaths"].as_array().is_some_and(|paths|
            paths.iter().filter_map(Value::as_str).any(|path|!meta_source(path)))
}
/// The software, not an old coordinator, selects a frontier target grounded
/// in current source bytes and previous INDEPENDENTLY adopted revisions.
pub(crate) fn propose(history:&[Value],frontier:&[Value],commit:&str,
    allowed:&[&str],regular:&Value,experiment:&Value)->Value {
    let parent=history.iter().rev().find(|g|
        rsi_mutual_program::adopted_source(g)
            && g["acceptedCommit"]==commit);
    let mut phase="seed";
    let mut root=Value::Null;
    let mut ancestor=Value::Null;
    let mut meta_origin=Value::Null;
    let mut changed_role=Value::Null;
    let mut inherited=false;
    let mut experiment_sha=Value::Null;
    let selected:Option<Value>=if let Some(p)=parent {
        let last=&p["reflexiveProgram"];
        if last["phase"]=="seed"
            && last["generatorCommit"]==p["generatorCommit"]
            && stage_executed(p)
            && last["target"]["path"].as_str()
                .is_some_and(|path|meta_source(path) && actual_source(p,path))
        {
            phase="transfer";inherited=true;
            root=p["id"].clone();
            ancestor=p["acceptedCommit"].clone();
            meta_origin=last["target"]["path"].clone();
            // The compiled new meta-algorithm chooses a DIFFERENT, real
            // subsystem. A previously accepted modification of the improver
            // is a prerequisite, not an automatic performance claim.
            frontier.iter().filter(|f|{
                f["role"]!="improvement_methods"
                    && f["path"].as_str().is_some_and(|path|!meta_source(path)
                        && rsi::safe_path(path))
                    && f["domain"].as_str().is_some_and(|d|allowed.contains(&d))
                    && f["sourceSha256"].as_str().is_some_and(|s|s.len()==64)
            }).min_by(|a,b|{
                a["sourceAttempts"].as_u64().unwrap_or(0).cmp(
                    &b["sourceAttempts"].as_u64().unwrap_or(0))
                .then_with(||a["role"].as_str().cmp(&b["role"].as_str()))
                .then_with(||a["path"].as_str().cmp(&b["path"].as_str()))
            }).cloned()
        }else if last["phase"]=="transfer"
            && last["generatorCommit"]==p["generatorCommit"]
            && capability_change(p)
            && last["target"]["role"].as_str().is_some_and(|role|
                rsi_mutual_program::implemented_roles(p).iter().any(|r|r==role))
            && last["target"]["path"].as_str()
                .is_some_and(|path|actual_source(p,path))
        {
            // A valid return must refer to the seed through a direct
            // parent→child commit chain, never through a prose assertion.
            let founding=history.iter().find(|g|
                g["id"]==last["rootGenerationId"]
                    && g["acceptedCommit"]==p["generatorCommit"]
                    && g["reflexiveProgram"]["phase"]=="seed"
                    && rsi_mutual_program::adopted_source(g)
                    && stage_executed(g)
                    && g["reflexiveProgram"]["target"]["path"].as_str()
                        .is_some_and(|path|meta_source(path)&&actual_source(g,path)));
            if let Some(first)=founding {
                phase="return";inherited=true;
                root=first["id"].clone();
                ancestor=p["acceptedCommit"].clone();
                meta_origin=first["reflexiveProgram"]["target"]["path"].clone();
                changed_role=last["target"]["role"].clone();
                current(frontier,meta_origin.as_str().unwrap_or(""),allowed).cloned()
            }else {
                seed(frontier,allowed).cloned()
            }
        }else if last["phase"]=="return"
            && last["generatorCommit"]==p["generatorCommit"]
            && stage_executed(p)
            && last["target"]["path"].as_str().is_some_and(|path|
                meta_source(path) && actual_source(p,path))
            && completed(history)["loops"].as_array().is_some_and(|loops|
                loops.iter().any(|row|row["returnGenerationId"]==p["id"]))
        {
            // The former three-generation loop now redesigns the actual
            // compiled A/B experiment generator as its next capability.
            let revised=current(frontier,"src/engine/rsi_factorial.rs",allowed).cloned();
            if revised.is_some() {
                phase="redesign_experiment";inherited=true;
                root=last["rootGenerationId"].clone();
                ancestor=p["acceptedCommit"].clone();
                meta_origin=last["originMetaPath"].clone();
                changed_role=last["transferredRole"].clone();
            }
            revised.or_else(||seed(frontier,allowed).cloned())
        }else if last["phase"]=="redesign_experiment"
            && last["generatorCommit"]==p["generatorCommit"]
            && stage_executed(p)
            && last["target"]["path"]=="src/engine/rsi_factorial.rs"
            && actual_source(p,"src/engine/rsi_factorial.rs")
        {
            // A recompiled experiment method must supply a CURRENT,
            // source-anchored missing isolation. An arbitrary old edit
            // target cannot masquerade as execution of the revised method.
            let selected=&experiment["selected"];
            let candidate=&selected["target"];
            let method_executable=experiment["source"]
                =="compiled_factorial_ablation_curriculum_v1"
                && experiment["generatorCommit"]==commit
                && experiment["historyCount"].as_u64()==Some(history.len() as u64)
                && matches!(selected["phase"].as_str(),
                    Some("isolate_left"|"isolate_right"))
                && experiment["sha256"].as_str().is_some_and(|h|
                    h.len()==64 && h.bytes().all(|b|b.is_ascii_hexdigit()))
                && candidate["role"]!="improvement_methods";
            let target=if method_executable {
                candidate["path"].as_str().and_then(|path|
                    current(frontier,path,allowed))
                    .filter(|row|*row==candidate).cloned()
            }else{None};
            if target.is_some() {
                phase="execute_revised_experiment";inherited=true;
                root=last["rootGenerationId"].clone();
                ancestor=p["acceptedCommit"].clone();
                meta_origin=last["originMetaPath"].clone();
                changed_role=last["transferredRole"].clone();
                experiment_sha=experiment["sha256"].clone();
            }
            target.or_else(||seed(frontier,allowed).cloned())
        }else {
            seed(frontier,allowed).cloned()
        }
    }else {seed(frontier,allowed).cloned()};
    let Some(target)=selected else{return Value::Null};
    // Repeated unsuccessful trials at the same source commit must release
    // the objective to other compiled search policies, not create a
    // self-reinforcing fixed-meta-edit trap.
    let stalled=history.iter().rev().filter(|g|
        g["generatorCommit"]==commit
        && g["reflexiveProgram"]["phase"]==phase
        && g["accepted"]!=true
        && (g["candidateId"].is_string()
            || g["error"].as_str().is_some_and(|e|!e.is_empty())))
        .take(2).count()>=2;
    if stalled {return Value::Null;}
    if !rsi::safe_path(target["path"].as_str().unwrap_or(""))
        || target["sourceSha256"].as_str().is_none_or(|s|s.len()!=64) {
        return Value::Null;
    }
    if phase=="seed" && !meta_source(target["path"].as_str().unwrap_or("")) {
        return Value::Null;
    }
    let mut plan=json!({
        "source":"compiled_reflexive_intelligence_curriculum_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "phase":phase,"target":target,
        "rootGenerationId":root,"parentAcceptedCommit":ancestor,
        "originMetaPath":meta_origin,"transferredRole":changed_role,
        "experimentPlanSha256":experiment_sha,
        "inheritedFromVerifiedParent":inherited,
        "next":match phase {
            "seed"=>"transfer_to_distinct_subsystem",
            "transfer"=>"return_to_meta_improver",
            "return"=>"redesign_factorial_experiment",
            "redesign_experiment"=>"execute_recompiled_factorial_experiment",
            _=>"seed_new_meta_method"
        },
        "editableSource":"src/engine/rsi_reflexive.rs",
        "independentEvaluationOwner":"supervisor",
        "currentSourceRebound":true,
        "qualityProven":false,"recursiveCapabilityProven":false,
        "regularTargetConsidered":regular["path"],
    });
    plan["sha256"]=json!(digest(&plan));
    plan
}
pub(crate) fn valid(plan:&Value,history:&[Value],frontier:&[Value],
    commit:&str)->bool {
    if plan.is_null(){return true;}
    let mut unsigned=plan.clone();
    let Some(obj)=unsigned.as_object_mut() else{return false};
    obj.remove("sha256");
    let target=&plan["target"];
    let Some(path)=target["path"].as_str() else{return false};
    if plan["source"]!="compiled_reflexive_intelligence_curriculum_v1"
        || plan["sha256"].as_str()!=Some(digest(&unsigned).as_str())
        || plan["generatorCommit"]!=commit
        || plan["historyCount"].as_u64()!=Some(history.len() as u64)
        || plan["independentEvaluationOwner"]!="supervisor"
        || plan["editableSource"]!="src/engine/rsi_reflexive.rs"
        || plan["qualityProven"]!=false
        || plan["recursiveCapabilityProven"]!=false
        || plan["currentSourceRebound"]!=true
        || !rsi::safe_path(path)
        || !frontier.iter().any(|f|f==target) {
        return false;
    }
    if plan["phase"]=="seed" {
        return meta_source(path)
            && target["role"]=="improvement_methods"
            && plan["inheritedFromVerifiedParent"]==false
            && plan["rootGenerationId"].is_null();
    }
    let Some(parent)=history.iter().rev().find(|g|
        rsi_mutual_program::adopted_source(g) && g["acceptedCommit"]==commit)
        else{return false};
    if plan["parentAcceptedCommit"]!=parent["acceptedCommit"]
        || plan["inheritedFromVerifiedParent"]!=true {return false;}
    match plan["phase"].as_str() {
        Some("transfer")=>{
            let origin=parent["reflexiveProgram"]["target"]["path"].as_str();
            parent["reflexiveProgram"]["phase"]=="seed"
                && stage_executed(parent)
                && origin.is_some_and(|p|meta_source(p)&&actual_source(parent,p))
                && plan["rootGenerationId"]==parent["id"]
                && plan["originMetaPath"]==parent["reflexiveProgram"]["target"]["path"]
                && target["role"]!="improvement_methods"
                && !meta_source(path)
        },
        Some("return")=>{
            let Some(first)=history.iter().find(|g|
                g["id"]==plan["rootGenerationId"]
                && g["acceptedCommit"]==parent["generatorCommit"]
                && rsi_mutual_program::adopted_source(g)
                && g["reflexiveProgram"]["phase"]=="seed"
                && stage_executed(g)
                && g["reflexiveProgram"]["target"]["path"].as_str()
                    .is_some_and(|p|actual_source(g,p)&&meta_source(p)))
                else{return false};
            parent["reflexiveProgram"]["phase"]=="transfer"
                && parent["reflexiveProgram"]["rootGenerationId"]==first["id"]
                && capability_change(parent)
                && parent["reflexiveProgram"]["target"]["path"].as_str()
                    .is_some_and(|p|actual_source(parent,p))
                && plan["transferredRole"]==parent["reflexiveProgram"]["target"]["role"]
                && target["role"]=="improvement_methods"
                && plan["originMetaPath"]==first["reflexiveProgram"]["target"]["path"]
                && target["path"]==plan["originMetaPath"]
        },
        Some("redesign_experiment")=>{
            parent["reflexiveProgram"]["phase"]=="return"
                && stage_executed(parent)
                && parent["reflexiveProgram"]["target"]["path"].as_str()
                    .is_some_and(|p|meta_source(p)&&actual_source(parent,p))
                && completed(history)["loops"].as_array().is_some_and(|loops|
                    loops.iter().any(|row|row["returnGenerationId"]==parent["id"]))
                && plan["rootGenerationId"]
                    ==parent["reflexiveProgram"]["rootGenerationId"]
                && target["role"]=="improvement_methods"
                && path=="src/engine/rsi_factorial.rs"
                && plan["experimentPlanSha256"].is_null()
        },
        Some("execute_revised_experiment")=>{
            parent["reflexiveProgram"]["phase"]=="redesign_experiment"
                && stage_executed(parent)
                && parent["reflexiveProgram"]["target"]["path"]
                    =="src/engine/rsi_factorial.rs"
                && actual_source(parent,"src/engine/rsi_factorial.rs")
                && plan["rootGenerationId"]
                    ==parent["reflexiveProgram"]["rootGenerationId"]
                && target["role"]!="improvement_methods"
                && plan["experimentPlanSha256"].as_str().is_some_and(|h|
                    h.len()==64 && h.bytes().all(|b|b.is_ascii_hexdigit()))
        },
        _=>false
    }
}
/// Completed source-level feedback loops are distinct from planned stages.
/// Every step must have an independently adopted patch and a different
/// compiled generator. No claim of unlimited recursion or causal benefit.
pub(crate) fn completed(history:&[Value])->Value {
    let mut loops=Vec::new();
    for last in history.iter().filter(|g|
        rsi_mutual_program::adopted_source(g)
        && stage_executed(g)
        && g["reflexiveProgram"]["phase"]=="return"
        && g["reflexiveProgram"]["target"]["path"].as_str()
            .is_some_and(|p|meta_source(p)&&actual_source(g,p))) {
        let Some(first)=history.iter().find(|g|
            g["id"]==last["reflexiveProgram"]["rootGenerationId"]
            && rsi_mutual_program::adopted_source(g)
            && stage_executed(g)
            && g["reflexiveProgram"]["phase"]=="seed"
            && g["reflexiveProgram"]["target"]["path"].as_str()
                .is_some_and(|p|meta_source(p)&&actual_source(g,p)))
        else{continue};
        let Some(middle)=history.iter().find(|g|
            g["generatorCommit"]==first["acceptedCommit"]
            && g["acceptedCommit"]==last["generatorCommit"]
            && g["reflexiveProgram"]["phase"]=="transfer"
            && g["reflexiveProgram"]["rootGenerationId"]==first["id"]
            && g["reflexiveProgram"]["originMetaPath"]==first["reflexiveProgram"]["target"]["path"]
            && capability_change(g)
            && g["reflexiveProgram"]["target"]["path"].as_str()
                .is_some_and(|p|actual_source(g,p))) else{continue};
        if last["reflexiveProgram"]["originMetaPath"]
            !=first["reflexiveProgram"]["target"]["path"]
            || !rsi_mutual_program::adopted_source(middle)
            || last["reflexiveProgram"]["transferredRole"]
                !=middle["reflexiveProgram"]["target"]["role"] {continue;}
        loops.push(json!({
            "seedGenerationId":first["id"],"transferGenerationId":middle["id"],
            "returnGenerationId":last["id"],
            "metaOriginCommit":first["acceptedCommit"],
            "capabilityImprovedCommit":middle["acceptedCommit"],
            "metaReturnCommit":last["acceptedCommit"],
            "executedRecompiledMetaPolicy":true,
            "threeIndependentSourceAdoptions":true,
            "causalBenefitProven":false
        }));
    }
    let mut out=json!({
        "source":"witnessed_reflexive_improvement_lineage_v1",
        "completedLoops":loops.len(),"loops":loops,
        "meaning":"three_independent_compiled_adoptions_not_indefinite_recursion"
    });
    out["sha256"]=json!(digest(&out));
    out
}
/// Five-stage proof: a verified meta->capability->meta loop must
/// independently rewrite the experiment METHOD, then the next compiled
/// descendant must USE the revised experiment to generate a verified
/// source patch on its chosen isolated capability target.
pub(crate) fn completed_method_cycles(history:&[Value])->Value {
    let prior=completed(history);
    let mut cycles=Vec::new();
    let mut seen=BTreeSet::new();
    for outcome in history.iter().filter(|g|
        g["reflexiveProgram"]["phase"]=="execute_revised_experiment"
            && rsi_mutual_program::adopted_source(g) && stage_executed(g)) {
        let Some(outcome_id)=outcome["id"].as_str() else{continue};
        let Some(path)=outcome["reflexiveProgram"]["target"]["path"].as_str() else{continue};
        if !actual_source(outcome,path)
            || outcome["reflexiveProgram"]["target"]["role"]=="improvement_methods" {
            continue;
        }
        let Some(method)=history.iter().find(|g|
            rsi_mutual_program::adopted_source(g) && stage_executed(g)
                && g["acceptedCommit"]==outcome["generatorCommit"]
                && g["reflexiveProgram"]["phase"]=="redesign_experiment"
                && g["reflexiveProgram"]["rootGenerationId"]
                    ==outcome["reflexiveProgram"]["rootGenerationId"]
                && g["reflexiveProgram"]["target"]["path"]
                    =="src/engine/rsi_factorial.rs"
                && actual_source(g,"src/engine/rsi_factorial.rs"))
            else{continue};
        let Some(returned)=history.iter().find(|g|
            rsi_mutual_program::adopted_source(g) && stage_executed(g)
                && g["acceptedCommit"]==method["generatorCommit"]
                && g["reflexiveProgram"]["phase"]=="return"
                && g["reflexiveProgram"]["rootGenerationId"]
                    ==outcome["reflexiveProgram"]["rootGenerationId"])
            else{continue};
        let Some(triple)=prior["loops"].as_array().into_iter().flatten()
            .find(|proof|proof["returnGenerationId"]==returned["id"])
            else{continue};
        if !seen.insert(outcome_id.to_string()){continue;}
        cycles.push(json!({
            "seedGenerationId":triple["seedGenerationId"],
            "capabilityGenerationId":triple["transferGenerationId"],
            "metaReturnGenerationId":returned["id"],
            "methodRedesignGenerationId":method["id"],
            "methodExecutionGenerationId":outcome["id"],
            "originMetaCommit":triple["metaOriginCommit"],
            "capabilityCommit":triple["capabilityImprovedCommit"],
            "returnedMetaCommit":returned["acceptedCommit"],
            "experimentMethodCommit":method["acceptedCommit"],
            "experimentOutcomeCommit":outcome["acceptedCommit"],
            "revisedExperimentPlanSha256":
                outcome["reflexiveProgram"]["experimentPlanSha256"],
            "executedMethodTarget":outcome["reflexiveProgram"]["target"],
            "fiveIndependentSourceAdoptions":true,
            "recompiledMethodApplied":true,
            "causalPerformanceAdvantageProven":false
        }));
    }
    let mut result=json!({
        "source":"independently_witnessed_experiment_method_evolution_v1",
        "completedMethodCycles":cycles.len(),"cycles":cycles,
        "claim":"source_adopted_meta_capability_meta_method_experiment_execution"
    });
    result["sha256"]=json!(digest(&result));
    result
}
pub(crate) fn instruction(plan:&Value)->String {
    let target=&plan["target"];
    if !target.is_object(){return String::new();}
    let phase=plan["phase"].as_str().unwrap_or("seed");
    format!(
        "RECURSIVE INTELLIGENCE LOOP phase={}; source={} sha={}; role={}; \
         root={}; parent={}. \
         seed: edit real Rust meta-improvement strategy. \
         transfer: the rebuilt descendant uses that strategy to change a DIFFERENT intelligence subsystem. \
         return: the capability-modified descendant changes the meta strategy AGAIN. \
         redesign_experiment: edit executable factorial experiment selection SOURCE. \
         execute_revised_experiment: use the RECOMPILED factorial selector to \
         identify and implement its isolated intervention, not a canned target. \
         Change real current source; preserve frozen paired evaluation and exact SHA lineage. \
         Your editable curriculum is src/engine/rsi_reflexive.rs. \
         A planned stage is not a verified improvement.",
        phase,target["path"],target["sourceSha256"],target["role"],
        plan["rootGenerationId"],plan["parentAcceptedCommit"])
}
#[cfg(test)]
mod tests {
    use super::*;
    fn frontier()->Vec<Value> {
        vec![
            json!({"role":"improvement_methods","domain":"improver",
                "path":"src/engine/rsi_meta_controller.rs",
                "sourceSha256":"a".repeat(64),"sourceAttempts":0}),
            json!({"role":"memory","domain":"memory","path":"src/engine/memory.rs",
                "sourceSha256":"b".repeat(64),"sourceAttempts":0}),
            json!({"role":"improvement_methods","domain":"improver",
                "path":"src/engine/rsi_factorial.rs",
                "sourceSha256":"f".repeat(64),"sourceAttempts":0}),
        ]
    }
    fn accepted(id:&str,commit:&str,generator:&str,role:&str,path:&str,
        plan:Value)->Value{
        let mut hashes=serde_json::Map::new();
        hashes.insert(path.to_string(),json!("c".repeat(64)));
        let mut role_files=serde_json::Map::new();
        role_files.insert(role.to_string(),json!([path]));
        let plan_sha=plan["sha256"].clone();
        let phase=plan["phase"].clone();
        json!({"id":id,"jobId":format!("job-{id}"),"candidateId":format!("cand-{id}"),
            "generatorCommit":generator,"acceptedCommit":commit,
            "accepted":true,"pairedRevalidated":true,
            "evaluation":{"metrics":{"eligible":true}},
            "changedSourcePaths":[path],
            "acceptedSourceHashes":Value::Object(hashes),
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary",
                "generatorCommit":generator,"metaRevision":"d".repeat(64),
                "reflexiveSha256":plan_sha,"reflexivePhase":phase},
            "metaPolicy":{"revision":"d".repeat(64),
                "selectionMode":"recompiled_reflexive_method_evolution",
                "reflexiveProgramSha256":plan["sha256"]},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":generator,"generationJobId":format!("job-{id}"),
                "components":[role],"componentFiles":Value::Object(role_files),
                "targetFiles":[path]},
            "reflexiveProgram":plan})
    }
    #[test]
    fn source_adopted_meta_to_capability_to_meta_is_a_compiled_feedback_loop(){
        let f=frontier();
        let p0=propose(&[],&f,&"0".repeat(40),&["improver","memory"],&Value::Null,&Value::Null);
        assert_eq!(p0["phase"],"seed");
        assert!(valid(&p0,&[],&f,&"0".repeat(40)));
        let first=accepted("first",&"1".repeat(40),&"0".repeat(40),
            "improvement_methods","src/engine/rsi_meta_controller.rs",p0);
        let p1=propose(&[first.clone()],&f,&"1".repeat(40),
            &["improver","memory"],&Value::Null,&Value::Null);
        assert_eq!(p1["phase"],"transfer");
        assert!(valid(&p1,&[first.clone()],&f,&"1".repeat(40)));
        let middle=accepted("middle",&"2".repeat(40),&"1".repeat(40),
            "memory","src/engine/memory.rs",p1);
        let p2=propose(&[first.clone(),middle.clone()],&f,&"2".repeat(40),
            &["improver","memory"],&Value::Null,&Value::Null);
        assert_eq!(p2["phase"],"return");
        assert!(valid(&p2,&[first.clone(),middle.clone()],&f,&"2".repeat(40)));
        let last=accepted("last",&"3".repeat(40),&"2".repeat(40),
            "improvement_methods","src/engine/rsi_meta_controller.rs",p2);
        assert_eq!(completed(&[first,middle,last])["completedLoops"],1);
    }
    #[test]
    fn missing_real_meta_change_cannot_advance_source_cycle(){
        let f=frontier();
        let p0=propose(&[],&f,&"0".repeat(40),&["improver","memory"],&Value::Null,&Value::Null);
        let mut fake=accepted("fake",&"1".repeat(40),&"0".repeat(40),
            "improvement_methods","src/engine/rsi_meta_controller.rs",p0);
        fake["changedSourcePaths"]=json!(["src/engine/tools.rs"]);
        let p=propose(&[fake],&f,&"1".repeat(40),
            &["improver","memory"],&Value::Null,&Value::Null);
        assert_eq!(p["phase"],"seed");
        assert_eq!(p["inheritedFromVerifiedParent"],false);
    }
    #[test]
    fn fabricated_executed_meta_sha_is_not_a_real_transfer(){
        let f=frontier();
        let p0=propose(&[],&f,&"0".repeat(40),&["improver","memory"],&Value::Null,&Value::Null);
        let mut first=accepted("first",&"1".repeat(40),&"0".repeat(40),
            "improvement_methods","src/engine/rsi_meta_controller.rs",p0);
        first["compiledPlanExecuted"]["reflexiveSha256"]=json!("e".repeat(64));
        let next=propose(&[first],&f,&"1".repeat(40),
            &["improver","memory"],&Value::Null,&Value::Null);
        assert_eq!(next["phase"],"seed");
    }
    #[test]
    fn repeated_failed_reflexive_edits_release_the_source_target(){
        let f=frontier();
        let initial=propose(&[],&f,&"0".repeat(40),
            &["improver","memory"],&Value::Null,&Value::Null);
        let failed=|id:&str|json!({
            "id":id,"jobId":id,"generatorCommit":"0".repeat(40),
            "candidateId":id,"accepted":false,"reflexiveProgram":initial.clone()
        });
        let plan=propose(&[failed("failure-one"),failed("failure-two")],&f,
            &"0".repeat(40),&["improver","memory"],&Value::Null,&Value::Null);
        assert!(plan.is_null());
    }
    #[test]
    fn adopted_experiment_rewrite_is_actually_executed_by_next_compiled_generation(){
        let mut f=frontier();
        f.push(json!({"role":"tools","domain":"tools",
            "path":"src/engine/tools.rs","sourceSha256":"c".repeat(64),
            "sourceAttempts":0}));
        let allowed=&["improver","memory","tools"];
        let zero="0".repeat(40);
        let one="1".repeat(40);
        let two="2".repeat(40);
        let three="3".repeat(40);
        let four="4".repeat(40);
        let five="5".repeat(40);
        let p0=propose(&[],&f,&zero,allowed,&Value::Null,&Value::Null);
        let a=accepted("meta-seed",&one,&zero,
            "improvement_methods","src/engine/rsi_meta_controller.rs",p0);
        let p1=propose(&[a.clone()],&f,&one,allowed,&Value::Null,&Value::Null);
        let b=accepted("capability",&two,&one,"memory","src/engine/memory.rs",p1);
        let p2=propose(&[a.clone(),b.clone()],&f,&two,allowed,
            &Value::Null,&Value::Null);
        let c=accepted("meta-return",&three,&two,
            "improvement_methods","src/engine/rsi_meta_controller.rs",p2);
        let ancestors=vec![a.clone(),b.clone(),c.clone()];
        assert_eq!(completed(&ancestors)["completedLoops"],1);
        let p3=propose(&ancestors,&f,&three,allowed,&Value::Null,&Value::Null);
        assert_eq!(p3["phase"],"redesign_experiment");
        assert!(valid(&p3,&ancestors,&f,&three));
        let d=accepted("experiment-redesign",&four,&three,
            "improvement_methods","src/engine/rsi_factorial.rs",p3);
        // A real frozen-case joint intervention provides the newly compiled
        // factorial method with an observed missing isolated arm.
        let joint=json!({"id":"joint-record","candidateId":"joint-candidate",
            "generatorCommit":four,"evaluationProfileSha256":"e".repeat(64),
            "pairedMeasured":true,"accepted":false,
            "changedSourcePaths":["src/engine/memory.rs","src/engine/tools.rs"],
            "evaluation":{"metrics":{"verdict":"measured_non_adoption",
                "measurements":[{"id":"holdout-1","holdout":true,
                    "baselinePass":false,"candidatePass":true}]}}});
        let mut history=vec![a,b,c,d,joint];
        let graph=json!({"sha256":"f".repeat(64),"edges":[]});
        let experiment=crate::engine::rsi_factorial::design(
            &history,&f,&graph,allowed,&four);
        assert_eq!(experiment["selected"]["phase"],"isolate_left");
        assert_eq!(experiment["selected"]["target"]["role"],"memory");
        let p4=propose(&history,&f,&four,allowed,&Value::Null,&experiment);
        assert_eq!(p4["phase"],"execute_revised_experiment");
        assert_eq!(p4["experimentPlanSha256"],experiment["sha256"]);
        assert!(valid(&p4,&history,&f,&four));
        let mut e=accepted("experiment-executed",&five,&four,
            "memory","src/engine/memory.rs",p4);
        e["factorialExperiment"]=experiment.clone();
        e["compiledPlanExecuted"]["factorialSha256"]=experiment["sha256"].clone();
        e["metaPolicy"]["revisedExperimentPlanSha256"]=experiment["sha256"].clone();
        history.push(e.clone());
        assert_eq!(completed_method_cycles(&history)["completedMethodCycles"],1);
        let last=history.len()-1;
        history[last]["compiledPlanExecuted"]["factorialSha256"]=json!("0".repeat(64));
        assert_eq!(completed_method_cycles(&history)["completedMethodCycles"],0,
            "a source patch with an unexecuted revision cannot close the method loop");
    }
    #[test]
    fn source_rebound_or_false_parent_commit_is_rejected(){
        let f=frontier();
        let p=propose(&[],&f,&"0".repeat(40),&["improver","memory"],&Value::Null,&Value::Null);
        let mut stale=p.clone();
        stale["target"]["sourceSha256"]=json!("wrong");
        let mut unsigned=stale.clone();
        unsigned.as_object_mut().unwrap().remove("sha256");
        stale["sha256"]=json!(digest(&unsigned));
        assert!(!valid(&stale,&[],&f,&"0".repeat(40)));
    }
}
