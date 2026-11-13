//! Turn-key multi-shot video through Runway's 2026-06 recipe.
//! Non-idempotent submissions are never retried without an operator review.
use super::progress;
use crate::engine::jobs::JobContext;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::{path::Path, time::{Duration, Instant}};
use tokio::io::AsyncWriteExt;

const BASE: &str = "https://api.dev.runwayml.com/v1";
const VERSION: &str = "2024-11-06";
const RECIPE: &str = "2026-06";
const MAX_BYTES: usize = 256 * 1024 * 1024;

/// Builds the exact billable recipe before a job reaches the remote API.
/// No model call, GPU, or user-maintained workflow is needed for auto mode.
pub(super) fn recipe_input(input: &Value) -> Result<Value, String> {
    let duration = input["duration"].as_u64().unwrap_or(10);
    if !matches!(duration, 5 | 10 | 15) {
        return Err("Runway 멀티샷은 5·10·15초를 지원합니다.".into());
    }
    let quality = input["videoQuality"].as_str().unwrap_or("1080p");
    let aspect = input["aspectRatio"].as_str().unwrap_or("9:16");
    let ratio = match (quality, aspect) {
        ("720p", "9:16") => "720:1280",
        ("720p", "16:9") => "1280:720",
        ("720p", "1:1") => "960:960",
        ("1080p", "9:16") => "1080:1920",
        ("1080p", "16:9") => "1920:1080",
        ("1080p", "1:1") => "1440:1440",
        _ => return Err("Runway 해상도는 720p·1080p, 비율은 9:16·16:9·1:1입니다.".into()),
    };
    let audio = input["generateAudio"].as_bool().unwrap_or(true);
    let mode = input["shotMode"].as_str().unwrap_or("auto");
    if mode == "custom" {
        let scenes = input["scenes"].as_array().ok_or("샷 3~5개의 내용이 필요합니다.")?;
        if !(3..=5).contains(&scenes.len()) || duration < scenes.len() as u64 {
            return Err("Runway 커스텀 샷은 3~5개여야 합니다.".into());
        }
        let base = duration / scenes.len() as u64;
        let extra = duration % scenes.len() as u64;
        let mut shots = Vec::with_capacity(scenes.len());
        for (index, scene) in scenes.iter().enumerate() {
            let prompt = scene.as_str().ok_or("각 샷은 문자열이어야 합니다.")?.trim();
            if !(3..=512).contains(&prompt.chars().count()) {
                return Err(format!("{}번 샷은 3~512자로 입력하세요.", index + 1));
            }
            shots.push(json!({
                "prompt":prompt,
                "duration":base + u64::from((index as u64) < extra),
            }));
        }
        return Ok(json!({"mode":"custom","version":RECIPE,"shots":shots,
            "duration":duration,"ratio":ratio,"audio":audio}));
    }
    if mode != "auto" { return Err("지원하지 않는 Runway 제작 모드입니다.".into()); }
    let prompt = input["brief"].as_str().unwrap_or("").trim();
    if prompt.is_empty() || prompt.chars().count() > 2500 {
        return Err("Runway 영상 지시문은 1~2,500자입니다.".into());
    }
    Ok(json!({"mode":"auto","version":RECIPE,"prompt":prompt,
        "duration":duration,"ratio":ratio,"audio":audio}))
}

/// Key material stays encrypted in the vault, outside persisted job records.
pub(super) async fn render(ctx: &JobContext, input: &Value, dir: &Path) -> Result<Value, String> {
    let payload = recipe_input(input)?;
    let key = ctx.state.read_secret("video:runway")
        .filter(|s|!s.trim().is_empty()).ok_or("Runway API 키를 영상 설정에서 등록하세요.")?;
    let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(3)).build().map_err(|e|e.to_string())?;
    std::fs::write(dir.join("storyboard.json"),json!({
        "title":input["title"],"engine":"runway","recipe":RECIPE,"mode":payload["mode"],
        "direction":payload["prompt"],"shots":payload["shots"],
        "duration":payload["duration"],"ratio":payload["ratio"],"audio":payload["audio"],
        "quality":input["videoQuality"]
    }).to_string()).map_err(|e|e.to_string())?;
    progress(ctx,"storyboard",5)?;
    ctx.checkpoint(json!({"videoState":{"inFlight":true,"remoteProvider":"runway","recipe":RECIPE},
        "phase":"submitting","progress":10}))?;
    let response=client.post(format!("{BASE}/recipes/multi_shot_video"))
        .bearer_auth(&key).header("X-Runway-Version",VERSION).json(&payload)
        .timeout(Duration::from_secs(40)).send().await
        .map_err(|_|"needs_review: Runway 제출 결과 불명확. 원격 결제·작업 상태를 먼저 확인하세요.")?;
    if !response.status().is_success() {
        let status=response.status();
        ctx.checkpoint(json!({"videoState":{"inFlight":false},"phase":"failed"}))?;
        return Err(format!("Runway 작업 거부 HTTP {status}: {}",
            response.text().await.unwrap_or_default().chars().take(250).collect::<String>()));
    }
    let created:Value=response.json().await.map_err(|_|"needs_review: Runway 제출 응답 해석 실패")?;
    let task_id=created["id"].as_str().filter(|s|uuid::Uuid::parse_str(s).is_ok())
        .ok_or("needs_review: Runway 제출 응답에 유효한 작업 ID가 없습니다.")?.to_string();
    ctx.checkpoint(json!({"videoState":{"inFlight":true,"remoteProvider":"runway","taskId":task_id},
        "phase":"rendering","progress":20}))?;
    let start=Instant::now();
    let mut delay=5u64;
    let output_url=loop {
        ctx.checkpoint(json!({}))?;
        if start.elapsed()>Duration::from_secs(1800) {
            return Err(format!("needs_review: Runway {task_id} 작업 제한 시간 초과"));
        }
        let result=client.get(format!("{BASE}/tasks/{task_id}")).bearer_auth(&key)
            .header("X-Runway-Version",VERSION).timeout(Duration::from_secs(20)).send().await;
        match result {
            Ok(r) if r.status().is_success() => {
                delay=5;
                let task:Value=r.json().await.map_err(|_|"needs_review: Runway 상태 응답 오류")?;
                match task["status"].as_str().unwrap_or("") {
                    "SUCCEEDED" => {
                        let uri=task["output"].as_array().and_then(|a|a.first())
                            .and_then(Value::as_str).ok_or("needs_review: Runway 출력 URL 없음")?;
                        break uri.to_owned();
                    }
                    "FAILED"|"CANCELED" => {
                        ctx.checkpoint(json!({"videoState":{"inFlight":false},"phase":"failed"}))?;
                        return Err(format!("Runway 작업 실패: {}",task["failureCode"].as_str().unwrap_or("생성 실패")));
                    }
                    "PENDING"|"THROTTLED"|"RUNNING" => (),
                    _ => return Err(format!("needs_review: Runway {task_id} 미지원 상태"))
                }
            }
            Ok(r) if r.status().as_u16()==429||r.status().is_server_error() => delay=(delay*2).min(60),
            Ok(r) => return Err(format!("needs_review: Runway 상태 조회 HTTP {}",r.status())),
            Err(_) => delay=(delay*2).min(60),
        }
        tokio::time::sleep(Duration::from_secs(delay)).await;
    };
    let url=reqwest::Url::parse(&output_url).map_err(|_|"needs_review: Runway 출력 URL 오류")?;
    if url.scheme()!="https" || !url.username().is_empty() || url.password().is_some() {
        return Err("needs_review: Runway 출력 URL은 HTTPS여야 합니다.".into());
    }
    progress(ctx,"downloading",82)?;
    let response=client.get(url).timeout(Duration::from_secs(180)).send().await
        .map_err(|_|"needs_review: 원격 영상 다운로드 실패")?;
    if !response.status().is_success(){return Err(format!("needs_review: 영상 다운로드 HTTP {}",response.status()));}
    if response.content_length().is_some_and(|n|n>MAX_BYTES as u64) {
        return Err("영상 크기가 256MiB를 초과합니다.".into());
    }
    let mut file=tokio::fs::File::create(dir.join("output.part.mp4")).await.map_err(|e|e.to_string())?;
    let mut bytes=0usize;
    let mut header=Vec::new();
    let mut stream=response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        ctx.checkpoint(json!({}))?;
        let chunk=chunk.map_err(|_|"needs_review: 영상 다운로드가 중단되었습니다.")?;
        bytes=bytes.saturating_add(chunk.len());
        if bytes>MAX_BYTES{return Err("영상 크기가 256MiB를 초과합니다.".into());}
        if header.len()<8{header.extend_from_slice(&chunk[..chunk.len().min(8-header.len())]);}
        file.write_all(&chunk).await.map_err(|e|e.to_string())?;
    }
    file.flush().await.map_err(|e|e.to_string())?;
    drop(file);
    if bytes<1024||header.get(4..8)!=Some(b"ftyp".as_slice()){
        return Err("needs_review: 제공자 응답이 MP4 영상이 아닙니다.".into());
    }
    tokio::fs::rename(dir.join("output.part.mp4"),dir.join("output.mp4")).await.map_err(|e|e.to_string())?;
    ctx.checkpoint(json!({"videoState":{"inFlight":false,"taskId":task_id},"phase":"finalizing","progress":95}))?;
    Ok(json!({"engine":"runway","recipe":RECIPE,"remoteTaskId":task_id,
        "mode":payload["mode"],"quality":input["videoQuality"],
        "ratio":payload["ratio"],"audio":payload["audio"],"duration":payload["duration"],"bytes":bytes}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_recipe_inputs() {
        assert_eq!(recipe_input(&json!({"brief":"film","duration":10,"aspectRatio":"9:16"})).unwrap()["ratio"],"1080:1920");
        assert_eq!(recipe_input(&json!({"brief":"film","duration":10,"videoQuality":"720p","aspectRatio":"1:1"})).unwrap()["ratio"],"960:960");
        assert!(recipe_input(&json!({"brief":"film","duration":13})).is_err());
        let custom = recipe_input(&json!({"brief":"story","shotMode":"custom","duration":10,
            "scenes":["First scene","Second scene","Third scene"]})).unwrap();
        assert_eq!(custom["mode"],"custom");
        assert_eq!(custom["shots"][0]["duration"],4);
        assert_eq!(custom["shots"][1]["duration"],3);
        assert_eq!(custom["shots"][2]["duration"],3);
        assert!(recipe_input(&json!({"brief":"film","shotMode":"custom","duration":10,
            "scenes":["Only one"]})).is_err());
    }
}
