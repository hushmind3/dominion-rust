//! Optional paid cinematic renderer. The queued job is the source of truth:
//! every remote task is recorded before polling, and ambiguous submissions
//! require review rather than spending a second set of credits on retry.
//!
//! Runway Dev /v1/text_to_video Kling 3.0 contract (2026-10-26):
//! https://dev.runwayml.com/learn/kling-3-0-api
use super::*;
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

const API: &str = "https://api.dev.runwayml.com/v1";
const API_VERSION: &str = "2024-11-06";
const MAX_VIDEO_BYTES: usize = 256 * 1024 * 1024;

fn model(cfg: &Value) -> &'static str {
    match cfg["runwayModel"].as_str() {
        Some("kling3.0_standard") => "kling3.0_standard",
        Some("kling3.0_4k") => "kling3.0_4k",
        Some("kling_4") => "kling_4",
        _ => "kling3.0_pro",
    }
}

fn ratio(model: &str, aspect: &str) -> &'static str {
    match (model, aspect) {
        ("kling3.0_standard", "9:16") => "720:1280",
        ("kling3.0_standard", "1:1") => "960:960",
        ("kling3.0_standard", _) => "1280:720",
        ("kling3.0_4k", "9:16") => "2160:3840",
        ("kling3.0_4k", "1:1") => "2880:2880",
        ("kling3.0_4k", _) => "3840:2160",
        (_, "9:16") => "1080:1920",
        (_, "1:1") => "1440:1440",
        _ => "1920:1080",
    }
}

/// Divide 3..180 seconds into 3..15-second native clips; no charged task
/// is created until the user explicitly selects the paid renderer.
pub(super) fn clip_durations(seconds: u64) -> Result<Vec<u64>, String> {
    if !(3..=180).contains(&seconds) {
        return Err("Runway 영상 길이는 3~180초입니다.".into());
    }
    let clips = seconds.div_ceil(15);
    let base = seconds / clips;
    let extra = seconds % clips;
    Ok((0..clips).map(|i| base + u64::from(i < extra)).collect())
}

async fn storyboard(ctx: &JobContext, input: &Value, count: usize) -> Vec<String> {
    let brief = input["brief"].as_str().unwrap_or("");
    let supplied = input["scenes"].as_array().map(|rows| rows.iter()
        .filter_map(Value::as_str).filter(|text| !text.trim().is_empty())
        .map(str::to_owned).collect::<Vec<_>>()).unwrap_or_default();
    if supplied.len() == count && supplied.iter().all(|s| s.chars().count() <= 1500) {
        return supplied;
    }
    let cfg = &input["config"];
    if cfg["useModel"] == true {
        if let (Some(provider), Some(model)) = (cfg["providerId"].as_str(), cfg["model"].as_str()) {
            if !provider.is_empty() && !model.is_empty() {
                let instruction = format!(
                    "You are a film director. Return ONLY a JSON array of exactly {count} strings.                      Each entry is an executable cinematic shot brief, with consistent cast,                      costume, lighting, location and action progression, matching the owner's brief.                      Include camera directions and sound cues. Each under 600 characters."
                );
                if let Ok(text) = crate::engine::llm::complete_for_session(
                    &ctx.state, provider, model, &instruction, brief, &ctx.job_id
                ).await {
                    let json_text = text.trim().trim_start_matches("```json")
                        .trim_start_matches("```").trim_end_matches("```").trim();
                    if let Ok(shots) = serde_json::from_str::<Vec<String>>(json_text) {
                        if shots.len() == count && shots.iter().all(|s| !s.trim().is_empty() && s.chars().count() <= 700) {
                            return shots;
                        }
                    }
                }
            }
        }
    }
    let camera = ["establishing wide shot", "tracking medium shot", "cinematic close-up", "revealing dolly shot"];
    (0..count).map(|i| format!(
        "Scene {} of {}, {}, narrative continuity and physical movement: {}",
        i + 1, count, camera[i % camera.len()],
        brief.chars().take(950).collect::<String>()
    )).collect()
}

fn safe_output_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw).map_err(|_| "영상 출력 URL 오류")?;
    let host = url.host_str().ok_or("영상 출력 서버가 없습니다.")?;
    if raw.len() > 4096 || url.scheme() != "https" || !url.username().is_empty()
        || url.password().is_some() || host.eq_ignore_ascii_case("localhost")
        || host.parse::<std::net::IpAddr>().is_ok()
    {
        return Err("영상 결과 URL은 공개 HTTPS 호스트만 허용합니다.".into());
    }
    Ok(url)
}

async fn download(ctx: &JobContext, client: &reqwest::Client, url: &str, path: &Path) -> Result<usize, String> {
    let url = safe_output_url(url)?;
    let response = client.get(url).timeout(Duration::from_secs(180)).send().await
        .map_err(|e| format!("영상 결과 다운로드 연결 실패: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("영상 결과 다운로드 실패: HTTP {}", response.status()));
    }
    if response.content_length().is_some_and(|length| length > MAX_VIDEO_BYTES as u64) {
        return Err("영상 파일이 256MiB 제한을 넘습니다.".into());
    }
    let part = path.with_extension("part");
    let mut file = tokio::fs::File::create(&part).await.map_err(|e| e.to_string())?;
    let mut received = 0usize;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        ctx.checkpoint(json!({}))?;
        let chunk = chunk.map_err(|e| e.to_string())?;
        received = received.saturating_add(chunk.len());
        if received > MAX_VIDEO_BYTES {
            return Err("영상 파일이 256MiB 제한을 넘습니다.".into());
        }
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
    }
    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);
    let mut magic = [0_u8; 8];
    let mut reader = std::fs::File::open(&part).map_err(|e| e.to_string())?;
    use std::io::Read;
    reader.read_exact(&mut magic).map_err(|_| "다운로드된 영상이 비어 있습니다.")?;
    if &magic[4..8] != b"ftyp" {
        return Err("다운로드된 출력이 MP4 컨테이너가 아닙니다.".into());
    }
    std::fs::rename(&part, path).map_err(|e| e.to_string())?;
    Ok(received)
}

async fn wait_for_task(ctx: &JobContext, client: &reqwest::Client, token: &str, id: &str) -> Result<String, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Runway 작업 ID 오류")?;
    for _ in 0..240 {
        ctx.checkpoint(json!({}))?;
        let response = client.get(format!("{API}/tasks/{id}"))
            .bearer_auth(token).header("X-Runway-Version", API_VERSION)
            .timeout(Duration::from_secs(30)).send().await.map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("needs_review: Runway 작업 조회 실패 HTTP {}", response.status()));
        }
        let task: Value = response.json().await.map_err(|e| e.to_string())?;
        match task["status"].as_str().unwrap_or("") {
            "SUCCEEDED" => {
                let uri = task["output"].as_array().and_then(|items| items.iter()
                    .find_map(|item| item.as_str().or_else(|| item["url"].as_str())));
                return uri.map(str::to_owned).ok_or_else(|| "needs_review: 완료된 작업에 출력 URL이 없습니다.".into());
            }
            "FAILED" | "CANCELLED" => {
                return Err(format!("needs_review: Runway 작업 {}. id={id}", task["status"]));
            }
            "PENDING" | "RUNNING" | "THROTTLED" => {}
            status => return Err(format!("needs_review: 알 수 없는 Runway 작업 상태: {status}")),
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    Err(format!("needs_review: Runway 작업 {id} 대기 제한. 같은 작업 ID를 조회해야 합니다."))
}

async fn merge_clips(ctx: &JobContext, input: &Value, dir: &Path, count: usize) -> Result<(), String> {
    if count == 1 {
        return std::fs::rename(dir.join("clip-00.mp4"), dir.join("output.mp4"))
            .map_err(|e| e.to_string());
    }
    let encoder = super::ffmpeg_from_config(&input["config"])
        .ok_or("15초 초과 영상 합성을 위해 FFmpeg가 필요합니다.")?;
    let list = (0..count).map(|i| format!("file 'clip-{i:02}.mp4'\n")).collect::<String>();
    std::fs::write(dir.join("clips.txt"), list).map_err(|e| e.to_string())?;
    let mut command = tokio::process::Command::new(encoder);
    command.current_dir(dir).args([
        "-hide_banner", "-loglevel", "error", "-nostdin", "-y",
        "-f", "concat", "-safe", "1", "-i", "clips.txt",
        "-c", "copy", "-movflags", "+faststart", "output.part.mp4",
    ]);
    let running = tools::run_command(&mut command, Duration::from_secs(600));
    tokio::pin!(running);
    let result = loop {
        tokio::select! {
            answer = &mut running => break answer?,
            _ = tokio::time::sleep(Duration::from_millis(500)) => { ctx.checkpoint(json!({}))?; }
        }
    };
    if !result.status.success() {
        return Err(format!("영상 병합 실패: {}", String::from_utf8_lossy(&result.stderr)
            .chars().take(600).collect::<String>()));
    }
    std::fs::rename(dir.join("output.part.mp4"), dir.join("output.mp4"))
        .map_err(|e| e.to_string())
}

pub(super) async fn run(ctx: &JobContext, input: &Value, dir: &Path) -> Result<Value, String> {
    // A previous submission with uncertain outcome must never auto-submit.
    if ctx.state.store.get("job", &ctx.job_id)
        .is_some_and(|job| job["videoState"]["inFlight"] == true)
    {
        return Err("needs_review: 이전 Runway 원격 작업의 상태를 확인해야 합니다.".into());
    }
    let token = ctx.state.read_secret("video:api")
        .or_else(|| std::env::var("RUNWAYML_API_SECRET").ok())
        .filter(|value| !value.is_empty())
        .ok_or("Runway Dev API 키를 영상 설정에 저장하세요.")?;
    let cfg = &input["config"];
    let chosen_model = model(cfg);
    let clips = if chosen_model == "kling_4" {
        super::kling4::clip_durations(input["duration"].as_u64().unwrap_or(15))?
    } else {
        clip_durations(input["duration"].as_u64().unwrap_or(15))?
    };
    if clips.len() > 1 && super::ffmpeg_from_config(cfg).is_none() {
        return Err("여러 클립을 합성하려면 FFmpeg가 필요합니다.".into());
    }
    let use_frames = cfg["runwayContinuityFrames"].as_bool().unwrap_or(true)
        && clips.len() > 1;
    let chosen_ratio = ratio(chosen_model, input["aspectRatio"].as_str().unwrap_or("9:16"));
    let scenes = storyboard(ctx, input, clips.len()).await;
    let shot_plans = clips.iter().enumerate()
        .map(|(index, seconds)| if chosen_model == "kling_4" {
            vec![json!({
                "prompt":super::kling4::direction(
                    input["brief"].as_str().unwrap_or(""), &scenes[index],
                    index, clips.len()),
                "duration":seconds,
            })]
        } else {
            super::shot_plan::shots(input["brief"].as_str().unwrap_or(""),
                &scenes[index], *seconds, index, clips.len())
        })
        .collect::<Vec<_>>();
    // Freeze the complete creative plan before a remote credit is spent.
    // Its exact scene and camera decisions survive a needs_review restart.
    ctx.checkpoint(json!({"videoState":{"scenes":scenes,"shotPlans":shot_plans,
        "inFlight":false,"completedClips":0},
        "phase":"storyboard","progress":5}))?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build().map_err(|e| e.to_string())?;
    let mut task_ids = Vec::<String>::new();
    for (index, seconds) in clips.iter().enumerate() {
        let mut payload = if chosen_model == "kling_4" {
            super::kling4::request(
                input["aspectRatio"].as_str().unwrap_or("9:16"), *seconds,
                shot_plans[index][0]["prompt"].as_str().unwrap_or(""))
        } else {
            super::shot_plan::request(
                chosen_model, chosen_ratio, *seconds,
                cfg["runwayAudio"].as_bool().unwrap_or(true), &shot_plans[index])
        };
        let endpoint = if use_frames && index > 0 {
            // Anchoring one film to the preceding clip's actual output frame
            // prevents independent text-only generations drifting identities.
            let frame = super::frame_anchor::first_frame_for_next(
                ctx, cfg, dir, index - 1).await?;
            if chosen_model == "kling_4" {
                super::kling4::attach_prior_frame(&mut payload, frame);
            } else {
                payload["promptImage"] = json!(frame);
            }
            "image_to_video"
        } else { "text_to_video" };
        let percent = 5 + 75 * index as u64 / clips.len() as u64;
        ctx.checkpoint(json!({"phase":"submitting","progress":percent,
            "videoState":{"inFlight":true,"clipIndex":index}}))?;
        let response = client.post(format!("{API}/{endpoint}"))
            .bearer_auth(&token).header("X-Runway-Version", API_VERSION)
            .json(&payload).timeout(Duration::from_secs(35)).send().await
            .map_err(|e| format!("needs_review: Runway 제출 결과가 불확실합니다: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("needs_review: Runway 요청 HTTP {}: {}",
                response.status(), response.text().await.unwrap_or_default()
                    .chars().take(400).collect::<String>()));
        }
        let response: Value = response.json().await
            .map_err(|_| "needs_review: Runway 제출 응답 파싱 실패")?;
        let task_id = response["id"].as_str()
            .ok_or("needs_review: Runway 제출 작업 ID가 없습니다.")?.to_owned();
        uuid::Uuid::parse_str(&task_id).map_err(|_| "needs_review: Runway 작업 ID 형식 오류")?;
        task_ids.push(task_id.clone());
        ctx.checkpoint(json!({"phase":"rendering","progress":percent + 3,
            "videoState":{"inFlight":true,"remoteTaskId":task_id,"taskIds":task_ids}}))?;
        let url = wait_for_task(ctx, &client, &token, &task_id).await?;
        let path = dir.join(format!("clip-{index:02}.mp4"));
        let bytes = download(ctx, &client, &url, &path).await
            .map_err(|e| format!("needs_review: 이미 생성된 Runway 영상 다운로드 실패: {e}"))?;
        ctx.checkpoint(json!({"phase":"rendering","progress":5 + 75 * (index + 1) as u64 / clips.len() as u64,
            "videoState":{"inFlight":false,"remoteTaskId":null,"completedClips":index+1,
                "lastClipBytes":bytes,"taskIds":task_ids}}))?;
    }
    progress(ctx, "encoding", 85)?;
    merge_clips(ctx, input, dir, clips.len()).await?;
    Ok(json!({
        "engine":"runway","provider":"Runway Dev","model":chosen_model,
        "description":if chosen_model=="kling_4" {"Kling 4.0 cinematic native-audio video"}
            else {"Kling 3.0 cinematic native-audio video"},
        "duration":input["duration"],
        "scenes":scenes.len(),"shotCount":shot_plans.iter().map(Vec::len).sum::<usize>(),
        "shotMode":if chosen_model=="kling_4"{"kling4_native_continuous"}else{"kling3_native_multishot"},
        "frameAnchoredClips":if use_frames{clips.len()-1}else{0},
        "taskIds":task_ids,
        "audio":cfg["runwayAudio"].as_bool().unwrap_or(true),
    }))
}
