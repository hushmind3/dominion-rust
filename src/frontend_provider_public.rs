//! Public model/provider discovery for the main-branch UI.
//! One models.dev document is shared by provider listing and per-provider
//! model browsing. Reading metadata never opens a credential or sends keys.
use serde_json::{json,Value};
use std::sync::{Arc,Mutex,OnceLock};
use std::time::{Duration,Instant};

const MAX_PUBLIC_DOCUMENT:usize=24*1024*1024;
const MODELS_DEV_TTL:Duration=Duration::from_secs(86_400);
const OPENROUTER_TTL:Duration=Duration::from_secs(21_600);

static MODELS_DEV_DOCUMENT:OnceLock<Mutex<Option<(Instant,Arc<Value>)>>>=OnceLock::new();
static MODELS_DEV_FETCH:tokio::sync::Mutex<()>=tokio::sync::Mutex::const_new(());
static OPENROUTER_DOCUMENT:OnceLock<Mutex<Option<(Instant,Arc<Value>)>>>=OnceLock::new();
static OPENROUTER_FETCH:tokio::sync::Mutex<()>=tokio::sync::Mutex::const_new(());

fn snapshot(slot:&'static OnceLock<Mutex<Option<(Instant,Arc<Value>)>>>,
    ttl:Option<Duration>)->Option<Arc<Value>>{
    let current=slot.get_or_init(||Mutex::new(None));
    let guard=current.lock().unwrap_or_else(|e|e.into_inner());
    guard.as_ref().and_then(|(at,value)|{
        if ttl.is_some_and(|ttl|at.elapsed()>=ttl){None}
        else{Some(Arc::clone(value))}
    })
}
fn publish(slot:&'static OnceLock<Mutex<Option<(Instant,Arc<Value>)>>>,value:Arc<Value>){
    let current=slot.get_or_init(||Mutex::new(None));
    *current.lock().unwrap_or_else(|e|e.into_inner())=Some((Instant::now(),value));
}
async fn bounded_public_json(url:&str)->Result<Value,String>{
    let url=reqwest::Url::parse(url).map_err(|error|error.to_string())?;
    crate::frontend_public_http::read_json(url,
        crate::frontend_public_http::ReadOptions{
            timeout:Duration::from_secs(20),max_bytes:MAX_PUBLIC_DOCUMENT,
            status_label:"public metadata endpoint returned HTTP",
            too_large:"public metadata exceeds 24 MiB limit",
            invalid_json:"invalid public model metadata"
        }).await
}
fn cached_models_dev_document()->Option<Arc<Value>>{
    snapshot(&MODELS_DEV_DOCUMENT,Some(MODELS_DEV_TTL))
}
async fn load_models_dev_document()->Result<(Arc<Value>,bool),String>{
    if let Some(document)=cached_models_dev_document(){return Ok((document,true));}
    let _fetch_guard=MODELS_DEV_FETCH.lock().await;
    if let Some(document)=cached_models_dev_document(){return Ok((document,true));}
    let data=bounded_public_json("https://models.dev/api.json").await?;
    if !data.is_object(){return Err("models.dev returned no provider map".into());}
    let document=Arc::new(data);
    publish(&MODELS_DEV_DOCUMENT,Arc::clone(&document));
    Ok((document,false))
}
fn public_models_from_metadata(provider:&Value)->Vec<Value>{
    let mut models=provider["models"].as_object().into_iter().flatten()
        .filter_map(|(key,model)|{
            let id=model["id"].as_str().filter(|s|!s.is_empty())
                .unwrap_or(key.as_str());
            if id.is_empty(){return None;}
            Some(json!({
                "id":id,
                "name":model["name"].as_str().filter(|s|!s.is_empty()).unwrap_or(id),
                "context":model["limit"]["context"].as_u64(),
                "reasoning":model["reasoning"].as_bool(),
                "toolCall":model["tool_call"].as_bool()
            }))
        }).collect::<Vec<_>>();
    models.sort_by(|a,b|a["name"].as_str().unwrap_or("")
        .cmp(b["name"].as_str().unwrap_or("")));
    models.truncate(1500);
    models
}
fn models_for_endpoint(document:&Value,base:&str)->Option<Vec<Value>>{
    let target=base.trim().trim_end_matches('/');
    if target.is_empty(){return None;}
    let provider=document.as_object()?.values()
        .filter(|record|record["api"].as_str().is_some_and(|api|
            api.trim().trim_end_matches('/').eq_ignore_ascii_case(target)))
        .max_by_key(|record|record["models"].as_object()
            .map_or(0,|models|models.len()))?;
    let mut models=public_models_from_metadata(provider);
    if models.is_empty(){return None;}
    for row in &mut models{
        row["free"]=Value::Null;
        row["freeStatus"]=json!("unknown");
        row["catalogSource"]=json!("models.dev");
    }
    Some(models)
}
/// Read-only public browsing for a registered endpoint with no API key.
pub async fn public_models_for_endpoint(base:&str)->Result<Option<Vec<Value>>,String>{
    let document=load_models_dev_document().await?.0;
    Ok(models_for_endpoint(&document,base))
}
pub async fn provider_catalog()->axum::Json<Value>{
    let response=load_models_dev_document().await;
    let (document,cached,error)=match response{
        Ok((doc,cached))=>(Some(doc),cached,None),
        Err(error)=>(snapshot(&MODELS_DEV_DOCUMENT,None),true,Some(error))
    };
    let Some(document)=document else{
        return axum::Json(json!({"providers":[],"cached":false,
            "error":error.unwrap_or_else(||"public catalog unavailable".into())}));
    };
    let mut providers=document.as_object().into_iter().flatten()
        .filter_map(|(id,item)|{
            let api=item["api"].as_str().filter(|s|!s.is_empty())?;
            let models=item["models"].as_object().map_or(0,|v|v.len());
            if models==0{return None;}
            Some(json!({"id":id,"name":item["name"].as_str().unwrap_or(id),
                "api":api,"models":models,"env":item["env"],"doc":item["doc"]}))
        }).collect::<Vec<_>>();
    providers.sort_by(|a,b|a["name"].as_str().unwrap_or("")
        .cmp(b["name"].as_str().unwrap_or("")));
    providers.truncate(300);
    axum::Json(json!({"providers":providers,"cached":cached,"error":error}))
}
pub async fn provider_models(axum::extract::Path(provider_id):
    axum::extract::Path<String>)->axum::Json<Value>{
    if provider_id.is_empty()||provider_id.len()>128
        ||!provider_id.bytes().all(|byte|byte.is_ascii_alphanumeric()
            ||matches!(byte,b'-'|b'_'|b'.')){
        return axum::Json(json!({"providerId":provider_id,
            "models":[],"error":"invalid provider ID"}));
    }
    let id=provider_id.to_ascii_lowercase();
    let response=load_models_dev_document().await;
    let (document,cached,error)=match response{
        Ok((doc,cached))=>(Some(doc),cached,None),
        Err(error)=>(snapshot(&MODELS_DEV_DOCUMENT,None),true,Some(error))
    };
    let models=document.as_ref().and_then(|doc|doc.get(&id))
        .map(public_models_from_metadata).unwrap_or_default();
    let error=error.or_else(||if document.as_ref().is_some_and(|doc|doc.get(&id).is_none()){
        Some("public provider not found".to_owned())
    }else{None});
    axum::Json(json!({"providerId":id,"models":models,"cached":cached,"error":error}))
}
fn public_openrouter_free_models(body:&Value)->Vec<Value>{
    let mut models=body["data"].as_array().into_iter().flatten()
        .filter_map(|item|{
            let id=item["id"].as_str()?;
            if !id.ends_with(":free"){return None;}
            let zero=|field:&str|item["pricing"][field].as_str()
                .and_then(|s|s.parse::<f64>().ok())
                .is_some_and(|v|v==0.0);
            if !zero("prompt")||!zero("completion"){return None;}
            if item["architecture"]["output_modalities"].as_array()
                .is_some_and(|modes|!modes.iter().any(|mode|mode.as_str()==Some("text"))){
                return None;
            }
            Some(json!({"id":id,
                "name":item["name"].as_str().unwrap_or(id),
                "context":item["context_length"].as_u64(),
                "free":true,"freeStatus":"free",
                "freeSource":"openrouter_public_pricing"}))
        }).collect::<Vec<_>>();
    models.sort_by(|a,b|a["name"].as_str().unwrap_or("")
        .cmp(b["name"].as_str().unwrap_or("")));
    models.truncate(800);
    models
}
pub async fn openrouter_free_catalog()->axum::Json<Value>{
    let cached=snapshot(&OPENROUTER_DOCUMENT,Some(OPENROUTER_TTL));
    if let Some(document)=cached{
        return axum::Json(json!({"models":public_openrouter_free_models(&document),
            "cached":true}));
    }
    let _guard=OPENROUTER_FETCH.lock().await;
    if let Some(document)=snapshot(&OPENROUTER_DOCUMENT,Some(OPENROUTER_TTL)){
        return axum::Json(json!({"models":public_openrouter_free_models(&document),
            "cached":true}));
    }
    match bounded_public_json("https://openrouter.ai/api/v1/models").await{
        Ok(doc)=>{
            let models=public_openrouter_free_models(&doc);
            publish(&OPENROUTER_DOCUMENT,Arc::new(doc));
            axum::Json(json!({"models":models,"cached":false}))
        }
        Err(error)=>{
            let stale=snapshot(&OPENROUTER_DOCUMENT,None);
            axum::Json(json!({"models":stale.as_ref()
                .map(|doc|public_openrouter_free_models(doc))
                .unwrap_or_default(),"cached":stale.is_some(),"error":error}))
        }
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn provider_model_listing_deduplicates_one_shared_document_without_keys(){
        let doc=json!({
            "sample":{"api":"https://api.example.test/v1","models":{
                "z":{"name":"Zed","limit":{"context":100000},"tool_call":true},
                "a":{"name":"Alpha","reasoning":false}}},
            "other":{"api":"https://example.test/v2","models":{}}
        });
        let models=public_models_from_metadata(&doc["sample"]);
        assert_eq!(models.len(),2);
        assert_eq!(models[0]["id"],"a");
        let scoped=models_for_endpoint(&doc,"https://api.example.test/v1/").unwrap();
        assert_eq!(scoped[0]["catalogSource"],"models.dev");
        assert_eq!(scoped[0]["freeStatus"],"unknown");
        assert!(models_for_endpoint(&doc,"https://api.example.test").is_none());
    }
    #[test]
    fn free_models_require_explicit_suffix_and_zero_prices(){
        let doc=json!({"data":[
            {"id":"real:free","name":"Real","pricing":{"prompt":"0","completion":"0"},
                "architecture":{"output_modalities":["text"]}},
            {"id":"paid:free","pricing":{"prompt":"0.1","completion":"0"}},
            {"id":"promo:free-trial","pricing":{"prompt":"0","completion":"0"}},
            {"id":"image:free","pricing":{"prompt":"0","completion":"0"},
                "architecture":{"output_modalities":["image"]}}
        ]});
        let free=public_openrouter_free_models(&doc);
        assert_eq!(free.len(),1);
        assert_eq!(free[0]["id"],"real:free");
        assert_eq!(free[0]["freeSource"],"openrouter_public_pricing");
    }
}
