//! Gemini Omni Flash video via the current Interactions API (2026).
//! Background interaction IDs are durable. Uncertain billed submissions are
//! never replayed, and the Google API key is never sent to a redirect host.
use super::{progress, JobContext};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
use base64::Engine as _;

const BASE: &str = "https://generativelanguage.googleapis.com/v1beta";
const MAX_BYTES: usize = 256 * 1024 * 1024;

fn interaction_id(id: &str) -> Result<&str, String> {
    if id.starts_with("v1_") && id.len() < 256
        && id.chars().all(|ch|ch.is_ascii_alphanumeric() || matches!(ch,'_'|'-'))
    {Ok(id)} else {Err("Omni 상호작용 ID 오류".into())}
}

fn download_url(raw: &str) -> Result<reqwest::Url, String> {
    let url=reqwest::Url::parse(raw).map_err(|_|"Omni 영상 URL 오류")?;
    if url.scheme()!="https" || url.username()!="" || url.password().is_some()
        || url.host_str()!=Some("generativelanguage.googleapis.com")
        || !url.path().starts_with("/v1beta/files/")
    { return Err("허용되지 않는 영상 다운로드 호스트".into()); }
    Ok(url)
}

fn video_content(result: &Value) -> Option<&Value> {
    if result["output_video"].is_object() {return Some(&result["output_video"]);}
    result["steps"].as_array()?.iter().rev().filter(|step|step["type"]=="model_output")
        .flat_map(|step| step["content"].as_array().into_iter().flatten())
        .find(|content|content["type"]=="video" && content["mime_type"]
            .as_str().unwrap_or("video/mp4")=="video/mp4")
}

async fn provider_response(response: reqwest::Response) -> Result<Value,String> {
    if !response.status().is_success(){
        return Err(format!("Gemini Omni API가 요청을 거부했습니다 (HTTP {})",response.status()));
    }
    if response.content_length().is_some_and(|size|size>400*1024*1024){
        return Err("영상 응답이 최대 크기를 초과합니다.".into());
    }
    response.json().await.map_err(|_|"Gemini Omni API 응답 해석 오류".into())
}

async fn save_video(ctx:&JobContext,client:&reqwest::Client,key:&str,output:&Value,dir:&Path)
    -> Result<usize,String> {
    progress(ctx,"downloading",90)?;
    let part=dir.join("output.part.mp4");
    if let Some(data)=output["data"].as_str(){
        if data.len()>MAX_BYTES*4/3+8{return Err("인라인 MP4 크기 초과".into());}
        let bytes=base64::engine::general_purpose::STANDARD.decode(data)
            .map_err(|_|"Omni 인라인 MP4 디코딩 오류")?;
        if bytes.len()<100||bytes.len()>MAX_BYTES{return Err("Omni MP4 크기 오류".into());}
        tokio::fs::write(&part,&bytes).await.map_err(|e|e.to_string())?;
        tokio::fs::rename(part,dir.join("output.mp4")).await.map_err(|e|e.to_string())?;
        return Ok(bytes.len());
    }
    let raw=output["uri"].as_str().ok_or("Omni 결과에 MP4 데이터나 URI가 없습니다.")?;
    let response=client.get(download_url(raw)?).header("x-goog-api-key",key)
        .timeout(Duration::from_secs(120)).send().await
        .map_err(|_|"Omni MP4 다운로드 연결 실패")?;
    let response=if response.status().is_redirection(){
        let loc=response.headers().get(reqwest::header::LOCATION)
            .and_then(|header|header.to_str().ok()).ok_or("Omni 리다이렉트 위치 없음")?;
        let target=reqwest::Url::parse(loc).map_err(|_|"Omni 리다이렉트 URL 오류")?;
        let host=target.host_str().unwrap_or("");
        if target.scheme()!="https" || target.username()!="" || target.password().is_some()
            || !(host=="storage.googleapis.com" || host.ends_with(".googleusercontent.com")
                || host.ends_with(".googleapis.com")){
            return Err("Omni MP4 다운로드 외부 호스트를 차단했습니다.".into());
        }
        client.get(target).timeout(Duration::from_secs(120)).send().await
            .map_err(|_|"Omni 서명된 영상 다운로드 실패")?
    } else {response};
    if !response.status().is_success(){return Err(format!("Omni MP4 HTTP {}",response.status()));}
    if response.content_length().is_some_and(|bytes|bytes>MAX_BYTES as u64) {
        return Err("Omni 영상 보관 크기 초과".into());
    }
    let mut out=tokio::fs::File::create(&part).await.map_err(|e|e.to_string())?;
    let mut size=0usize;
    let mut stream=response.bytes_stream();
    while let Some(chunk)=stream.next().await {
        ctx.checkpoint(json!({}))?;
        let chunk=chunk.map_err(|_|"Omni MP4 스트림 중단")?;
        size=size.checked_add(chunk.len()).ok_or("Omni MP4 크기 초과")?;
        if size>MAX_BYTES{return Err("Omni 영상이 256MiB를 초과했습니다.".into());}
        out.write_all(&chunk).await.map_err(|e|e.to_string())?;
    }
    out.flush().await.map_err(|e|e.to_string())?; drop(out);
    if size<100{return Err("Omni가 빈 MP4를 반환했습니다.".into());}
    tokio::fs::rename(part,dir.join("output.mp4")).await.map_err(|e|e.to_string())?;
    Ok(size)
}

pub(super) async fn render(ctx:&JobContext,job:&Value,dir:&Path)->Result<Value,String>{
    let key=ctx.state.read_secret("video:api").filter(|value|!value.trim().is_empty())
        .ok_or("Gemini API 키를 영상 설정에서 등록하세요.")?;
    let input=&job["input"];
    let model=input["config"]["omniModel"].as_str().unwrap_or("gemini-omni-1.2-flash");
    if !matches!(model,"gemini-omni-1.2-flash"|"gemini-omni-1.1-flash"){
        return Err("지원하지 않는 Gemini Omni 모델입니다.".into());
    }
    let aspect=input["aspectRatio"].as_str().unwrap_or("16:9");
    if !matches!(aspect,"9:16"|"16:9"){return Err("Omni는 9:16 또는 16:9만 지원합니다.".into());}
    let resolution=input["config"]["omniResolution"].as_str().unwrap_or("1080p");
    if !matches!(resolution,"360p"|"720p"|"1080p"|"4k"){
        return Err("Omni 출력 화질 오류".into());
    }
    let client=reqwest::Client::builder().redirect(reqwest::redirect::Policy::none())
        .build().map_err(|e|e.to_string())?;
    let saved=job["videoState"]["interactionId"].as_str();
    let (id,mut result)=if let Some(id)=saved {
        (interaction_id(id)?.to_owned(),Value::Null)
    } else {
        if job["videoState"]["submissionStarted"]==true {
            return Err("needs_review: 기존 유료 생성 요청을 확인하기 전에는 자동으로 재요청하지 않습니다.".into());
        }
        progress(ctx,"submitting",4)?;
        ctx.checkpoint(json!({"phase":"submitting","progress":5,
            "videoState":{"inFlight":true,"submissionStarted":true,
            "provider":"gemini-omni","interactionId":null}}))?;
        let prompt=input["brief"].as_str().unwrap_or("");
        let body=json!({
            "model":model,"input":prompt,"background":true,"store":true,"stream":false,
            "response_format":{"type":"video","aspect_ratio":aspect,
                "resolution":resolution,"delivery":"uri"}
        });
        let response=client.post(format!("{BASE}/interactions"))
            .header("x-goog-api-key",&key).json(&body)
            .timeout(Duration::from_secs(60)).send().await
            .map_err(|_|"needs_review: Omni 생성 요청 접수 상태 확인 필요")?;
        if !response.status().is_success(){
            let status=response.status();
            ctx.checkpoint(json!({"videoState":{"inFlight":false}}))?;
            return Err(format!("Omni 영상 생성 거부 (HTTP {status})"));
        }
        let value=provider_response(response).await
            .map_err(|_|"needs_review: Omni 생성 응답 식별자 확인 필요")?;
        let id=interaction_id(value["id"].as_str()
            .ok_or("needs_review: Omni 상호작용 ID 없음")?)?.to_owned();
        ctx.checkpoint(json!({"phase":"generating","progress":12,
            "videoState":{"inFlight":true,"submissionStarted":true,
            "provider":"gemini-omni","interactionId":id,"model":model}}))?;
        (id,value)
    };
    for tick in 0..90u64 {
        if result["status"]=="completed"{break;}
        if matches!(result["status"].as_str(),Some("failed"|"cancelled"|"incomplete")){
            ctx.checkpoint(json!({"videoState":{"inFlight":false}}))?;
            return Err(format!("Omni 영상 작업 종료: {}",result["status"]));
        }
        ctx.checkpoint(json!({"phase":"generating","progress":(12+tick).min(84)}))?;
        let response=client.get(format!("{BASE}/interactions/{}",interaction_id(&id)?))
            .header("x-goog-api-key",&key).timeout(Duration::from_secs(30))
            .send().await.map_err(|_|"needs_review: Omni 영상 작업 조회 실패")?;
        result=provider_response(response).await?;
        if result["status"]=="completed"{break;}
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
    if result["status"]!="completed"{
        return Err("needs_review: Omni 영상 작업이 대기 시간을 초과했습니다.".into());
    }
    let output=video_content(&result)
        .ok_or("Omni 응답에 영상 파일 데이터가 없습니다.")?;
    let bytes=save_video(ctx,&client,&key,output,dir).await?;
    ctx.checkpoint(json!({"phase":"finalizing","progress":98,
        "videoState":{"inFlight":false,"interactionId":id}}))?;
    Ok(json!({"engine":"omni","model":model,"bytes":bytes,
        "aspectRatio":aspect,"resolution":resolution,"interactionId":id}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn video_reference_path_is_private_to_google() {
        assert!(download_url("https://generativelanguage.googleapis.com/v1beta/files/x:download?alt=media").is_ok());
        assert!(download_url("https://evil.example/v1beta/files/x").is_err());
        assert!(interaction_id("../other").is_err());
    }
}
