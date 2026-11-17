//! Public GitHub/Hugging Face market signals and dated attention metrics.
 //! Stars/downloads/likes measure platform attention, NOT customers or revenue.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{path::Path,time::Duration};
use tokio::sync::Mutex;

static CATALOG_LOCK:Mutex<()> = Mutex::const_new(());
fn id(text:&str)->String{format!("{:x}",Sha256::digest(text.as_bytes()))}
fn strval(row:&Value,key:&str,n:usize)->String{
    row[key].as_str().unwrap_or("").chars().take(n).collect()
}
fn at()->String{crate::frontend_state::timestamp(crate::domain::now_ms())}
fn metric(entity:&str,kind:&str,value:u64,url:&str,time:&str)->Value{
    let day=&time[..time.len().min(10)];
    json!({"id":format!("metric:{}",id(&format!("{entity}:{kind}:{day}"))),
        "entityId":entity,"metric":kind,"value":value,"url":url,
        "observedAt":time,"metricStatus":"platform_attention_not_customer_sales"})
}
fn github(source:&Value,time:&str,minimum_date:&str)->(Vec<Value>,Vec<Value>){
    let mut signals=Vec::new();
    let mut counters=Vec::new();
    for row in source["items"].as_array().into_iter().flatten().take(25){
        let name=strval(row,"full_name",170);
        let url=strval(row,"html_url",1000);
        if name.is_empty()||!url.starts_with("https://github.com/"){continue}
        let created=strval(row,"created_at",70);
        let mut signal=json!({"id":id(&url),
            "title":format!("GitHub repository: {name}"),
            "description":strval(row,"description",400),
            "url":url,"sourceName":"GitHub emerging repositories",
            "publishedAt":created,"capturedAt":time,
            "contentStatus":"platform_api_metadata",
            "verification":"platform_listing_not_customer_demand"});
        signal=super::intelligence::enrich_signal(signal);
        if created.get(..10).is_some_and(|d|d>=minimum_date){
            signal["changeTags"]=json!(["launch"]);
        }
        if let Some(stars)=row["stargazers_count"].as_u64(){
            counters.push(metric(&format!("github:{name}"),"stars",stars,&url,time));
        }
        signals.push(signal);
    }
    (signals,counters)
}
fn huggingface(source:&Value,time:&str,minimum_date:&str)->(Vec<Value>,Vec<Value>){
    let mut signals=Vec::new();
    let mut counters=Vec::new();
    for row in source.as_array().into_iter().flatten().take(25){
        let name=strval(row,"id",170);
        if name.is_empty()||name.contains("..")||name.starts_with('/'){continue;}
        let url=format!("https://huggingface.co/{name}");
        let created=strval(row,"createdAt",70);
        let mut signal=json!({"id":id(&url),
            "title":format!("Hugging Face model: {name}"),
            "description":format!("Public model listing. Task: {}",strval(row,"pipeline_tag",110)),
            "url":url,"sourceName":"Hugging Face trending models",
            "publishedAt":created,"capturedAt":time,
            "contentStatus":"platform_api_metadata",
            "verification":"platform_listing_not_customer_demand"});
        signal=super::intelligence::enrich_signal(signal);
        if created.get(..10).is_some_and(|d|d>=minimum_date){
            signal["changeTags"]=json!(["launch"]);
        }
        if let Some(downloads)=row["downloads"].as_u64(){
            counters.push(metric(&format!("hf:{name}"),"rolling_downloads",downloads,&url,time));
        }
        if let Some(likes)=row["likes"].as_u64(){
            counters.push(metric(&format!("hf:{name}"),"likes",likes,&url,time));
        }
        signals.push(signal);
    }
    (signals,counters)
}
async fn fetch_json(client:&reqwest::Client,url:&str)->Result<Value,String>{
    // Caller selects only the two hardcoded public API origins. No redirects.
    let mut response=client.get(url).send().await.map_err(|e|e.to_string())?;
    if !response.status().is_success(){return Err(format!("HTTP {}",response.status()));}
    let mut bytes=Vec::new();
    while let Some(chunk)=response.chunk().await.map_err(|e|e.to_string())?{
        if bytes.len()+chunk.len()>600_000{return Err("catalog payload over 600 KB".into());}
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|e|e.to_string())
}
pub async fn collect(root:&Path,force:bool)->Result<Value,String>{
    let _gate=CATALOG_LOCK.lock().await;
    let bucket=crate::domain::now_ms()/21_600_000;
    if !force{
        let prior=super::state(root,"catalog")?;
        if prior.as_ref().and_then(|v|v["bucket"].as_u64())==Some(bucket){
            return Ok(prior.unwrap());
        }
    }
    let now=at();
    let lookback=crate::frontend_state::timestamp(
        crate::domain::now_ms().saturating_sub(10*24*60*60*1000));
    let since=&lookback[..10];
    let github_url=format!("https://api.github.com/search/repositories?q=created:%3E%3D{since}+stars:%3E%3D20&sort=stars&order=desc&per_page=25");
    let hf_url="https://huggingface.co/api/models?sort=trendingScore&limit=25";
    let client=reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Dominion Money Research (public project signals)")
        .build().map_err(|e|e.to_string())?;
    let mut signals=Vec::new();
    let mut metrics=Vec::new();
    let mut errors=Vec::new();
    match fetch_json(&client,&github_url).await{
        Ok(payload)=>{
            let (mut rows,mut stats)=github(&payload,&now,since);
            signals.append(&mut rows);metrics.append(&mut stats);
        },
        Err(error)=>errors.push(format!("GitHub: {}",error.chars().take(180).collect::<String>()))
    }
    match fetch_json(&client,hf_url).await{
        Ok(payload)=>{
            let (mut rows,mut stats)=huggingface(&payload,&now,since);
            signals.append(&mut rows);metrics.append(&mut stats);
        },
        Err(error)=>errors.push(format!("Hugging Face: {}",error.chars().take(180).collect::<String>()))
    }
    if signals.is_empty(){return Err(format!("public catalog APIs unavailable: {}",json!(errors)))}
    let observed=signals.len();
    let new_signals=super::save_signals(root,&signals)?;
    let new_metrics=super::save_metrics(root,&metrics)?;
    let report=json!({"bucket":bucket,"observed":observed,
        "newSignals":new_signals,"newMetrics":new_metrics,
        "metricObservations":metrics.len(),"errors":errors,
        "capturedAt":now,"modelCalls":0});
    super::set_state(root,"catalog",&report)?;
    Ok(report)
}
#[cfg(test)]
mod tests {
 use super::*;
 #[test]fn catalogs_never_elevate_attention_to_revenue(){
    let data=json!({"items":[{"full_name":"test/new-agent","html_url":"https://github.com/test/new-agent",
        "created_at":"2026-11-12T00:00:00Z","stargazers_count":500,
        "description":"New open-source agent for factories"}]});
    let (signals,metrics)=github(&data,"2026-11-17T00:00:00Z","2026-11-07");
    assert_eq!(signals.len(),1);
    assert_eq!(metrics.len(),1);
    assert_eq!(metrics[0]["metricStatus"],"platform_attention_not_customer_sales");
    assert_eq!(signals[0]["verification"],"platform_listing_not_customer_demand");
 }
}
