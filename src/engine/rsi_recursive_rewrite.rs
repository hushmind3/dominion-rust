//! Recursively executed policy coupling: invented intelligence -> better meta
//! improver -> improved invented intelligence -> better meta improver again.
//!
//! This is executable descendant policy, not a predicted causal result.
//! The frozen paired evaluator alone decides whether an actual source edit
//! is adopted. Every edge below comes from independently revalidated,
//! source-attributed commits in the same successive compiled lineage.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;

fn sha(s:&str,n:usize)->bool{
    s.len()==n&&s.bytes().all(|b|b.is_ascii_hexdigit())
}
fn accepted(g:&Value)->bool{
    g["accepted"]==true && g["pairedRevalidated"]==true
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["evaluation"]["metrics"]["eligible"]==true
        && g["systemDesign"]["source"]=="model_authored_system_design"
        && g["systemDesign"]["generatorCommit"]==g["generatorCommit"]
        && g["systemDesign"]["generationJobId"]==g["jobId"]
        && g["generatorCommit"].as_str().is_some_and(|s|sha(s,40))
        && g["acceptedCommit"].as_str().is_some_and(|s|sha(s,40))
        && g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
}
fn changed_role_path(g:&Value,role:&str,filter:impl Fn(&str)->bool)->bool{
    if !accepted(g){return false;}
    let Some(files)=g["systemDesign"]["componentFiles"][role].as_array()
        .filter(|rows|!rows.is_empty()) else{return false};
    let Some(targets)=g["systemDesign"]["targetFiles"].as_array() else{return false};
    let Some(actual)=g["changedSourcePaths"].as_array() else{return false};
    files.iter().filter_map(Value::as_str).any(|path|
        filter(path) && crate::engine::rsi::safe_path(path)
        && targets.iter().any(|p|p.as_str()==Some(path))
        && actual.iter().any(|p|p.as_str()==Some(path))
        && g["acceptedSourceHashes"][path].as_str().is_some_and(|s|sha(s,64)))
}
fn method_rewritten(g:&Value)->bool{
    // The meta-program may invent files unknown to this source revision.
    // Judge the changed METHOD role and executable Rust scope, not names
    // from a fixed enum of today's algorithm modules.
    changed_role_path(g,"improvement_methods",|p|
        p.starts_with("src/engine/") && p.ends_with(".rs")
        && (p.contains("rsi_") || p.contains("algorith")
            || p.contains("policy") || p.contains("learning")))
}
fn capability_rewritten(g:&Value,role:&str,sources:&BTreeSet<String>)->bool{
    changed_role_path(g,role,|p|sources.contains(p))
}

/// Produce a source-grounded recurrent state machine for EACH novel role.
/// A cycle only advances after separately adopted, recompiled descendants
/// actually edit the meta-improver and then the originally invented module.
pub(crate) fn ledger(history:&[Value])->Vec<Value>{
    let inventions=crate::engine::rsi_open_topology::inherited_roles(history);
    let mut records=Vec::new();
    for invention in inventions.into_iter().take(20){
        let Some(role)=invention["role"].as_str() else{continue};
        let Some(birth)=invention["bornAtCommit"].as_str() else{continue};
        let Some(position)=history.iter().position(|g|g["acceptedCommit"]==birth
            && accepted(g)) else{continue};
        let sources=invention["sourcePaths"].as_array().into_iter().flatten()
            .filter_map(Value::as_str).map(str::to_owned)
            .collect::<BTreeSet<_>>();
        if sources.is_empty(){continue;}
        let mut cursor=birth.to_string();
        let mut expecting_meta=true;
        let mut cycles=0u64;
        let mut method=Value::Null;
        let mut returned=Value::Null;
        let mut transitions=Vec::<Value>::new();
        for g in history.iter().skip(position+1){
            if !accepted(g) || g["generatorCommit"]!=cursor {continue;}
            let Some(next)=g["acceptedCommit"].as_str() else{continue};
            // Failed and research-only generations never change the
            // chain. An accepted unrelated edit advances ancestry but
            // cannot claim an improvement of the meta search algorithm.
            if expecting_meta && method_rewritten(g){
                expecting_meta=false;
                method=json!(next);
                if transitions.len()<12{
                    transitions.push(json!({"event":"meta_improver_rewritten",
                        "commit":next,"generationJobId":g["jobId"]}));
                }
            } else if !expecting_meta && capability_rewritten(g,role,&sources){
                expecting_meta=true;
                cycles=cycles.saturating_add(1);
                returned=json!(next);
                if transitions.len()<12{
                    transitions.push(json!({"event":"invented_capability_rewritten",
                        "commit":next,"generationJobId":g["jobId"],
                        "completedCycle":cycles}));
                }
            }
            cursor=next.to_string();
        }
        records.push(json!({
            "source":"independently_revalidated_recursive_policy_ledger_v1",
            "role":role,"domain":invention["domain"],
            "inventionCommit":birth,"currentHead":cursor,
            "phase":if expecting_meta{"rewrite_meta_method"}
                else{"apply_rewritten_method_to_invention"},
            "completedReturnCycles":cycles,
            "lastMetaMethodCommit":method,"lastCapabilityReturnCommit":returned,
            "transitions":transitions,"requiresRealFutureEdit":true,
            "causalAdvantageProven":false
        }));
    }
    records
}

/// Add the real method-editing source programs to the descendant's
/// architectural frontier. This avoids the one-representative-file bottleneck
/// where "improvement_methods" could resolve to an unrelated RSI report.
pub(crate) fn extend_frontier(blueprint:&Value,frontier:&mut Vec<Value>){
    let Some(method)=blueprint["architecture"].as_array().into_iter().flatten()
        .find(|role|role["role"]=="improvement_methods") else{return};
    let candidates=["src/engine/rsi_recursive_rewrite.rs",
        "src/engine/rsi_meta_controller.rs"];
    for name in candidates {
        if frontier.len()>=32{break;}
        if frontier.iter().any(|r|r["path"]==name){continue;}
        let Some(entry)=method["representativeSources"].as_array()
            .into_iter().flatten().find(|r|r["path"]==name) else{continue};
        let Some(hash)=entry["sha256"].as_str()
            .filter(|value|sha(value,64)) else{continue};
        let Some(bytes)=entry["bytes"].as_u64() else{continue};
        frontier.push(json!({"role":"improvement_methods",
            "domain":"improver","path":name,"sourceSha256":hash,
            "sourceBytes":bytes,"sourceAttempts":0,
            "verifiedRoleImproved":false,
            "reason":"editable_compiled_meta_improvement_algorithm",
            "qualityProven":false}));
    }
}

/// Turn the inherited (editable) meta-policy into a new executed search
/// distribution. Unlike prose-only memory, this changes source ranking,
/// the actual selected architecture, and the next descendant's edit policy.
/// The recurrence has NO fixed number of generations or original role names.
pub(crate) fn reconfigure(
    history:&[Value],frontier:&[Value],mut meta:Value,
    allowed_domains:&[&str],auto:bool,stage_active:bool
)->Value{
    let loops=ledger(history);
    let mut ranked=meta["alternatives"].as_array().cloned().unwrap_or_default();
    let mut options=Vec::<(Value,f64)>::new();
    for cycle in &loops {
        let Some(role)=cycle["role"].as_str() else{continue};
        let expect_meta=cycle["phase"]=="rewrite_meta_method";
        let target_role=if expect_meta{"improvement_methods"}else{role};
        let domain=if expect_meta{"improver"}else{
            cycle["domain"].as_str().unwrap_or("")
        };
        if !allowed_domains.contains(&domain){continue;}
        for row in &ranked{
            if row["role"]!=target_role || row["domain"]!=domain{continue;}
            let Some(target)=frontier.iter().find(|f|*f==&row["target"])
                else{continue};
            let cycles=cycle["completedReturnCycles"].as_u64().unwrap_or(0);
            let source=target["path"].as_str().unwrap_or("");
            let actual_meta=source=="src/engine/rsi_recursive_rewrite.rs"
                || source=="src/engine/rsi_meta_controller.rs";
            let priority=(3.2/(cycles as f64+1.0))
                + if expect_meta && actual_meta{0.55}else{0.0};
            options.push((json!({
                "role":role,"targetRole":target_role,"target":target,
                "phase":cycle["phase"],"priority":priority,
                "completedReturnCycles":cycles,"birth":cycle["inventionCommit"],
                "lastMetaMethodCommit":cycle["lastMetaMethodCommit"],
                "lastCapabilityReturnCommit":cycle["lastCapabilityReturnCommit"],
                "nextVerifiedOutcomeRequired":true
            }),priority));
        }
    }
    options.sort_by(|(a,ap),(b,bp)|
        bp.total_cmp(ap).then_with(||a["role"].as_str().cmp(&b["role"].as_str()))
            .then_with(||a["target"]["path"].as_str()
                .cmp(&b["target"]["path"].as_str())));
    let chosen=options.first().map(|(v,_)|v.clone()).unwrap_or(Value::Null);
    let editable=auto && !stage_active;
    let old_revision=meta["revision"].clone();
    if editable && chosen.is_object(){
        let chosen_target=&chosen["target"];
        for row in &mut ranked {
            if row["target"]==*chosen_target{
                row["score"]=json!(row["score"].as_f64().unwrap_or(0.0)
                    + chosen["priority"].as_f64().unwrap_or(0.0));
                row["recursiveCouplingPriority"]=chosen["priority"].clone();
                row["recursiveCouplingSourceRole"]=chosen["role"].clone();
            }
        }
        ranked.sort_by(|a,b|b["score"].as_f64().unwrap_or(0.0)
            .total_cmp(&a["score"].as_f64().unwrap_or(0.0))
            .then_with(||a["target"]["path"].as_str()
                .cmp(&b["target"]["path"].as_str())));
        if let Some(winner)=ranked.first(){
            meta["target"]=winner["target"].clone();
            meta["selectionEvidence"]=winner.clone();
            meta["forecast"]["acceptanceProbability"]=
                winner["predictedAdoptionProbability"].clone();
            meta["forecast"]["uncertainty"]=winner["uncertainty"].clone();
            meta["mode"]=json!("source_verified_recursive_meta_recomposition");
        }
        meta["alternatives"]=json!(ranked);
    }
    meta["recursiveProgram"]=json!({
        "source":"compiled_inherited_recursive_policy_rewriter_v1",
        "active":editable&&chosen.is_object(),
        "phase":chosen["phase"],"selectedInvention":chosen["role"],
        "selectedMethodOrCapability":chosen["target"],
        "nextRequiredSourceChange":chosen["target"]["path"],
        "completedReturnCycles":chosen["completedReturnCycles"],
        "validatedLoopLedger":loops,
        "ownerGoalRespected":true,"adoptionAuthority":"frozen_paired_oracle",
        "causalAdvantageProven":false
    });
    meta["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&json!([old_revision,meta["recursiveProgram"],
            meta["target"],meta["alternatives"]])).unwrap_or_default())));
    meta
}
pub(crate) fn instruction(policy:&Value)->String{
    let program=&policy["recursiveProgram"];
    if program["active"]!=true{return String::new();}
    let role=program["selectedInvention"].as_str().unwrap_or("");
    let target=program["nextRequiredSourceChange"].as_str().unwrap_or("");
    let phase=program["phase"].as_str().unwrap_or("");
    format!("[EXECUTED RECURSIVE META-RECOMPOSITION] Invented role={role}; \
        source-grounded phase={phase}; actual next editable source={target}. \
        If rewriting the meta-method, CHANGE its executable search, evidence \
        use or tool-selection algorithm so the NEXT compiled descendant can \
        redesign {role} better. If applying an improved method, CHANGE the \
        invented role's real source using that newer planner. Do not merely \
        repeat a plan. Every completed loop requires separate adopted Rust \
        commits in descendant ancestry. The external frozen evaluator alone \
        can accept a revision; predicted benefits are NOT evidence.")
}

#[cfg(test)]
mod tests{
    use super::*;
    fn generation(base:&str,commit:&str,role:&str,path:&str)->Value{
        let digest="c".repeat(64);
        let mut g=json!({"accepted":true,"pairedRevalidated":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "candidateId":format!("candidate-{commit}"),
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary"},
            "evaluation":{"metrics":{"eligible":true}},
            "generatorCommit":base,"acceptedCommit":commit,"jobId":format!("j-{commit}"),
            "changedSourcePaths":[path],
            "acceptedSourceHashes":{},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":base,"generationJobId":format!("j-{commit}"),
                "component":role,"componentFiles":{},
                "targetFiles":[path]}});
        g["acceptedSourceHashes"][path]=json!(digest);
        g["systemDesign"]["componentFiles"][role]=json!([path]);
        g
    }
    fn born()->Value{
        let mut g=generation(&"a".repeat(40),&"b".repeat(40),
            "causal_mapper","src/engine/causal_mapper.rs");
        g["systemDesign"]["component"]=json!("system");
        g["systemDesign"]["components"]=json!(["causal_mapper","harness"]);
        g["systemDesign"]["targetFiles"]=json!([
            "src/engine/causal_mapper.rs","src/engine/mod.rs"]);
        g["systemDesign"]["componentFiles"]["harness"]=json!(["src/engine/mod.rs"]);
        g["systemDesign"]["newComponents"]=json!([{
            "role":"causal_mapper","domain":"learning",
            "purpose":"Executable causal model and source-grounded redesign skills",
            "sourcePaths":["src/engine/causal_mapper.rs"],
            "integrationPaths":["src/engine/mod.rs"]
        }]);
        g["acceptedSourceHashes"]["src/engine/mod.rs"]=json!("d".repeat(64));
        g["changedSourcePaths"]=json!([
            "src/engine/causal_mapper.rs","src/engine/mod.rs"]);
        g
    }
    fn frontier()->Vec<Value>{
        vec![
            json!({"role":"causal_mapper","domain":"learning",
                "path":"src/engine/causal_mapper.rs",
                "sourceSha256":"c".repeat(64)}),
            json!({"role":"improvement_methods","domain":"improver",
                "path":"src/engine/rsi_recursive_rewrite.rs",
                "sourceSha256":"d".repeat(64)})
        ]
    }
    fn meta(frontier:&[Value])->Value{
        let options=frontier.iter().map(|target|json!({
            "role":target["role"],"domain":target["domain"],
            "target":target,"score":0.6,"predictedAdoptionProbability":0.5,
            "uncertainty":0.3
        })).collect::<Vec<_>>();
        json!({"revision":"a".repeat(64),"alternatives":options,
            "target":frontier[0],
            "forecast":{"acceptanceProbability":0.5,"uncertainty":0.3}})
    }
    #[test]
    fn invention_rewrites_which_meta_algorithm_is_edited(){
        let history=vec![born()];let frontier=frontier();
        let policy=reconfigure(&history,&frontier,meta(&frontier),
            &["improver","learning"],true,false);
        assert_eq!(policy["target"]["role"],"improvement_methods");
        assert_eq!(policy["target"]["path"],"src/engine/rsi_recursive_rewrite.rs");
        assert_eq!(policy["recursiveProgram"]["phase"],"rewrite_meta_method");
        assert_eq!(policy["recursiveProgram"]["active"],true);
    }
    #[test]
    fn real_child_meta_patch_flips_its_next_target_to_invented_intelligence(){
        let mut history=vec![born()];
        history.push(generation(&"b".repeat(40),&"e".repeat(40),
            "improvement_methods","src/engine/rsi_recursive_rewrite.rs"));
        let frontier=frontier();
        let policy=reconfigure(&history,&frontier,meta(&frontier),
            &["improver","learning"],true,false);
        assert_eq!(policy["target"]["role"],"causal_mapper");
        assert_eq!(policy["recursiveProgram"]["phase"],
            "apply_rewritten_method_to_invention");
        assert_eq!(policy["recursiveProgram"]["validatedLoopLedger"][0]
            ["lastMetaMethodCommit"],"e".repeat(40));
    }
    #[test]
    fn second_child_return_closes_first_loop_and_opens_next(){
        let mut history=vec![born()];
        history.push(generation(&"b".repeat(40),&"e".repeat(40),
            "improvement_methods","src/engine/rsi_recursive_rewrite.rs"));
        history.push(generation(&"e".repeat(40),&"f".repeat(40),
            "causal_mapper","src/engine/causal_mapper.rs"));
        let frontier=frontier();
        let policy=reconfigure(&history,&frontier,meta(&frontier),
            &["improver","learning"],true,false);
        assert_eq!(policy["target"]["role"],"improvement_methods");
        assert_eq!(policy["recursiveProgram"]["completedReturnCycles"],1);
        assert_eq!(policy["recursiveProgram"]["validatedLoopLedger"][0]
            ["lastCapabilityReturnCommit"],"f".repeat(40));
    }
    #[test]
    fn failed_paired_adoption_and_non_descendant_never_advance_policy(){
        let mut history=vec![born()];
        let mut false_child=generation(&"b".repeat(40),&"e".repeat(40),
            "improvement_methods","src/engine/rsi_recursive_rewrite.rs");
        false_child["pairedRevalidated"]=json!(false);
        history.push(false_child);
        history.push(generation(&"0".repeat(40),&"f".repeat(40),
            "improvement_methods","src/engine/rsi_recursive_rewrite.rs"));
        let state=ledger(&history);
        assert_eq!(state[0]["phase"],"rewrite_meta_method");
        assert!(state[0]["lastMetaMethodCommit"].is_null());
    }
    #[test]
    fn explicit_owner_or_inherited_stage_cannot_be_overridden(){
        let frontier=frontier();let history=vec![born()];
        let owned=reconfigure(&history,&frontier,meta(&frontier),
            &["learning"],false,false);
        assert_eq!(owned["target"]["role"],"causal_mapper");
        assert_eq!(owned["recursiveProgram"]["active"],false);
        let stage=reconfigure(&history,&frontier,meta(&frontier),
            &["improver","learning"],true,true);
        assert_eq!(stage["target"]["role"],"causal_mapper");
        assert_eq!(stage["recursiveProgram"]["active"],false);
    }
}
