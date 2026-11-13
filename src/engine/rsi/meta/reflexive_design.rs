//! Model-authored reflexive source-editing program and mutable role selector.
use super::*;

pub(super) fn digest(v:&Value)->String {
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
pub(super) fn actual_source(g:&Value,path:&str)->bool {
    rsi_mutual_program::adopted_source(g)
        && g["changedSourcePaths"].as_array().is_some_and(|paths|
            paths.iter().any(|p|p.as_str()==Some(path)))
        && g["acceptedSourceHashes"][path].as_str().is_some_and(|h|
            h.len()==64 && h.bytes().all(|b|b.is_ascii_hexdigit()))
}
pub(super) fn meta_source(path:&str)->bool {
    matches!(path,
        "src/engine/rsi_meta_controller.rs"
        |"src/engine/rsi/meta/controller_selection.rs"
        |"src/engine/rsi/meta/controller_policy.rs"
        |"src/engine/rsi/meta/evolution.rs"
        |"src/engine/rsi/meta/composition.rs"
        |"src/engine/rsi_factorial.rs"
        |"src/engine/rsi_reflexive.rs"
        |"src/engine/rsi_mutual_program.rs"
        |"src/engine/rsi_evolve.rs"
        |"src/engine/rsi_system.rs"
        |"src/engine/rsi_interaction.rs")
}
pub(super) fn current<'a>(frontier:&'a [Value],path:&str,allowed:&[&str])->Option<&'a Value> {
    frontier.iter().find(|f|f["path"]==path
        && f["domain"].as_str().is_some_and(|d|allowed.contains(&d))
        && f["sourceSha256"].as_str().is_some_and(|sha|sha.len()==64)
        && rsi::safe_path(path))
}
pub(super) fn seed<'a>(frontier:&'a [Value],allowed:&[&str])->Option<&'a Value> {
    const PREFERRED:&[&str]=&[
        "src/engine/rsi/meta/controller_selection.rs",
        "src/engine/rsi/meta/evolution.rs",
        "src/engine/rsi_meta_controller.rs",
        "src/engine/rsi/meta/reflexive_design.rs",
        "src/engine/rsi_reflexive.rs",
        "src/engine/rsi_mutual_program.rs",
        "src/engine/rsi_factorial.rs",
        "src/engine/rsi_evolve.rs"];
    PREFERRED.iter().find_map(|path|current(frontier,path,allowed))
        .filter(|row|row["role"]=="improvement_methods")
}
pub(super) fn applied_meta(g:&Value)->bool {
    // A plan being printed is insufficient: the descendant's binary must
    // have executed the exact meta-policy it is recorded to have chosen.
    g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
        && g["compiledPlanExecuted"]["generatorCommit"]==g["generatorCommit"]
        && g["compiledPlanExecuted"]["metaRevision"]==g["metaPolicy"]["revision"]
        && g["metaPolicy"]["revision"].as_str().is_some_and(|s|s.len()==64)
}
pub(super) fn stage_executed(g:&Value)->bool {
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
pub(super) fn capability_change(g:&Value)->bool {
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
