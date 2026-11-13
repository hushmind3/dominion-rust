//! Instructions exposing editable capabilities to the model.
use serde_json::Value;

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
            "domain={} adopted={} verdict={} holdoutGain={} metaGain={}",
            row["domain"],row["accepted"],row["verdict"],
            row["improvedHoldouts"],row["metaHoldoutGains"]
        )).collect::<Vec<_>>().join("\n");
    format!(
        "[COMPILED DESCENDANT: SYSTEM SELF-REDESIGN]\n\
        You are running from revision {}. The user goal is: {}.\n\
        You control design, source analysis, algorithm choice, method learning and \
        actual code edits for your next descendant. Do not treat a prewritten \
        source mutation or a single benchmark speedup as system evolution.\n\
        Inspect real source and identify a causal limitation in the current \
        algorithms, harness, memory, learning, tools, model architectures, \
        Python learning modules or the meta-improver itself. Propose a new \
        executable cross-domain design and inspect exact source revisions. \
        You may change Rust, TypeScript and Python source; for Rust functions \
        use workspace_replace_function, for any language use workspace_replace \
        (line mode or exact oldText), and create new modules with workspace_write. \
        Before editing call rsi_system_plan(component, nextDomain, hypothesis, design, targetFiles). For a NEW intelligence component, use component=system, assign the invented role and an existing consumer role separate source files, then author moduleIntroductions [{{role,newFiles,integrationFiles,interfaceContract,integrationSymbol,improvementMechanism}}], create the module AND update its actual caller or execution interface; the integrationSymbol must occur in BOTH edited sources and a successorStages entry must schedule the new role for further improvement. A new module alone is not an integrated capability. For a coupled redesign of algorithm, harness, memory, models, learning, tools and improvement methods, use component=system and explicitly assign each file to one role with components and componentFiles. For actual cross-subsystem redesign, declare interactionEdges [{{fromRole,toRole,fromPath,toPath,mechanism,expectedEffect}}] that bind distinct source edits, and successorStages for the NEXT compiled descendant. Explain how A improves B's ability to improve A again. Then WRITE ALL the named source files: partially implemented designs receive no credit. New roles and source modules are allowed. Optionally author successorStages [{{role,domain,path,objective,learningPolicy,toolPolicy,executionPolicy}}]: the accepted descendant will apply observed-failure task ranking, source-edit focus and restricted tool access from these fields; only after your source is independently adopted will the NEW compiled descendant read its own code and attempt those next tasks. A design and a saved stage alone earn zero. At least one adopted generation must INVENT an executable method with rsi_method_propose and a later compiled descendant must EXECUTE it with verified postconditions. \
        You can AUTHOR an openProgram in rsi_system_plan with entry, nodes [{{id,role,domain,path,objective,mechanism}}] and edges [{{from,to,when}}]. Nodes may be ANY currently recognized or newly invented intelligence role, including the experiment-graph interpreter. Edges form arbitrary cycles and outcome-conditioned branches; the rebuilt descendant reads independent results to choose which actual source to edit. An adopted successor may REPLACE this entire program rather than following fixed four-stage or two-stage schedules. Each plan must still bind the current source SHA and pass independent evaluation; graph prose is not quality evidence. \
        You may propose improvementPrinciple: a MODEL-AUTHORED mechanistic theory that joins one real task intelligence source (models, memory, learning, tools, harness, algorithms) to an EDITABLE method-learning source. For its paired experiment use component=system, disjoint componentFiles, real edits to BOTH files, and fields name,taskRole,taskFile,methodFile,failureMode,mechanism,counterfactual,transferPrediction,learningSignal. Its task-level quality and the NEXT descendant's actual improvement ability will be learned separately; source adoption is not evidence of transfer. \
        Predict source-candidate creation, independent adoption, meta improvement and regressions with expectedOutcomes in rsi_system_plan BEFORE editing. The independent evaluator later calibrates this forecast and your descendant learns from its errors. Improve the intelligence control loop, not only a benchmark number. \
        Make your next-generation planning and implementation behavior better, \
        not only an external product function. The independently frozen paired \
        evaluator alone decides adoption; do not edit the evaluator. \
        The accepted source will be compiled as your successor, which must \
        design and execute the subsequent change.\n\
        Source architecture (representatives; the full repository is searchable):\n{}\n\
        Measured predecessor outcomes (not claims about the new generation):\n{}\n\
        Prior accepted design intent: {}\nReport source changes and receipts, not a claimed AGI or RSI completion.",
        self_model["generatorCommit"],owner_objective,components,history,inherited)
}



