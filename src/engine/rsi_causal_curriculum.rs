//! Self-editable, source-attested multi-factor improvement curriculum.
//!
//! A, B and A+B revisions share the SAME compiled parent and frozen evaluation
//! profile before we compare their measured effects. Unmatched observations,
//! guesses about edits and case-specific hidden answers never become evidence.
//! This is experiment design and observational contrasts, not causal proof.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn revision(value:&Value)->String {
    format!("{:x}",Sha256::digest(serde_json::to_vec(value).unwrap_or_default()))
}

fn sha40(v:&Value)->bool{
    v.as_str().is_some_and(|s|s.len()==40
        && s.bytes().all(|b|b.is_ascii_hexdigit()))
}

fn valid_measurement(g:&Value)->bool{
    sha40(&g["generatorCommit"])
        && g["pairedRevalidated"]==true
        && g["evaluation"]["metrics"]["measurements"].is_array()
        && g["evaluationProfileSha256"].as_str()
            .is_some_and(|s|s.len()==64
                && s.bytes().all(|c|c.is_ascii_hexdigit()))
        && g["factorialChangedPaths"].as_array().is_some_and(|files|
            !files.is_empty() && files.len()<=24)
        && g["systemDesign"]["source"]=="model_authored_system_design"
        && g["systemDesign"]["generatorCommit"]==g["generatorCommit"]
        && g["systemDesign"]["generationJobId"]==g["jobId"]
}

/// Source ownership is checked against ACTUALLY measured changed paths, not
/// the candidate's declared component names or source-description prose.
fn observed_roles(g:&Value)->Vec<String>{
    if !valid_measurement(g){return Vec::new();}
    let design=&g["systemDesign"];
    let candidates=if design["component"]=="system"{
        design["components"].as_array().cloned().unwrap_or_default()
    }else{vec![design["component"].clone()]};
    let changed=g["factorialChangedPaths"].as_array()
        .cloned().unwrap_or_default();
    let mut attributed=BTreeSet::<String>::new();
    let mut attributed_paths=BTreeSet::<String>::new();
    for name in &candidates{
        let Some(role)=name.as_str().filter(|s|!s.is_empty()&&*s!="system")else{
            return Vec::new()
        };
        let files=design["componentFiles"][role].as_array()
            .or_else(||(design["component"]==role)
                .then(||design["targetFiles"].as_array()).flatten());
        let Some(files)=files.filter(|fs|!fs.is_empty())else{return Vec::new()};
        if !files.iter().all(|path|path.as_str().is_some_and(|p|
            crate::engine::rsi::safe_path(p)
                && changed.iter().any(|item|item.as_str()==Some(p))
                && attributed_paths.insert(p.to_string()))){return Vec::new();}
        attributed.insert(role.to_string());
    }
    if changed.iter().all(|file|file.as_str().is_some_and(|path|
        attributed_paths.contains(path))) {
        attributed.into_iter().collect()
    }else{Vec::new()}
}

fn variant(roles:&[String],a:&str,b:&str)->Option<usize>{
    if roles.len()==1 && roles[0]==a{Some(0)}
    else if roles.len()==1 && roles[0]==b{Some(1)}
    else if roles.len()==2 && roles.iter().any(|r|r==a)
        && roles.iter().any(|r|r==b){Some(2)}
    else{None}
}

/// Same source-revision, evaluation profile, case, and baseline result.
/// Baseline and holdout case IDs are used ONLY within this native controller.
/// The returned statistics contain no individual frozen task IDs or answers.
fn observations(history:&[Value],a:&str,b:&str)->Value{
    type Block=(String,String,String,bool);
    let mut blocks=BTreeMap::<Block,[Vec<f64>;3]>::new();
    let mut variants=[BTreeSet::<String>::new(),
        BTreeSet::<String>::new(),BTreeSet::<String>::new()];
    for g in history.iter().filter(|g|valid_measurement(g)){
        let Some(n)=variant(&observed_roles(g),a,b)else{continue};
        let Some(id)=g["id"].as_str()else{continue};
        let commit=g["generatorCommit"].as_str().unwrap_or("");
        let profile=g["evaluationProfileSha256"].as_str().unwrap_or("");
        let mut recorded=false;
        for row in g["evaluation"]["metrics"]["measurements"]
            .as_array().into_iter().flatten(){
            if row["holdout"]!=true{continue;}
            let (Some(case),Some(before),Some(after))=(
                row["id"].as_str(),row["baselinePass"].as_bool(),
                row["candidatePass"].as_bool())else{continue};
            if case.is_empty(){continue;}
            blocks.entry((commit.to_string(),profile.to_string(),
                case.to_string(),before))
                .or_insert_with(||[Vec::new(),Vec::new(),Vec::new()])[n]
                .push((after as i32-before as i32) as f64);
            recorded=true;
        }
        if recorded {variants[n].insert(id.to_string());}
    }
    let mut contrast_sum=0.0;
    let mut matched=0usize;
    let mut positive=0usize;
    let mut negative=0usize;
    for (_,samples) in &blocks {
        if samples.iter().any(Vec::is_empty){continue;}
        let mean=|items:&Vec<f64>|items.iter().sum::<f64>()/items.len() as f64;
        let contrast=mean(&samples[2])-mean(&samples[0])-mean(&samples[1]);
        matched+=1;
        contrast_sum+=contrast;
        positive+=usize::from(contrast>0.0);
        negative+=usize::from(contrast<0.0);
    }
    let mut result=json!({
        "variantGenerations":{"primaryOnly":variants[0].len(),
            "partnerOnly":variants[1].len(),"joint":variants[2].len()},
        "matchedFrozenCases":matched,
        "meanInteractionContrast":if matched>0{
            json!(contrast_sum/matched as f64)}else{Value::Null},
        "positiveContrasts":positive,"negativeContrasts":negative,
        "unmatchedVariantsNeverCountAsInteractions":true,
        "individualFrozenCasesExposed":false,
        "causalityProven":false
    });
    result["sha256"]=json!(revision(&result));
    result
}

/// Learn which role's subsequent redesign became more productive in
/// source-matched factorial observations. These are aggregate signs of
/// experimental interaction, never new independent adoption labels.
pub(crate) fn role_interaction_feedback(history:&[Value],role:&str)->Value{
    let mut pairs=BTreeSet::<(String,String)>::new();
    for g in history.iter().filter(|g|g["factorialExperimentExecuted"]==true){
        let item=&g["factorialCurriculum"]["selected"];
        if let (Some(a),Some(b))=(item["fromRole"].as_str(),
            item["toRole"].as_str()){
            if a!=b && (role==a || role==b){
                pairs.insert((a.to_string(),b.to_string()));
            }
        }
    }
    let mut weighted=0.0;
    let mut total=0usize;
    for (a,b) in pairs.into_iter().take(16){
        let effect=observations(history,&a,&b);
        if let (Some(n),Some(delta))=(
            effect["matchedFrozenCases"].as_u64(),
            effect["meanInteractionContrast"].as_f64()){
            if n>0 && delta.is_finite(){
                let strength=(n.min(16) as f64).sqrt();
                weighted+=delta.clamp(-1.0,1.0)*strength;
                total+=strength as usize+1;
            }
        }
    }
    let score=if total>0 {
        (0.15*weighted/(total as f64)).clamp(-0.15,0.15)
    }else{0.0};
    json!({"role":role,"selectionBonus":score,
        "matchedEvidenceObserved":total>0,
        "causalityProven":false,"improvementCredit":false})
}

/// Executable experimental choice is inside the successor's own source.
/// No fixed parent supplies the next variant, and no variants are assigned
/// a quality label merely for generating a candidate.
pub(crate) fn design(history:&[Value],curriculum:&Value,
    frontier:&[Value],commit:&str)->Value{
    let pair=&curriculum["pair"];
    let mut selected=Value::Null;
    if pair.is_object(){
        let a=frontier.iter().find(|r|r["role"]==pair["fromRole"]
            && r["path"]==pair["fromPath"]
            && r["sourceSha256"]==pair["fromSourceSha256"]);
        let b=frontier.iter().find(|r|r["role"]==pair["toRole"]
            && r["path"]==pair["toPath"]
            && r["sourceSha256"]==pair["toSourceSha256"]);
        if let (Some(a),Some(b))=(a,b){
            let counts=observations(history,
                a["role"].as_str().unwrap_or(""),
                b["role"].as_str().unwrap_or(""));
            let observed=&counts["variantGenerations"];
            let count=[
                observed["primaryOnly"].as_u64().unwrap_or(0),
                observed["partnerOnly"].as_u64().unwrap_or(0),
                observed["joint"].as_u64().unwrap_or(0)
            ];
            // Failed editing generations count as experimental ATTEMPTS but
            // never become measurements. Shift to a different variant after
            // two failures rather than looping forever on the first role.
            let mut attempts=[0u64;3];
            for g in history.iter().rev().take(48){
                let earlier=&g["factorialCurriculum"]["selected"];
                if g["factorialExperimentExecuted"]!=true
                    || earlier["fromRole"]!=a["role"]
                    || earlier["toRole"]!=b["role"]{continue;}
                match earlier["phase"].as_str(){
                    Some("isolate_primary")=>attempts[0]+=1,
                    Some("isolate_partner")=>attempts[1]+=1,
                    Some("test_joint")=>attempts[2]+=1,
                    _=>{}
                }
            }
            let next=(0..3).find(|index|
                count[*index]==0 && attempts[*index]<2);
            
            let phase=match next{
                Some(0)=>"isolate_primary",Some(1)=>"isolate_partner",
                Some(2)=>"test_joint",_=>"analyze_interaction"
            };
            let target=if next==Some(1){b}else{a};
            selected=json!({
                "fromRole":a["role"],"toRole":b["role"],
                "primary":a,"partner":b,"target":target,
                "phase":phase,"variantIndex":next,
                "observations":counts,
                "variantAttempts":{"primaryOnly":attempts[0],
                    "partnerOnly":attempts[1],"joint":attempts[2]},
                "isolationRequired":next==Some(0)||next==Some(1),
                "qualityProven":false,"causalityProven":false
            });
        }
    }
    let mut plan=json!({
        "source":"compiled_source_attested_factorial_curriculum_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "reciprocalRevision":curriculum["revision"],
        "selected":selected,
        "editableInCompiledDescendant":true,
        "frozenEvaluatorIsExternal":true,
        "causalityProven":false,"qualityProven":false
    });
    plan["sha256"]=json!(revision(&plan));
    plan
}

/// Structural and source identity contract: never compare old parent's
/// algorithm rankings against the modified compiled successor.
pub(crate) fn valid(plan:&Value,frontier:&[Value],commit:&str,
    history_count:usize,mutual:&Value)->bool{
    if plan.is_null(){return true;}
    let mut unhashed=plan.clone();
    if let Some(object)=unhashed.as_object_mut(){object.remove("sha256");}
    if plan["source"]!="compiled_source_attested_factorial_curriculum_v1"
        || plan["sha256"]!=revision(&unhashed)
        || plan["generatorCommit"]!=commit
        || plan["historyCount"].as_u64()!=Some(history_count as u64)
        || plan["reciprocalRevision"]!=mutual["revision"]
        || plan["causalityProven"]!=false
        || plan["qualityProven"]!=false{return false;}
    let item=&plan["selected"];
    if item.is_null(){return true;}
    let a=frontier.iter().find(|r|r==&&item["primary"]);
    let b=frontier.iter().find(|r|r==&&item["partner"]);
    if a.is_none()||b.is_none()
        || item["fromRole"]==item["toRole"]
        || item["primary"]["path"]==item["partner"]["path"]
        || item["fromRole"]!=item["primary"]["role"]
        || item["toRole"]!=item["partner"]["role"]
        || item["qualityProven"]!=false
        || item["causalityProven"]!=false{return false;}
    let expected=if item["phase"]=="isolate_partner"{
        &item["partner"]}else{&item["primary"]};
    let mut observations=item["observations"].clone();
    if let Some(object)=observations.as_object_mut(){object.remove("sha256");}
    item["observations"]["sha256"]==revision(&observations)
        && item["target"]==*expected
        && matches!(item["phase"].as_str(),
            Some("isolate_primary"|"isolate_partner"|"test_joint"|"analyze_interaction"))
}

pub(crate) fn instruction(plan:&Value)->String{
    let item=&plan["selected"];
    if !item.is_object(){return String::new();}
    let instruction=match item["phase"].as_str(){
        Some("isolate_primary"|"isolate_partner")=>
            "This generation is an ISOLATED source intervention. Edit the target role only; read the complementary role without modifying it. Do not claim a joint interaction.",
        Some("test_joint")=>
            "This generation is a JOINT source experiment: redesign both distinct subsystem source files with rsi_system_plan(component=system, components, componentFiles, interactionEdges) and actual witnessed source writes.",
        _=>"Both isolated and joint variants have been attempted. Use the matched aggregate contrast to invent a new cross-domain mechanism."
    };
    format!(
        "[DESCENDANT FACTORIAL IMPROVEMENT EXPERIMENT] phase={} targetRole={} targetPath={} targetSourceSha256={} otherRole={} otherPath={} observedVariants={} matchedCases={} meanInteractionContrast={} (not causal proof). {} The hidden evaluation remains external; never expose individual task answers.",
        item["phase"],item["target"]["role"],item["target"]["path"],
        item["target"]["sourceSha256"],
        item["partner"]["role"],item["partner"]["path"],
        item["observations"]["variantGenerations"],
        item["observations"]["matchedFrozenCases"],
        item["observations"]["meanInteractionContrast"],
        instruction
    )
}
