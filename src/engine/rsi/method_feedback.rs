//! Evidence-linked improvement method inheritance and downstream feedback.
//! This learner is editable by the compiled descendant; the frozen proof independently replays its outputs.
use super::*;

fn method_source(path:&str)->bool {
    !path.ends_with("/tests.rs")
        && (path=="src/engine/rsi/descendant_plan.rs"
            || path.starts_with("src/engine/rsi/meta_controller/") && path.ends_with(".rs")
            || path.starts_with("src/engine/rsi/system/") && path.ends_with(".rs"))
}

fn measured(g:&Value)->bool {
    generation_is_verified(g)
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["acceptedCommit"].as_str().is_some_and(|s|s.len()==40)
}

/// Source identity of the newly compiled improver, derived only from an
/// independently accepted edit in the immediate parent generation.
pub(super) fn method_snapshot(state:&AppState,history:&[Value],binary_commit:&str)->Value {
    let Some(a)=history.last() else{return Value::Null};
    let verified=revalidated_history(state,&[a.clone()]);
    if !verified.first().is_some_and(measured)
        || a["acceptedCommit"]!=binary_commit
        || a["candidateTarget"]!="improver"
        || !crate::engine::rsi_meta_controller::meta_source_changed(a)
        || a["evaluation"]["metrics"]["metaHoldoutGains"].as_u64().unwrap_or(0)==0
    {return Value::Null;}
    let Some(paths)=a["changedSourcePaths"].as_array() else{return Value::Null};
    // A single meta source edit removes ambiguity about which changed method
    // the next executable could have inherited.
    let [only]=paths.as_slice() else{return Value::Null};
    let Some(path)=only.as_str().filter(|p|method_source(p)
        && crate::engine::rsi::safe_path(p)) else{return Value::Null};
    let hash=&a["acceptedSourceHashes"][path];
    if !hash.as_str().is_some_and(|s|s.len()==64
        && s.bytes().all(|b|b.is_ascii_hexdigit())) {return Value::Null;}
    json!({
        "source":"compiled_inherited_improvement_method_v1",
        "methodPath":path,"methodSourceSha256":hash,
        "methodGenerationId":a["id"],"methodCandidateId":a["candidateId"],
        "methodAcceptedCommit":a["acceptedCommit"],
        "methodEvaluationSha256":a["evaluation"]["evidenceSha256"],
        "binaryCommit":binary_commit,
        "independentMethodGain":true
    })
}

fn task_gain(b:&Value)->bool {
    b["domain"]!="improver" && b["candidateTarget"]!="improver"
        && b["evaluation"]["metrics"]["holdoutEvidence"].as_array()
            .is_some_and(|cases|cases.iter().any(|case|
                case["domain"]==b["domain"] && case["improved"]==true
                    && case["candidatePass"]==true
                    && case["metricRegressions"].as_u64()==Some(0)
                    && (case["baselinePass"]==false
                        || case["metricGains"].as_u64().unwrap_or(0)>0)))
}

/// Original B job execution + source A + independent B holdout: this is
/// material for a NEW method rewrite, not proof that the rewrite succeeded.
pub(super) fn downstream_feedback(state:&AppState,history:&[Value],binary_commit:&str)->Value {
    let [a,b]=history else{return Value::Null};
    let checked=revalidated_history(state,history);
    if !checked.iter().all(measured)
        || b["generatorCommit"]!=a["acceptedCommit"]
        || b["acceptedCommit"]!=binary_commit
        || !task_gain(b)
    {return Value::Null;}
    let inherited=method_snapshot(state,&history[..1],
        b["generatorCommit"].as_str().unwrap_or(""));
    if !inherited.is_object(){return Value::Null;}
    let Some(job)=b["jobId"].as_str()
        .and_then(|id|state.store.get("job",id)) else{return Value::Null};
    if job["kind"]!="rsi_generation"
        || job["id"]!=b["jobId"]
        || job["input"]["rsiGeneratorCommit"]!=b["generatorCommit"]
        || job["input"]["rsiMethodSnapshot"]!=inherited
        || job["input"]["rsiNativePlan"]["methodSnapshot"]!=inherited
        || job["rsiMetaPolicyExecuted"]!=b["metaPolicyExecuted"]
        || job["rsiMetaPolicyExecuted"]["source"]!="running_compiled_descendant_binary"
        || job["rsiMetaPolicyExecuted"]["generatorCommit"]!=b["generatorCommit"]
        || job["rsiSystemBlueprintVerified"]!=true
        || job["rsiSelfControllerVerified"]!=true
        || job["rsiDomain"]!=b["domain"]
        || job["result"]["rsi"]["candidateId"]!=b["candidateId"]
    {return Value::Null;}
    let Some(paths)=b["changedSourcePaths"].as_array() else{return Value::Null};
    if paths.is_empty() || paths.iter().any(|p|p==&inherited["methodPath"])
    {return Value::Null;}
    json!({
        "source":"independent_downstream_feedback_for_method_rewrite_v1",
        "methodGenerationId":a["id"],"taskGenerationId":b["id"],
        "methodPath":inherited["methodPath"],
        "methodSourceSha256":inherited["methodSourceSha256"],
        "methodAcceptedCommit":a["acceptedCommit"],
        "methodEvaluationSha256":inherited["methodEvaluationSha256"],
        "taskCandidateId":b["candidateId"],
        "taskEvaluationSha256":b["evaluation"]["evidenceSha256"],
        "taskDomain":b["domain"],"taskAcceptedCommit":b["acceptedCommit"],
        "compiledTaskBinarySha256":b["generatorBinarySha256"],
        "nextBinaryCommit":binary_commit,
        "causalAblationProven":false
    })
}
