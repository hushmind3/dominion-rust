//! Provider-neutral, side-effect-free goal admission and owner-visible routing.
//! This selects a *configured* model endpoint; it never calls the provider,
//! starts a job, changes permissions, or guesses a provider bill.
use crate::state::AppState;
use serde_json::{json,Value};

fn field<'a>(value:&'a Value,key:&str)->&'a str{
    value.get(key).and_then(Value::as_str).unwrap_or("").trim()
}
fn active(value:&Value)->bool{value["enabled"]!=false}
fn configured(provider:&Value,model:&str)->bool{
    !field(provider,"id").is_empty() && active(provider) && !model.is_empty()
}
fn category(goal:&str)->&'static str{
    let text=goal.to_lowercase();
    if ["코드","버그","프로그래밍","소스","리포지토리","저장소","컴파일","개발",
        "coding","source code","repository","typescript","rust","python",
        "compile","pull request","fix tests","test suite"].iter().any(|w|text.contains(w)){
        "coding"
    }else if ["영상","동영상","렌더","자막","비디오","video","render","clip"].iter()
        .any(|w|text.contains(w)){
        "video"
    }else if ["조사","검색","자료","분석","연구","비교","research","search",
        "analyze","investigate"].iter().any(|w|text.contains(w)){
        "research"
    }else{"general"}
}
fn affinity(agent:&Value,kind:&str,goal:&str)->usize{
    let mut descriptor=String::new();
    for key in ["name","description","role","systemPrompt"]{
        if let Some(s)=agent.get(key).and_then(Value::as_str){
            descriptor.push_str(&s.chars().take(200).collect::<String>());
            descriptor.push(' ');
        }
    }
    // Aliases supplied by the owner are strong opt-in routing signals.
    if let Some(items)=agent["aliases"].as_array(){
        for item in items.iter().take(20){
            if let Some(s)=item.as_str(){descriptor.push_str(s);descriptor.push(' ');}
        }
    }
    let d=descriptor.to_lowercase();
    let patterns:&[&str]=match kind {
        "coding"=>&["코드","개발","프로그래밍","엔지니어","coding","developer","programming","software","coder"],
        "video"=>&["영상","비디오","편집","video","editing","filmmaker","film"],
        "research"=>&["조사","연구","분석","검색","research","analysis","analyst"],
        _=>&[],
    };
    let mut score=patterns.iter().filter(|w|d.contains(**w)).count()*4;
    // Long task-specific words from the owner's request can disambiguate
    // several similarly capable agents without asking a second model.
    for token in goal.to_lowercase().split(|c:char|!c.is_alphanumeric()){
        if token.chars().count()>=4 && d.contains(token){score+=1;}
    }
    score
}

/// Owner preview and actual goal creation call this identical deterministic
/// selector. A preview is advisory; a concurrent provider change is checked
/// again at enqueue, which never silently uses a stale removed endpoint.
pub fn prepare(state:&AppState,body:&Value)->Result<Value,String>{
    let goal=field(body,"goal");
    if goal.is_empty()||goal.chars().count()>12_000 {
        return Err("목표를 1~12000자로 입력하세요.".into());
    }
    let kind=category(goal);
    let providers=state.store.all("provider");
    let agents=state.store.all("agent");
    let mut ranked=Vec::<(usize,usize,Value,Value)>::new();
    for (index,agent) in agents.iter().enumerate(){
        if !active(agent){continue;}
        let provider_id=field(agent,"providerId");
        let Some(provider)=providers.iter().find(|p|field(p,"id")==provider_id)else{continue;};
        let model=if field(agent,"model").is_empty(){field(provider,"model")}
            else{field(agent,"model")};
        if !configured(provider,model){continue;}
        ranked.push((affinity(agent,kind,goal),index,agent.clone(),provider.clone()));
    }
    ranked.sort_by(|a,b|b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let (executor,source,agent_name,provider_name)=if let Some((_,_,agent,provider))=ranked.first(){
        let model=if field(agent,"model").is_empty(){field(provider,"model")}else{field(agent,"model")};
        (json!({"agentId":field(agent,"id"),"providerId":field(provider,"id"),"model":model}),
            "registered_agent",field(agent,"name").to_owned(),field(provider,"name").to_owned())
    }else{
        // If the user has no ready agent, the registered provider itself is
        // sufficient. Local configured routes are preferred before cloud ones;
        // neither is assumed to be free or remotely reachable.
        let mut choices=providers.iter().filter(|p|configured(p,field(p,"model")))
            .collect::<Vec<_>>();
        choices.sort_by_key(|p|{
            let local=matches!(field(p,"kind"),"ollama"|"llamacpp"|"llama"|"mlx"|"local");
            if local{0}else{1}
        });
        if let Some(provider)=choices.first(){
            (json!({"providerId":field(provider,"id"),"model":field(provider,"model")}),
                "configured_provider",String::new(),field(provider,"name").to_owned())
        }else{
            return Ok(json!({"ready":false,"taskKind":kind,
                "message":"사용 가능한 모델이 없습니다. 모델 메뉴에서 로컬 모델이나 제공자를 먼저 연결하세요.",
                "nextPage":"models","createsJob":false,"estimatedCostKrw":null}));
        }
    };
    Ok(json!({"ready":true,"taskKind":kind,"executor":executor,
        "selection":{"source":source,"agentName":agent_name,
            "providerName":provider_name,"model":executor["model"]},
        "recommendedWorktree":kind=="coding",
        "requiresChargeConfirmation":true,
        "estimatedCostKrw":null,
        "message":"설정된 모델을 선택했습니다. 실제 연결 및 요금은 실행 과정에서 확인됩니다.",
        "createsJob":false}))
}
