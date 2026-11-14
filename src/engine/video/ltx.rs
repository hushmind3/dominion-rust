//! Hosted cinematic generation. No GPU, workflow JSON, or local model required.
//! LTX V2 is asynchronous: the submission is a non-idempotent paid side effect.
//! Uncertain submission and interrupted remote work must never auto-replay.
use crate::{engine::jobs::JobContext, state::AppState};
use serde_json::{json, Value};
use std::{io::Read, path::Path, time::{Duration, Instant}};

const API: &str = "https://api.ltx.io/v2/text-to-video";
const MAX_BYTES: usize = 256 * 1024 * 1024;

pub(super) fn ready(state: &AppState) -> bool {
    state.store.has_secret("video:ltx:api")
}

/// One authoritative 2026 LTX 2.5 model/resolution/frame-rate matrix.
pub(super) fn maximum_duration(model:&str,quality:&str,fps:u64)->Result<u64,String> {
    if !matches!(fps,24|25|48|50) {
        return Err("LTX 2.5는 24·25·48·50 FPS만 지원합니다.".into());
    }
    if !matches!(quality,"720p"|"1080p"|"1440p"|"4k") {
        return Err("LTX 해상도를 확인하세요.".into());
    }
    match model {
        "ltx-2-5-pro" => Ok(10),
        "ltx-2-5-fast" if matches!(quality,"1440p"|"4k") || fps>=48 => Ok(10),
        "ltx-2-5-fast" => Ok(20),
        _ => Err("지원하지 않는 LTX 모델입니다.".into()),
    }
}

/// Exact model capabilities and dimensions as published in November 2026.
pub(super) fn spec(input: &Value, cfg: &Value) -> Result<Value, String> {
    let model = input["videoModel"].as_str().or(cfg["videoModel"].as_str())
        .unwrap_or("ltx-2-5-pro");
    if !matches!(model, "ltx-2-5-pro" | "ltx-2-5-fast") {
        return Err("지원하지 않는 LTX 모델입니다.".into());
    }
    let duration = input["duration"].as_u64()
        .or(cfg["duration"].as_u64()).unwrap_or(8);
    let aspect = input["aspectRatio"].as_str()
        .or(cfg["aspectRatio"].as_str()).unwrap_or("9:16");
    let quality = input["videoQuality"].as_str()
        .or(cfg["videoQuality"].as_str()).unwrap_or("1080p");
    let (landscape, portrait, square) = match quality {
        "720p" => ("1280x720", "720x1280", "720x720"),
        "1080p" => ("1920x1080", "1080x1920", "1080x1080"),
        "1440p" => ("2560x1440", "1440x2560", "1440x1440"),
        "4k" => ("3840x2160", "2160x3840", "2160x2160"),
        _ => return Err("해상도는 720p, 1080p, 1440p, 4k 중 선택하세요.".into()),
    };
    let resolution = match aspect {
        "16:9" => landscape,
        "9:16" => portrait,
        "1:1" => square,
        _ => return Err("지원하지 않는 영상 비율입니다.".into()),
    };
    let fps = input["fps"].as_u64().unwrap_or(24);
    let max_duration = maximum_duration(model,quality,fps)?;
    if duration < 6 || duration > max_duration || duration % 2 != 0 {
        return Err(format!(
            "선택한 LTX 모델·해상도·FPS 조합은 6~{max_duration}초의 짝수 길이를 지원합니다."
        ));
    }
    let generate_audio = input["generateAudio"].as_bool().unwrap_or(true);
    let prompt = input["brief"].as_str().unwrap_or("").trim();
    if prompt.is_empty() || prompt.chars().count() > 5000 {
        return Err("LTX 영상 프롬프트는 1~5,000자입니다.".into());
    }
    Ok(json!({"model":model,"duration":duration,"resolution":resolution,
        "fps":fps,"generate_audio":generate_audio,"prompt":prompt,
        "aspectRatio":aspect,"quality":quality}))
}

// Provider-supplied signed URLs are not instructions to access arbitrary hosts.
// These storage origins are the LTX-documented Google Cloud example and CDN domains.
fn trusted_download(url: &reqwest::Url) -> bool {
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let Some(host) = url.host_str() else { return false; };
    let host = host.to_ascii_lowercase();
    host == "storage.googleapis.com"
        || host == "cdn.ltx.io"
        || host.ends_with(".ltx.io")
        || host.ends_with(".cloudfront.net")
}

async fn download(client: &reqwest::Client, url: &str, dir: &Path, ctx: &JobContext)
    -> Result<usize, String> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;
    let parsed = reqwest::Url::parse(url).map_err(|_| "영상 다운로드 주소 오류")?;
    if !trusted_download(&parsed) {
        return Err("영상 제공자가 허용되지 않은 다운로드 주소를 반환했습니다.".into());
    }
    let response = client.get(parsed).timeout(Duration::from_secs(150)).send().await
        .map_err(|e| format!("영상 다운로드 실패: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("영상 다운로드 HTTP {}",response.status()));
    }
    if response.content_length().is_some_and(|length| length > MAX_BYTES as u64) {
        return Err("영상 결과가 256MiB 제한을 넘었습니다.".into());
    }
    let target = dir.join("output.part.mp4");
    let mut output = tokio::fs::File::create(&target).await.map_err(|e|e.to_string())?;
    let mut total = 0usize;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        ctx.checkpoint(json!({}))?;
        let chunk = chunk.map_err(|e|e.to_string())?;
        total = total.checked_add(chunk.len()).ok_or("영상 크기 초과")?;
        if total > MAX_BYTES { return Err("영상 결과가 256MiB 제한을 넘었습니다.".into()); }
        output.write_all(&chunk).await.map_err(|e|e.to_string())?;
    }
    output.flush().await.map_err(|e|e.to_string())?;
    drop(output);
    if total < 1024 { return Err("영상 파일이 비어 있습니다.".into()); }
    let mut header = [0u8;8];
    std::fs::File::open(&target).and_then(|mut f|f.read_exact(&mut header))
        .map_err(|e|e.to_string())?;
    if &header[4..8] != b"ftyp" { return Err("영상 컨테이너가 MP4가 아닙니다.".into()); }
    std::fs::rename(target,dir.join("output.mp4")).map_err(|e|e.to_string())?;
    Ok(total)
}

pub(super) async fn run(ctx: &JobContext, input: &Value, dir: &Path) -> Result<Value, String> {
    let secret = ctx.state.read_secret("video:ltx:api")
        .filter(|s| !s.trim().is_empty()).ok_or("LTX API 키를 영상 설정에서 등록하세요.")?;
    let spec = input["render"].clone();
    let model = spec["model"].as_str().ok_or("LTX 모델 설정 누락")?.to_string();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if trusted_download(attempt.url()) { attempt.follow() }
            else { attempt.stop() }
        }))
        .build().map_err(|e|e.to_string())?;
    // Persist potential billing exposure and the uncertain-effects guard in
    // ONE job checkpoint before the non-idempotent paid POST.
    let attempts=ctx.state.store.get("job",&ctx.job_id)
        .and_then(|job|job["paidSubmissionAttempts"].as_u64()).unwrap_or(0);
    ctx.checkpoint(json!({"phase":"submitting","progress":5,
        "paidSubmissionAttempts":attempts.saturating_add(1),
        "videoState":{"inFlight":true,"provider":"ltx","submitted":false}}))?;
    let response = client.post(API).bearer_auth(&secret).json(&json!({
        "prompt":spec["prompt"],"model":model,"duration":spec["duration"],
        "resolution":spec["resolution"],"fps":spec["fps"],
        "generate_audio":spec["generate_audio"]
    })).timeout(Duration::from_secs(30)).send().await
        .map_err(|_|"needs_review: LTX 유료 작업의 접수 여부가 불확실합니다. 작업 기록을 확인하세요.")?;
    if !response.status().is_success() {
        let status = response.status();
        ctx.checkpoint(json!({"videoState":{"inFlight":false}}))?;
        return Err(format!("LTX 생성 요청 거부: HTTP {status}"));
    }
    let receipt: Value = response.json().await
        .map_err(|_|"needs_review: LTX 작업 ID 응답을 읽지 못했습니다.")?;
    let id = receipt["id"].as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128
            && s.chars().all(|c|c.is_ascii_alphanumeric() || c=='-' || c=='_'))
        .ok_or("needs_review: LTX 작업 ID가 유효하지 않습니다.")?.to_string();
    ctx.checkpoint(json!({"phase":"rendering","progress":20,
        "videoState":{"inFlight":true,"submitted":true,"remoteJobId":id}}))?;
    let started = Instant::now();
    let endpoint = format!("{API}/{id}");
    let mut errors = 0u8;
    loop {
        ctx.checkpoint(json!({}))?;
        if started.elapsed() >= Duration::from_secs(1500) {
            return Err(format!("needs_review: LTX 작업 {id}가 대기시간을 초과했습니다. 자동 재제출하지 않습니다."));
        }
        tokio::time::sleep(Duration::from_millis(5250)).await;
        let response = match client.get(&endpoint).bearer_auth(&secret)
            .timeout(Duration::from_secs(20)).send().await {
            Ok(r) => r,
            Err(_) => { errors+=1; if errors>=5 { return Err("needs_review: LTX 조회 장애. 중복 제출을 방지하기 위해 중단했습니다.".into()); } continue; }
        };
        if response.status().is_server_error() {
            errors+=1;
            if errors>=5 { return Err("needs_review: LTX 서버 오류가 계속됩니다.".into()); }
            continue;
        }
        if !response.status().is_success() {
            return Err(format!("needs_review: LTX 작업 조회 HTTP {}",response.status()));
        }
        errors = 0;
        let status: Value = response.json().await.map_err(|_|"needs_review: LTX 작업 상태 JSON 오류")?;
        match status["status"].as_str().unwrap_or("") {
            "completed" => {
                let url = status["result"]["video_url"].as_str()
                    .ok_or("needs_review: LTX 완료 응답에 영상 주소가 없습니다.")?;
                ctx.checkpoint(json!({"phase":"downloading","progress":90,
                    "videoState":{"inFlight":true,"remoteJobId":id}}))?;
                let bytes = download(&client,url,dir,ctx).await?;
                ctx.checkpoint(json!({"phase":"finalizing","progress":98,
                    "videoState":{"inFlight":false,"remoteJobId":id}}))?;
                return Ok(json!({"engine":"ltx","model":model,"remoteJobId":id,
                    "quality":spec["quality"],"resolution":spec["resolution"],
                    "audio":spec["generate_audio"],"duration":spec["duration"],"bytes":bytes}));
            }
            "failed" => {
                let reason = status["error"]["message"].as_str().unwrap_or("영상 제공자 오류");
                ctx.checkpoint(json!({"videoState":{"inFlight":false,"remoteJobId":id}}))?;
                return Err(format!("LTX 생성 실패: {}",reason.chars().take(240).collect::<String>()));
            }
            "pending" | "processing" => {
                ctx.checkpoint(json!({"phase":"rendering","progress":35,
                    "videoState":{"inFlight":true,"remoteJobId":id}}))?;
            }
            _ => return Err("needs_review: 알 수 없는 LTX 작업 상태입니다.".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quality_profiles_are_validated_before_billing() {
        let cfg=json!({"videoModel":"ltx-2-5-pro","videoQuality":"1080p"});
        let good=spec(&json!({"brief":"A lighthouse at dawn","duration":8,"aspectRatio":"9:16"}),&cfg).unwrap();
        assert_eq!(good["resolution"],"1080x1920");
        assert!(spec(&json!({"brief":"x","duration":20}),&cfg).is_err());
        assert!(spec(&json!({"brief":"x","duration":10,"videoModel":"ltx-2-5-fast"}),&cfg).is_ok());
        assert!(spec(&json!({"brief":"x","duration":8,"videoQuality":"5k"}),&cfg).is_err());
        assert_eq!(maximum_duration("ltx-2-5-fast","1080p",24).unwrap(),20);
        assert_eq!(maximum_duration("ltx-2-5-fast","4k",24).unwrap(),10);
        assert_eq!(maximum_duration("ltx-2-5-fast","720p",48).unwrap(),10);
        assert_eq!(maximum_duration("ltx-2-5-pro","4k",50).unwrap(),10);
        assert!(spec(&json!({"brief":"x","duration":20,"videoModel":"ltx-2-5-fast",
            "videoQuality":"4k"}),&cfg).is_err());
    }
    #[test]
    fn reject_local_or_non_tls_download_hosts() {
        for s in ["http://storage.googleapis.com/a.mp4","https://localhost/a.mp4",
            "https://127.0.0.1/a.mp4","https://example.org/a.mp4"] {
            assert!(!trusted_download(&reqwest::Url::parse(s).unwrap()));
        }
        assert!(trusted_download(&reqwest::Url::parse("https://storage.googleapis.com/bucket/movie.mp4").unwrap()));
    }
}
