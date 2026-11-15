//! Original main ModelsPage API backed by real configured model routes.
//! No guessed catalogs, shared token leakage or fake provider test success.
use crate::{
    config::ModelSettings,engine::Engine,
    provider_registry::{self,Draft,Provider},
};
use axum::{
    extract::{Path,State},http::StatusCode,routing::{get,post},Json,Router
};
use serde::Deserialize;
use serde_json::{json,Value};
use std::{sync::Arc,time::Duration};
type Fault=(StatusCode,Json<Value>);
fn fail(code:StatusCode,message:impl Into<String>)->Fault{
    (code,Json(json!({"error":message.into()})))
}
fn bad(message:impl Into<String>)->Fault{fail(StatusCode::BAD_REQUEST,message)}
fn missing()->Fault{fail(StatusCode::NOT_FOUND,"provider not found")}
fn unsupported(message:&str)->Fault{fail(StatusCode::NOT_IMPLEMENTED,message)}
pub fn routes()->Router<Arc<Engine>>{
    Router::new()
        .route("/providers",get(all).post(save))
        .route("/providers/cli",get(cli))
        .route("/providers/catalog",get(catalog))
        .route("/providers/free-catalog",get(free_catalog))
        .route("/providers/{id}",axum::routing::delete(remove))
        .route("/providers/{id}/models",get(models))
        .route("/providers/{id}/test",post(test))
}
pub fn builtin(engine:&Engine)->Value{
    let override_model=crate::frontend_local_models::selected(&engine.settings);
    let local_settings=override_model.as_deref().and_then(|name|
        crate::frontend_local_models::native_override(&engine.settings,name));
    let url=local_settings.as_ref().map(|s|&s.base_url)
        .unwrap_or(&engine.settings.main.base_url);
    let local=local_settings.is_some()||
        url.contains("127.0.0.1")||url.contains("localhost");
    let mut clean=reqwest::Url::parse(url).ok();
    if let Some(ref mut url)=clean{
        let _=url.set_username("");
        let _=url.set_password(None);
        url.set_fragment(None);url.set_query(None);
    }
    json!({
        "id":"dominion-native","name":"Dominion Native",
        "baseUrl":clean.map_or_else(String::new,|url|url.to_string().trim_end_matches('/').to_owned()),
        "kind":"openai","local":local,"enabled":local||!engine.settings.main.api_key.is_empty(),
        "hasKey":local||!engine.settings.main.api_key.is_empty(),
        "model":local_settings.as_ref().map(|s|s.name.as_str())
            .unwrap_or(&engine.settings.main.name),"reasoningEffort":"","reflection":false,
        "pricing":"environment_configured",
    })
}
pub async fn all(State(engine):State<Arc<Engine>>)->Json<Value>{
    let mut providers=vec![builtin(&engine)];
    providers.extend(engine.providers.list().iter().map(provider_registry::public));
    Json(Value::Array(providers))
}
async fn cli()->Json<Value>{Json(json!([]))}
async fn catalog(State(engine):State<Arc<Engine>>)->Json<Value>{
    let mut providers=vec![json!({
        "id":"dominion-native","name":"Dominion Native",
        "api":engine.settings.main.base_url,"models":1
    })];
    providers.extend(engine.providers.list().iter().map(|p|json!({
        "id":p.id,"name":p.name,"api":p.base_url,
        "models":usize::from(!p.model.is_empty())
    })));
    Json(json!({"providers":providers,"cached":false}))
}
async fn free_catalog()->Json<Value>{
    Json(json!({"providers":[],"status":"not_assessed",
        "counts":{"free":0,"paid":0,"unknown":0}}))
}
async fn save(State(engine):State<Arc<Engine>>,Json(body):Json<Draft>)
    ->Result<Json<Value>,Fault>{
    if body.id.as_deref()==Some("dominion-native"){
        return Err(bad("native provider is configured by the server owner, not overwritten by the UI"));
    }
    if body.reflection{
        return Err(unsupported("Provider-specific reflection model routing is not configured; disabling it silently would be misleading."));
    }
    if let Some(id)=body.id.as_deref(){
        // Prevent invalidating in-progress runs, still-bound agents or chat
        // sessions by changing a provider's actual model endpoint.
        let before=engine.providers.get(id).ok_or_else(missing)?;
        let changed=before.base_url.trim_end_matches('/')!=body.base_url.trim_end_matches('/')
            || before.model!=body.model.trim()
            || before.kind!=body.kind;
        if changed && engine.store.list().await.iter().any(|run|
                run.provider_id==id&&!run.terminal()){
            return Err(fail(StatusCode::CONFLICT,"provider is used by a live native run; complete or cancel it first"));
        }
        if changed && (engine.frontend.agents().await.iter().any(|a|a.provider_id==id)
            ||engine.frontend.sessions().await.iter().any(|s|s.provider_id==id)){
            return Err(fail(StatusCode::CONFLICT,"provider is still bound to agent or chat profiles; move them before changing its model"));
        }
    }
    let provider=engine.providers.upsert(body).map_err(bad)?;
    Ok(Json(provider_registry::public(&provider)))
}
async fn remove(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
    ->Result<Json<Value>,Fault>{
    if id=="dominion-native"{return Err(bad("native server model cannot be removed from the UI"));}
    if engine.frontend.agents().await.iter().any(|a|a.provider_id==id)
        ||engine.frontend.sessions().await.iter().any(|s|s.provider_id==id)
        ||engine.store.list().await.iter().any(|r|r.provider_id==id&&!r.terminal()){
        return Err(fail(StatusCode::CONFLICT,"provider is in use; migrate agents, sessions or active runs first"));
    }
    engine.providers.delete(&id).map_err(bad)?;
    Ok(Json(json!({"ok":true})))
}
fn active<'a>(engine:&'a Engine,id:&str)->Result<(ModelSettings,Option<Provider>),Fault>{
    if id=="dominion-native"{
        if let Some(model)=crate::frontend_local_models::selected(&engine.settings){
            if let Some(settings)=crate::frontend_local_models::native_override(&engine.settings,&model){
                return Ok((settings,None));
            }
        }
        return Ok((engine.settings.main.clone(),None));
    }
    let item=engine.providers.get(id).ok_or_else(missing)?;
    let settings=engine.providers.resolve(id,&item.model,&engine.settings.main).map_err(bad)?;
    Ok((settings,Some(item)))
}
async fn fetch_models(settings:&ModelSettings,protocol:&str)->Result<Vec<Value>,String>{
    let client=reqwest::Client::builder().timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::none())
        .build().map_err(|e|e.to_string())?;
    let url=format!("{}/models",settings.base_url.trim_end_matches('/'));
    let mut req=client.get(url);
    if protocol=="anthropic"{
        req=req.header("anthropic-version","2023-06-01");
        if !settings.api_key.is_empty(){req=req.header("x-api-key",&settings.api_key);}
    }else if !settings.api_key.is_empty(){
        req=req.bearer_auth(&settings.api_key);
    }
    let response=req.send().await.map_err(|e|e.to_string())?;
    if !response.status().is_success(){
        return Err(format!("provider models returned HTTP {}",response.status()));
    }
    let value=response.json::<Value>().await.map_err(|e|e.to_string())?;
    let list=value.get("data").and_then(Value::as_array)
        .or_else(||value.get("models").and_then(Value::as_array))
        .ok_or("model catalog response contains no models array")?;
    let mut ids=std::collections::HashSet::new();
    Ok(list.iter().filter_map(|item|{
        let id=item.as_str().or_else(||item.get("id").and_then(Value::as_str))
            .or_else(||item.get("name").and_then(Value::as_str))?;
        if id.is_empty()||!ids.insert(id.to_owned()){return None;}
        Some(json!({"id":id,"name":id}))
    }).take(1000).collect())
}
async fn models(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
    ->Result<Json<Value>,Fault>{
    let (settings,selected)=active(&engine,&id)?;
    let protocol=selected.as_ref().map(|p|p.kind.as_str()).unwrap_or("openai");
    let list=match fetch_models(&settings,protocol).await{
        Ok(models)=>models,
        Err(error)=>{
            // An explicitly configured model is not a live-discovered model.
            // Preserve it with a truthful stale marker and a diagnostic.
            if settings.name.is_empty(){return Err(fail(StatusCode::BAD_GATEWAY,error));}
            vec![json!({"id":settings.name,"catalogStale":true,"catalogError":error})]
        }
    };
    Ok(Json(Value::Array(list)))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct Probe{
    #[serde(default)]base_url:Option<String>,
    #[serde(default)]kind:Option<String>,
    #[serde(default)]api_key:Option<String>,
    #[serde(default)]model:Option<String>,
}
async fn test(State(engine):State<Arc<Engine>>,Path(id):Path<String>,
    Json(body):Json<Probe>)->Result<Json<Value>,Fault>{
    let (mut settings,record) = if id=="new"{
        let url=body.base_url.as_deref().ok_or_else(||bad("provider test requires a URL"))?;
        let (base_url,_)=provider_registry::normalized_url(url).map_err(bad)?;
        let kind=body.kind.as_deref().unwrap_or("openai");
        if !matches!(kind,"openai"|"ollama"|"anthropic"){
            return Err(unsupported("This provider protocol has no native model adapter"));
        }
        let mut settings=engine.settings.main.clone();
        settings.base_url=base_url;settings.name=body.model.as_deref()
            .filter(|x|!x.is_empty()).ok_or_else(||bad("select a model before testing"))?.into();
        settings.api_key=body.api_key.clone().unwrap_or_default();
        settings.input_krw_per_million=0.0;
        settings.cached_input_krw_per_million=0.0;
        settings.output_krw_per_million=0.0;
        settings.cache_write_krw_per_million=0.0;
        (settings,None)
    }else{active(&engine,&id)?};
    if id!="new"{
        if let Some(url)=body.base_url.as_deref(){
            let (normalized,_)=provider_registry::normalized_url(url).map_err(bad)?;
            if normalized!=settings.base_url{
                return Err(bad("tested URL differs from the saved native provider"));
            }
        }
        if body.model.as_deref().is_some_and(|v|v!=settings.name){
            return Err(bad("tested model differs from the saved native provider"));
        }
        if let Some(key)=body.api_key.filter(|s|!s.is_empty()){settings.api_key=key;}
    }
    let protocol=if id=="new"{body.kind.as_deref().unwrap_or("openai")}
        else{record.as_ref().map(|p|p.kind.as_str()).unwrap_or("openai")};
    let model=engine.model.with_primary_protocol(settings,protocol).map_err(bad)?;
    let response=model.complete(&[json!({"role":"user",
        "content":"Reply with the single word OK."})],&[],false).await;
    Ok(Json(match response{
        Ok(reply)=>json!({"ok":true,"message":format!(
            "Model returned {} output tokens",reply.output_tokens
        ),"model":reply.model}),
        Err(error)=>json!({"ok":false,"message":error}),
    }))
}
