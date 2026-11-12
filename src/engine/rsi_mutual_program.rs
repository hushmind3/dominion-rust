//! Recompilable A -> B -> A intelligence-improvement curriculum.
//!
//! The prior system's contribution must survive into the *next* compiled
//! system's work. This code only plans experiments and observes outcomes;
//! it cannot mint adoption, change a frozen oracle, or promote production.
//! The curriculum and its selection algorithm are ordinary editable source.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn sha(value:&Value)->String {
    format!("{:x}",Sha256::digest(serde_json::to_vec(value).unwrap_or_default()))
}
pub(crate) fn adopted_source(g:&Value)->bool {
    g["accepted"]==true && g["pairedRevalidated"]==true
        && g["candidateId"].as_str().is_some()
        && g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
        && g["compiledPlanExecuted"]["generatorCommit"]==g["generatorCommit"]
        && g["evaluation"]["metrics"]["eligible"]==true
        && g["acceptedCommit"].as_str().is_some_and(|v|
            v.len()==40 && v.bytes().all(|b|b.is_ascii_hexdigit()))
        && g["generatorCommit"].as_str().is_some_and(|v|v.len()==40)
        && g["systemDesign"]["source"]=="model_authored_system_design"
        && g["systemDesign"]["generatorCommit"]==g["generatorCommit"]
        && g["systemDesign"]["generationJobId"]==g["jobId"]
}
pub(crate) fn implemented_roles(g:&Value)->Vec<String> {
    if !adopted_source(g){return vec![];}
    let design=&g["systemDesign"];
    let changed=g["changedSourcePaths"].as_array()
        .or_else(||design["targetFiles"].as_array());
    let roles=design["components"].as_array().cloned()
        .unwrap_or_else(||vec![design["component"].clone()]);
    let mut result=BTreeSet::new();
    for role in roles.iter().filter_map(Value::as_str).filter(|r|*r!="system") {
        let files=design["componentFiles"][role].as_array().or_else(||
            (roles.len()==1).then(||design["targetFiles"].as_array()).flatten());
        if files.is_some_and(|files|!files.is_empty() && files.iter().all(|f|
            f.as_str().is_some_and(|p|changed.is_some_and(|changed|
                changed.iter().any(|v|v.as_str()==Some(p)))
                && g["acceptedSourceHashes"][p].as_str().is_some_and(|h|
                    h.len()==64 && h.bytes().all(|b|b.is_ascii_hexdigit()))))){
            result.insert(role.to_owned());
        }
    }
    result.into_iter().collect()
}
/// Credit the actual, independently evaluated *next* generator, not a
/// successful patch in the same generation. Preserve negative observations.
pub(crate) fn transfer_graph(history:&[Value])->Value {
    let mut edges=BTreeMap::<(String,String),(u64,u64,u64,u64,BTreeSet<String>)>::new();
    for parent in history.iter().filter(|g|adopted_source(g)) {
        let Some(next_commit)=parent["acceptedCommit"].as_str() else{continue};
        let roles=implemented_roles(parent);
        if roles.is_empty(){continue;}
        for child in history.iter().filter(|g|
            g["generatorCommit"].as_str()==Some(next_commit)
            && g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
            && g["compiledPlanExecuted"]["generatorCommit"]==g["generatorCommit"]) {
            let Some(target_role)=child["metaPolicy"]["target"]["role"].as_str() else{continue};
            let measured=child["pairedMeasured"]==true || child["pairedRevalidated"]==true;
            let adopted=adopted_source(child)
                && implemented_roles(child).iter().any(|role|role==target_role);
            for role in &roles {
                if role==target_role {continue;}
                let row=edges.entry((role.clone(),target_role.to_owned())).or_default();
                row.0+=1;row.1+=u64::from(measured);
                row.2+=u64::from(adopted);
                row.3+=u64::from(measured && !adopted);
                if let Some(id)=child["id"].as_str(){if row.4.len()<12{row.4.insert(id.into());}}
            }
        }
    }
    let lines=edges.into_iter().map(|((from,to),(attempts,measured,adopted,rejected,ids))|
        json!({"fromRole":from,"toRole":to,"attempts":attempts,
            "independentlyMeasured":measured,
            "nextGenerationAdoptions":adopted,"measuredNonAdoptions":rejected,
            "posterior":(adopted as f64+1.0)/(measured as f64+2.0),
            "observations":ids.into_iter().collect::<Vec<_>>(),
            "causalityProven":false})).collect::<Vec<_>>();
    let mut graph=json!({"source":"compiled_intergenerational_transfer_v1",
        "edges":lines,"observationalOnly":true,
        "rewardSource":"independent_child_source_adoption"});
    graph["sha256"]=json!(sha(&graph));
    graph
}
fn matching<'a>(frontier:&'a [Value],role:&str,preferred:&str,
    allowed:&[&str])->Option<&'a Value> {
    frontier.iter().filter(|row|row["role"]==role
        && row["domain"].as_str().is_some_and(|d|allowed.contains(&d))
        && row["path"].as_str().is_some_and(crate::engine::rsi::safe_path))
        .min_by(|a,b|{
            let prefer_a=if a["path"]==preferred{0}else{1};
            let prefer_b=if b["path"]==preferred{0}else{1};
            prefer_a.cmp(&prefer_b).then_with(||
                a["sourceAttempts"].as_u64().cmp(&b["sourceAttempts"].as_u64()))
                .then_with(||a["path"].as_str().cmp(&b["path"].as_str()))
        })
}
fn pairing_score(a:&Value,b:&Value,within:&Value,transfer:&Value)->f64{
    let source=a["role"].as_str().unwrap_or("");
    let target=b["role"].as_str().unwrap_or("");
    let observed=transfer["edges"].as_array().into_iter().flatten()
        .filter(|row|row["fromRole"]==source&&row["toRole"]==target)
        .map(|row|row["posterior"].as_f64().unwrap_or(0.5))
        .next().unwrap_or(0.5);
    let reverse=transfer["edges"].as_array().into_iter().flatten()
        .find(|row|row["fromRole"]==target&&row["toRole"]==source)
        .and_then(|row|row["posterior"].as_f64()).unwrap_or(0.5);
    let within_gain=within["edges"].as_array().into_iter().flatten()
        .filter(|edge|edge["sourceRole"]==source&&edge["targetRole"]==target
            && edge["isolatedObservations"].as_u64().unwrap_or(0)>0)
        .map(|edge|edge["directionalScore"].as_f64().unwrap_or(0.0))
        .next().unwrap_or(0.0);
    let novelty=if target=="improvement_methods"{0.17}else{0.08};
    0.4*observed+0.4*reverse+0.25*within_gain+novelty
        -0.015*(b["sourceAttempts"].as_u64().unwrap_or(0).min(15) as f64)
}
/// This executable policy chooses a new goal from an actual source-backed
/// frontier, or continues a committed A -> B -> A handoff after acceptance.
/// Every phase is compiled into the successor that uses it. A failed child
/// does NOT advance the state machine or manufacture an inherited capability.
pub(crate) fn propose(history:&[Value],frontier:&[Value],commit:&str,
    selected:&Value,within:&Value,allowed:&[&str])->Value {
    let graph=transfer_graph(history);
    let predecessor=history.iter().rev().find(|g|
        adopted_source(g) && g["acceptedCommit"]==commit);
    let continuation=predecessor.and_then(|g|{
        let prior=&g["mutualProgram"];
        if prior["generatorCommit"]!=g["generatorCommit"]
            || !matches!(prior["phase"].as_str(),Some("seed"|"complement")) {
            return None;
        }
        let (next_role,next_path,phase)=if prior["phase"]=="seed" {
            (prior["partner"]["role"].as_str()?,
                prior["partner"]["path"].as_str()?,"complement")
        }else{
            (prior["origin"]["role"].as_str()?,
                prior["origin"]["path"].as_str()?,"return")
        };
        let match_row=matching(frontier,next_role,next_path,allowed)?;
        let origin=if prior["phase"]=="seed"{prior["target"].clone()}
            else{prior["origin"].clone()};
        let partner=if prior["phase"]=="seed"{prior["partner"].clone()}
            else{prior["target"].clone()};
        Some((phase,match_row.clone(),origin,partner,
            prior["rootGenerationId"].as_str().filter(|v|!v.is_empty())
                .or_else(||g["id"].as_str()).unwrap_or("").to_owned()))
    });
    let (phase,target,origin,partner,root_id)=if let Some(next)=continuation {
        next
    }else{
        let Some(primary)=selected.as_object().map(|_|selected.clone())
            else{return Value::Null};
        let Some(primary_role)=primary["role"].as_str() else{return Value::Null};
        let mut peers=frontier.iter().filter(|row|
            row["role"].as_str().is_some_and(|role|role!=primary_role)
            && row["domain"].as_str().is_some_and(|d|allowed.contains(&d)))
            .map(|row|(pairing_score(&primary,row,within,&graph),row.clone()))
            .collect::<Vec<_>>();
        peers.sort_by(|a,b|b.0.total_cmp(&a.0)
            .then_with(||a.1["path"].as_str().cmp(&b.1["path"].as_str())));
        let Some((_,complement))=peers.into_iter().next() else{return Value::Null};
        ("seed",primary.clone(),primary,complement,String::new())
    };
    let core=json!({"source":"compiled_reciprocal_improvement_program_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "phase":phase,"target":target,"origin":origin,
        "partner":partner,"rootGenerationId":root_id,
        "transitionGraphSha256":graph["sha256"],"transferGraph":graph,
        "sequence":["improve_primary","improve_complement","return_improved_method"],
        "nextPhase":match phase {"seed"=>"complement",
            "complement"=>"return",_=>"completed"},
        "validation":"independent_candidate_evaluation_then_source_adoption",
        "currentSourceRebound":true,
        "qualityProven":false});
    let mut program=core;
    program["revision"]=json!(sha(&program));
    program
}
/// Authenticates the descendant's reported observations, not the old
/// supervisor's version of their ranking policy. The descendant can evolve
/// transition_graph while the supervisor retains immutable source checks.
pub(crate) fn valid_transfer_graph(graph:&Value)->bool {
    let mut core=graph.clone();
    let Some(obj)=core.as_object_mut() else{return false};
    obj.remove("sha256");
    let digest=sha(&core);
    graph["source"]=="compiled_intergenerational_transfer_v1"
        && graph["sha256"].as_str()==Some(digest.as_str())
        && graph["observationalOnly"]==true
        && graph["rewardSource"]=="independent_child_source_adoption"
        && graph["edges"].as_array().is_some_and(|v|v.len()<=128
            && v.iter().all(|row|{
                let observations=row["attempts"].as_u64().unwrap_or(0);
                let measured=row["independentlyMeasured"].as_u64().unwrap_or(0);
                let adopted=row["nextGenerationAdoptions"].as_u64().unwrap_or(0);
                let rejected=row["measuredNonAdoptions"].as_u64().unwrap_or(0);
                observations>0 && measured<=observations && adopted<=measured
                    && rejected<=measured && row["causalityProven"]==false
                    && row["fromRole"].as_str().is_some_and(|v|!v.is_empty())
                    && row["toRole"].as_str().is_some_and(|v|!v.is_empty())
            }))
}
pub(crate) fn valid_program(program:&Value,frontier:&[Value],commit:&str,
    history_len:usize,graph:&Value)->bool {
    if program.is_null(){return true;}
    let mut copy=program.clone();
    let Some(obj)=copy.as_object_mut() else{return false};
    obj.remove("revision");
    let revision=sha(&copy);
    let target=&program["target"];
    let Some(path)=target["path"].as_str() else{return false};
    program["source"]=="compiled_reciprocal_improvement_program_v1"
        && program["revision"].as_str()==Some(revision.as_str())
        && program["generatorCommit"]==commit
        && program["historyCount"].as_u64()==Some(history_len as u64)
        && valid_transfer_graph(graph)
        && program["transferGraph"]==graph
        && program["transitionGraphSha256"]==graph["sha256"]
        && matches!(program["phase"].as_str(),Some("seed"|"complement"|"return"))
        && program["currentSourceRebound"]==true
        && program["qualityProven"]==false
        && crate::engine::rsi::safe_path(path)
        && frontier.iter().any(|row|row==target)
}
/// End-to-end lineage proof for an accepted A -> B -> A loop. The
/// transitions and source hashes are independently adopted; the graph does
/// not claim that A caused B without an intervention-control experiment.
pub(crate) fn completed_cycles(history:&[Value])->Value {
    let mut cycles=Vec::new();
    let mut seen=BTreeSet::new();
    for returned in history.iter().filter(|g|
        adopted_source(g) && g["mutualProgram"]["phase"]=="return") {
        let program=&returned["mutualProgram"];
        let Some(root_id)=program["rootGenerationId"].as_str()
            .filter(|s|!s.is_empty()) else{continue};
        let Some(root)=history.iter().find(|g|g["id"]==root_id
            && adopted_source(g)
            && g["mutualProgram"]["phase"]=="seed") else{continue};
        let (Some(origin),Some(partner))=(
            program["origin"]["role"].as_str(),
            program["partner"]["role"].as_str()) else{continue};
        if origin==partner || !implemented_roles(root).iter().any(|r|r==origin)
            || !implemented_roles(returned).iter().any(|r|r==origin) {continue;}
        let Some(middle)=history.iter().find(|g|
            adopted_source(g)
            && g["generatorCommit"]==root["acceptedCommit"]
            && g["acceptedCommit"]==returned["generatorCommit"]
            && g["mutualProgram"]["phase"]=="complement"
            && g["mutualProgram"]["rootGenerationId"]==root_id
            && implemented_roles(g).iter().any(|r|r==partner))
        else{continue};
        if program["generatorCommit"]!=returned["generatorCommit"]
            || middle["mutualProgram"]["origin"]["role"]!=origin
            || middle["mutualProgram"]["partner"]["role"]!=partner {
            continue;
        }
        let Some(returned_id)=returned["id"].as_str() else{continue};
        if !seen.insert(returned_id.to_owned()){continue;}
        cycles.push(json!({
            "originGenerationId":root_id,"complementGenerationId":middle["id"],
            "returnGenerationId":returned["id"],
            "sourceRole":origin,"complementRole":partner,
            "sourceCommit":root["acceptedCommit"],
            "complementCommit":middle["acceptedCommit"],
            "returnCommit":returned["acceptedCommit"],
            "allThreeIndependentAdoptions":true,
            "causalityProven":false
        }));
    }
    let mut result=json!({"source":"independently_adopted_reciprocal_lineage_v1",
        "completedCycles":cycles.len(),"cycles":cycles,
        "claim":"three_real_sequential_adoptions_not_proof_of_causal_effect"});
    result["sha256"]=json!(sha(&result));
    result
}
pub(crate) fn instruction(program:&Value)->String {
    if !program.is_object(){return String::new();}
    format!(
        "COMPILED MUTUAL INTELLIGENCE PROGRAM phase={}: target role={} source={} sha={}.\n\
        Origin role={} partner role={}, planned return role={}.\n\
        Current generation must change source code, not just describe a plan. \
        After independent adoption the NEXT compiled agent rebinds its own \
        sources and decides whether to proceed. If validation fails, the \
        current phase remains unproven. Model-written next methods, memory, \
        harness or tools may replace these heuristics in a new generation.",
        program["phase"],program["target"]["role"],program["target"]["path"],
        program["target"]["sourceSha256"],program["origin"]["role"],
        program["partner"]["role"],program["origin"]["role"])
}
#[cfg(test)]
mod tests {
    use super::*;
    fn row(role:&str,path:&str)->Value {
        json!({"role":role,"domain":if role=="harness"{"context"}else if role=="memory"{"memory"}else{"improver"},
            "path":path,"sourceSha256":"f".repeat(64),"sourceAttempts":0})
    }
    #[test]
    fn accepted_source_activates_complement_then_return_only_after_adoption() {
        let a=row("harness","src/engine/jobs.rs");
        let b=row("memory","src/engine/memory.rs");
        let frontier=vec![a.clone(),b.clone()];
        let first=propose(&[],&frontier,&"a".repeat(40),&a,&json!({"edges":[]}),&["context","memory"]);
        assert_eq!(first["phase"],"seed");
        assert!(valid_program(&first,&frontier,&"a".repeat(40),0,&transfer_graph(&[])));
        let accepted=json!({"id":"g1","generatorCommit":"a","acceptedCommit":"b",
            "candidateId":"candidate1",
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary",
                "generatorCommit":"a"},
            "jobId":"job1","accepted":true,"pairedRevalidated":true,
            "evaluation":{"metrics":{"eligible":true}},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":"a","generationJobId":"job1",
                "component":"harness","targetFiles":["src/engine/jobs.rs"]},
            "changedSourcePaths":["src/engine/jobs.rs"],
            "acceptedSourceHashes":{"src/engine/jobs.rs":"f".repeat(64)},
            "mutualProgram":first});
        // In the tests, use real 40-byte commits like the production API.
        let mut accepted=accepted;
        accepted["generatorCommit"]=json!("a".repeat(40));
        accepted["acceptedCommit"]=json!("b".repeat(40));
        accepted["systemDesign"]["generatorCommit"]=json!("a".repeat(40));
        accepted["compiledPlanExecuted"]["generatorCommit"]=json!("a".repeat(40));
        let one=propose(&[accepted.clone()],&frontier,&"b".repeat(40),&a,
            &json!({"edges":[]}),&["context","memory"]);
        assert_eq!(one["phase"],"complement");
        assert_eq!(one["target"]["role"],"memory");
        let mut failed=accepted.clone();
        failed["id"]=json!("g2");failed["accepted"]=json!(false);
        failed["generatorCommit"]=json!("b".repeat(40));
        let after_failure=propose(&[accepted.clone(),failed],&frontier,
            &"b".repeat(40),&a,&json!({"edges":[]}),&["context","memory"]);
        assert_eq!(after_failure["phase"],"complement");
        let mut child=accepted.clone();
        child["id"]=json!("g3"); child["generatorCommit"]=json!("b".repeat(40));
        child["acceptedCommit"]=json!("c".repeat(40));
        child["jobId"]=json!("job3");
        child["systemDesign"]["generatorCommit"]=json!("b".repeat(40));
        child["compiledPlanExecuted"]["generatorCommit"]=json!("b".repeat(40));
        child["systemDesign"]["generationJobId"]=json!("job3");
        child["systemDesign"]["component"]=json!("memory");
        child["systemDesign"]["targetFiles"]=json!(["src/engine/memory.rs"]);
        child["changedSourcePaths"]=json!(["src/engine/memory.rs"]);
        child["acceptedSourceHashes"]=json!({"src/engine/memory.rs":"f".repeat(64)});
        child["mutualProgram"]=one;
        let finish=propose(&[accepted.clone(),child.clone()],&frontier,
            &"c".repeat(40),&b,&json!({"edges":[]}),&["context","memory"]);
        assert_eq!(finish["phase"],"return");
        assert_eq!(finish["target"]["role"],"harness");
        assert_eq!(completed_cycles(&[accepted.clone(),child.clone()])["completedCycles"],0);
        let mut returned=accepted.clone();
        returned["id"]=json!("g4");
        returned["jobId"]=json!("job4");
        returned["generatorCommit"]=json!("c".repeat(40));
        returned["compiledPlanExecuted"]["generatorCommit"]=json!("c".repeat(40));
        returned["systemDesign"]["generatorCommit"]=json!("c".repeat(40));
        returned["systemDesign"]["generationJobId"]=json!("job4");
        returned["acceptedCommit"]=json!("d".repeat(40));
        returned["mutualProgram"]=finish;
        assert_eq!(completed_cycles(&[accepted.clone(),child.clone(),returned.clone()])
            ["completedCycles"],1);
        returned["accepted"]=json!(false);
        assert_eq!(completed_cycles(&[accepted,child,returned])["completedCycles"],0,
            "an unadopted return does not complete recursive improvement");
    }
}
