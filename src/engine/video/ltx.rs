//! LTX 2.5 turnkey text-to-video connector (2026 V2 asynchronous API).
//! The API key never enters a job record or the browser. The original job
//! persists the remote id before polling; uncertain POSTs are not retried.
use super::*;
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

const ENDPOINT: &str = "https://api.ltx.io/v2/text-to-video";
const MAX_BYTES: usize = 256 * 1024 * 1024;

pub(super) fn resolution(aspect: &str, quality: &str) -> Result<&'static str, String> {
    match (quality, aspect) {
        ("720p", "16:9") => Ok("1280x720"),
        ("720p", "9:16") => Ok("720x1280"),
        ("720p", "1:1") => Ok("720x720"),
        ("1080p", "16:9") => Ok("1920x1080"),
        ("1080p", "9:16") => Ok("1080x1920"),
        ("1080p", "1:1") => Ok("1080x1080"),
        ("1440p", "16:9") => Ok("2560x1440"),
        ("1440p", "9:16") => Ok("1440x2560"),
        ("1440p", "1:1") => Ok("1440x1440"),
        ("4k", "16:9") => Ok("3840x2160"),
        ("4k", "9:16") => Ok("2160x3840"),
        ("4k", "1:1") => Ok("2160x2160"),
        _ => Err("영상 화질과 화면비를 확인하세요.".into()),
    }
}

/// LTX-2.5 Fast at 720p/1080p supports twenty-second clips at 24fps;
/// Pro and higher-resolution Fast clips are limited to ten seconds.
pub(super) fn valid_duration(model:&str,quality:&str,seconds:u64)->bool {
    let max=if model=="ltx-2-5-fast" && matches!(quality,"720p"|"1080p") {20} else {10};
    (6..=max).contains(&seconds) && seconds%2==0
}

/// Optional camera/subject guidance compiled locally, without an additional
/// language-model request or hidden prompt cost. Original content is retained.
pub(super) fn cinematic_prompt(raw:&str,style:&str)->Result<String,String>{
    let prefix=match style {
        "original"=>"",
        "cinematic"=>"Create a premium cinematic shot. Keep identities, wardrobe, lighting and setting consistent across any cuts. Use deliberate camera movement, detailed natural motion, precise lens framing and synchronized environmental audio. No watermarks or accidental text overlays. ",
        "product"=>"Create a high-end product commercial. Preserve exact product shape, markings and materials. Use clean studio lighting, realistic reflections, close-up texture details and deliberate smooth camera moves. Audio should fit the scene. ",
        "social"=>"Create a polished vertical social video. Strong first-second visual hook, coherent subject across shots, clear action and framing, dynamic but readable camera movement, crisp natural audio. Do not add unrequested captions. ",
        _=>return Err("지원하지 않는 영상 연출 프리셋입니다.".into()),
    };
    if raw.trim().is_empty(){return Err("영상 프롬프트가 비어 있습니다.".into());}
    let combined=if prefix.is_empty(){raw.trim().to_string()}else{format!("{prefix} Scene description: {}",raw.trim())};
    if combined.chars().count()>5_000{return Err("연출 지시를 포함한 전체 프롬프트가 5,000자를 초과했습니다.".into());}
    Ok(combined)
}

/// Do not follow arbitrary URLs returned by a compromised video endpoint.
fn safe_download_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw).map_err(|_| "영상 다운로드 주소 오류")?;
    let host = url.host_str().unwrap_or("");
    if url.scheme() != "https"
        || !url.username().is_empty() || url.password().is_some()
        || !matches!(url.port(), None | Some(443))
        || !(host == "storage.googleapis.com"
            || host.ends_with(".storage.googleapis.com")
            || host.ends_with(".googleusercontent.com")
            || host.ends_with(".cloudfront.net")
            || host.ends_with(".amazonaws.com")
            || host.ends_with(".ltx.io"))
    {
        return Err("영상 제공자의 다운로드 호스트가 허용되지 않았습니다.".into());
    }
    Ok(url)
}

pub(super) async fn render(ctx: &JobContext, input: &Value, dir: &Path) -> Result<Value, String> {
    let key = ctx.state.read_secret("video:ltx-api")
        .filter(|value| !value.trim().is_empty())
        .ok_or("LTX API 키가 설정되지 않았습니다.")?;
    let model = input["ltxModel"].as_str().unwrap_or("ltx-2-5-pro");
    if !matches!(model, "ltx-2-5-pro" | "ltx-2-5-fast") {
        return Err("지원하지 않는 LTX 모델입니다.".into());
    }
    let quality = input["ltxQuality"].as_str().unwrap_or("1080p");
    let aspect = input["aspectRatio"].as_str().unwrap_or("9:16");
    let size = resolution(aspect, quality)?;
    let duration = input["duration"].as_u64().ok_or("영상 길이가 없습니다.")?;
    if !valid_duration(model,quality,duration) {
        return Err("선택한 LTX 모델·화질에서 지원하지 않는 영상 길이입니다.".into());
    }
    let prompt=cinematic_prompt(input["brief"].as_str().unwrap_or(""),
        input["ltxStyle"].as_str().unwrap_or("cinematic"))?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(45))
        .build().map_err(|e| e.to_string())?;
    let body = json!({"model":model,"prompt":prompt,"duration":duration,
        "resolution":size,"fps":24,"generate_audio":true});
    // Exposure is the estimated amount submitted, NOT the actual charged
    // amount. Freeze it before the HTTP request and audit without credentials.
    let clip_quote=super::pricing::quote_durations(model,quality,&[duration])?;
    ctx.checkpoint(json!({"phase":"submitting","progress":5,
        "videoState":{"inFlight":true,"provider":"ltx","model":model,
            "estimatedAttemptKrw":clip_quote["estimatedKrw"],
            "priceVersion":clip_quote["priceVersion"],
            "fxAsOf":clip_quote["fxAsOf"],"actualBilledKrw":null}}))?;
    ctx.state.store.audit("video.paid_submit_attempt",json!({
        "jobId":ctx.job_id,"provider":"ltx","model":model,
        "duration":duration,"quality":quality,
        "estimatedAttemptKrw":clip_quote["estimatedKrw"],
        "priceVersion":clip_quote["priceVersion"],"actualBilledKrw":null
    }));
    // A lost HTTP response may already represent paid work: no blind replay.
    let response = client.post(ENDPOINT).bearer_auth(&key).json(&body).send().await
        .map_err(|_| "needs_review: LTX 제출 결과가 불확실합니다. 과금 기록을 확인하세요.")?;
    if !response.status().is_success() {
        let code = response.status();
        ctx.checkpoint(json!({"videoState":{"inFlight":false}}))?;
        return Err(format!("LTX 영상 생성 요청 거부: HTTP {code}"));
    }
    let submitted: Value = response.json().await
        .map_err(|_| "needs_review: LTX 작업 ID를 확인할 수 없습니다.")?;
    let remote_id = submitted["id"].as_str()
        .filter(|id| !id.is_empty() && id.len() <= 128
            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
        .ok_or("needs_review: LTX 작업 ID가 올바르지 않습니다.")?;
    ctx.checkpoint(json!({"phase":"rendering","progress":15,
        "videoState":{"inFlight":true,"provider":"ltx","remoteId":remote_id}}))?;

    let started = std::time::Instant::now();
    let mut failures = 0u8;
    let video_url = loop {
        ctx.checkpoint(json!({}))?;
        if started.elapsed() >= Duration::from_secs(1800) {
            return Err("needs_review: LTX 원격 작업이 아직 진행 중입니다. 원격 ID를 확인하세요.".into());
        }
        tokio::time::sleep(Duration::from_secs(6)).await;
        let response = client.get(format!("{ENDPOINT}/{remote_id}")).bearer_auth(&key).send().await;
        let response = match response {
            Ok(response) => response,
            Err(_) => {
                failures += 1;
                if failures > 5 { return Err("needs_review: LTX 작업 상태 확인이 중단됐습니다.".into()); }
                continue;
            }
        };
        if response.status().is_server_error()
            || response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            failures += 1;
            if failures > 5 { return Err("needs_review: LTX 작업 상태 조회가 반복 실패했습니다.".into()); }
            continue;
        }
        if !response.status().is_success() {
            return Err(format!("needs_review: LTX 작업 상태 조회 HTTP {}",response.status()));
        }
        let status: Value = response.json().await
            .map_err(|_| "needs_review: LTX 상태 응답 해석 실패")?;
        failures = 0;
        match status["status"].as_str().unwrap_or("") {
            "pending" => ctx.checkpoint(json!({"phase":"queued","progress":20}))?,
            "processing" => ctx.checkpoint(json!({"phase":"rendering","progress":50}))?,
            "failed" => {
                ctx.checkpoint(json!({"videoState":{"inFlight":false}}))?;
                let code = status["error"]["type"].as_str().unwrap_or("generation_failed");
                return Err(format!("LTX 영상 생성 실패: {code}"));
            }
            "completed" => {
                let url = status["result"]["video_url"].as_str()
                    .ok_or("needs_review: LTX 완료 결과에 영상 주소가 없습니다.")?;
                break safe_download_url(url)?;
            }
            _ => return Err("needs_review: 알 수 없는 LTX 원격 작업 상태입니다.".into()),
        }
    };
    ctx.checkpoint(json!({"phase":"downloading","progress":80}))?;
    let response = client.get(video_url).timeout(Duration::from_secs(180)).send().await
        .map_err(|_| "needs_review: LTX 영상 다운로드 연결 실패")?;
    if !response.status().is_success() {
        return Err(format!("needs_review: LTX 영상 다운로드 HTTP {}", response.status()));
    }
    if response.content_length().is_some_and(|n| n > MAX_BYTES as u64) {
        return Err("생성 영상이 256MiB 제한을 초과했습니다.".into());
    }
    let destination = dir.join("output.part.mp4");
    let mut file = tokio::fs::File::create(&destination).await.map_err(|e|e.to_string())?;
    let mut stream = response.bytes_stream();
    let mut total = 0usize;
    while let Some(chunk) = stream.next().await {
        ctx.checkpoint(json!({}))?;
        let chunk = chunk.map_err(|_| "needs_review: 영상 다운로드가 중단되었습니다.")?;
        total = total.saturating_add(chunk.len());
        if total > MAX_BYTES { return Err("영상 다운로드 크기 제한을 초과했습니다.".into()); }
        file.write_all(&chunk).await.map_err(|e|e.to_string())?;
    }
    file.flush().await.map_err(|e|e.to_string())?;
    drop(file);
    // Reject JSON error payloads masquerading as a successful video download.
    let mut check = std::fs::File::open(&destination).map_err(|e|e.to_string())?;
    let mut header = [0u8; 12];
    use std::io::Read;
    check.read_exact(&mut header).map_err(|_| "LTX 영상이 비어 있습니다.")?;
    if &header[4..8] != b"ftyp" { return Err("LTX 결과가 MP4 파일이 아닙니다.".into()); }
    drop(check);
    std::fs::rename(destination, dir.join("output.mp4")).map_err(|e|e.to_string())?;
    ctx.checkpoint(json!({"videoState":{"inFlight":false,"remoteId":remote_id}}))?;
    Ok(json!({"engine":"ltx","provider":"LTX 2.5","model":model,
        "remoteId":remote_id,"resolution":size,"duration":duration,"audio":"generated",
        "bytes":total}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn three_aspect_resolutions_are_explicit() {
        assert_eq!(resolution("9:16","1080p").unwrap(),"1080x1920");
        assert_eq!(resolution("1:1","4k").unwrap(),"2160x2160");
        assert!(resolution("2:1","1080p").is_err());
    }
    #[test]
    fn style_prompt_preserves_owner_content_and_validates_budget(){
        let raw="은빛 바다 위를 선회하는 드론";
        let result=cinematic_prompt(raw,"cinematic").unwrap();
        assert!(result.contains(raw));
        assert_eq!(cinematic_prompt(raw,"original").unwrap(),raw);
        assert!(cinematic_prompt(raw,"unknown").is_err());
    }
    #[test]
    fn fast_supports_twenty_seconds_only_at_lower_resolutions() {
        assert!(valid_duration("ltx-2-5-fast","1080p",20));
        assert!(!valid_duration("ltx-2-5-pro","1080p",20));
        assert!(!valid_duration("ltx-2-5-fast","4k",20));
        assert!(!valid_duration("ltx-2-5-fast","720p",7));
    }
    #[test]
    fn signed_media_urls_require_trusted_https_hosts() {
        assert!(safe_download_url("https://storage.googleapis.com/a.mp4?sig=abc").is_ok());
        assert!(safe_download_url("http://storage.googleapis.com/a").is_err());
        assert!(safe_download_url("https://127.0.0.1/a").is_err());
        assert!(safe_download_url("https://storage.googleapis.com.evil.io/a").is_err());
    }
}
