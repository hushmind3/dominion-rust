//! Compiled descendant prompt instructions and evidence-derived repair direction.
use serde_json::{json,Value};

/// A system-generation *model* chooses the actual design and edits. The
/// descendant's source-grounded map is context, not a fixed list of allowed
/// mutations. The code index and source-edit tools still cover the repository.
pub(crate) fn instruction(self_model:&Value,owner_objective:&str)->String {
    let components=self_model["architecture"].as_array()
        .into_iter().flatten()
        .filter_map(|row|{
            let name=row["role"].as_str()?;
            let paths=row["representativeSources"].as_array()?
                .iter().filter_map(|item|item["path"].as_str()).take(3)
                .collect::<Vec<_>>().join(", ");
            Some(format!("{name}: {paths}"))
        }).collect::<Vec<_>>().join("\n");
    let inherited=self_model["inheritedDesign"].as_object().map(|_|format!("Previously accepted design: nextDomain={} hypothesis={}",self_model["inheritedDesign"]["nextDomain"],self_model["inheritedDesign"]["hypothesis"])).unwrap_or_default();
    let history=self_model["recentMeasuredGenerations"].as_array()
        .into_iter().flatten().take(4).map(|row|format!(
            "domain={} adopted={} verdict={} holdoutGain={} metaGain={} methodControl={} methodQualityWin={} methodCaseWin={}",
            row["domain"],row["accepted"],row["verdict"],
            row["improvedHoldouts"],row["metaHoldoutGains"],
            row["methodAblation"]["controlId"],
            row["methodAblation"]["treatmentQualityMetricWins"],
            row["methodAblation"]["treatmentOnlyWins"]
        )).collect::<Vec<_>>().join("\n");
    format!(
        "[COMPILED DESCENDANT: VERSIONED INTELLIGENCE RUNTIME]\n\
        Running source revision: {}. Assigned task: {}.\n\
        Your own code contains the domain selector, search policy, planning \
        harness, context, memory, learning, source tools and improvement \
        methods. These are evolvable source, not fixed hand-written edit rules.\n\
        Use the CURRENT revision's indexed evidence, previous compiler failures \
        and accepted/rejected candidate receipts to decide what to try next. \
        Inspect current source and its SHA via code_search/workspace_read_range. \
        Use workspace_replace_range, workspace_replace or workspace_write for \
        REAL source edits; a proposal or read-only report is not an improvement.\n\
        If the task is whole-system self-redesign: find a causal limitation of \
        your own algorithm/harness/memory/learning/tools/improver, propose a \
        falsifiable structural change through rsi_system_plan, and implement \
        the actual code, including new modules when justified. For coupled redesigns use \
        component=system, list components, and map every component to its \
        targetFiles via componentFiles. Implement ALL declared target files. \
        When inventing a NEW intelligence role, declare newComponents with \
        role, domain, purpose, sourcePaths and DISTINCT integrationPaths. \
        Both the module code and actual runtime wiring must change and be \
        independently adopted before the new role enters future search. \
        A rebuilt descendant may then improve this newly invented module. \
        You may replace the experiment grammar itself with evolutionProtocol: \
        entry plus arbitrary operators (id, role, domain, path, objective, \
        mechanism, experiment) and outcome-conditioned transitions (from, \
        to, when). Operator experiments include paired_source, isolated_source, \
        factorial_source, method_control, cross_component and meta_policy_rewrite. \
        Valid outcomes: adopted, quality_gain, regressed, rejected, inconclusive, \
        meta_gain and method_gain. Source ground every operator and update \
        src/engine/rsi/meta/evolution.rs or src/engine/rsi/meta/composition.rs \
        when the selection/experiment algorithm itself needs to evolve. \
        New protocols only become executable after independent source adoption. \
        Better: author an openProgram containing ANY named improvement \
        nodes, novel role sources, testable mechanisms, entry and outcome \
        edges; the accepted compiled descendant executes this editable \
        graph, and a newer model-authored graph may replace its entire \
        strategy. Include src/engine/rsi_program_graph.rs as an editable \
        target when improvement-program synthesis itself is the limitation. \
        Feedback branches may revisit earlier roles or synthesize new ones; \
        no prescribed A->B->A topology limits the descendant. \
        Submit successorStages in rsi_system_plan to program how the NEXT \
        compiled agent chooses its domain, source target, learning policy, \
        and source tool policy. Each stage needs role, domain, path, objective; \
        learningPolicy controls retries and toolPolicy can prohibit new files. \
        This inherited program is rebound to actual source SHA by the rebuilt \
        descendant and competes with its meta-controller for selection. \
        You can rewrite the source of rsi_meta_controller.rs itself to \
        improve that selection and its prediction calibration. A changed \
        improvement procedure must be executed and evaluated in its successor.\n\
        METHOD FEEDBACK: if a prior descendant has a measured method ON/OFF \
        receipt, inspect the exact procedure revision, control evaluation ID \
        and heldout quality delta. Propose a DIFFERENT executable DAG with the \
        same method name and domain via rsi_method_propose, driven by observed \
        failures or quality gaps. After adoption, its successor must actually \
        execute the revised DAG and independently beat the disabled-method \
        control. Copying a receipt into prose or renaming a rule is not learning.\n\
        Learn from independently paired held-out TASK QUALITY and the actual\n\
        improvement ability of later recompiled descendants, not merely\n\
        acceptance counts. Rewrite rsi_principle_learning.rs when its own\n\
        evidence model, reward assignment or generalization logic is weak.\n\
        Improve actual API-driven intelligence: models.rs, router.rs, gateway.rs, memory, learning\n\
        and tools: use actual source edits and compare task behavior.\n\
        If assigned a concrete downstream coding task: solve THAT task with \
        your current improved intelligence. Do not substitute unrelated agent \
        redesign for the requested patch. The downstream result measures \
        whether the improved generator really solves more problems.\n\
        You may propose reusable executable methods with rsi_method_propose \
        and exploratory algorithm selectors with algorithm_propose. Registration \
        is not success. The independent paired evaluator alone decides \
        experimental adoption; never edit the evaluator or claim unevaluated gain.\n\
        Current architecture (representative files; ALL indexed source is open):\n{}\n\
        Measured predecessor generations (not present-tense claims):\n{}\n\
        Last independently inherited design hypothesis: {}\n\
        Report exact changed source and what still requires evaluation.",
        self_model["generatorCommit"],owner_objective,components,history,inherited)
}



/// Behavioral successor instruction derived from *observed* episodes, not
/// model optimism. The rebuilt descendant uses the outcome to choose whether
/// to discover, implement, repair, change approach or transfer an adopted gain.
pub(crate) fn improvement_directive(history:&[Value])->Value {
    let Some(latest)=history.last() else {
        return json!({"mode":"discover","sourceGenerationId":null,
            "reason":"no_recorded_generation"});
    };
    // A measured capability B must remain the feedback source for a later
    // meta revision A2 even if attempts to write A2 have failed meanwhile.
    // The last failure is separately recorded below and is never promoted.
    let previous=history.iter().rev().find(|g|g["accepted"]==true
        && g["domain"]!="improver"
        && g["methodAblation"]["methodAssociationObserved"]==true
        && g["methodAblation"]["qualityProven"]==true)
        .filter(|g|history.iter().rev().find(|g|g["accepted"]==true)
            .is_some_and(|last|last["id"]==g["id"]))
        .unwrap_or(latest);
    let observed=previous["evaluation"]["metrics"]["eligible"]==true;
    let has_patch=previous["candidateId"].is_string();
    let new_findings=previous["newFindings"].as_u64().unwrap_or(0);
    let prior=previous["improvementDirective"]["mode"].as_str().unwrap_or("");
    let failed_build=previous["evaluation"]["verdict"]=="build_failed"
        || previous["error"].as_str().is_some_and(|s|
            s.contains("build") || s.contains("compiler") || s.contains("compile"));
    let measured_method=previous["methodAblation"]["methodAssociationObserved"]==true
        && previous["methodAblation"]["qualityProven"]==true;
    let mode=if previous["accepted"]==true && observed && measured_method {
        "evolve_executed_method_from_feedback"
    }else if previous["accepted"]==true && observed {
        "extend_verified_capability"
    }else if failed_build {
        if prior=="repair_counterexample"{"change_approach"}else{"repair_counterexample"}
    }else if has_patch {
        if prior=="revise_candidate"{"change_approach"}else{"revise_candidate"}
    }else if new_findings>0 {
        if prior=="implement_research"{"change_approach"}else{"implement_research"}
    }else{"change_approach"};
    json!({
        "sourceGenerationId":previous["id"],
        "mode":mode,"domain":previous["domain"],
        "previousAccepted":previous["accepted"]==true && observed,
        "previousVerdict":previous["evaluation"]["verdict"],
        "previousCandidateId":previous["candidateId"],
        "methodFeedback":if measured_method {json!({
            "controlId":previous["methodAblation"]["controlId"],
            "controlEvaluationId":previous["methodAblation"]["controlEvaluationId"],
            "treatmentEvaluationId":previous["methodAblation"]["treatmentEvaluationId"],
            "pairedHoldoutCases":previous["methodAblation"]["pairedHoldoutCases"],
            "treatmentQualityMetricWins":previous["methodAblation"]["treatmentQualityMetricWins"],
            "treatmentOnlyWins":previous["methodAblation"]["treatmentOnlyWins"],
            "sourceGenerationId":previous["id"],
            "requiresNewRevisionAndSuccessorExecution":true
        })}else{Value::Null},
        "latestAttemptGenerationId":latest["id"],
        "latestAttemptAccepted":latest["accepted"],
        "latestAttemptError":latest["error"],
        "previousFocusFiles":previous["focusFiles"].as_array()
            .map(|a|a.iter().take(4).cloned().collect::<Vec<_>>()).unwrap_or_default(),
        "previousError":previous["error"].as_str().unwrap_or("")
            .chars().take(500).collect::<String>(),
        "researchArtifactHashes":previous["knowledgeArtifactHashes"].as_array()
            .map(|a|a.iter().take(6).cloned().collect::<Vec<_>>()).unwrap_or_default(),
        "newFindings":new_findings
    })
}
