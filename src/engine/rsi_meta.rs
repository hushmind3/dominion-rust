//! Coordinator-owned evaluation of an improver, through real downstream
//! source-edit episodes. The grader and subject revision stay fixed across arms.
//! This module is excluded from candidate discovery and source mutation.
use super::{jobs::JobContext, rsi_evolve::command, rsi_worktree};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::{Path, PathBuf}, time::{Duration, Instant}};
use tokio::process::Command;

pub fn cases_for(profile:&Value)->Vec<Value> {
    profile["tasks"].as_array().into_iter().flatten().map(|task|json!({
        "id":format!("meta-downstream-{}",task["id"].as_str().unwrap_or("")),"domain":"improver","holdout":true,
        "filter":format!("rsi_meta_downstream_{}",task["id"].as_str().unwrap_or("")),
        "metrics":[{"key":"quality","direction":"higher","minAbsoluteGain":0.025,"minRelativeGain":0.02}]
    })).collect()
}

#[cfg(test)]
pub fn cases()->Vec<Value>{cases_for(&super::rsi_profile::default_profile())}

pub fn is_case(value:&Value)->bool {
    value["id"].as_str().is_some_and(|id|id.starts_with("meta-downstream-"))
}

#[cfg(test)]
pub fn case()->Value {cases().remove(0)}

fn sha(bytes: impl AsRef<[u8]>) -> String { format!("{:x}",Sha256::digest(bytes.as_ref())) }

/// Frozen downstream capabilities exercise different parts of the same
/// compiled agent, instead of two paraphrases targeting a single product bug.
/// Both arms get identical tasks, source bytes, evaluation and token limits.
#[cfg(test)]
const META_TASK_COUNT:usize=4;

#[derive(serde::Deserialize)]
struct MetaTask {
    domain:String, file:String, request:String, oracle:String, metric:String, guard:String,
}

#[cfg(test)]
fn meta_task(task:usize)->Option<MetaTask> {
    serde_json::from_value(super::rsi_profile::default_profile()["tasks"].get(task)?.clone()).ok()
}

/// The independent runner repeats the measured case and enforces guard tests.
/// A new patch earns credit for a quality gain or a substantial resource saving
/// while holding quality; an unchanged or regressed subject earns zero.
fn measured_subject_gain(measured:&Value, metric:&str)->(f64,f64,f64) {
    let subject=measured["cases"].as_array().into_iter().flatten()
        .find(|row|row["id"]=="subject-capability");
    let Some(subject)=subject else {return (0.0,0.0,0.0)};
    let before=subject["metrics"]["quality"]["baseline"].as_f64().unwrap_or(0.0);
    let after=subject["metrics"]["quality"]["candidate"].as_f64().unwrap_or(0.0);
    let complete=measured["candidateBuildOk"]==true
        && measured["cases"].as_array()
            .is_some_and(|cases|cases.iter().all(|case|case["candidatePass"]==true));
    if !complete || after+1e-9<before {return (0.0,before,after);}
    let quality_gain=if before<1.0 {
        ((after-before)/(1.0-before)).clamp(0.0,1.0)
    }else{0.0};
    let resource_gain=if metric!="quality" {
        let baseline=subject["metrics"][metric]["baseline"].as_f64().unwrap_or(0.0);
        let candidate=subject["metrics"][metric]["candidate"].as_f64().unwrap_or(f64::INFINITY);
        // A 10% minimum avoids credit for tiny timer fluctuations. Relative
        // improvement is capped so cross-domain tasks share a common scale.
        if baseline.is_finite() && baseline>0.0 && candidate.is_finite()
            && candidate>=0.0 && candidate<=baseline*0.90 {
            ((baseline-candidate)/baseline).clamp(0.0,1.0)
        }else{0.0}
    }else{0.0};
    (quality_gain.max(resource_gain),before,after)
}



async fn git(context:&JobContext, dir:&Path, args:&[&str]) -> Result<String,String> {
    let mut cmd=Command::new("git");cmd.current_dir(dir).args(args);
    let out=command(context,cmd,Duration::from_secs(30)).await?;
    if !out.status.success(){return Err(String::from_utf8_lossy(&out.stderr).into());}
    Ok(String::from_utf8_lossy(&out.stdout).trim().into())
}

/// Development evidence identifies a real failure of the improver itself,
/// rather than turning an unrelated product bug into the default RSI task.
pub fn development_evidence(state:&crate::state::AppState) -> Option<Value> {
    state.store.all("job").into_iter().find_map(|job| {
        let replan=&job["rsiTargetReplan"];
        if job["kind"]!="rsi_generation" || job["input"]["rsiDiagnostic"]["failed"].as_u64().unwrap_or(0)==0
            || !replan["coordinatorPlan"]["domain"].is_string()
            || replan["coordinatorPlan"]["domain"]==replan["descendantPlan"]["domain"] {return None;}
        Some(json!({"jobId":job["id"],"generatorCommit":job["input"]["rsiGeneratorCommit"],
            "diagnosedDomain":job["input"]["rsiDiagnostic"]["domain"],
            "originalFocus":job["input"]["rsiDiagnostic"]["focusFiles"],
            "actualFocus":job["input"]["rsiFocusFiles"],"replan":replan,
            "objective":"실제 진단 근거가 있는데도 후속 개선기가 다른 분야·파일로 이탈하는 원인을 제거한다.",
            "focusFiles":["src/engine/rsi_evolve.rs"]}))
    })
}

async fn build(context:&JobContext, source:&Path, target:&Path) -> Result<PathBuf,String> {
    // APFS clone keeps the paid comparison from recompiling dependencies or
    // duplicating gigabytes of cache. Candidate and baseline executables remain
    // separate; every build still validates its embedded source identity.
    #[cfg(target_os="macos")]
    if !target.exists() {
        let seed=context.state.config.data_dir.join("rsi-build").join("generator");
        if seed.is_dir() {
            if let Some(parent)=target.parent(){std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;}
            let mut copy=Command::new("cp");copy.arg("-cR").arg(seed).arg(target);
            let _=command(context,copy,Duration::from_secs(120)).await?;
        }
    }
    let revision=git(context,source,&["rev-parse","HEAD"]).await?;
    let mut cmd=Command::new(rsi_worktree::cargo_executable());
    cmd.current_dir(source).args(["build","--release","--locked","--bin","dominion-rust","--target-dir"])
        .arg(target).env("DOMINION_RSI_BUILD_COMMIT",&revision);
    let out=command(context,cmd,Duration::from_secs(600)).await?;
    if !out.status.success(){return Err(format!("메타 평가 실행기 빌드 실패: {}",String::from_utf8_lossy(&out.stderr).chars().rev().take(1600).collect::<String>().chars().rev().collect::<String>()));}
    let binary=target.join("release").join(format!("dominion-rust{}",std::env::consts::EXE_SUFFIX));
    let mut check=Command::new(&binary);check.arg("--rsi-worker-identity");
    let checked=command(context,check,Duration::from_secs(10)).await?;
    if !checked.status.success() || String::from_utf8_lossy(&checked.stdout).trim()!=revision {
        return Err("후속 과제를 실행할 개선기 바이너리와 실제 소스 버전이 다릅니다.".into());
    }
    Ok(binary)
}

/// The paired improver comparison has two arms for every task. Reserve the
/// maximum equally before starting either arm: an affordable half-comparison
/// is not evidence of improved intelligence, and should not burn paid tokens.
fn fair_meta_episode_limit(remaining:Option<u64>,task_count:usize)->Result<u64,String>{
    let per=remaining.map(|tokens|
        tokens.saturating_sub(8_000)/(2*task_count.max(1)) as u64
    ).unwrap_or(24_000).min(24_000);
    if per<4_000 {
        return Err("후속 지능 A/B에 필요한 전체 동등 예산이 부족합니다. 비용을 쓰기 전에 평가를 중단했습니다.".into());
    }
    Ok(per)
}

fn remaining_meta_tokens(context:&JobContext)->Option<u64>{
    let budget=context.state.store.get("rsi_campaign",&context.job_id)?
        ["limits"]["tokenBudget"].as_u64()?;
    let sessions=context.state.store.all("job").into_iter()
        .filter(|job|job["kind"]=="rsi_generation"
            && job["input"]["rsiCampaignId"]==context.job_id)
        .filter_map(|job|job["sessionId"].as_str().map(str::to_string))
        .collect::<BTreeSet<_>>();
    let used=sessions.iter().map(|id|
        context.state.store.session_usage(id)["totalTokens"]
            .as_u64().unwrap_or(0)
    ).fold(0u64,u64::saturating_add);
    Some(budget.saturating_sub(used))
}

async fn episode(context:&JobContext, binary:&Path, generator_commit:&str,
    generator_source:&Path, source:&Path, source_commit:&str,
    outer_input:&Value, task:usize, arm:&str, root:&Path,
    token_limit:u64) -> Result<Value,String> {
    let profile=super::rsi_profile::from_input(outer_input)?;
    let task_spec:MetaTask=serde_json::from_value(profile["tasks"][task].clone()).map_err(|e|e.to_string())?;
    let request=task_spec.request;
    let index=super::code_index::refresh(&context.state,source)?;
    let strategy=super::algorithms::active(&context.state,"selfimprove.search")
        .unwrap_or_else(||json!({"implementation":"failure-clusters"}));
    let strategy=json!({"id":strategy["id"],"revision":strategy["revision"],"implementation":strategy["implementation"],
        "name":strategy["name"],"mode":"active"});
    // Reproduce the observed state transition, not an artificial broken source:
    // an old coordinator supplied a diagnostic and target plan but lost the
    // optional selected-domain hint. The new generator must interpret evidence.
    let mut input=json!({"providerId":outer_input["providerId"],"model":outer_input["model"],
        "reasoningEffort":outer_input["reasoningEffort"],"prompt":request,"rsiPromptBase":request,
        "rsiPromptEvidence":"","rsiPromptResearch":"","rsiObjective":request,
        "rsiAutonomous":false,"rsiAutoFocus":false,"rsiSourceAdvanced":false,
        "rsiSourceDir":source,"rsiKnowledgeSource":source,"rsiBaseCommit":source_commit,
        "rsiCampaignId":context.job_id,"rsiGeneratorCommit":generator_commit,
        "rsiIndexId":index["id"],"rsiFocusFiles":[task_spec.file],
        "rsiSelectedDomain":task_spec.domain,"rsiTargetDomain":task_spec.domain,
        "rsiTargetPlan":{"domain":task_spec.domain,"selection":"algorithm_active","strategy":strategy,"generatorCommit":generator_commit},
        "rsiFrozenCases":rsi_worktree::metric_cases(),"rsiNativeInduction":false,
        "rsiLocalOnly":outer_input["rsiLocalOnly"],
        "rsiEpisodeTokenLimit":token_limit,"rsiRemainingCampaignTokens":token_limit,
        "toolsEnabled":true,"toolsExecutionEnabled":true,"autoLearning":false});
    // A downstream task exercises the ACTUAL compiled generator's intelligent
    // controller, not a featureless worker shim. Baseline and candidate work
    // against identical subject bytes; each carries its own pinned agent
    // algorithm/harness/learning source for the verified self-model.
    if outer_input["rsiSystemEvolution"]==true {
        input["rsiSystemEvolution"]=json!(true);
        input["rsiGeneratorSourceDir"]=json!(generator_source);
        input["rsiFocusSelection"]=json!({"source":"owner_selected"});
        // The A/B arms must exercise the SAME normal compiled successor
        // planner used by production generations. The fixed task-specific
        // focus and selected domain are owner constraints, not a distinct
        // evaluator-only execution algorithm.
        let plan_file=root.join(format!("successor-{task}-{arm}.json"));
        let snapshot=json!({"generatorCommit":generator_commit,"attempt":1,"input":input});
        std::fs::write(&plan_file,serde_json::to_vec(&snapshot).map_err(|e|e.to_string())?)
            .map_err(|e|e.to_string())?;
        let mut planner=Command::new(binary);
        planner.arg("--rsi-plan-generation").arg(&plan_file)
            .current_dir(generator_source)
            .env("DOMINION_DATA",&context.state.config.data_dir)
            .env("DOMINION_WORKSPACE",source)
            .env_remove("DOMINION_DESKTOP_PARENT");
        let planned=command(context,planner,Duration::from_secs(30)).await;
        let _=std::fs::remove_file(&plan_file);
        let planned=planned?;
        if !planned.status.success(){
            return Err(format!("개선기 A/B 후손 제어기 계획 실패: {}",
                String::from_utf8_lossy(&planned.stderr)));
        }
        let plan:Value=serde_json::from_slice(&planned.stdout)
            .map_err(|e|format!("개선기 A/B 후손 계획 파싱 실패: {e}"))?;
        if plan["source"]!="compiled_descendant_system_controller_v2"
            || plan["generatorCommit"]!=generator_commit
            || plan["domain"]!=task_spec.domain
            || plan["focusFiles"]!=json!([task_spec.file])
            || plan["systemBlueprint"]["generatorCommit"]!=generator_commit
            || !plan["editPolicy"].is_object()
            || !plan["improvementDirective"].is_object(){
            return Err("개선기 A/B 후손이 지정된 동일 소스 과제를 계획하지 않았습니다.".into());
        }
        input["rsiControllerPlan"]=plan.clone();
        input["rsiSystemBlueprint"]=plan["systemBlueprint"].clone();
        input["rsiImprovementDirective"]=plan["improvementDirective"].clone();
        input["rsiEditPolicy"]=plan["editPolicy"].clone();
        input["rsiSelectedDomain"]=plan["domain"].clone();
        input["rsiTargetDomain"]=plan["domain"].clone();
        input["rsiFocusFiles"]=plan["focusFiles"].clone();
        input["rsiTargetPlan"]=json!({
            "domain":plan["domain"],"selection":plan["selection"],
            "strategy":plan["strategy"],"generatorCommit":generator_commit
        });
    }
    // Use the same frozen registration cases in each arm; never nest meta eval.
    input["rsiFrozenCases"]=outer_input["rsiFrozenCases"].as_array().map(|cases|
        cases.iter().filter(|c|!is_case(c)).cloned().collect::<Vec<_>>()).map_or(Value::Null,|c|json!(c));
    let child=context.state.store.put("job",&json!({"kind":"rsi_generation","status":"running",
        "rsiEvaluationEpisode":true,"rsiEvaluationArm":arm,"rsiEvaluationTask":task,
        "input":input,"attempts":1,"queuedAt":chrono::Utc::now().to_rfc3339()})).ok_or("후속 평가 작업 저장 실패")?;
    let id=child["id"].as_str().ok_or("후속 평가 작업 ID 없음")?;
    context.checkpoint(json!({"rsiMetaActiveJobId":id,"rsiMetaArm":arm,"rsiMetaTask":task}))?;
    let started=Instant::now();
    let mut cmd=Command::new(binary);cmd.arg("--rsi-worker-job").arg(id).current_dir(source)
        .env("DOMINION_DATA",std::fs::canonicalize(&context.state.config.data_dir).map_err(|e|e.to_string())?)
        .env("DOMINION_WORKSPACE",source).env_remove("DOMINION_DESKTOP_PARENT");
    let outcome=command(context,cmd,Duration::from_secs(240)).await;
    let child=context.state.store.get("job",id).ok_or("후속 평가 기록 없음")?;
    if matches!(child["status"].as_str(),Some("running"|"queued")) {
        if let Some(session)=child["sessionId"].as_str(){context.state.cancel_session(session);}
        context.state.store.patch_if("job",id,"status",&child["status"],
            &json!({"status":"cancelled","cancelRequested":true,"error":"후속 평가 시간 상한"}));
    }
    let usage=child["sessionId"].as_str().map(|session|context.state.store.session_usage(session)).unwrap_or(Value::Null);
    let routes=child["sessionId"].as_str().map(|session|context.state.store.session_model_routes(session)).transpose()?.unwrap_or_default();
    if routes.iter().any(|route|route["providerId"]!=outer_input["providerId"] || route["model"]!=outer_input["model"]) {
        return Err("후속 평가에서 지정한 고정 모델과 다른 경로가 사용되어 채택하지 않았습니다.".into());
    }
    let candidate=child["result"]["rsi"]["candidateId"].as_str().and_then(|id|context.state.store.get("rsi_candidate",id));
    let mut result=json!({"jobId":id,"task":task,"arm":arm,"generatorCommit":generator_commit,
        "subjectCommit":source_commit,"generatorBinarySha256":sha(std::fs::read(binary).map_err(|e|e.to_string())?),
        "generatorSourceTreeSha256":child["input"]["rsiSystemBlueprint"]["sourceTreeSha256"],
        "controllerPlannerSource":child["input"]["rsiControllerPlan"]["source"],
        "systemBlueprintVerified":child["rsiSystemBlueprintVerified"],
        "generationRuntimeMode":if child["input"]["rsiSystemEvolution"]==true {
            "compiled_system_controller"
        } else {"regular_worker"},
        "providerId":outer_input["providerId"],"model":outer_input["model"],
        "tokenLimit":token_limit,"timeLimitSeconds":240,"usage":usage,"elapsedMs":started.elapsed().as_millis() as u64,
        "workerOk":outcome.as_ref().is_ok_and(|o|o.status.success()),"quality":0.0,"actualModelRoutes":routes,
        "sourceWrites":child["result"]["rsi"]["editEvidence"],"candidateId":child["result"]["rsi"]["candidateId"],
        "actualFocus":child["input"]["rsiFocusFiles"],"error":child["error"]});
    if let Some(candidate)=candidate.filter(|_|child["result"]["rsi"]["checksPassed"]==true) {
        let patch=candidate["patch"].as_str().ok_or("후속 패치 없음")?;
        let dir=root.join(format!("grade-{task}-{arm}"));std::fs::create_dir_all(&dir).map_err(|e|e.to_string())?;
        let patch_file=dir.join("subject.patch");std::fs::write(&patch_file,patch).map_err(|e|e.to_string())?;
        let fixture=dir.join("oracle.rs");let oracle=profile["oracleSource"].as_str().ok_or("동결 평가 원문 없음")?;
        std::fs::write(&fixture,oracle).map_err(|e|e.to_string())?;
        let plan=dir.join("plan.json");
        let mut metrics=vec![json!({"key":"quality"})];
        if task_spec.metric!="quality" {
            metrics.push(json!({"key":task_spec.metric}));
        }
        std::fs::write(&plan,json!({"oracleVersion":"meta-multidomain-v2",
            "oracleFixture":fixture,"oracleSha256":sha(oracle),"cases":[
                {"id":"subject-capability","domain":task_spec.domain,"holdout":true,
                    "filter":task_spec.oracle,"metrics":metrics},
                {"id":"subject-guard","domain":task_spec.domain,"holdout":true,
                    "filter":task_spec.guard}]}).to_string()).map_err(|e|e.to_string())?;
        let report=dir.join("report.json");let runner=dir.join("rsi-lab.mjs");
        std::fs::write(&runner,include_str!("../../scripts/rsi-lab.mjs")).map_err(|e|e.to_string())?;
        std::fs::write(dir.join("rsi-measure.mjs"),include_str!("../../scripts/rsi-measure.mjs")).map_err(|e|e.to_string())?;
        let mut cmd=Command::new(rsi_worktree::node_executable());cmd.current_dir(source).arg(&runner).arg(source_commit).arg(&patch_file).arg(&plan).arg(&report)
            .env("PATH",super::dev::toolchain_path().unwrap_or_default())
            .env("RSI_BUILD_CACHE",context.state.config.data_dir.join("rsi-paired-build").join(&context.job_id));
        let checked=command(context,cmd,Duration::from_secs(600)).await?;
        if checked.status.success() {
            let measured:Value=serde_json::from_slice(&std::fs::read(&report).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            let (gain,before,after)=measured_subject_gain(&measured,&task_spec.metric);
            result["quality"]=json!(gain);
            result["domain"]=json!(task_spec.domain);
            result["subjectMetric"]=json!(task_spec.metric);
            result["baselineSubjectQuality"]=json!(before);
            result["candidateSubjectQuality"]=json!(after);
            result["subjectReport"]=measured;
        }else{result["gradingError"]=json!(String::from_utf8_lossy(&checked.stderr).chars().take(1000).collect::<String>());}
    }
    context.state.store.put("rsi_meta_episode",&json!({"id":id,"campaignId":context.job_id,"result":result}));
    Ok(result)
}

pub async fn evaluate(context:&JobContext, candidate:&Value, source:&Path, output:&Path) -> Result<Value,String> {
    let outer=context.state.store.get("job",&context.job_id).ok_or("상위 개선 작업 없음")?;
    let profile=super::rsi_profile::from_input(&outer["input"])?;
    let task_count=profile["tasks"].as_array().ok_or("후속 평가 목록 없음")?.len();
    let base=candidate["baseCommit"].as_str().ok_or("기준 버전 없음")?;
    let variant=output.join("meta-generator");
    git(context,source,&["worktree","add","--detach",variant.to_str().ok_or("경로 오류")?,base]).await?;
    let result=async {
        let patch=output.join("candidate.patch");
        git(context,&variant,&["apply",patch.to_str().ok_or("패치 경로 오류")?]).await?;
        git(context,&variant,&["add","-A"]).await?;
        git(context,&variant,&["-c","user.name=DOMINION Meta Evaluation","-c","user.email=rsi@localhost",
            "-c","core.hooksPath=","commit","-m","Ephemeral generator for independent downstream evaluation"]).await?;
        let commit=git(context,&variant,&["rev-parse","HEAD"]).await?;
        let target=context.state.config.data_dir.join("rsi-meta-build").join("generators");
        let baseline=build(context,source,&target).await?;
        let saved=output.join(format!("baseline{}",std::env::consts::EXE_SUFFIX));
        std::fs::copy(&baseline,&saved).map_err(|e|e.to_string())?;
        let upgraded=build(context,&variant,&target).await?;
        // Count ALL actually charged sessions, including earlier failed
        // generations. Both arms receive one immutable allowance.
        let token_limit=fair_meta_episode_limit(remaining_meta_tokens(context),task_count)?;
        let mut rows=Vec::new();
        for task in 0..task_count {
            // Balance ordering, run sequentially, use the same fixed model and
            // identical subject bytes. No previous arm's patch is inherited.
            for arm in if task%2==0 {["baseline","candidate"]}else{["candidate","baseline"]} {
                let binary=if arm=="baseline"{&saved}else{&upgraded};
                let revision=if arm=="baseline"{base}else{&commit};
                let generator_source=if arm=="baseline"{source}else{&variant};
                rows.push(episode(context,binary,revision,generator_source,source,base,
                    &outer["input"],task,arm,output,token_limit).await?);
            }
        }
        if rows.iter().any(|row|row["usage"]["calls"].as_u64().unwrap_or(0)==0) {
            return Err("후속 과제 양쪽의 실제 모델 호출 증거가 부족합니다. 재귀 성과로 채택하지 않습니다.".into());
        }
        let mean=|arm:&str,key:&str| rows.iter().filter(|r|r["arm"]==arm).map(|r|
            if key=="quality"{r["quality"].as_f64().unwrap_or(0.0)}else{r["usage"]["calls"].as_f64().unwrap_or(0.0)}).sum::<f64>()/task_count as f64;
        let measured=json!({"id":"meta-downstream-editing","baselinePass":true,"candidatePass":true,
            "metrics":{"quality":{"baseline":mean("baseline","quality"),"candidate":mean("candidate","quality"),"samples":task_count},
                "modelCalls":{"baseline":mean("baseline","calls"),"candidate":mean("candidate","calls"),"samples":task_count}}});
        let measurements=cases_for(&profile).into_iter().enumerate().map(|(task,spec)| {
            let score=|arm:&str|rows.iter().find(|row|row["task"].as_u64()==Some(task as u64) && row["arm"]==arm)
                .and_then(|row|row["quality"].as_f64()).unwrap_or(0.0);
            json!({"id":spec["id"],"baselinePass":true,"candidatePass":true,
                "metrics":{"quality":{"baseline":score("baseline"),"candidate":score("candidate"),"samples":1}}})
        }).collect::<Vec<_>>();
        let receipt=json!({"version":2,"campaignId":context.job_id,"candidateId":candidate["id"],
            "baselineGeneratorCommit":base,"candidateGeneratorCommit":commit,"subjectCommit":base,
            "providerId":outer["input"]["providerId"],"model":outer["input"]["model"],
            "tasks":task_count,"domains":profile["tasks"].as_array().unwrap().iter().map(|task|task["domain"].clone()).collect::<Vec<_>>(),
            "evaluationProfileId":profile["id"],"evaluationProfileSha256":profile["sha256"],
            "perEpisodeTokenLimit":token_limit,"pairedEpisodeCount":2*task_count,
            "episodes":rows,"measurement":measured,"measurements":measurements,
            "limitation":"Registered held-out tasks with matched model budgets; this tests downstream capability, not unbounded recursive intelligence."});
        std::fs::write(output.join("meta-receipt.json"),serde_json::to_vec_pretty(&receipt).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        Ok(receipt)
    }.await;
    // Cleanup is best-effort; evidence and the ephemeral commit stay inspectable.
    let mut cleanup=Command::new("git");cleanup.current_dir(source).args(["worktree","remove","--force"]).arg(&variant);
    let _=super::tools::run_command(&mut cleanup,Duration::from_secs(30)).await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pair_budget_is_split_before_any_billable_model_episode() {
        assert_eq!(fair_meta_episode_limit(None,4).unwrap(),24_000);
        assert_eq!(fair_meta_episode_limit(Some(400_000),4).unwrap(),24_000);
        assert_eq!(fair_meta_episode_limit(Some(104_000),4).unwrap(),12_000);
        assert!(fair_meta_episode_limit(Some(39_999),4).is_err());
    }
    #[test]
    fn meta_plan_requires_an_actual_downstream_quality_gain() {
        let spec=case();
        let plan=super::super::rsi_metrics::plan(&spec["metrics"]).unwrap();
        let (gain,loss,_)=super::super::rsi_metrics::compare(&plan,&json!({"quality":{"baseline":0.0,"candidate":0.0}})).unwrap();
        assert_eq!((gain,loss),(0,0));
        let (gain,loss,_)=super::super::rsi_metrics::compare(&plan,&json!({"quality":{"baseline":0.5,"candidate":0.75}})).unwrap();
        assert_eq!((gain,loss),(1,0));
        let (gain,loss,_)=super::super::rsi_metrics::compare(&plan,&json!({"quality":{"baseline":0.75,"candidate":0.5}})).unwrap();
        assert_eq!((gain,loss),(0,1));
    }
    #[test]
    fn a_domain_regression_cannot_hide_behind_an_improved_mean() {
        let mut plan=cases();
        plan.extend((0..4).map(|i|json!({"id":format!("guard-{i}"),"domain":"memory","holdout":true,"metrics":[]})));
        let mut measured=plan.iter().map(|spec|json!({"id":spec["id"],"baselinePass":true,"candidatePass":true,
            "metrics":{"quality":{"baseline":0.2,"candidate":0.2}}})).collect::<Vec<_>>();
        measured[0]["metrics"]["quality"]["candidate"]=json!(0.9);
        measured[1]["metrics"]["quality"]["candidate"]=json!(0.1);
        // Aggregate quality rises from .2 to .35, but memory has regressed.
        let report=super::super::rsi::evaluate_cases(&plan,&measured,"improver").unwrap();
        assert_eq!(report["eligible"],false);
        assert_eq!(report["verdict"],"regressed");
        assert_eq!(report["metricRegressions"],1);
        measured[1]["metrics"]["quality"]["candidate"]=json!(0.2);
        assert_eq!(super::super::rsi::evaluate_cases(&plan,&measured,"improver").unwrap()["eligible"],true);
    }
    #[test]
    fn downstream_suite_is_cross_domain_and_rewards_only_measured_change() {
        let domains=(0..META_TASK_COUNT).map(|i|meta_task(i).unwrap().domain)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(domains.len(),4);
        assert!(meta_task(META_TASK_COUNT).is_none());
        let unchanged=json!({"candidateBuildOk":true,"cases":[{
            "id":"subject-capability","candidatePass":true,"metrics":{
                "quality":{"baseline":1.0,"candidate":1.0},
                "elapsedMs":{"baseline":200.0,"candidate":200.0}}
        },{"id":"subject-guard","candidatePass":true}]});
        assert_eq!(measured_subject_gain(&unchanged,"elapsedMs").0,0.0);
        let mut gain=unchanged.clone();
        gain["cases"][0]["metrics"]["elapsedMs"]["candidate"]=json!(130.0);
        assert!(measured_subject_gain(&gain,"elapsedMs").0>0.3);
        gain["cases"][0]["metrics"]["quality"]["candidate"]=json!(0.9);
        assert_eq!(measured_subject_gain(&gain,"elapsedMs").0,0.0,
            "performance must never compensate for quality regression");
        let mut failed=unchanged;
        failed["cases"][1]["candidatePass"]=json!(false);
        assert_eq!(measured_subject_gain(&failed,"quality").0,0.0);
    }
    #[test]
    fn independent_graders_cannot_be_candidate_source_targets() {
        assert!(!super::super::rsi::safe_path("src/engine/rsi_meta.rs"));
        assert!(!super::super::rsi::safe_path("src/engine/rsi_oracle_fixture.rs"));
        for file in ["src/engine/rsi_evolve.rs","src/engine/chat/runtime.rs","src/engine/algorithms.rs",
            "src/engine/memory.rs","src/engine/learning.rs","src/engine/tools.rs","web/src/pages/SelfImprovement.tsx"] {
            assert!(super::super::rsi::safe_path(file),"algorithm, harness and system source must be evolvable: {file}");
        }
    }
}
