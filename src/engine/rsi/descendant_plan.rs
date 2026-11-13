//! Compiled descendant planning with durable method history.
//! Compiled RSI child module; external APIs are re-exported by rsi_evolve.
use super::*;

/// A new campaign inherits the last *outcome*, not only the last discovery.
/// Otherwise a research→failed implementation sequence restarts as
/// research→the identical failed implementation forever. A newer accepted
/// candidate (or a newer changed source revision) ends that failed lineage.
pub(super) fn improvement_history(state:&AppState,input:&Value,history:&[Value],campaign_id:&str)->Vec<Value>{
    if !history.is_empty() {return history.to_vec();}
    let partition=research_scope(input).partition_key;
    let objective=crate::engine::memory::stable_id(&[
        input["rsiObjective"].as_str().unwrap_or("")]);
    let mut previous=state.store.all("rsi_generation").into_iter()
        .filter(|row|row["campaignId"]!=campaign_id
            && row["knowledgePartition"]==partition
            && row["objectiveId"]==objective)
        .collect::<Vec<_>>();
    // Preserve the actual latest outcome across campaign boundaries.
    // completedAt is high-resolution and survives Store::get, which replaces
    // createdAt with the SQLite-created seconds-resolution timestamp.
    previous.sort_by(|a,b| {
        generation_completion_millis(b).cmp(&generation_completion_millis(a))
            .then_with(|| {
                if a["campaignId"]==b["campaignId"] {
                    b["attempt"].as_u64().cmp(&a["attempt"].as_u64())
                } else {std::cmp::Ordering::Equal}
            })
            .then_with(||b["id"].as_str().cmp(&a["id"].as_str()))
    });
    let Some(latest)=previous.first() else {return Vec::new();};
    // Never resurrect an old research directive after a newer implementation
    // was adopted, rejected, or evaluated as a candidate.
    if latest["accepted"]==true || latest["candidateId"].is_string() {
        return Vec::new();
    }
    let active_research=latest["newFindings"].as_u64().unwrap_or(0)>0
        && latest["knowledgeId"].as_str().is_some_and(|id|
            state.store.get("experience",id).is_some_and(|record|
                record["status"]=="active" && record["partitionKey"]==partition));
    if active_research {return vec![latest.clone()];}
    // An empty/no-patch generation matters only if it actually attempted to
    // follow a prior improvement instruction on the SAME source revision.
    // Prior evidence remains in the experience store; it is not mistaken for
    // a fresh discovery when the model fails to turn it into source code.
    let attempted=matches!(latest["improvementDirective"]["mode"].as_str(),
        Some("implement_research"|"repair_counterexample"|"revise_candidate"|"change_approach"))
        && latest["newFindings"].as_u64().unwrap_or(0)==0;
    let source=input["rsiBaseCommit"].as_str()
        .or_else(||input["rsiGeneratorCommit"].as_str())
        .filter(|sha|!sha.is_empty());
    let same_source=source.is_some_and(|sha|
        latest["generatorCommit"].as_str()==Some(sha));
    if attempted && same_source {vec![latest.clone()]} else {Vec::new()}
}

/// The directive is a real control input. It changes the worker's required
/// sequence of work and (through choose_focus_files) its source candidates.
pub(super) fn improvement_work_contract(directive:&Value) -> &'static str {
    match directive["mode"].as_str().unwrap_or("discover") {
        "implement_research" => "이번 세대는 연구 반복이 아니라 구현 단계입니다. 사용자가 지정한 파일이나 이전 세대의 실제 소스 근거에서 구체적인 수정 지점을 찾아 workspace_replace, workspace_replace_range 또는 workspace_write를 실행하고 검증 사례를 시험하세요. 제공된 발췌로 수정이 가능하면 같은 소스 검색·자료 요약만 반복하지 마세요. 편집 도구가 실패하면 오류를 읽고 다른 실제 수정안을 시도하세요.",
        "repair_counterexample" => "이전 후보는 검증에서 실패했습니다. 실패한 holdout 사례와 코드 차이를 먼저 확인하고 그 실패 원인을 해결하는 별도의 수정을 시험하세요. 과거 패치를 그대로 반복하지 마세요.",
        "revise_candidate" => "이전 실제 수정 후보의 검증 실패를 교정하세요. 이전 후보의 경로·패치·컴파일 오류를 확인하고, 원래 소스 기준으로 workspace_replace 또는 SHA 고정 workspace_replace_range 변경을 실행해 후보를 다시 평가하세요. 검사 실패를 단순 아이디어나 성공으로 취급하지 마세요.",
        "change_approach" => "같은 접근의 반복이 성과를 내지 못했습니다. 이전에 시도한 파일·가설·실패 원인을 근거로 다른 코드 위치나 구현 전략을 선택하여 변경과 검증을 시도하세요. 반복 연구만 하는 것은 실패입니다.",
        "extend_verified_capability" => "이전 세대에서 검증된 구현이 있습니다. 그 코드를 다시 만드는 대신 계승된 실행기의 개선 방법으로 새로운 결함이나 다른 능력을 찾아 독립적으로 검증하세요.",
        _ => "시스템의 실제 실패 원인을 찾고, 수정 가설과 검사 가능한 비교 사례를 만든 뒤 구현하고 비교하세요. 추측이나 연구 목록은 채택 성과가 아닙니다.",
    }
}

/// Separate instruction delivery from actual execution. Small models
/// frequently remember the lesson but repeat research instead of editing.
/// This receipt is computed from operations and candidate evidence, not
/// from the model claiming it followed a plan.
pub(super) fn improvement_action_observation(generation:&Value, diff_bytes:u64) -> Value {
    let mode=generation["improvementDirective"]["mode"].as_str().unwrap_or("discover");
    let completed_writes=generation["writeReceipts"].as_array()
        .into_iter().flatten().filter(|row|row["written"]==true).count() as u64;
    let rejected_writes=generation["writeReceipts"].as_array()
        .into_iter().flatten().filter(|row|row["written"]!=true).count() as u64;
    let produced_candidate=generation["candidateId"].is_string()
        || generation["attemptedCandidateId"].is_string();
    let attempted_change=completed_writes>0 || diff_bytes>0 || produced_candidate;
    let provider_interrupted=!attempted_change
        && generation["error"].as_str().is_some_and(|error|{
            let lower=error.to_ascii_lowercase();
            ["timeout","http 502","connection reset","connection closed",
                "slow model","too slow","timed out"].iter().any(|part|lower.contains(part))
                || error.contains("너무 느렸") || error.contains("응답 시간 초과")
        });
    let previous_files=generation["improvementDirective"]["previousFocusFiles"]
        .as_array().cloned().unwrap_or_default();
    let current_files=generation["focusFiles"].as_array().cloned().unwrap_or_default();
    let source_changed=!previous_files.is_empty() && previous_files!=current_files;
    let observed=match mode {
        "implement_research"|"repair_counterexample"|"revise_candidate"=>attempted_change,
        "change_approach"=>attempted_change || source_changed,
        "extend_verified_capability"=>attempted_change
            || generation["newFindings"].as_u64().unwrap_or(0)>0,
        _=>attempted_change || generation["newFindings"].as_u64().unwrap_or(0)>0,
    };
    json!({
        "mode":mode,
        "followed":observed,
        "status":if generation["accepted"]==true
                    && generation["pairedRevalidated"]==true {"verified_adoption"}
                 else if generation["accepted"]==true {"reported_adoption_unverified"}
                 else if observed {"attempted"}
                 else if provider_interrupted {"provider_interrupted"}
                 else {"not_followed"},
        "providerInterrupted":provider_interrupted,
        "completedWrites":completed_writes,
        "rejectedWrites":rejected_writes,
        "diffBytes":diff_bytes,
        "candidateProduced":produced_candidate,
        "focusChanged":source_changed,
        "accepted":generation["accepted"]
    })
}

/// Aggregate observed gains and costs per action. These are measurements of
/// campaign throughput, not causal attribution or a proxy for improvement.
pub(super) fn improvement_action_results(history:&[Value]) -> Vec<Value> {
    let mut rows=BTreeMap::<String,(u64,u64,u64,u64,u64)>::new();
    for generation in history {
        let mode=generation["improvementDirective"]["mode"].as_str().unwrap_or("legacy");
        let entry=rows.entry(mode.to_string()).or_default();
        entry.0+=1;
        if generation["accepted"]==true && generation["pairedRevalidated"]==true {
            entry.1+=1;
        }
        entry.2+=generation["newFindings"].as_u64().unwrap_or(0);
        entry.3+=generation["tokens"].as_u64().unwrap_or(0);
        if generation["improvementActionObservation"]["followed"]==true {
            entry.4+=1;
        }
    }
    rows.into_iter().map(|(mode,(attempts,adoptions,findings,tokens,followed))|json!({
        "mode":mode,"attempts":attempts,"verifiedAdoptions":adoptions,
        "actionsFollowed":followed,
        "newFindings":findings,"tokens":tokens,
        "adoptionsPer100kTokens":if tokens>0 {100000.0*adoptions as f64/tokens as f64} else {0.0}
    })).collect()
}

// A generation's research continuation must be decided identically by the
// coordinator (which chooses source files) and by the native child worker
// (which actually runs the algorithm). Otherwise a trial policy can appear
// in focusSelection even though no trial was executed.
pub(super) fn research_continuation(history: &[Value]) -> Option<String> {
    history.last()
        .filter(|row| row["candidateId"].is_null()
            && row["newFindings"].as_u64().unwrap_or(0) > 0
            // A follow-up that already tried to implement the findings
            // must change approach, even if it found additional sources.
            && row["improvementDirective"]["mode"]!="implement_research")
        .and_then(|row| row["domain"].as_str())
        .filter(|domain| DOMAINS.contains(domain))
        .map(str::to_string)
}

pub(super) fn generation_target_plan(domain: &str, selection: &str, strategy: &Value, commit: &str) -> Value {
    json!({"domain":domain,"selection":selection,"strategy":strategy,"generatorCommit":commit})
}

pub(super) fn select_generation_strategy(
    state: &AppState,
    history: &[Value],
    objective: &str,
    continuation: Option<String>,
    trial_id: Option<String>,
    selected_domain: Option<String>,
    generator_commit: &str,
) -> (String, &'static str, Value) {
    let active = crate::engine::algorithms::active(state, "selfimprove.search");
    if let Some(domain) = continuation {
        return (domain, "research_continuation", Value::Null);
    }
    if let Some(id) = trial_id {
        if let Some(algorithm) = state.store.get("algorithm", &id).filter(|algorithm| {
            algorithm["kind"] == "selfimprove.search"
                && algorithm["source"] == "rsi-generated"
                && algorithm["rsiTrial"]["status"] != "trial_rejected"
        }) {
            if valid_domain_scores(&algorithm,history,objective).is_none() {
                let baseline=active.unwrap_or_else(||json!({
                    "implementation":"failure-clusters","parameters":{}
                }));
                let mut strategy=search_strategy(&baseline,"active");
                strategy["trialFallback"]=json!("candidate_expression_invalid_for_runtime_features");
                return (select_domain(&baseline,history,objective),"algorithm_active",strategy);
            }
            let domain = select_domain(&algorithm, history, objective);
            let generation = json!({"domain":domain,"generatorCommit":generator_commit});
            if comparable_trial_baseline(history, &generation, &algorithm).is_some() {
                return (domain, "algorithm_trial", search_strategy(&algorithm, "trial"));
            }
            // Use this already scheduled generation normally. Do not spend a
            // trial on a missing/reused control or create another generation.
            let active = active
                .unwrap_or_else(|| json!({"implementation":"failure-clusters","parameters":{}}));
            let mut selected = search_strategy(&active, "active");
            selected["trialFallback"] = json!("unused_comparable_baseline_missing");
            return (select_domain(&active, history, objective), "algorithm_active", selected);
        }
        let algorithm = active
            .unwrap_or_else(|| json!({"implementation":"failure-clusters","parameters":{}}));
        let mut selected = search_strategy(&algorithm, "active");
        selected["trialFallback"] = json!("candidate_missing_or_rejected");
        return (
            select_domain(&algorithm, history, objective),
            "algorithm_active",
            selected,
        );
    }
    let algorithm =
        active.unwrap_or_else(|| json!({"implementation":"failure-clusters","parameters":{}}));
    if let Some(domain) = selected_domain.filter(|domain| DOMAINS.contains(&domain.as_str())) {
        let mut strategy = search_strategy(&algorithm, "active");
        strategy["decisionSource"] = json!("parent_next_decision");
        return (domain, "algorithm_active", strategy);
    }
    (
        select_domain(&algorithm, history, objective),
        "algorithm_active",
        search_strategy(&algorithm, "active"),
    )
}

/// Reconcile a measured post-adoption target with the real new executable's
/// durable campaign input. The parent request remains unchanged; this only
/// derives the next isolated worker's objective.
pub(super) fn native_plan_source_input(parent:&Value,campaign:&Value)->Value {
    let mut input=parent.clone();
    if input["rsiNativeInduction"]==true && input["rsiExpandAfterGain"]==true
        && input["selectionPolicy"]=="automatic" {
        if let Some(domain)=campaign["planObjectiveOverride"].as_str()
            .filter(|domain|DOMAINS.contains(domain)) {
            input["rsiObjective"]=json!(domain);
            input["rsiAutonomous"]=json!(false);
            input["rsiFocusFiles"]=json!([]);
        }
    }
    input
}

/// A descendant can only plan source files that actually exist in its own
/// pinned checkout with the same bytes hashed by its source index. The source
/// index contains non-editable docs/assets too; a raw top-ranked path may
/// otherwise abort the campaign before any model edit is attempted.
pub(super) fn verified_descendant_focus(
    state:&AppState,index_id:&str,lab:&Path,ranked:Vec<Value>
)->Vec<Value>{
    let Ok(root)=std::fs::canonicalize(lab) else{return Vec::new()};
    ranked.into_iter().filter(|row|{
        let Some(path)=row["path"].as_str() else{return false};
        if !crate::engine::rsi::safe_path(path){return false;}
        let Some(indexed)=crate::engine::code_index::indexed_source_hash(
            state,index_id,path) else{return false};
        let Ok(file)=std::fs::canonicalize(root.join(path)) else{return false};
        if !file.starts_with(&root)||!file.is_file(){return false;}
        let Ok(bytes)=std::fs::read(file) else{return false};
        format!("{:x}",Sha256::digest(&bytes))==indexed
    }).take(2).collect()
}

/// The model-bearing generation is planned by its own compiled executable,
/// not by an older coordinator. This method only reads durable observations
/// and returns a bounded proposal; it does not invoke a model or mutate code.
pub(super) fn native_generation_plan(state:&AppState, campaign:&Value, job:&Value, binary_commit:&str)
    -> Result<Value,String> {
    if job["kind"]!="rsi_evolve" || job["status"]!="running"
        || job["cancelRequested"]==true || campaign["id"]!=job["id"]
        || campaign["headCommit"]!=binary_commit
        || campaign["planAttempt"].as_u64().is_none()
    {
        return Err("계획 작업의 캠페인 상태 또는 개선기 revision이 일치하지 않습니다.".into());
    }
    let campaign_id=campaign["id"].as_str().ok_or("캠페인 ID가 없습니다.")?;
    let mut history=state.store.all("rsi_generation").into_iter()
        .filter(|generation|generation["campaignId"]==campaign_id)
        .collect::<Vec<_>>();
    history.sort_by_key(|generation|generation["attempt"].as_u64().unwrap_or(0));
    if campaign["history"].as_array().is_none_or(|rows|rows.len()!=history.len())
        || campaign["planAttempt"].as_u64()!=Some(history.len() as u64+1) {
        return Err("계획 시점의 세대 이력과 저장된 실험 계보가 다릅니다.".into());
    }
    let scoped_input=native_plan_source_input(&job["input"],campaign);
    let source_input=&scoped_input;
    let objective=selection_objective(source_input);
    let action_history=improvement_history(state,source_input,&history,campaign_id);
    let system_mode=source_input["rsiSystemEvolution"]==true;
    let automatic=system_mode && source_input["selectionPolicy"]=="automatic"
        && source_input["rsiFocusFiles"].as_array().is_none_or(Vec::is_empty);
    let lab=campaign["labDir"].as_str().unwrap_or("");
    // The newly compiled intelligence, rather than the old supervisor,
    // models the whole versioned source and decides what to redesign.
    let model=if system_mode {
        if lab.is_empty(){return Err("후손의 실제 소스 디렉터리가 없습니다.".into());}
        crate::engine::rsi_system::blueprint(Path::new(lab),binary_commit,&history)
    }else{Value::Null};
    let mut agenda=if system_mode {
        crate::engine::rsi_system::system_agenda(state,&model,&history,
            campaign["planIndexId"].as_str().unwrap_or(""),Path::new(lab))
    }else{Value::Null};
    let inherited=if automatic {
        crate::engine::rsi_system::inherited_domain(&history)
    }else{None};
    // The current child's meta-controller is compiled with its task-agent:
    // its evolving decision algorithm, not the predecessor's ordering,
    // chooses the redesign with the highest evidence-weighted potential.
    let meta_policy=if system_mode {
        let frontier=agenda["frontier"].as_array().cloned().unwrap_or_default();
        crate::engine::rsi_meta_controller::synthesize(
            &history,&frontier,DOMAINS,inherited.as_deref())
    }else{Value::Null};
    if automatic && agenda["inheritedStage"].is_null()
        && meta_policy["target"].is_object()
        && agenda["frontier"].as_array().is_some_and(|f|f.contains(&meta_policy["target"])) {
        agenda["target"]=meta_policy["target"].clone();
        agenda["focus"]=json!([meta_policy["target"]]);
    }
    // A descendant-selected two-role experiment must become actual source
    // focus, not an unexecuted prompt suggestion. The selected source hashes
    // will be checked again before any edit. Explicit user focus and inherited
    // model-authored stages are not replaced by this exploratory policy.
    if automatic && agenda["inheritedStage"].is_null() {
        crate::engine::rsi_meta_controller::attach_reciprocal_focus(
            &mut agenda,&meta_policy);
    }
    // Same-baseline factorial design is produced by the RUNNING descendant,
    // not by a parent template. It changes executable source focus.
    let factorial=if system_mode {
        crate::engine::rsi_causal_ecology::design(
            &history,agenda["frontier"].as_array().map(Vec::as_slice)
                .unwrap_or(&[]),
            &meta_policy["reciprocalExperiment"],binary_commit)
    }else{Value::Null};
    if automatic && agenda["inheritedStage"].is_null(){
        crate::engine::rsi_causal_ecology::apply(&mut agenda,&factorial);
    }
    // A new compiled descendant can self-modify its scientific discovery
    // method and also create intelligence components it will later develop.
    let laboratory=if system_mode {
        crate::engine::rsi_recursive_laboratory::plan(
            &history,agenda["frontier"].as_array().map(Vec::as_slice)
                .unwrap_or(&[]),
            &factorial,&meta_policy,binary_commit)
    }else{Value::Null};
    if automatic && agenda["inheritedStage"].is_null() {
        crate::engine::rsi_recursive_laboratory::apply(&mut agenda,&laboratory);
    }
    // A second-order learning loop: source-backed METHOD improvements are
    // selected by what their independently measured COMPILED CHILDREN can do
    // to models, tools, memory, learning and any newly invented roles.
    let principle_model=if system_mode {
        crate::engine::rsi_principle_learner::learn(
            &history,agenda["frontier"].as_array()
                .map(Vec::as_slice).unwrap_or(&[]))
    }else{Value::Null};
    let principle_policy=if system_mode {
        crate::engine::rsi_principle_learner::plan(
            &history,agenda["frontier"].as_array()
                .map(Vec::as_slice).unwrap_or(&[]),
            &principle_model,binary_commit)
    }else{Value::Null};
    if automatic && agenda["inheritedStage"].is_null(){
        crate::engine::rsi_principle_learner::apply(
            &mut agenda,&principle_policy);
    }
    // The whole intelligence, not the preferred model or its meta-controller,
    // learns the weaknesses seen in independent paired agent evaluations.
    // The compiled child uses these weaknesses to select a real source edit.
    let whole_dominion=if system_mode {
        crate::engine::rsi_whole_agent::plan(
            &history,agenda["frontier"].as_array()
                .map(Vec::as_slice).unwrap_or(&[]),binary_commit)
    }else{Value::Null};
    if automatic && agenda["inheritedStage"].is_null(){
        crate::engine::rsi_whole_agent::apply(&mut agenda,&whole_dominion);
    }
    let mut peer_input=source_input.clone();
    peer_input["rsiIndexId"]=campaign["planIndexId"].clone();
    let peers=if system_mode {
        crate::engine::rsi_meta_controller::peer_lineages(
            state,campaign_id,&peer_input,
            agenda["target"]["role"].as_str())
    }else{Value::Null};
    let method_hypothesis=if system_mode {
        crate::engine::rsi_meta_controller::next_method_hypothesis(
            &meta_policy,&history)
    }else{Value::Null};
    let hybrid=if system_mode {
        crate::engine::rsi_meta_controller::compositional_hypothesis(
            &peers,&meta_policy)
    }else{Value::Null};
    let self_domain=if automatic {
        agenda["target"]["domain"].as_str()
            .filter(|domain|DOMAINS.contains(domain)).map(str::to_string)
    }else{None};
    let explicit=campaign["planSelectedDomain"].as_str()
        .filter(|domain|DOMAINS.contains(domain)).map(str::to_string);
    let selected=if automatic && agenda["inheritedStage"].is_object() {
        // Model-authored successor stage can override a generic inherited
        // domain when the current source and observed failures support it.
        self_domain.or(inherited).or(explicit)
    }else if automatic && agenda["principleCurriculum"].is_object() {
        // Capability-level evidence can override a stale meta-domain bias.
        self_domain.or(inherited).or(explicit)
    }else if automatic && agenda["recursiveLaboratory"].is_object() {
        // A compiled scientific cycle must execute its own currently selected
        // role/domain, not a predecessor's generic domain preference.
        self_domain.or(inherited).or(explicit)
    }else if automatic && agenda["factorialExperiment"].is_object() {
        // The factorial phase is an actual intervention, not an inherited
        // unrelated domain preference; the runtime must execute its choice.
        self_domain.or(inherited).or(explicit)
    }else{inherited.or(self_domain).or(explicit)};
    let trial_id=campaign["planTrialAlgorithmId"].as_str()
        .filter(|id|!id.is_empty()).map(str::to_string);
    let owns_choice=automatic && selected.is_some();
    let trial=if owns_choice{None}else{trial_id};
    let continuation=if owns_choice{None}else{research_continuation(&action_history)};
    let (domain,selection,strategy)=select_generation_strategy(
        state,if trial.is_some(){&history}else{&action_history},&objective,
        continuation,trial,selected,binary_commit);
    let mut focus_input=source_input.clone();
    focus_input["rsiTargetDomain"]=json!(domain);
    focus_input["rsiIndexId"]=campaign["planIndexId"].clone();
    let ranked=if focus_input["rsiFocusFiles"].as_array().is_none_or(Vec::is_empty) {
        let self_focus=if automatic {
            agenda["focus"].as_array().into_iter().flatten().take(2)
                .filter_map(|row|{
                    let path=row["path"].as_str()?;
                    Some(json!({"path":path,"sourceHash":row["sourceSha256"],
                        "systemRole":row["role"],"systemDomain":row["domain"]}))
                }).collect::<Vec<_>>()
        }else{Vec::new()};
        if !self_focus.is_empty(){self_focus}
        else{
            let policy=strategy["id"].as_str()
                .and_then(|id|state.store.get("algorithm",id))
                .or_else(||crate::engine::algorithms::active(state,"selfimprove.search"));
            choose_focus_files(state,&focus_input,&action_history,policy.as_ref())
        }
    }else{Vec::new()};
    let ranked=if system_mode {
        verified_descendant_focus(state,
            campaign["planIndexId"].as_str().unwrap_or(""),
            Path::new(lab),ranked)
    }else{ranked};
    let target_plan=generation_target_plan(&domain,selection,&strategy,binary_commit);
    Ok(json!({
        "source":"compiled_generator_plan_v1",
        "generatorCommit":binary_commit,
        "attempt":campaign["planAttempt"],
        "historyCount":history.len(),
        "objectiveId":crate::engine::memory::stable_id(&[&objective]),
        "indexId":campaign["planIndexId"],
        "domain":domain,"selection":selection,"strategy":strategy,
        "targetPlan":target_plan,
        "improvementDirective":generation_improvement_directive(source_input,&action_history),
        "systemBlueprint":model,"systemAgenda":agenda,
        "metaPolicy":meta_policy,"factorialExperiment":factorial,
        "recursiveLaboratory":laboratory,
        "principleModel":principle_model,"principleCurriculum":principle_policy,
        "wholeDominion":whole_dominion,
        "peerLineages":peers,
        "metaMethodHypothesis":method_hypothesis,
        "crossLineageHypothesis":hybrid,
        "rankedFiles":ranked
    }))
}


