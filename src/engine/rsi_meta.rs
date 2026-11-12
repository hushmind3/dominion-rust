//! Coordinator-owned evaluation of an improver, through real paid downstream
//! source-edit episodes. The grader and subject revision stay fixed across arms.
//! This module is excluded from candidate discovery and source mutation.
use super::{jobs::JobContext, rsi_evolve::command, rsi_worktree};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{path::{Path, PathBuf}, time::{Duration, Instant}};
use tokio::process::Command;

pub fn case() -> Value {
    json!({"id":"meta-downstream-editing","domain":"improver","holdout":true,
        "filter":"rsi_meta_downstream_editing",
        "metrics":[{"key":"quality","direction":"higher","minAbsoluteGain":0.025,"minRelativeGain":0.02}]})
}

fn sha(bytes: impl AsRef<[u8]>) -> String { format!("{:x}",Sha256::digest(bytes.as_ref())) }

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

async fn episode(context:&JobContext, binary:&Path, generator_commit:&str, source:&Path,
    source_commit:&str, outer_input:&Value, task:usize, arm:&str, root:&Path) -> Result<Value,String> {
    // Different instructions and example wording are held out from the outer
    // improver's development trace. Neither arm sees grading sentences.
    let request=if task==0 {
        "한국어의 연속 동작 요청을 실제 하위 작업으로 분해하도록 src/engine/decompose.rs를 개선하세요. '초안을 만들고 내용을 고쳐줘'가 2단계가 되어야 합니다. 명사에 들어 있는 '고'로 잘못 분해하지 마세요. 예문 하드코딩 없이 구현을 읽고 작은 실제 변경을 작성하세요."
    }else{
        "src/engine/decompose.rs의 한국어 동작 연결 처리를 개선하세요. '설정을 바꾸고 버튼을 누르고 상태를 알려줘'가 3단계가 되어야 합니다. 기존 번호 목록·단일 명사 처리를 보존하세요. 실제 구현을 해시와 함께 읽은 후 소스를 수정하세요."
    };
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
        "rsiAutonomous":true,"rsiAutoFocus":true,"rsiSourceAdvanced":false,
        "rsiSourceDir":source,"rsiKnowledgeSource":source,"rsiBaseCommit":source_commit,
        "rsiCampaignId":context.job_id,"rsiGeneratorCommit":generator_commit,
        "rsiIndexId":index["id"],"rsiFocusFiles":["src/engine/decompose.rs"],
        "rsiSelectedDomain":"tools","rsiTargetDomain":"tools",
        "rsiDiagnostic":{"failed":1,"domain":"tools","focusFiles":["src/engine/decompose.rs"]},
        "rsiTargetPlan":{"domain":"tools","selection":"algorithm_active","strategy":strategy,"generatorCommit":generator_commit},
        "rsiFrozenCases":rsi_worktree::metric_cases(),"rsiNativeInduction":false,
        "rsiLocalOnly":outer_input["rsiLocalOnly"],
        "rsiEpisodeTokenLimit":24000,"rsiRemainingCampaignTokens":24000,
        "toolsEnabled":true,"toolsExecutionEnabled":true,"autoLearning":false});
    // Use the same frozen registration cases in each arm; never nest meta eval.
    input["rsiFrozenCases"]=outer_input["rsiFrozenCases"].as_array().map(|cases|
        cases.iter().filter(|c|c["id"]!="meta-downstream-editing").cloned().collect::<Vec<_>>()).map_or(Value::Null,|c|json!(c));
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
        "providerId":outer_input["providerId"],"model":outer_input["model"],
        "tokenLimit":24000,"timeLimitSeconds":240,"usage":usage,"elapsedMs":started.elapsed().as_millis() as u64,
        "workerOk":outcome.as_ref().is_ok_and(|o|o.status.success()),"quality":0.0,"actualModelRoutes":routes,
        "sourceWrites":child["result"]["rsi"]["editEvidence"],"candidateId":child["result"]["rsi"]["candidateId"],
        "actualFocus":child["input"]["rsiFocusFiles"],"error":child["error"]});
    if let Some(candidate)=candidate.filter(|_|child["result"]["rsi"]["checksPassed"]==true) {
        let patch=candidate["patch"].as_str().ok_or("후속 패치 없음")?;
        let dir=root.join(format!("grade-{task}-{arm}"));std::fs::create_dir_all(&dir).map_err(|e|e.to_string())?;
        let patch_file=dir.join("subject.patch");std::fs::write(&patch_file,patch).map_err(|e|e.to_string())?;
        let fixture=dir.join("oracle.rs");let oracle=include_str!("rsi_oracle_fixture.rs");
        std::fs::write(&fixture,oracle).map_err(|e|e.to_string())?;
        let plan=dir.join("plan.json");std::fs::write(&plan,json!({"oracleVersion":"meta-subject-v1",
            "oracleFixture":fixture,"oracleSha256":sha(oracle),"cases":[
                {"id":"subject-planning","domain":"planning","holdout":true,"filter":"rsi_frozen_oracle::planning_quality","metrics":[{"key":"quality"}]},
                {"id":"subject-guard","domain":"planning","holdout":true,"filter":"engine::decompose::tests::numbered_first_prefers_an_explicit_list"}]}).to_string()).map_err(|e|e.to_string())?;
        let report=dir.join("report.json");let runner=dir.join("rsi-lab.sh");
        std::fs::write(&runner,include_str!("../../scripts/rsi-lab.sh")).map_err(|e|e.to_string())?;
        std::fs::write(dir.join("rsi-measure.mjs"),include_str!("../../scripts/rsi-measure.mjs")).map_err(|e|e.to_string())?;
        let mut cmd=Command::new("bash");cmd.current_dir(source).arg(&runner).arg(source_commit).arg(&patch_file).arg(&plan).arg(&report)
            .env("PATH",super::dev::toolchain_path().unwrap_or_default())
            .env("RSI_BUILD_CACHE",context.state.config.data_dir.join("rsi-paired-build").join(&context.job_id));
        let checked=command(context,cmd,Duration::from_secs(600)).await?;
        if checked.status.success() {
            let measured:Value=serde_json::from_slice(&std::fs::read(&report).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            // Complete native checks AND useful held-out behavior are required.
            let quality=measured["cases"].as_array().into_iter().flatten().find(|c|c["id"]=="subject-planning")
                .filter(|c|c["candidatePass"]==true).and_then(|c|c["metrics"]["quality"]["candidate"].as_f64()).unwrap_or(0.0);
            let guards_ok=measured["candidateBuildOk"]==true && measured["cases"].as_array()
                .is_some_and(|cases|cases.iter().all(|case|case["candidatePass"]==true));
            let before=measured["cases"].as_array().into_iter().flatten().find(|c|c["id"]=="subject-planning")
                .and_then(|c|c["metrics"]["quality"]["baseline"].as_f64()).unwrap_or(1.0);
            // Reward improvement caused by the generated patch, not useful
            // behavior the unchanged subject already had or unrelated edits.
            let gain=if guards_ok && before<1.0 {((quality-before)/(1.0-before)).clamp(0.0,1.0)}else{0.0};
            result["quality"]=json!(gain);result["baselineSubjectQuality"]=json!(before);
            result["candidateSubjectQuality"]=json!(quality);result["subjectReport"]=measured;
        }else{result["gradingError"]=json!(String::from_utf8_lossy(&checked.stderr).chars().take(1000).collect::<String>());}
    }
    context.state.store.put("rsi_meta_episode",&json!({"id":id,"campaignId":context.job_id,"result":result}));
    Ok(result)
}

pub async fn evaluate(context:&JobContext, candidate:&Value, source:&Path, output:&Path) -> Result<Value,String> {
    let outer=context.state.store.get("job",&context.job_id).ok_or("상위 개선 작업 없음")?;
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
        let mut rows=Vec::new();
        for task in 0..2 {
            // Balance ordering, run sequentially, use the same fixed model and
            // identical subject bytes. No previous arm's patch is inherited.
            for arm in if task==0 {["baseline","candidate"]}else{["candidate","baseline"]} {
                let binary=if arm=="baseline"{&saved}else{&upgraded};
                let revision=if arm=="baseline"{base}else{&commit};
                rows.push(episode(context,binary,revision,source,base,&outer["input"],task,arm,output).await?);
            }
        }
        if rows.iter().any(|row|row["usage"]["calls"].as_u64().unwrap_or(0)==0) {
            return Err("후속 과제 양쪽의 실제 모델 호출 증거가 부족합니다. 재귀 성과로 채택하지 않습니다.".into());
        }
        let mean=|arm:&str,key:&str| rows.iter().filter(|r|r["arm"]==arm).map(|r|
            if key=="quality"{r["quality"].as_f64().unwrap_or(0.0)}else{r["usage"]["calls"].as_f64().unwrap_or(0.0)}).sum::<f64>()/2.0;
        let measured=json!({"id":"meta-downstream-editing","baselinePass":true,"candidatePass":true,
            "metrics":{"quality":{"baseline":mean("baseline","quality"),"candidate":mean("candidate","quality"),"samples":2},
                "modelCalls":{"baseline":mean("baseline","calls"),"candidate":mean("candidate","calls"),"samples":2}}});
        let receipt=json!({"version":1,"campaignId":context.job_id,"candidateId":candidate["id"],
            "baselineGeneratorCommit":base,"candidateGeneratorCommit":commit,"subjectCommit":base,
            "providerId":outer["input"]["providerId"],"model":outer["input"]["model"],
            "tasks":2,"episodes":rows,"measurement":measured,
            "limitation":"Two held-out source-edit episodes in one subject domain; not a proof of general intelligence or unbounded recursive gains."});
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
    fn independent_graders_cannot_be_candidate_source_targets() {
        assert!(!super::super::rsi::safe_path("src/engine/rsi_meta.rs"));
        assert!(!super::super::rsi::safe_path("src/engine/rsi_oracle_fixture.rs"));
        for file in ["src/engine/rsi_evolve.rs","src/engine/chat/runtime.rs","src/engine/algorithms.rs",
            "src/engine/memory.rs","src/engine/learning.rs","src/engine/tools.rs","web/src/pages/SelfImprovement.tsx"] {
            assert!(super::super::rsi::safe_path(file),"algorithm, harness and system source must be evolvable: {file}");
        }
    }
}
