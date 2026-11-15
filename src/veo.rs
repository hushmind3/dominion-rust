//! Optional Gemini Veo 3.1 video generation using Google's OpenAI-compatible
//! asynchronous videos API. Remote billing is outside the LLM token meter.
use reqwest::{Client, Url};
use serde_json::{json, Value};
use std::{env, net::IpAddr, path::Path, time::Duration};
use tokio::{fs, io::AsyncWriteExt};

const VIDEO_SIZE_LIMIT: usize = 160 * 1024 * 1024;

pub struct Veo {
    client: Client,
    api_key: String,
    base: String,
    model: String,
    billed_krw_per_second: f64,
}
fn allowed(url:&Url,localhost_ok:bool)->bool{
    let Some(host)=url.host_str() else{return false;};
    let local=host.eq_ignore_ascii_case("localhost") || host.parse::<IpAddr>().is_ok_and(|v|v.is_loopback());
    (url.scheme()=="https" && host=="generativelanguage.googleapis.com") ||
        (localhost_ok && url.scheme()=="http" && local)
}
fn validate_base(raw:&str)->Result<String,String>{
    let url=Url::parse(raw).map_err(|e|e.to_string())?;
    if !allowed(&url,true) || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some(){
        return Err("Veo base URL must be HTTPS Google Gemini API or local test endpoint".into());
    }
    Ok(raw.trim_end_matches('/').to_string())
}
impl Veo {
    pub fn from_env()->Result<Self,String>{
        let api_key=env::var("DOMINION_VEO_API_KEY").unwrap_or_default();
        if api_key.is_empty(){return Err("Set DOMINION_VEO_API_KEY before selecting generative video".into());}
        let base=validate_base(&env::var("DOMINION_VEO_BASE_URL")
            .unwrap_or_else(|_|"https://generativelanguage.googleapis.com/v1beta/openai".into()))?;
        let model=env::var("DOMINION_VEO_MODEL").unwrap_or_else(|_|"veo-3.1-generate-preview".into());
        if model.is_empty()||model.len()>100||!model.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'.'|b'_')){
            return Err("invalid Veo model name".into());
        }
        let billed_krw_per_second=env::var("DOMINION_VEO_KRW_PER_SECOND").ok()
            .and_then(|s|s.parse::<f64>().ok()).unwrap_or(0.0);
        if !billed_krw_per_second.is_finite()||billed_krw_per_second<0.0||billed_krw_per_second>1_000_000.0{
            return Err("invalid DOMINION_VEO_KRW_PER_SECOND".into());
        }
        let client=Client::builder().timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .build().map_err(|e|e.to_string())?;
        Ok(Self{client,api_key,base,model,billed_krw_per_second})
    }
    pub fn estimate_krw(&self,seconds:u32)->f64{seconds as f64*self.billed_krw_per_second}
    pub async fn start(&self,prompt:&str,seconds:u32)->Result<String,String>{
        if !matches!(seconds,4|6|8){return Err("Veo supports only 4, 6, 8 second clips".into());}
        let form=reqwest::multipart::Form::new()
            .text("model",self.model.clone())
            .text("prompt",prompt.to_string())
            .text("duration_seconds",seconds.to_string());
        let response=self.client.post(format!("{}/videos",self.base))
            .bearer_auth(&self.api_key).multipart(form).send().await
            .map_err(|e|format!("Veo start transport: {e}"))?;
        let status=response.status();
        let data:Value=response.json().await.map_err(|e|format!("Veo start JSON: {e}"))?;
        if !status.is_success(){return Err(format!("Veo start HTTP {status}: {}",data.to_string().chars().take(450).collect::<String>()));}
        let id=data.get("id").and_then(Value::as_str).ok_or("Veo response missing id")?;
        if id.len()>512||id.is_empty(){return Err("invalid Veo operation id".into());}
        Ok(id.into())
    }
    pub async fn wait(&self,id:&str)->Result<String,String>{
        if id.is_empty()||id.len()>512||id.contains('/')||id.contains('?')||id.contains('#'){
            return Err("invalid video operation id".into());
        }
        let path=format!("{}/videos/{}",self.base,id);
        for _ in 0..90 {
            let response=self.client.get(&path).bearer_auth(&self.api_key)
                .send().await.map_err(|e|format!("Veo status transport: {e}"))?;
            let status=response.status();
            let data:Value=response.json().await.map_err(|e|format!("Veo status JSON: {e}"))?;
            if !status.is_success(){return Err(format!("Veo status HTTP {status}: {}",data.to_string().chars().take(450).collect::<String>()));}
            match data.get("status").and_then(Value::as_str).unwrap_or("") {
                "completed"=> {
                    let url=data.get("url").and_then(Value::as_str).ok_or("Veo completed without video URL")?;
                    return Ok(url.into());
                },
                "failed"|"cancelled"=>return Err(format!("Veo failed: {}",data.get("error").unwrap_or(&Value::Null))),
                "queued"|"processing"|"in_progress"|"running"=>{},
                unknown=>return Err(format!("unrecognized Veo state: {unknown}")),
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
        Err("Veo generation did not complete within 15 minutes".into())
    }
    pub async fn download(&self,video_uri:&str,path:&Path)->Result<(),String>{
        let url=Url::parse(video_uri).map_err(|e|format!("invalid video URL: {e}"))?;
        let base=Url::parse(&self.base).map_err(|e|e.to_string())?;
        // Never send a provider credential to an attacker-supplied video URL.
        if !allowed(&url,base.host_str().is_some_and(|h|h=="127.0.0.1"||h=="localhost")){return Err("Veo download URL host not authorized".into());}
        let mut response=self.client.get(url).bearer_auth(&self.api_key).send().await.map_err(|e|e.to_string())?;
        if response.status().is_redirection(){
            let dest=response.headers().get(reqwest::header::LOCATION).and_then(|v|v.to_str().ok()).ok_or("redirect without location")?;
            let parsed=Url::parse(dest).map_err(|e|e.to_string())?;
            let host=parsed.host_str().unwrap_or("");
            // Signed Google storage link; never forward the Bearer token.
            if parsed.scheme()!="https" || !(host=="storage.googleapis.com" || host.ends_with(".storage.googleapis.com")
                    || host=="generativelanguage.googleapis.com"){
                return Err("Veo redirect destination is not permitted".into());
            }
            response=self.client.get(parsed).send().await.map_err(|e|e.to_string())?;
        }
        if !response.status().is_success(){return Err(format!("Veo download HTTP {}",response.status()));}
        let temp=path.with_extension("download");
        let mut out=fs::File::create(&temp).await.map_err(|e|e.to_string())?;
        let mut size=0usize;
        while let Some(chunk)=response.chunk().await.map_err(|e|e.to_string())? {
            size=size.checked_add(chunk.len()).ok_or("video size overflow")?;
            if size>VIDEO_SIZE_LIMIT{return Err("remote MP4 exceeds 160 MiB".into());}
            out.write_all(&chunk).await.map_err(|e|e.to_string())?;
        }
        out.flush().await.map_err(|e|e.to_string())?;
        if size<1024{return Err("Veo download unexpectedly small".into());}
        fs::rename(&temp,path).await.map_err(|e|e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_provider_origin(){
        assert!(validate_base("https://generativelanguage.googleapis.com/v1beta/openai").is_ok());
        assert!(validate_base("https://untrusted.example.com/fake").is_err());
        assert!(validate_base("http://127.0.0.1:8080/v1beta/openai").is_ok());
    }
    #[tokio::test]
    async fn asynchronous_operation_retrieval_mock(){
        use axum::{routing::{get,post},Json,Router};
        async fn start()->Json<Value>{Json(json!({"id":"job_1","status":"processing"}))}
        async fn poll()->Json<Value>{Json(json!({"id":"job_1","status":"completed","url":"http://127.0.0.1:1234/video"}))}
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr=listener.local_addr().unwrap();
        let server=tokio::spawn(async move{
            axum::serve(listener,Router::new().route("/v1beta/openai/videos",post(start))
                .route("/v1beta/openai/videos/{id}",get(poll))).await.unwrap();
        });
        let client=Client::new();
        let veo=Veo{client,api_key:"test".into(),base:format!("http://{addr}/v1beta/openai"),
            model:"veo-3.1-generate-preview".into(),billed_krw_per_second:20.0};
        let id=veo.start("A scene",4).await.unwrap();
        let url=veo.wait(&id).await.unwrap();
        assert_eq!(url,"http://127.0.0.1:1234/video");
        assert_eq!(veo.estimate_krw(4),80.0);
        server.abort();
    }
}
