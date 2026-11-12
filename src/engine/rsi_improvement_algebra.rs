//! Model-authored, source-backed improvement operators and compositional
//! intelligence graph. The set of roles, improvement rules, graph shape and
//! cycle length are NOT pre-enumerated by a human stage machine.
//!
//! Accepted code may add/edit its own rule generator. Child binaries rebind
//! every target to current source SHA and independently rederive the agenda.
//! An adopted rule is a validated source hypothesis, never a causal proof.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn digest(v:&Value)->String {
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn hash_ok(s:&str,n:usize)->bool {
    s.len()==n && s.bytes().all(|c|c.is_ascii_hexdigit())
}
fn adopted(g:&Value)->bool {
    g["accepted"]==true && g["pairedRevalidated"]==true
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["evaluation"]["metrics"]["eligible"]==true
        && g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
        && g["compiledPlanExecuted"]["generatorCommit"]==g["generatorCommit"]
        && g["systemDesign"]["source"]=="model_authored_system_design"
        && g["systemDesign"]["generationJobId"]==g["jobId"]
        && g["systemDesign"]["generatorCommit"]==g["generatorCommit"]
        && g["acceptedCommit"].as_str().is_some_and(|s|hash_ok(s,40))
}
fn roles(v:&Value)->Option<Vec<String>> {
    let a=v.as_array()?;
    if a.is_empty()||a.len()>8 {return None}
    let mut set=BTreeSet::new();
    for row in a {
        let role=row.as_str()?;
        if !crate::engine::rsi_open_topology::valid_role(role)
            || !set.insert(role.to_owned()) {return None}
    }
    Some(set.into_iter().collect())
}
fn witnesses(g:&Value,rule:&Value)->bool {
    if !adopted(g){return false}
    let Some(path)=rule["methodPath"].as_str()
        .filter(|s|crate::engine::rsi::safe_path(s))else{return false};
    let Some(method_role)=rule["methodRole"].as_str()else{return false};
    g["systemDesign"]["targetFiles"].as_array().is_some_and(|rows|
        rows.iter().any(|p|p==path))
        && g["systemDesign"]["componentFiles"][method_role].as_array()
            .is_some_and(|rows|rows.iter().any(|p|p==path))
        && g["changedSourcePaths"].as_array().is_some_and(|rows|
            rows.iter().any(|p|p==path))
        && g["acceptedSourceHashes"][path].as_str().is_some_and(|s|hash_ok(s,64))
        && roles(&rule["premises"]).is_some()
        && roles(&rule["products"]).is_some()
        && rule["name"].as_str().is_some_and(|s| !s.is_empty() && s.len()<=64)
        && rule["hypothesis"].as_str().is_some_and(|s|s.len()>=25)
}
fn source<'a>(frontier:&'a [Value],role:&str)->Option<&'a Value> {
    frontier.iter().find(|row|
        row["role"]==role && row["path"].as_str().is_some_and(
            crate::engine::rsi::safe_path)
        && row["sourceSha256"].as_str().is_some_and(|s|hash_ok(s,64)))
}
fn path_source<'a>(frontier:&'a [Value],role:&str,path:&str)->Option<&'a Value> {
    frontier.iter().find(|row|row["role"]==role && row["path"]==path
        && row["sourceSha256"].as_str().is_some_and(|s|hash_ok(s,64)))
}
fn changed_roles(g:&Value,frontier:&[Value])->BTreeSet<String> {
    if !adopted(g){return BTreeSet::new()}
    let changed=g["changedSourcePaths"].as_array();
    frontier.iter().filter_map(|row|{
        let path=row["path"].as_str()?;
        let role=row["role"].as_str()?;
        (changed.is_some_and(|files|files.iter().any(|p|p==path))
            && g["acceptedSourceHashes"][path].as_str()
                .is_some_and(|s|hash_ok(s,64))).then(||role.to_owned())
    }).collect()
}
fn record_trial(g:&Value,rule_id:&str,target:&Value)->bool {
    g["candidateId"].as_str().is_some()
        && g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
        && g["improvementAlgebraPlan"]["selected"]["ruleId"]==rule_id
        && g["improvementAlgebraPlan"]["selected"]["target"]["path"]==target["path"]
        && g["improvementAlgebraPlan"]["generatorCommit"]==g["generatorCommit"]
}
/// Parent-authored source rules are admitted ONLY after independent accepted
/// compilation. Accepted rules may freely refer to invented future roles.
/// The executing child can evolve this entire algorithm in ordinary Rust.
pub(crate) fn compile(history:&[Value],frontier:&[Value],commit:&str)->Value{
    let mut all=BTreeMap::<String,Value>::new();
    for g in history.iter().filter(|g|adopted(g)){
        for rule in g["systemDesign"]["improvementRules"].as_array()
            .into_iter().flatten(){
            if !witnesses(g,rule){continue}
            let (Some(method_role),Some(path),Some(premises),Some(products))=(
                rule["methodRole"].as_str(),rule["methodPath"].as_str(),
                roles(&rule["premises"]),roles(&rule["products"]))else{continue};
            let Some(method)=path_source(frontier,method_role,path)else{continue};
            if premises.iter().any(|r|source(frontier,r).is_none())
                || products.iter().any(|r|source(frontier,r).is_none()){
                continue;
            }
            let witness=json!([g["acceptedCommit"],g["jobId"],
                rule["name"],rule["premises"],rule["products"],
                rule["methodRole"],rule["methodPath"]]);
            let key=digest(&witness);
            let rule_record=json!({
                "id":key,
                "name":rule["name"],"premises":premises,"products":products,
                "methodRole":method_role,"method":method,
                "mechanism":rule["hypothesis"],
                "measurement":rule["observable"],
                "acceptedSourceHash":g["acceptedSourceHashes"][path],
                "acceptedAtCommit":g["acceptedCommit"],
                "originGeneration":g["jobId"],
                "source":"independently_adopted_source_written_operator",
                "causalityProven":false
            });
            all.insert(key,rule_record);
        }
    }
    let rules=all.into_values().take(128).collect::<Vec<_>>();
    let mut edges=Vec::<Value>::new();
    for r in &rules {
        for premise in r["premises"].as_array().into_iter().flatten() {
            for product in r["products"].as_array().into_iter().flatten() {
                if premise!=product {
                    edges.push(json!({"from":premise,"to":product,
                        "ruleId":r["id"]}));
                }
            }
        }
    }
    // Generate a cycle of ANY length and across ANY observed/intelligible
    // roles. Breadth-first search is source-editable, not hard-coded A-B-A.
    let mut best_cycle=Vec::<Value>::new();
    let mut best_origin=String::new();
    for root in frontier.iter().filter_map(|r|r["role"].as_str()).take(64) {
        let mut queue=vec![(root.to_string(),Vec::<Value>::new())];
        let mut seen=BTreeSet::<String>::new();
        for _ in 0..12 {
            let mut next=Vec::new();
            for (role,chain) in queue{
                for edge in edges.iter().filter(|e|
                    e["from"].as_str()==Some(role.as_str())) {
                    let Some(to)=edge["to"].as_str()else{continue};
                    let mut extended=chain.clone();
                    extended.push(edge.clone());
                    if to==root && extended.len()>1 &&
                        (best_cycle.is_empty()||extended.len()<best_cycle.len()) {
                        best_cycle=extended.clone();
                        best_origin=root.to_string();
                    }
                    if seen.insert(to.to_owned()) && to!=root {
                        next.push((to.to_owned(),extended));
                    }
                }
            }
            if next.is_empty(){break;}
            queue=next;
        }
    }
    // The active frontier is derived from the REAL most recent independently
    // adopted source change, not from a pre-enumerated A->B->A stage index.
    // Multi-input operators can require evidence from several generations.
    let changed_after_adoption=history.iter().rev().filter(|g|adopted(g))
        .map(|g|changed_roles(g,frontier))
        .filter(|roles|!roles.is_empty()).collect::<Vec<_>>();
    let latest_changed=changed_after_adoption.first().cloned()
        .unwrap_or_default();
    let accumulated_roles=changed_after_adoption.iter().take(16)
        .flat_map(|set|set.iter().cloned()).collect::<BTreeSet<_>>();
    let mut candidates=Vec::<(i64,Value)>::new();
    for rule in &rules {
        let Some(id)=rule["id"].as_str()else{continue};
        let Some(products)=roles(&rule["products"])else{continue};
        let Some(premises)=roles(&rule["premises"])else{continue};
        let premise_observed=premises.iter()
            .all(|role|accumulated_roles.contains(role));
        let fresh_activation=premises.iter()
            .any(|role|latest_changed.contains(role));
        for product in &products{
            let Some(target)=source(frontier,product)else{continue};
            let trials=history.iter().filter(|g|record_trial(g,id,target))
                .collect::<Vec<_>>();
            let accepted=trials.iter().filter(|g|adopted(g)
                && changed_roles(g,frontier).contains(product)).count();
            let regressions=trials.len().saturating_sub(accepted);
            // Rewriting the real methodology is a general-purpose fallback,
            // not a fixed list of domain-specific stages.
            let revise=regressions>=2;
            let actual_target=if revise{&rule["method"]}else{target};
            let science_evidence=json!({"observedCandidateTrials":trials.len(),
                "acceptedTargetEdits":accepted,"rejectedOrInconclusive":regressions,
                "betaMean":(accepted as f64+1.0)/(trials.len() as f64+2.0),
                "declaredClaimsNeverCountAsObservations":true});
            let cycle_member=best_cycle.iter().any(|e|e["ruleId"]==id
                && e["to"]==product.as_str().unwrap_or(""));
            let score=(if cycle_member{500}else{0})
                + (if revise{1100}else{0})
                + (if fresh_activation && premise_observed{900}else{0})
                + (if premise_observed{180}else{0})
                + (if trials.is_empty(){160}else{0})
                + (accepted as i64).min(10)*12
                - (regressions as i64).min(12)*24;
            candidates.push((score,json!({
                "ruleId":id,"rule":rule,"target":actual_target,
                "productTarget":target,"intent":if revise{
                    "rewrite_accepted_method_source"
                }else{"improve_composed_intelligence"},
                "observations":science_evidence,
                "inVerifiedCycle":cycle_member,
                "premiseObservedInAcceptedGenerations":premise_observed,
                "activatedByLatestIndependentAdoption":fresh_activation,
                "qualityProven":false
            })));
        }
    }
    candidates.sort_by(|(ascore,a),(bscore,b)|bscore.cmp(ascore)
        .then_with(||a["ruleId"].as_str().cmp(&b["ruleId"].as_str()))
        .then_with(||a["target"]["path"].as_str().cmp(&b["target"]["path"].as_str())));
    let selected=candidates.first().map(|(_,v)|v.clone()).unwrap_or(Value::Null);
    let mut result=json!({
        "source":"compiled_model_authored_improvement_algebra_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "operators":rules,"interactionEdges":edges,
        "selected":selected,
        "cycle":{"startingRole":best_origin,
            "edges":best_cycle,"causalityProven":false},
        "candidateCount":candidates.len(),
        "selectionPolicy":"editable_source_compositional_rule_search",
        "latestIndependentlyChangedRoles":latest_changed,
        "acceptedRevisionPremiseMemory":accumulated_roles,
        "externalEvaluationOwner":"frozen_paired_supervisor",
        "qualityProven":false
    });
    result["sha256"]=json!(digest(&result));
    result
}
pub(crate) fn valid(plan:&Value,frontier:&[Value],commit:&str,
    history_len:usize)->bool {
    let mut unsigned=plan.clone();
    let Some(obj)=unsigned.as_object_mut()else{return false};
    obj.remove("sha256");
    if plan["source"]!="compiled_model_authored_improvement_algebra_v1"
        || plan["sha256"].as_str()!=Some(digest(&unsigned).as_str())
        || plan["generatorCommit"]!=commit
        || plan["historyCount"].as_u64()!=Some(history_len as u64)
        || plan["externalEvaluationOwner"]!="frozen_paired_supervisor"
        || plan["qualityProven"]!=false {return false}
    let rule_valid=|rule:&Value|{
        rule["id"].as_str().is_some_and(|s|hash_ok(s,64))
            && frontier.iter().any(|f|f==&rule["method"])
            && roles(&rule["premises"]).is_some()
            && roles(&rule["products"]).is_some()
            && rule["acceptedAtCommit"].as_str().is_some_and(|s|hash_ok(s,40))
            && rule["causalityProven"]==false
    };
    if !plan["operators"].as_array().is_some_and(|xs|xs.len()<=128
        && xs.iter().all(rule_valid)) {return false}
    let selected=&plan["selected"];
    selected.is_null() || (selected["qualityProven"]==false
        && plan["operators"].as_array().is_some_and(|xs|
            xs.iter().any(|r|r==&selected["rule"]))
        && frontier.iter().any(|f|f==&selected["target"])
        && frontier.iter().any(|f|f==&selected["productTarget"]))
}
pub(crate) fn instruction(plan:&Value)->String{
    let Some(target)=plan["selected"]["target"]["path"].as_str()else{
        return "No independently adopted source-backed improvement rules exist yet. Invent executable improvementRules through rsi_system_plan in an ordinary source-changing descendant and have the frozen evaluator decide adoption.".into();
    };
    format!(
        "COMPOSITIONAL INTELLIGENCE IMPROVEMENT: rule={} method={} acceptedBirth={}, currentTarget={} sourceSha={} purpose={} measuredResults={}. \
         The descendant may write new improvementRules with ANY role combination and change their Rust implementation. \
         An independently accepted method may become the method which rewrites its own next improvement rule. \
         Following one human-authored stage list is NOT required; the next code target is selected from current source and adopted rule composition. \
         No outcome, competence, or causality is established by a proposed rule.",
        plan["selected"]["rule"]["name"],plan["selected"]["rule"]["method"]["path"],
        plan["selected"]["rule"]["acceptedAtCommit"],
        target,plan["selected"]["target"]["sourceSha256"],
        plan["selected"]["intent"],plan["selected"]["observations"])
}
#[cfg(test)]
mod tests{
    use super::*;
    fn row(role:&str,path:&str)->Value{
        json!({"role":role,"path":path,"domain":"improver",
            "sourceSha256":"d".repeat(64)})
    }
    fn birth(head:&str,next:&str,job:&str,name:&str,method:&str,
        role:&str,from:&str,to:&str)->Value{
        json!({
            "accepted":true,"pairedRevalidated":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "evaluation":{"metrics":{"eligible":true}},
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary",
                "generatorCommit":head},
            "generatorCommit":head,"acceptedCommit":next,"jobId":job,
            "changedSourcePaths":[method],
            "acceptedSourceHashes":{(method):"a".repeat(64)},
            "systemDesign":{"source":"model_authored_system_design",
                "generationJobId":job,"generatorCommit":head,
                "componentFiles":{(role):[method]},
                "targetFiles":[method],
                "improvementRules":[{
                    "name":name,"premises":[from],"products":[to],
                    "methodRole":role,"methodPath":method,
                    "hypothesis":"Improved source-based cross-intelligence operators increase downstream edit success",
                    "observable":"independent downstream holdout gains"}]}
        })
    }
    #[test]
    fn arbitrary_model_written_rules_compose_longer_cycles(){
        let h="a".repeat(40);let a="b".repeat(40);let b="c".repeat(40);let c="d".repeat(40);
        let m="src/engine/rsi_improvement_algebra.rs";
        let frontier=vec![row("scientific_method",m),
            row("memory","src/engine/memory.rs"),
            row("learning","src/engine/learning.rs"),
            row("tools","src/engine/tools.rs")];
        let hist=vec![
            birth(&h,&a,"one","learn_memory",m,"scientific_method","memory","learning"),
            birth(&a,&b,"two","learn_tools",m,"scientific_method","learning","tools"),
            birth(&b,&c,"three","learn_memory_again",m,"scientific_method","tools","memory"),
        ];
        let plan=compile(&hist,&frontier,&c);
        assert_eq!(plan["operators"].as_array().unwrap().len(),3);
        assert_eq!(plan["cycle"]["edges"].as_array().unwrap().len(),3);
        assert!(valid(&plan,&frontier,&c,hist.len()));
    }
    #[test]
    fn adopted_role_change_activates_composable_rule_without_fixed_phase_names(){
        let h="a".repeat(40);let a="b".repeat(40);
        let b="c".repeat(40);let c="d".repeat(40);
        let m="src/engine/rsi_improvement_algebra.rs";
        let frontier=vec![
            row("scientific_method",m),
            row("memory","src/engine/memory.rs"),
            row("learning","src/engine/learning.rs"),
            row("tools","src/engine/tools.rs")];
        let rule1=birth(&h,&a,"author-1","memory_to_learning",
            m,"scientific_method","memory","learning");
        let rule2=birth(&a,&b,"author-2","learning_to_tools",
            m,"scientific_method","learning","tools");
        let mut accepted_memory=birth(&b,&c,"act","unrelated",
            "src/engine/memory.rs","memory","memory","memory");
        accepted_memory["systemDesign"]["improvementRules"]=json!([]);
        let plan=compile(&[rule1,rule2,accepted_memory],&frontier,&c);
        assert_eq!(plan["latestIndependentlyChangedRoles"],json!(["memory"]));
        assert_eq!(plan["selected"]["rule"]["name"],"memory_to_learning");
        assert_eq!(plan["selected"]["activatedByLatestIndependentAdoption"],true);
        assert_eq!(plan["selected"]["target"]["role"],"learning");
    }
    #[test]
    fn two_independent_rejections_redirect_source_edit_to_method_itself(){
        let h="a".repeat(40);let a="b".repeat(40);
        let m="src/engine/rsi_improvement_algebra.rs";
        let frontier=vec![row("scientific_method",m),
            row("memory","src/engine/memory.rs"),
            row("learning","src/engine/learning.rs")];
        let birth=birth(&h,&a,"one","memory_to_learning",
            m,"scientific_method","memory","learning");
        let original=compile(&[birth.clone()],&frontier,&a);
        let rule_id=original["selected"]["ruleId"].clone();
        let rejects=(0..2).map(|n|json!({
            "candidateId":format!("rejected-{n}"),"accepted":false,
            "generatorCommit":a,
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary"},
            "improvementAlgebraPlan":{
                "generatorCommit":a,
                "selected":{"ruleId":rule_id,
                    "target":{"path":"src/engine/learning.rs"}}
            }
        })).collect::<Vec<_>>();
        let mut history=vec![birth];
        history.extend(rejects);
        let next=compile(&history,&frontier,&a);
        assert_eq!(next["selected"]["intent"],"rewrite_accepted_method_source");
        assert_eq!(next["selected"]["target"]["path"],m);
        assert!(valid(&next,&frontier,&a,history.len()));
    }
    #[test]
    fn no_source_change_no_independent_adoption_no_rule(){
        let m="src/engine/rsi_improvement_algebra.rs";
        let frontier=vec![row("scientific_method",m),row("memory","src/engine/memory.rs"),
            row("learning","src/engine/learning.rs")];
        let mut forged=birth(&"a".repeat(40),&"b".repeat(40),
            "one","weak",m,"scientific_method","memory","learning");
        forged["changedSourcePaths"]=json!([]);
        assert!(compile(&[forged],&frontier,&"b".repeat(40))["operators"]
            .as_array().unwrap().is_empty());
    }
}
