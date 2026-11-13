//! Direct Gemini Veo 3.1 video rendering. Never replay an uncertain paid request.
use super::{progress, JobContext};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
const BASE: &str = "https://generativelanguage.googleapis.com/v1beta";
const MAX_BYTES: usize = 256 * 1024 * 1024;

fn operation_path(name: &str) -> Result<&str, String> {
    if name.starts_with("models/") && name.contains("/operations/")
        && name.len() < 256 && !name.contains("..")
        && name.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '_' | '.'))
    { Ok(name) } else { Err("Veo 작업 ID 형식이 유효하지 않습니다.".into()) }
}

fn file_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw).map_err(|_| "Veo 다운로드 주소 오류")?;
    if url.scheme() != "https" || url.username() != "" || url.password().is_some()
        || url.host_str() != Some("generativelanguage.googleapis.com")
        || !url.path().starts_with("/v1beta/files/")
    { return Err("Veo 다운로드 호스트가 신뢰되지 않습니다.".into()); }
    Ok(url)
}

async fn json_response(response: reqwest::Response) -> Result<Value, String> {
    if !response.status().is_success() {
        return Err(format!("Veo HTTP {} 오류", response.status()));
    }
    response.json().await.map_err(|_| "Veo 응답 JSON 오류".into())
}

async fn download(ctx: &JobContext, client: &reqwest::Client, key: &str,
    url: &str, dir: &Path) -> Result<usize, String> {
    let response = client.get(file_url(url)?).header("x-goog-api-key", key)
        .timeout(Duration::from_secs(120)).send().await
        .map_err(|_| "Veo 다운로드 연결 실패")?;
    // Do not forward the API key across redirects. Only Google-signed URLs.
    let response = if response.status().is_redirection() {
        let location = response.headers().get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok()).ok_or("다운로드 위치 없음")?;
        let target = reqwest::Url::parse(location).map_err(|_| "다운로드 위치 오류")?;
        let host = target.host_str().unwrap_or("");
        if target.scheme() != "https" || target.username() != "" || target.password().is_some()
            || !(host == "storage.googleapis.com" || host.ends_with(".googleusercontent.com")
                || host.ends_with(".googleapis.com"))
        { return Err("Veo 다운로드 리다이렉트 차단".into()); }
        client.get(target).timeout(Duration::from_secs(120)).send().await
            .map_err(|_| "Veo 리다이렉트 다운로드 실패")?
    } else { response };
    if !response.status().is_success() { return Err(format!("Veo MP4 HTTP {}",response.status())); }
    if response.content_length().is_some_and(|n| n > MAX_BYTES as u64) {
        return Err("Veo MP4 크기 초과".into());
    }
    progress(ctx, "downloading", 90)?;
    let partial = dir.join("output.part.mp4");
    let mut out = tokio::fs::File::create(&partial).await.map_err(|e| e.to_string())?;
    let mut total = 0usize;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        ctx.checkpoint(json!({}))?;
        let chunk = chunk.map_err(|_| "Veo MP4 스트림 중단")?;
        total = total.checked_add(chunk.len()).ok_or("Veo MP4 크기 초과")?;
        if total > MAX_BYTES { return Err("Veo MP4 256MiB 제한".into()); }
        out.write_all(&chunk).await.map_err(|e| e.to_string())?;
    }
    out.flush().await.map_err(|e|e.to_string())?;
    drop(out);
    if total < 100 { return Err("Veo가 빈 MP4를 반환했습니다.".into()); }
    tokio::fs::rename(partial,dir.join("output.mp4")).await.map_err(|e|e.to_string())?;
    Ok(total)
}

pub(super) async fn render(ctx: &JobContext, job: &Value, dir: &Path) -> Result<Value, String> {
    let key=ctx.state.read_secret("video:api")
        .filter(|value| !value.trim().is_empty()).ok_or("Gemini 영상 API 키를 설정하세요.")?;
    let input=&job["input"];
    let model=input["config"]["veoModel"].as_str().unwrap_or("veo-3.1-generate-preview");
    if !matches!(model, "veo-3.1-generate-preview" | "veo-3.1-fast-generate-preview") {
        return Err("Veo 모델 선택이 유효하지 않습니다.".into());
    }
    let aspect=input["aspectRatio"].as_str().unwrap_or("16:9");
    if !matches!(aspect,"9:16"|"16:9") {return Err("Veo는 9:16 또는 16:9만 지원합니다.".into());}
    let client=reqwest::Client::builder().redirect(reqwest::redirect::Policy::none())
        .build().map_err(|e|e.to_string())?;
    let operation=if let Some(saved)=job["videoState"]["operationName"].as_str() {
        operation_path(saved)?.to_string()
    } else {
        if job["videoState"]["submissionStarted"]==true {
            return Err("needs_review: 과금 가능성이 있는 이전 Veo 요청을 중복 전송하지 않습니다.".into());
        }
        progress(ctx,"submitting",4)?;
        ctx.checkpoint(json!({"phase":"submitting","progress":5,
            "videoState":{"inFlight":true,"submissionStarted":true,"operationName":null}}))?;
        let params=json!({"instances":[{"prompt":input["brief"]}],
            "parameters":{"aspectRatio":aspect,"durationSeconds":8,
                "resolution":input["config"]["veoResolution"].as_str().unwrap_or("1080p")}});
        let response=client.post(format!("{BASE}/models/{model}:predictLongRunning"))
            .header("x-goog-api-key",&key).json(&params)
            .timeout(Duration::from_secs(60)).send().await
            .map_err(|_|"needs_review: Veo 요청 접수 여부 확인 필요")?;
        if !response.status().is_success() {
            let status=response.status();
            ctx.checkpoint(json!({"videoState":{"inFlight":false,
                "submissionStarted":true,"operationName":null}}))?;
            return Err(format!("Veo 영상 요청이 거부되었습니다 (HTTP {status})."));
        }
        let response=json_response(response).await
            .map_err(|_|"needs_review: Veo 생성 응답 검토 필요")?;
        let name=operation_path(response["name"].as_str()
            .ok_or("needs_review: Veo 작업 ID가 없습니다.")?)?.to_string();
        ctx.checkpoint(json!({"phase":"generating","progress":12,
            "videoState":{"inFlight":true,"submissionStarted":true,
                "operationName":name,"model":model}}))?;
        name
    };
    let mut done=None;
    for tick in 0..90u64 {
        ctx.checkpoint(json!({"phase":"generating","progress":(12+tick).min(84)}))?;
        let response=client.get(format!("{BASE}/{operation_path(&operation)?}"))
            .header("x-goog-api-key",&key).timeout(Duration::from_secs(30))
            .send().await.map_err(|_|"needs_review: Veo 상태 조회 실패")?;
        let status=json_response(response).await?;
        if status["done"]==true {done=Some(status);break;}
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
    let result=done.ok_or("needs_review: Veo 작업이 대기 제한을 초과했습니다.")?;
    if result["error"].is_object() {
        ctx.checkpoint(json!({"videoState":{"inFlight":false,
            "submissionStarted":true,"operationName":operation}}))?;
        return Err(format!("Veo 생성 오류 코드 {}",result["error"]["code"]));
    }
    let url=result.pointer("/response/generateVideoResponse/generatedSamples/0/video/uri")
        .and_then(Value::as_str).ok_or("Veo 응답에 MP4가 없습니다.")?;
    let bytes=download(ctx,&client,&key,url,dir).await?;
    ctx.checkpoint(json!({"phase":"finalizing","progress":98,
        "videoState":{"inFlight":false,"operationName":operation}}))?;
    Ok(json!({"engine":"veo","model":model,"duration":8,"bytes":bytes,
        "aspectRatio":aspect,"operationName":operation}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_urls_are_restricted() {
        assert!(file_url("https://generativelanguage.googleapis.com/v1beta/files/a:download?alt=media").is_ok());
        assert!(file_url("http://localhost/v1beta/files/a").is_err());
        assert!(file_url("https://evil.example/v1beta/files/a").is_err());
        assert!(operation_path("../operations/x").is_err());
    }
}
