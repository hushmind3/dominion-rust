//! Canonical stateful A(method) -> B(task) -> A'(method) -> B'(task)
//! verdict. This is not derived from generation summary flags.
use super::*;

pub(super) fn report(state:&AppState,history:&[Value])->Value{
    let checked=revalidated_history(state,history);
    for quartet in checked.windows(4){
        let [a,b,c,d]=quartet else{continue};
        if closing_triplet(&quartet[..3]).is_none()
            || !generation_is_verified(d)
            || !compiled_plan_executed(d)
            || !source_change_recorded(d)
            || d["candidateOrigin"]!="model_driven_detached_worktree"
            || d["candidateTarget"]=="improver"
            || d["domain"]!=b["domain"]
            || d["generatorCommit"]!=c["acceptedCommit"]
            || d["generatorCommit"]==d["acceptedCommit"]
            || b["methodContrastVerified"]!=true
            || d["methodContrastVerified"]!=true
            || b["methodContrast"]["controlId"]==d["methodContrast"]["controlId"]
            || b["methodContrast"]["taskHoldoutPairs"]!=d["methodContrast"]["taskHoldoutPairs"]
            || !improved_domain(d,d["domain"].as_str().unwrap_or(""))
            || !d["methodSnapshot"].is_object()
            || d["methodSnapshot"]["methodGenerationId"]!=c["id"]
            || d["methodSnapshot"]["methodAcceptedCommit"]!=c["acceptedCommit"]
            || d["nativePlan"]["methodSnapshot"]!=d["methodSnapshot"]
            || d["metaPolicyExecuted"]["methodSnapshot"]!=d["methodSnapshot"] {
            continue;
        }
        let Some(path)=c["downstreamFeedback"]["methodPath"].as_str() else{continue};
        if d["methodSnapshot"]["methodPath"]!=path
            || d["methodSnapshot"]["methodSourceSha256"]!=c["acceptedSourceHashes"][path]
            || d["methodContrast"]["methodSourceSha256"]!=c["acceptedSourceHashes"][path]
        {continue;}
        let Some(job)=d["jobId"].as_str().and_then(|id|
            state.store.get("job",id)) else{continue};
        if job["kind"]!="rsi_generation"
            || job["id"]!=d["jobId"]
            || job["input"]["rsiGeneratorCommit"]!=d["generatorCommit"]
            || job["input"]["rsiMethodSnapshot"]!=d["methodSnapshot"]
            || job["rsiMetaPolicyExecuted"]!=d["metaPolicyExecuted"]
            || job["result"]["rsi"]["candidateId"]!=d["candidateId"] {
            continue;
        }
        // Measured ON-vs-OFF advantage must recur after the independent
        // source-method rewrite. No claim that stochastic sampling was eliminated.
        return json!({
            "source":"replayed_four_generation_method_task_method_task_v1",
            "closedLoopObserved":true,
            "checkedGenerations":checked.len(),
            "sequence":["improver_A","capability_B","improver_A2","capability_B2"],
            "generatorCommits":[a["generatorCommit"],b["generatorCommit"],
                c["generatorCommit"],d["generatorCommit"]],
            "acceptedCommits":[a["acceptedCommit"],b["acceptedCommit"],
                c["acceptedCommit"],d["acceptedCommit"]],
            "candidateIds":[a["candidateId"],b["candidateId"],c["candidateId"],d["candidateId"]],
            "methodSource":path,
            "methodSourceBeforeSha256":a["acceptedSourceHashes"][path],
            "methodSourceAfterSha256":c["acceptedSourceHashes"][path],
            "independentPairedReceipts":[a["evaluation"]["evidenceSha256"],
                b["evaluation"]["evidenceSha256"],c["evaluation"]["evidenceSha256"],
                d["evaluation"]["evidenceSha256"]],
            "taskMethodContrasts":[b["methodContrast"],d["methodContrast"]],
            "methodFeedbackConsumed":true,
            "compiledNextDescendantExecuted":true,
            "causalAblationProven":false,
            "unboundedSelfImprovementProven":false
        });
    }
    json!({
        "source":"replayed_four_generation_method_task_method_task_v1",
        "closedLoopObserved":false,"checkedGenerations":checked.len(),
        "requires":"four consecutive independently adopted generations with exact source-method handoff, B and B2 real method-off paired control advantages and new A2 source rewrite driven by B feedback",
        "causalAblationProven":false,
        "unboundedSelfImprovementProven":false
    })
}
