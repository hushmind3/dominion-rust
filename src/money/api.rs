//! Money presentation/API adapter: no second model loop or unapproved
//! commercial action. Optional inference is a native model-only Run.
use crate::{domain::{CreateRun,RunStatus},engine::Engine,sdk::{Dominion,RunOptions}};
use axum::{extract::{Path,State},http::StatusCode,routing::{get,post},Json,Router};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::{HashMap,HashSet},future::Future,path::PathBuf,sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

type Fault=(StatusCode,Json<Value>);
static FIND_LOCK:Mutex<()> = Mutex::const_new(());
fn err(msg:impl Into<String>)->Fault{
    (StatusCode::BAD_REQUEST,Json(json!({"error":msg.into()})))
}
fn internal(msg:String)->Fault{
    (StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"error":msg})))
}
async fn blocking<T:Send+'static>(
    job:impl FnOnce()->Result<T,String>+Send+'static
)->Result<T,Fault>{
    tokio::task::spawn_blocking(job).await.map_err(|e|internal(e.to_string()))?
        .map_err(internal)
}
fn short(value:&Value,key:&str,max:usize)->String{
    value[key].as_str().unwrap_or("").trim().chars().take(max).collect()
}
fn digest(input:&str)->String{format!("{:x}",Sha256::digest(input.as_bytes()))}
fn evidence(row:&Value)->Value{
    json!({"sourceId":row["id"],"url":row["url"],"claim":row["title"],
        "publishedAt":row["publishedAt"],"sourceName":row["sourceName"],
        "contentStatus":row["contentStatus"],"kind":"source_observation_unverified"})
}
fn business(kind:&str)->(&'static str,&'static str,&'static str,&'static str){
    match kind{
       "buyer_supply_spread"|"new_tool_to_buyer"=>
        ("구매 요청에 맞춘 납품·구축","공식 구매 규격과 공급 가격·기능을 실제로 맞출 수 있다면 납품 또는 구축 대금을 받을 수 있습니다.",
         "구매 문서의 마감일·규격을 확인하고 공급 기술의 라이선스·견적과 비교해 최소 납품안을 설계합니다.",
         "구매 담당자에게 요구사항 일치 여부와 유료 시험 납품 가능성을 문의합니다."),
       "automation_transfer"=>
        ("반복 비용 절감 자동화","고객이 이미 반복 지출하는 노동·외주비를 줄여 절감액의 일부를 서비스 대금으로 받을 수 있다는 가설입니다.",
         "대상 업무·반복량·현재 지출을 조사하고 1개 작업 자동화 시연을 만듭니다.",
         "실제 업무 보유자에게 유료 파일럿 비용을 제안하고 반응을 기록합니다."),
       "compliance_product"=>
        ("규정 변화 대응 도구","새로 발생하는 규정 대응 작업을 문서화·점검 서비스로 제공할 가능성을 조사합니다.",
         "규정 원문·관할·시행일·적용 대상부터 확인하고 반복 점검 항목을 추출합니다.",
         "대상 기업에 준수 비용과 유료 점검 수요를 확인합니다."),
       "input_substitution"|"demand_cost_spread"|"bottleneck_bypass"|"supply_substitution"=>
        ("대체 공급·원가 절감","비용 상승이나 공급 제약을 낮은 가격 또는 빠른 공급으로 해결하고 도입비·납품대금을 얻는 가설입니다.",
         "두 출처의 규격·단가·납기·운영 조건을 확인하고 대체 가능성을 검증합니다.",
         "실제 구매자에게 유료 대체 공급의 조건과 견적 요청 의사를 확인합니다."),
       _=>("시장 변화 대응 서비스","관측된 비용·구매 신호의 해결책에 실제로 지출할 고객이 있는지 조사합니다.",
          "공개 자료의 원문을 확인하고 구매자·지불 경로·최소 기능을 설계합니다.",
          "구매 가능 고객에게 구체적인 유료 시험을 제안합니다.")
    }
}
fn from_pair(pair:&Value,by_id:&HashMap<String,Value>)->Option<Value>{
    let a=by_id.get(pair["supplySignalId"].as_str()?)?;
    let b=by_id.get(pair["buyerSignalId"].as_str()?)?;
    let kind=pair["mechanism"].as_str().unwrap_or("");
    let (name,why,implementation,step)=business(kind);
    Some(json!({
        "id":pair["id"],"title":name,"offer":pair["question"],
        "customer":"구매 신호에 등장하는 조직 또는 비용을 지출하는 실제 담당자(추가 확인 필요)",
        "whyPay":why,"implementationPlan":implementation,"firstPaymentStep":step,
        "firstTest":pair["nextProof"],"counterfactual":"두 자료의 시장·제품·시점이 다르거나 실제 유료 구매자가 없으면 이 가설은 폐기합니다.",
        "origin":"source_pair_draft","evidenceStatus":"two_independent_source_mentions_not_causality",
        "researchPriority":pair["researchPriority"],"status":"draft",
        "evidence":[evidence(a),evidence(b)],
        "sourceIds":[a["id"],b["id"]],
        "moneyMechanism":kind
    }))
}
fn from_signal(item:&Value)->Option<Value>{
    let tags=item["changeTags"].as_array()?.iter().filter_map(Value::as_str).collect::<Vec<_>>();
    let kind=if tags.contains(&"buyer_request"){"buyer_supply_spread"}
        else if tags.contains(&"cost_rise"){"automation_transfer"}
        else if tags.contains(&"shortage"){"supply_substitution"}
        else if tags.contains(&"policy"){"compliance_product"}
        else if tags.contains(&"cost_drop"){"input_substitution"}
        else {return None};
    let (name,why,implementation,step)=business(kind);
    let id=format!("lead:{}",item["id"].as_str()?);
    Some(json!({
        "id":id,"title":format!("{name} — 조사 후보"),
        "offer":format!("원문 '{}'에서 관측된 변화가 실제 유료 문제인지 조사",short(item,"title",170)),
        "customer":"원문에 등장하는 지출·구매 주체(실제 고객 미확인)",
        "whyPay":why,"implementationPlan":implementation,"firstPaymentStep":step,
        "firstTest":"현재 계약·단가·구매 담당자를 원문에서 우선 확인",
        "counterfactual":"기사의 언급이 실제 구매 의사가 아니거나 대체재가 이미 충분하면 폐기",
        "origin":"single_source_research_draft",
        "evidenceStatus":"single_source_headline_not_validated",
        "researchPriority":if tags.contains(&"buyer_request"){50}else{20},
        "status":"draft","evidence":[evidence(item)],
        "sourceIds":[item["id"]],"moneyMechanism":kind
    }))
}
fn shortlist(rows:&[Value],previous:&Value,rejected:&HashSet<String>)->(Vec<Value>,Vec<String>){
    let by_id=rows.iter().filter_map(|r|r["id"].as_str().map(|id|(id.to_string(),r.clone())))
        .collect::<HashMap<_,_>>();
    let mut pool=super::mechanism::discover(rows,24).iter()
        .filter_map(|p|from_pair(p,&by_id)).collect::<Vec<_>>();
    let mut singles=rows.iter().filter_map(from_signal).collect::<Vec<_>>();
    singles.sort_by(|a,b|b["researchPriority"].as_u64().cmp(&a["researchPriority"].as_u64()));
    pool.extend(singles);
    let mut seen=HashSet::<String>::new();
    let previous_ids=previous["shownCandidateIds"].as_array().into_iter().flatten()
        .filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>();
    let prior=previous_ids.iter().cloned().collect::<HashSet<_>>();
    let mut selected=Vec::new();
    let mut used_sources=HashSet::new();
    for unseen_only in [true,false] {
        for draft in &pool {
            let Some(id)=draft["id"].as_str()else{continue};
            if seen.contains(id)||rejected.contains(id)||(unseen_only&&prior.contains(id)){continue;}
            let ids=draft["sourceIds"].as_array().cloned().unwrap_or_default();
            // Avoid saturating one shortlist with the same news article.
            if ids.iter().any(|x|x.as_str().is_some_and(|s|used_sources.contains(s))){continue;}
            seen.insert(id.to_string());
            for source_id in ids.iter().filter_map(Value::as_str){used_sources.insert(source_id.to_string());}
            selected.push(draft.clone());
            if selected.len()==3 {break}
        }
        if selected.len()==3 {break}
    }
    let mut history=previous_ids;
    for item in &selected{
        if let Some(id)=item["id"].as_str(){
            if !prior.contains(id){history.push(id.to_owned());}
        }
    }
    if history.len()>128{history.drain(..history.len()-128);}
    (selected,history)
}
async fn snapshot(engine:&Arc<Engine>)->Result<Value,Fault>{
    let root=engine.settings.data_dir.clone();
    let mut report=blocking(move||super::latest(&root)).await?;
    if let Some(id)=report["reasoningRunId"].as_str().and_then(|s|Uuid::parse_str(s).ok()){
        if let Some(run)=engine.store.get(id).await {
            report["reasoningStatus"]=json!(run.status);
            report["inferenceCostKrw"]=json!(run.cost_krw);
            report["inferenceInputTokens"]=json!(run.input_tokens);
            report["inferenceOutputTokens"]=json!(run.output_tokens);
            if run.terminal() && report["reasoningFinalized"]!=true{
                let _lock=FIND_LOCK.lock().await;
                let root=engine.settings.data_dir.clone();
                let latest=blocking(move||super::latest(&root)).await?;
                if latest["reasoningRunId"]==json!(id.to_string()) && latest["reasoningFinalized"]!=true{
                    let mut updated=latest;
                    updated["reasoningFinalized"]=json!(true);
                    updated["reasoningStatus"]=json!(run.status);
                    let mut model_ideas=Vec::<Value>::new();
                    if run.status==RunStatus::Completed{
                        match parse_ideas(run.answer.as_deref().unwrap_or(""),&updated,id){
                            Ok(items) if !items.is_empty()=>{
                                updated["candidateIds"]=json!(items.iter()
                                    .filter_map(|v|v["id"].as_str()).collect::<Vec<_>>());
                                updated["message"]=json!("AI 추론 후보입니다. 수익·구매 의사는 별도 검증이 필요합니다.");
                                model_ideas=items;
                            },
                            Ok(_)=>updated["inferenceError"]=json!("근거를 충족한 AI 후보가 없습니다."),
                            Err(error)=>updated["inferenceError"]=json!(error)
                        }
                    }else{
                        updated["inferenceError"]=json!("AI 분석이 완료되지 않았습니다. 기존 DB 후보는 보존됐습니다.");
                    }
                    let root=engine.settings.data_dir.clone();
                    blocking(move||super::save_report(&root,&model_ideas,&updated)).await?;
                }
                let root=engine.settings.data_dir.clone();
                report=blocking(move||super::latest(&root)).await?;
            }
        }
    }
    Ok(report)
}
fn parse_ideas(raw:&str,report:&Value,run:Uuid)->Result<Vec<Value>,String>{
    let raw=raw.trim();
    let raw=raw.strip_prefix("&#96;&#96;&#96;json").unwrap_or(raw);
    let parsed:Value=serde_json::from_str(raw).map_err(|_|"AI 출력이 유효한 JSON 형식이 아닙니다.".to_string())?;
    let ideas=parsed["ideas"].as_array().ok_or("AI 출력에 ideas 배열이 없습니다.")?;
    let allowed=report["drafts"].as_array().into_iter().flatten()
        .flat_map(|d|d["evidence"].as_array().into_iter().flatten()).filter_map(|r|
            r["sourceId"].as_str().map(|id|(id.to_string(),r.clone()))
        ).collect::<HashMap<_,_>>();
    let mut out=Vec::new();
    for (i,row) in ideas.iter().take(3).enumerate(){
        let ids=row["sourceIds"].as_array().into_iter().flatten().filter_map(Value::as_str)
            .filter(|id|allowed.contains_key(*id)).collect::<Vec<_>>();
        if ids.is_empty(){continue;}
        let title=short(row,"title",180);
        let why=short(row,"whyPay",650);
        let plan=short(row,"implementationPlan",1000);
        let paid=short(row,"firstPaymentStep",650);
        if title.is_empty()||why.is_empty()||plan.is_empty()||paid.is_empty(){continue;}
        let originals=ids.iter().filter_map(|id|allowed.get(*id).cloned()).collect::<Vec<_>>();
        out.push(json!({
            "id":format!("model:{}:{}",run,i),
            "title":title,"customer":short(row,"customer",260),"offer":short(row,"offer",550),
            "whyPay":why,"implementationPlan":plan,"firstPaymentStep":paid,
            "counterfactual":short(row,"counterfactual",600),
            "evidence":originals,"sourceIds":ids,"origin":"model_hypothesis",
            "evidenceStatus":"unverified_model_hypothesis","status":"draft",
            "modelRunId":run,"moneyMechanism":short(row,"moneyMechanism",100)
        }));
    }
    Ok(out)
}
pub fn routes()->Router<Arc<Engine>>{
    Router::new()
        .route("/money/find",get(latest).post(find))
        .route("/money/stats",get(stats))
        .route("/money/collect",post(collect))
        .route("/money/automation",post(automation))
        .route("/money/inference/cancel",post(cancel))
        .route("/money/candidates/{id}/reject",post(reject))
}
pub async fn latest(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
    snapshot(&engine).await.map(Json)
}
pub async fn stats(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
    let root=engine.settings.data_dir.clone();
    blocking(move||super::stats(&root)).await.map(Json)
}
pub async fn collect(State(engine):State<Arc<Engine>>,Json(body):Json<Value>)
    ->Result<Json<Value>,Fault>{
    let report=super::sources::collect(&engine.settings.data_dir,
        body["force"].as_bool().unwrap_or(false)).await.map_err(internal)?;
    Ok(Json(report))
}
pub async fn automation(State(engine):State<Arc<Engine>>,Json(body):Json<Value>)
    ->Result<Json<Value>,Fault>{
    let enabled=body["enabled"].as_bool().ok_or_else(||err("enabled boolean required"))?;
    let record=json!({"enabled":enabled,"periodMinutes":30,
        "updatedAt":crate::frontend_state::timestamp(crate::domain::now_ms()),
        "modelCallsPerCycle":0});
    let root=engine.settings.data_dir.clone();
    let saved=record.clone();
    blocking(move||super::set_state(&root,"automation",&saved)).await?;
    if enabled{
        let root=engine.settings.data_dir.clone();
        tokio::spawn(async move{
            if let Err(error)=super::sources::collect(&root,false).await{
                tracing::warn!("money public feed collection failed: {error}");
            }
        });
    }
    Ok(Json(record))
}
pub async fn reject(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
    ->Result<Json<Value>,Fault>{
    let _lock=FIND_LOCK.lock().await;
    let root=engine.settings.data_dir.clone();
    blocking(move||{
        let Some(mut candidate)=super::candidate(&root,&id)? else{return Err("candidate not found".into())};
        candidate["status"]=json!("rejected");
        super::replace_candidate(&root,&candidate)?;
        Ok(candidate)
    }).await.map(Json)
}
pub async fn cancel(State(engine):State<Arc<Engine>>)->Result<Json<Value>,Fault>{
    let current=snapshot(&engine).await?;
    let id=current["reasoningRunId"].as_str().ok_or_else(||err("no inference run"))?;
    let id=Uuid::parse_str(id).map_err(|e|err(e.to_string()))?;
    let cancelled=engine.cancel(id).await.map_err(err)?;
    Ok(Json(json!({"id":id,"status":cancelled.status})))
}
pub async fn find(State(engine):State<Arc<Engine>>,Json(body):Json<Value>)
    ->Result<Json<Value>,Fault>{
    let _lock=FIND_LOCK.lock().await;
    let mode=body["mode"].as_str().unwrap_or("db");
    if !matches!(mode,"db"|"agent"){return Err(err("mode must be db or agent"));}
    let root=engine.settings.data_dir.clone();
    let prior=blocking({
        let root=root.clone();move||super::latest(&root)
    }).await?;
    if let Some(id)=prior["reasoningRunId"].as_str().and_then(|s|Uuid::parse_str(s).ok()){
        if engine.store.get(id).await.is_some_and(|run|!run.terminal()){
            return Ok(Json(prior));
        }
    }
    let mut records=blocking({
        let root=root.clone();move||super::signals(&root,180)
    }).await?;
    if records.is_empty(){
        // The first click acquires real evidence rather than inventing leads.
        super::sources::collect(&root,false).await.map_err(internal)?;
        records=blocking({
            let root=root.clone();move||super::signals(&root,180)
        }).await?;
    }
    let rejected=blocking({
        let root=root.clone();move||super::candidates(&root,100)
    }).await?.iter().filter(|v|v["status"]=="rejected")
        .filter_map(|v|v["id"].as_str().map(str::to_owned)).collect::<HashSet<_>>();
    let (drafts,history)=shortlist(&records,&prior,&rejected);
    let ids=drafts.iter().filter_map(|d|d["id"].as_str()).collect::<Vec<_>>();
    let mut report=json!({"candidateIds":ids,"shownCandidateIds":history,
        "sourceCount":records.len(),"modelCalls":0,"reasoningRunId":null,
        "searchedAt":crate::frontend_state::timestamp(crate::domain::now_ms()),
        "message":if drafts.is_empty(){"공개 자료에서 구매·비용 신호가 부족합니다. 다음 수집 후 다시 확인하세요."}
            else{"공개 근거로 만든 검증 전 사업 가설입니다. 구매자와 매출은 아직 입증되지 않았습니다."}});
    if mode=="agent" && !drafts.is_empty(){
        let agent_id=body["agentId"].as_str().unwrap_or("general");
        let agent=engine.frontend.agents().await.into_iter().find(|a|a.id==agent_id)
            .ok_or_else(||err("selected agent not found"))?;
        let budget=body["budgetKrw"].as_f64().unwrap_or(100.0);
        if !budget.is_finite()||!(0.0..=1_000_000.0).contains(&budget){
            return Err(err("invalid budgetKrw"));
        }
        let evidence=serde_json::to_string(&drafts).map_err(|e|internal(e.to_string()))?;
        let prompt=format!("You are evaluating possible commercial opportunities, NOT executing business actions.\n\
Only use the following archived, unverified source-backed leads:\n{evidence}\n\
Invent up to 3 distinct causal commercial hypotheses. Use an observed sourceId from the input for EVERY idea.\n\
State likely buyer, concrete offer, why that buyer might pay, implementation plan, first paid validation and a falsifying counterfactual.\n\
Never invent revenue figures, customer commitments, procurement deadline, or source URL.\n\
Return only valid JSON: {{\"ideas\":[{{\"title\":\"\",\"customer\":\"\",\"offer\":\"\",\"whyPay\":\"\",\"implementationPlan\":\"\",\"firstPaymentStep\":\"\",\"counterfactual\":\"\",\"moneyMechanism\":\"\",\"sourceIds\":[\"exact sourceId from evidence\"]}}]}}.");
        let sdk=Dominion::from_engine(Arc::clone(&engine));
        let run=sdk.prepare_run(CreateRun{instruction:prompt,budget_krw:Some(budget),max_steps:Some(1)},
            RunOptions{provider_id:agent.provider_id,model:agent.model,
                strategy_learning:false,model_only:true}).await.map_err(err)?;
        report["reasoningRunId"]=json!(run.id.to_string());
        report["reasoningStatus"]=json!("queued");
        report["reasoningFinalized"]=json!(false);
        report["modelCalls"]=json!(1);
        let chosen=report.clone();
        let root_clone=root.clone();
        blocking(move||super::save_report(&root_clone,&drafts,&chosen)).await?;
        if let Err(e)=engine.launch(run.id).await{
            let _=engine.store.mutate(run.id,|item|{
                item.status=RunStatus::Failed;
                item.event("money_launch_error",e.clone(),None);
                Ok(())
            }).await;
            return Err(err(e));
        }
    }else{
        blocking(move||super::save_report(&root,&drafts,&report)).await?;
    }
    snapshot(&engine).await.map(Json)
}

/// Zero-token source gathering is an operator opt-in background process. It
/// never starts a provider inference, performs a payment, or changes app code.
pub fn start_scheduler(engine:&Arc<Engine>){
    let weak=Arc::downgrade(engine);
    tokio::spawn(async move {
        let mut ticks=tokio::time::interval(std::time::Duration::from_secs(1800));
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop{
            ticks.tick().await;
            let Some(engine)=weak.upgrade()else{break;};
            let root=engine.settings.data_dir.clone();
            drop(engine);
            let check=root.clone();
            let enabled=tokio::task::spawn_blocking(move||
                super::state(&check,"automation").ok().flatten()
                    .is_some_and(|v|v["enabled"]==true)).await.unwrap_or(false);
            if enabled{
                if let Err(error)=super::sources::collect(&root,false).await{
                    tracing::warn!("money scheduled collection: {error}");
                }
            }
        }
    });
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test] fn shortlist_pins_independent_sources_and_never_claims_payment(){
    let a=super::super::intelligence::enrich_signal(json!({
        "id":"a","title":"New factory automation LLM released","description":"Open source AI tool",
        "url":"https://supplier.example/new","sourceName":"supplier","contentStatus":"headline_observed"}));
    let b=super::super::intelligence::enrich_signal(json!({
        "id":"b","title":"Factory procurement request for software automation","description":"Request for proposal factory",
        "url":"https://buyer.example/rfp","sourceName":"buyer","contentStatus":"headline_observed"}));
    let (items,_)=shortlist(&[a,b],&Value::Null,&HashSet::new());
    assert!(!items.is_empty());
    assert!(items.iter().all(|r|r["evidenceStatus"]!="paid_customer_verified"));
 }
}
