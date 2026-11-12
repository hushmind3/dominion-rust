//! Open-ended recursive science controller: experiments invent intelligence,
//! an accepted new component develops a further child, the child improves its
//! originating capability, and the resulting descendant rewrites the way new
//! improvement experiments are invented.
//!
//! All stages are source-changing hypotheses. Only independent paired
//! evaluation can accept an actual candidate. Nothing here grants quality.
//! The compiled descendant owns this program and may rewrite it in source.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;

fn adopted(g:&Value)->bool {
    g["accepted"]==true && g["pairedRevalidated"]==true
        && g["evaluation"]["metrics"]["eligible"]==true
        && g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
        && g["compiledPlanExecuted"]["generatorCommit"]==g["generatorCommit"]
        && g["acceptedCommit"].as_str().is_some_and(|s|
            s.len()==40 && s.bytes().all(|b|b.is_ascii_hexdigit()))
        && g["systemDesign"]["source"]=="model_authored_system_design"
        && g["systemDesign"]["generatorCommit"]==g["generatorCommit"]
        && g["systemDesign"]["generationJobId"]==g["jobId"]
}

fn altered(g:&Value,role:&str)->bool {
    if !adopted(g){return false;}
    let assigned=g["systemDesign"]["componentFiles"][role].as_array()
        .or_else(||(g["systemDesign"]["component"]==role)
            .then(||g["systemDesign"]["targetFiles"].as_array()).flatten());
    assigned.is_some_and(|paths|!paths.is_empty() && paths.iter().all(|v|
        v.as_str().is_some_and(|path|
            g["changedSourcePaths"].as_array()
                .is_some_and(|changed|changed.iter().any(|p|p==path))
            && g["acceptedSourceHashes"][path].as_str().is_some_and(|hash|
                hash.len()==64 && hash.bytes().all(|b|b.is_ascii_hexdigit())))))
}

/// Walk through *actual accepted* rebuilt descendants. An unrelated valid
/// intermediate redesign must not permanently strand an inherited experiment.
/// No edge exists without an independently adopted source commit.
fn adopted_descendant_edit<'a>(history:&'a [Value],
    origin_commit:&str,role:&str)->Option<&'a Value> {
    let mut next=origin_commit.to_string();
    let mut visited=BTreeSet::new();
    for _ in 0..history.len() {
        if !visited.insert(next.clone()){return None;}
        let child=history.iter().find(|g|
            g["generatorCommit"].as_str()==Some(next.as_str())
            && adopted(g))?;
        if altered(child,role){return Some(child);}
        next=child["acceptedCommit"].as_str()?.to_owned();
    }
    None
}

fn current<'a>(frontier:&'a [Value],role:&str)->Option<&'a Value> {
    frontier.iter().find(|source|source["role"]==role
        && source["path"].as_str().is_some_and(crate::engine::rsi::safe_path)
        && source["sourceSha256"].as_str().is_some_and(|s|s.len()==64))
}

fn role_name(a:&str,b:&str,existing:&BTreeSet<String>)->String {
    let sanitize=|name:&str|name.chars().filter(|c|
        c.is_ascii_lowercase()||c.is_ascii_digit()||*c=='_')
        .take(12).collect::<String>();
    let a=sanitize(a);let b=sanitize(b);
    let base=format!("bridge_{}_{}",a,b);
    if !existing.contains(&base){return base;}
    for i in 2..=99 {
        let proposal=format!("{base}_{i}");
        if !existing.contains(&proposal){return proposal;}
    }
    "new_hypothesis_compiler".to_string()
}

/// Trace the actual A -> invented B -> improved A -> improved meta method
/// ancestry, never the user-declared intent alone. Every hop must be built
/// from the previous accepted commit with actual model-authored source edits.
fn unfinished_discovery(history:&[Value],frontier:&[Value])->Option<Value>{
    for birth in history.iter().rev().filter(|g|adopted(g)){
        for novel in birth["systemDesign"]["newComponents"].as_array()
            .into_iter().flatten() {
            let Some(role)=novel["role"].as_str() else {continue};
            let Some(source)=current(frontier,role) else{continue};
            if !altered(birth,role){continue;}
            let improved=novel["improvesRole"].as_str()
                .filter(|old|*old!=role && current(frontier,old).is_some())
                .unwrap_or("improvement_methods");
            let Some(origin)=current(frontier,improved) else{continue};
            let born=birth["acceptedCommit"].as_str()?;
            let child=adopted_descendant_edit(history,born,role);
            if child.is_none(){
                return Some(json!({"phase":"develop_invented_intelligence",
                    "role":role,"target":source,"origin":origin,
                    "birthCommit":born,"birthJob":birth["jobId"],
                    "lineage":[]}));
            }
            let child=child?;
            let next=child["acceptedCommit"].as_str()?;
            let back=adopted_descendant_edit(history,next,improved);
            if back.is_none(){
                return Some(json!({"phase":"return_and_redesign_origin",
                    "role":improved,"target":origin,"origin":source,
                    "birthCommit":born,"birthJob":birth["jobId"],
                    "lineage":[child["acceptedCommit"]]}));
            }
            let back=back?;
            let meta=current(frontier,"scientific_method")
                .or_else(||current(frontier,"improvement_methods"))?;
            let meta_role=meta["role"].as_str()?;
            let next=back["acceptedCommit"].as_str()?;
            let finished=adopted_descendant_edit(history,next,meta_role)
                .is_some();
            if !finished {
                return Some(json!({"phase":"rewrite_discovery_algorithm",
                    "role":meta_role,"target":meta,"origin":origin,
                    "birthCommit":born,"birthJob":birth["jobId"],
                    "lineage":[child["acceptedCommit"],back["acceptedCommit"]]}));
            }
        }
    }
    None
}

/// The descendant can choose discovery, isolation, combinatorial testing, or
/// recursive inheritance. The policy is one editable piece of the same agent,
/// not an additional unmodifiable human-written meta-meta layer.
pub(crate) fn plan(history:&[Value],frontier:&[Value],factorial:&Value,
    portfolio:&Value,commit:&str)->Value{
    let frontier_roles=frontier.iter()
        .filter_map(|row|row["role"].as_str())
        .map(str::to_owned).collect::<BTreeSet<_>>();
    let pending=unfinished_discovery(history,frontier);
    let experiment=&factorial["selected"];
    // This is an evolvable improvement-method learner, not a fixed
    // completion counter. Only actual candidates from rebuilt binaries
    // count as experimental trials; self-reported gains are excluded.
    let method_evidence=["isolate_left","isolate_right",
        "test_joint_interaction","discover_new_mechanism",
        "develop_invented_intelligence","return_and_redesign_origin",
        "rewrite_discovery_algorithm"].iter().map(|method|{
        let trials=history.iter().filter(|g|
            g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
            && g["recursiveScientificProgram"]["source"]==
                "compiled_recursive_scientific_discovery_v1"
            && g["recursiveScientificProgram"]["phase"]==*method
            && g["candidateId"].as_str().is_some()).collect::<Vec<_>>();
        let wins=trials.iter().filter(|g|adopted(g)).count();
        json!({"method":method,"observedCandidateTrials":trials.len(),
            "independentlyAdopted":wins,
            "posterior":(wins as f64+1.0)/(trials.len() as f64+2.0),
            "selfReportedSuccessExcluded":true})
    }).collect::<Vec<_>>();
    let mut phase="discover_new_mechanism".to_string();
    let mut focus=Vec::<Value>::new();
    let mut proposed=Value::Null;
    let mut ancestry=Value::Null;
    // After the frozen A-only/B-only/A+B contrasts are complete, the
    // unresolved negative-transfer interaction becomes a source-grounded
    // architectural problem. An invention must respond to an observed gap
    // rather than just an arbitrary co-edit pair.
    let learned_gap=factorial["pairFrontier"].as_array()
        .into_iter().flatten()
        .filter(|pair|pair["observations"]["matchedFrozenHoldoutCases"]
            .as_u64().unwrap_or(0)>0)
        .filter_map(|pair|pair["observations"]["meanInteractionContrast"]
            .as_f64().filter(|n|n.is_finite())
            .map(|contrast|(contrast,pair)))
        .min_by(|a,b|a.0.total_cmp(&b.0))
        .map(|(contrast,pair)|json!({
            "left":pair["left"],"right":pair["right"],
            "matchedFrozenHoldoutCases":
                pair["observations"]["matchedFrozenHoldoutCases"],
            "observedInteractionContrast":contrast,
            "causalIdentification":false,
            "source":"matched_frozen_case_experiment_diagnosis"
        })).unwrap_or(Value::Null);
    if let Some(loop_stage)=pending {
        // A genuinely accepted invented intelligence has scheduling priority
        // over an arbitrary fresh regression experiment.
        phase=loop_stage["phase"].as_str().unwrap_or("discover_new_mechanism").into();
        focus.push(loop_stage["target"].clone());
        if phase=="rewrite_discovery_algorithm"{
            if let Some(self_source)=frontier.iter().find(|row|
                row["path"]=="src/engine/rsi_recursive_scientist.rs"){
                focus.insert(0,self_source.clone());
            }
        }
        ancestry=loop_stage;
    }else if experiment.is_object() {
        phase=match experiment["phase"].as_str().unwrap_or("") {
            "isolate_left"=>"isolate_left",
            "isolate_right"=>"isolate_right",
            "test_joint_interaction"=>"test_joint_interaction",
            _=>"discover_new_mechanism",
        }.into();
        if phase!="discover_new_mechanism" {
            let path=experiment["target"]["path"].as_str().unwrap_or("");
            // Three real failed attempts at the same intervention trigger a
            // new mechanistic experiment rather than endless replays.
            let stalled=history.iter().rev().take(16).filter(|g|
                g["compiledPlanExecuted"]["source"]==
                    "running_compiled_descendant_binary"
                && g["recursiveScientificProgram"]["phase"]==phase
                && g["recursiveScientificProgram"]["focus"][0]["path"]==path
                && g["candidateId"].is_string()
                && !adopted(g)).count();
            if stalled>=3 {
                phase="discover_new_mechanism".into();
            }else{
                focus.push(experiment["target"].clone());
                if phase=="test_joint_interaction"{
                    focus.push(experiment["companion"].clone());
                }
            }
        }
    }
    if phase=="discover_new_mechanism"{
        let sources=if learned_gap.is_object(){
            vec![learned_gap["left"].clone(),learned_gap["right"].clone()]
        }else{
            portfolio["focus"].as_array().cloned().unwrap_or_default()
        };
        if sources.len()>=2 {
            let a=&sources[0];let b=&sources[1];
            if let (Some(ar),Some(br))=(a["role"].as_str(),b["role"].as_str()){
                let candidate=role_name(ar,br,&frontier_roles);
                // A novel invented module must be compiled into a real
                // execution path. This is a MODEL design requirement, not
                // an automatically adopted capability claim.
                proposed=json!({
                    "role":candidate,"domain":"improver",
                    "improvesRole":ar,
                    "purpose":format!(
                        "Build a compiled intelligence component that learns why improving {} changes {}, and uses those experiments to improve the next descendant's generation method.",
                        ar,br),
                    "sourcePaths":[format!("src/engine/rsi_{}.rs",candidate)],
                    "integrationPaths":["src/engine/mod.rs"],
                    "fromRole":ar,"toRole":br,
                    "fromPath":a["path"],"toPath":b["path"],
                    "fromSha256":a["sourceSha256"],
                    "toSha256":b["sourceSha256"],
                    "diagnosedCrossRoleGap":learned_gap,
                    "requiresModelWrittenExecutableMechanism":true,
                    "requiresIndependentAdoption":true,
                    "qualityProven":false
                });
                focus.extend([a.clone(),b.clone()]);
            }
        }
    }
    // A paired assignment may have no viable candidates; never invent a
    // fake source or a successful observation in that situation.
    focus.retain(|row|row["path"].as_str().is_some_and(
        crate::engine::rsi::safe_path));
    focus.dedup_by(|a,b|a["path"]==b["path"]);
    let core=json!({
        "source":"compiled_recursive_scientific_discovery_v1",
        "generatorCommit":commit,
        "historyCount":history.len(),
        "phase":phase,"focus":focus,"invention":proposed,
        "recursiveAncestry":ancestry,
        "experimentDiagnosis":learned_gap,
        "scientificMethodLearning":method_evidence,
        "factorialEvidenceSha256":factorial["sha256"],
        "portfolioEvidenceSha256":portfolio["sha256"],
        "experimentalPolicyIsEditableSource":"src/engine/rsi_recursive_scientist.rs",
        "nextIndependentJudge":"frozen_paired_evaluator",
        "requiresAcceptedSourceAndRebuiltDescendant":true,
        "qualityProven":false,"causalityProven":false
    });
    let revision=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&core).unwrap_or_default()));
    let mut out=core;
    out["revision"]=json!(revision);
    out
}

pub(crate) fn valid(plan:&Value,frontier:&[Value],commit:&str,
    history_count:usize,factorial:&Value,portfolio:&Value)->bool {
    let mut core=plan.clone();
    let Some(map)=core.as_object_mut() else{return false};
    map.remove("revision");
    let fingerprint=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&core).unwrap_or_default()));
    plan["source"]=="compiled_recursive_scientific_discovery_v1"
        && plan["revision"].as_str()==Some(fingerprint.as_str())
        && plan["generatorCommit"]==commit
        && plan["historyCount"].as_u64()==Some(history_count as u64)
        && plan["factorialEvidenceSha256"]==factorial["sha256"]
        && plan["portfolioEvidenceSha256"]==portfolio["sha256"]
        && plan["requiresAcceptedSourceAndRebuiltDescendant"]==true
        && plan["qualityProven"]==false
        && plan["causalityProven"]==false
        && plan["focus"].as_array().is_some_and(|paths|
            paths.len()<=3 && paths.iter().all(|source|
                frontier.iter().any(|f|f==source)))
}

pub(crate) fn instruction(plan:&Value)->String{
    if plan["source"]!="compiled_recursive_scientific_discovery_v1" {
        return String::new();
    }
    let phase=plan["phase"].as_str().unwrap_or("");
    let step=match phase {
        "develop_invented_intelligence"=>
            "The previous independently accepted generation invented a new capability. Actually change the invented module's code, and verify the rebuilt child's independently evaluated improvement.",
        "return_and_redesign_origin"=>
            "The invented capability was improved in a rebuilt and accepted child. NOW use that changed capability to rewrite the original intelligence, not merely write a summary of transfer.",
        "rewrite_discovery_algorithm"=>
            "The B->A return was independently adopted. Rewrite THIS scientific experiment designer and improve its ability to generate better source-changing experiments in future descendants.",
        "isolate_left"|"isolate_right"=>
            "Edit ONLY the selected role; read the companion without changing its sources. Match the original frozen evaluation identity to measure the missing isolated intervention.",
        "test_joint_interaction"=>
            "Edit the two specified modules and compare the resulting independently measured joint effects to the matching isolated variants.",
        _=>
            "Design an actually executable NEW intelligence mechanism from source-grounded failure and transfer evidence. Implement a new Rust module AND its integration path in the same candidate. Use rsi_system_plan(component=system, components=[new role, harness/improvement_methods], componentFiles, newComponents, successorStages, interactionEdges); the model writes real code, and no unaccepted invention is inherited."
    };
    format!("RECURSIVE SCIENTIFIC DISCOVERY phase={phase}, revision={}, \
        focus={}, invention={}, ancestry={}. {} \
        Improvements must change running source and reproduce in the NEXT compiled descendant. \
        Change this experiment designer's code when its hypotheses fail; \
        the frozen independent evaluator must remain untouched.",
        plan["revision"],plan["focus"],plan["invention"],
        plan["recursiveAncestry"],step)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(role:&str,path:&str)->Value{
        json!({"role":role,"path":path,"domain":"improver",
            "sourceSha256":"c".repeat(64)})
    }
    fn adopted(role:&str,head:&str,next:&str,job:&str,path:&str)->Value {
        json!({"accepted":true,"pairedRevalidated":true,
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary",
                "generatorCommit":head},
            "evaluation":{"metrics":{"eligible":true}},
            "generatorCommit":head,"acceptedCommit":next,"jobId":job,
            "changedSourcePaths":[path],
            "acceptedSourceHashes":{(path):"d".repeat(64)},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":head,"generationJobId":job,
                "component":role,"componentFiles":{(role):[path]},
                "targetFiles":[path]}})
    }
    #[test]
    fn selects_missing_factorial_isolation_as_a_real_single_edit(){
        let a=target("harness","src/engine/jobs.rs");
        let b=target("memory","src/engine/memory.rs");
        let frontier=vec![a.clone(),b.clone()];
        let factorial=json!({"sha256":"e".repeat(64),
            "selected":{"phase":"isolate_left","target":a,"companion":b}});
        let plan=plan(&[],&frontier,&factorial,&Value::Null,"a");
        assert_eq!(plan["phase"],"isolate_left");
        assert_eq!(plan["focus"].as_array().unwrap().len(),1);
        assert!(valid(&plan,&frontier,"a",0,&factorial,&Value::Null));
    }
    #[test]
    fn invents_new_executable_mechanism_when_no_isolation_is_pending(){
        let a=target("harness","src/engine/jobs.rs");
        let b=target("learning","src/engine/learning.rs");
        let frontier=vec![a.clone(),b.clone()];
        let portfolio=json!({"sha256":"f".repeat(64),"focus":[a,b]});
        let plan=plan(&[],&frontier,&Value::Null,&portfolio,"a");
        assert_eq!(plan["phase"],"discover_new_mechanism");
        assert_eq!(plan["invention"]["role"],"bridge_harness_learning");
        assert_eq!(plan["invention"]["qualityProven"],false);
        assert!(valid(&plan,&frontier,"a",0,&Value::Null,&portfolio));
    }
    #[test]
    fn repeated_independent_rejection_changes_the_experimental_method(){
        let a=target("harness","src/engine/jobs.rs");
        let b=target("memory","src/engine/memory.rs");
        let frontier=vec![a.clone(),b.clone()];
        let factorial=json!({"sha256":"e".repeat(64),
            "selected":{"phase":"isolate_left","target":a.clone(),"companion":b.clone()}});
        let portfolio=json!({"sha256":"f".repeat(64),"focus":[a.clone(),b]});
        let failures=(0..3).map(|n|json!({
            "candidateId":format!("c-{n}"),"accepted":false,
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary"},
            "recursiveScientificProgram":{
                "source":"compiled_recursive_scientific_discovery_v1",
                "phase":"isolate_left","focus":[a]}
        })).collect::<Vec<_>>();
        let revised=plan(&failures,&frontier,&factorial,&portfolio,"a");
        assert_eq!(revised["phase"],"discover_new_mechanism");
        assert!(revised["invention"].is_object());
        assert_eq!(revised["scientificMethodLearning"][0]["observedCandidateTrials"],3);
        assert!(valid(&revised,&frontier,"a",3,&factorial,&portfolio));
        let mut tampered=revised.clone();
        tampered["qualityProven"]=json!(true);
        assert!(!valid(&tampered,&frontier,"a",3,&factorial,&portfolio));
    }
    #[test]
    fn unaccepted_born_role_cannot_trigger_a_false_recursive_handoff(){
        let role="bridge_memory_learning";
        let path="src/engine/rsi_bridge_memory_learning.rs";
        let frontier=vec![target(role,path),target("memory","src/engine/memory.rs")];
        let mut birth=adopted(role,&"a".repeat(40),&"b".repeat(40),"birth",path);
        birth["pairedRevalidated"]=json!(false);
        birth["systemDesign"]["newComponents"]=json!([
            {"role":role,"improvesRole":"memory"}]);
        let result=plan(&[birth],&frontier,&Value::Null,
            &Value::Null,&"b".repeat(40));
        assert_eq!(result["phase"],"discover_new_mechanism");
        assert!(result["recursiveAncestry"].is_null());
    }
    #[test]
    fn only_real_source_adoptions_advance_invented_child_return_and_meta(){
        let a="a".repeat(40);let b="b".repeat(40);
        let c="c".repeat(40);let d="d".repeat(40);
        let role="bridge_memory_learning";
        let novel_path="src/engine/rsi_bridge_memory_learning.rs";
        let from_path="src/engine/memory.rs";
        let frontier=vec![target(role,novel_path),
            target("memory",from_path),
            target("improvement_methods","src/engine/rsi_recursive_scientist.rs")];
        let mut birth=adopted(role,&a,&b,"birth",novel_path);
        birth["systemDesign"]["newComponents"]=json!([
            {"role":role,"improvesRole":"memory"}]);
        let phase=plan(&[birth.clone()],&frontier,&Value::Null,
            &Value::Null,&b);
        assert_eq!(phase["phase"],"develop_invented_intelligence");
        let child=adopted(role,&b,&c,"child",novel_path);
        let phase=plan(&[birth.clone(),child.clone()],
            &frontier,&Value::Null,&Value::Null,&c);
        assert_eq!(phase["phase"],"return_and_redesign_origin");
        let returned=adopted("memory",&c,&d,"returned",from_path);
        let phase=plan(&[birth,child,returned],&frontier,
            &Value::Null,&Value::Null,&d);
        assert_eq!(phase["phase"],"rewrite_discovery_algorithm");
    }
}
