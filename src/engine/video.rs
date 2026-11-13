//! DOMINION's durable media pipeline. Native cards are rendered by Rust; FFmpeg
//! only encodes them. Optional ComfyUI is an explicit HTTP rendering connector.
use crate::{
    engine::{
        jobs::{self, JobContext},
        oauth, tools,
    },
    state::AppState,
};
use serde_json::{json, Value};

#[path = "video/ltx.rs"]
mod ltx;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub fn config(state: &AppState) -> Value {
    let mut value = json!({"id":"default","engine":"native","duration":15,"aspectRatio":"9:16","comfyUrl":"","comfyWorkflow":null,"workflowDir":"","outputDir":"","ffmpegPath":"","providerId":"","model":"","useModel":false});
    if let Some(saved) = state.store.get("video_config", "default") {
        if let Some(map) = saved.as_object() {
            for (k, v) in map {
                if k != "videoApiKey" {
                    value[k] = v.clone();
                }
            }
        }
    }
    value["hasVideoKey"] = json!(state.store.has_secret("video:api"));
    value
}
pub fn ffmpeg(state: &AppState) -> Option<PathBuf> {
    ffmpeg_from_config(&config(state))
}

/// Resolve the encoder from the job's frozen config so a later settings change
/// cannot redirect an already queued render.
fn ffmpeg_from_config(cfg: &Value) -> Option<PathBuf> {
    let selected = cfg["ffmpegPath"].as_str().unwrap_or("");
    if !selected.is_empty() {
        let path = PathBuf::from(selected);
        return path.is_file().then_some(path);
    }
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let mut paths: Vec<PathBuf> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|p| p.join(name))
            .collect();
    paths.extend(
        [
            "/opt/homebrew/bin/ffmpeg",
            "/usr/local/bin/ffmpeg",
            "/usr/bin/ffmpeg",
        ]
        .into_iter()
        .map(PathBuf::from),
    );
    paths.into_iter().find(|p| p.is_file())
}
pub fn directory(state: &AppState, id: &str) -> Result<PathBuf, String> {
    // Job IDs are database-owned UUIDs, never arbitrary request paths.
    uuid::Uuid::parse_str(id).map_err(|_| "잘못된 영상 작업 ID")?;
    Ok(state.config.data_dir.join("video").join(id))
}
pub fn output_path(state: &AppState, id: &str) -> Result<PathBuf, String> {
    let record = state
        .store
        .get("job", id)
        .filter(|j| j["kind"] == "video")
        .ok_or("영상 작업이 없습니다.")?;
    if record["status"] != "completed" {
        return Err("완료된 영상이 없습니다.".into());
    }
    let root = directory(state, id)?
        .canonicalize()
        .map_err(|_| "영상 파일이 없습니다.")?;
    let path = root
        .join("output.mp4")
        .canonicalize()
        .map_err(|_| "영상 파일이 없습니다.")?;
    if !path.starts_with(root) {
        return Err("영상 경로를 확인할 수 없습니다.".into());
    }
    Ok(path)
}
pub fn public_job(job: &Value) -> Value {
    let id = job["id"].as_str().unwrap_or("");
    let ready = job["kind"] == "video" && job["status"] == "completed";
    json!({"id":id,"kind":job["kind"],"title":job["input"]["title"],"brief":job["input"]["brief"],"engine":job["input"]["engine"],"status":job["status"],"phase":job.get("phase").unwrap_or(&job["status"]),"progress":job["progress"],"error":job["error"],"createdAt":job.get("createdAt").or(job.get("created_at")),"videoJobId":job["input"]["videoJobId"],"privacyStatus":job["input"]["privacyStatus"],"outputUrl":ready.then(||format!("/api/video/jobs/{id}/content")),"storyboard":job["videoState"]["scenes"],"result":job["result"],"duration":job["input"]["duration"],"aspectRatio":job["input"]["aspectRatio"]})
}
pub fn enqueue(state: &AppState, body: &Value) -> Result<Value, String> {
    let cfg = config(state);
    let brief = body["brief"].as_str().unwrap_or("").trim();
    if brief.is_empty() || brief.len() > 16_000 {
        return Err("영상 주제를 1~16,000바이트로 입력하세요.".into());
    }
    let engine = body["engine"]
        .as_str()
        .or(cfg["engine"].as_str())
        .unwrap_or("native");
    if !matches!(engine, "native" | "comfyui" | "ltx") {
        return Err("지원하지 않는 영상 엔진입니다.".into());
    }
    if engine == "native" && ffmpeg(state).is_none() {
        return Err("FFmpeg가 필요합니다. 고급 설정에서 실행 파일을 지정하세요.".into());
    }
    if engine == "comfyui"
        && (cfg["comfyUrl"].as_str().unwrap_or("").is_empty() || !cfg["comfyWorkflow"].is_object())
    {
        return Err("ComfyUI 주소와 API 형식 워크플로 JSON을 먼저 저장하세요.".into());
    }
    if engine == "ltx" && state.read_secret("video:api").is_none() {
        return Err("LTX 2.5 API 키를 영상 설정에서 등록하세요.".into());
    }
    let duration = body["duration"]
        .as_u64()
        .or(cfg["duration"].as_u64())
        .unwrap_or(15);
    if !(3..=180).contains(&duration) {
        return Err("영상 길이는 3~180초입니다.".into());
    }
    let aspect = body["aspectRatio"]
        .as_str()
        .or(cfg["aspectRatio"].as_str())
        .unwrap_or("9:16");
    if !matches!(aspect, "9:16" | "16:9" | "1:1") {
        return Err("지원 비율은 9:16,16:9,1:1입니다.".into());
    }
    let ltx_model = body["ltxModel"].as_str().unwrap_or("ltx-2-5-pro");
    let ltx_quality = body["ltxQuality"].as_str().unwrap_or("1080p");
    let ltx_style = body["ltxStyle"].as_str().unwrap_or("cinematic");
    if engine == "ltx" {
        if !matches!(ltx_model,"ltx-2-5-fast" | "ltx-2-5-pro") {
            return Err("지원하지 않는 LTX 모델입니다.".into());
        }
        ltx::resolution(aspect,ltx_quality)?;
        if !ltx::valid_duration(ltx_model,ltx_quality,duration) {
            return Err("LTX 2.5 모델·화질별 지원 영상 길이를 확인하세요.".into());
        }
        ltx::cinematic_prompt(brief,ltx_style)?;
    }
    let title = body["title"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(brief)
        .chars()
        .take(90)
        .collect::<String>();
    let fps = body["fps"].as_u64().unwrap_or(16);
    let seed = body["seed"].as_i64().unwrap_or(-1);
    let negative_prompt = body["negativePrompt"].as_str().unwrap_or("");
    if !(1..=60).contains(&fps) || seed < -1 || negative_prompt.len() > 16_000 {
        return Err("FPS, 시드, 네거티브 프롬프트 값을 확인하세요.".into());
    }
    let (width, height) = match aspect {
        "16:9" => (640, 360),
        "1:1" => (480, 480),
        _ => (360, 640),
    };
    let seed = if seed == -1 {
        // Freeze one seed for this job so a safe retry uses identical input.
        i64::from(u32::from_be_bytes(uuid::Uuid::new_v4().as_bytes()[..4].try_into().unwrap()))
    } else {
        seed
    };
    let input = json!({
        "brief":brief, "title":title, "engine":engine, "duration":duration,
        "aspectRatio":aspect, "config":cfg, "publish":false,
        "ltxModel":ltx_model,"ltxQuality":ltx_quality,"ltxStyle":ltx_style,
        "render":{"prompt":brief,"negative":negative_prompt,"negativePrompt":negative_prompt,
                  "duration":duration,"width":width,"height":height,"fps":fps,"frames":duration*fps,"seed":seed}
    });
    jobs::enqueue(state, "video", input)
        .map(|j| public_job(&j))
        .ok_or("영상 작업을 저장할 수 없습니다.".into())
}
fn progress(ctx: &JobContext, phase: &str, percent: u64) -> Result<(), String> {
    ctx.checkpoint(json!({"phase":phase,"progress":percent}))
}
#[path = "video/native.rs"]
mod native;
#[path = "video/comfy.rs"]
mod comfy;
#[path = "video/upload.rs"]
mod upload;
pub use upload::{enqueue_upload, upload};

pub async fn run(ctx: &JobContext, job: &Value) -> Result<Value, String> {
    let dir = directory(&ctx.state, &ctx.job_id)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let input = &job["input"];
    let result = match input["engine"].as_str().unwrap_or("native") {
        "comfyui" => comfy::comfy(ctx,input,&dir).await,
        "ltx" => ltx::render(ctx,input,&dir).await,
        _ => native::native(ctx,input,&dir).await,
    };
    match result {
        Ok(mut value) => {
            let bytes = std::fs::metadata(dir.join("output.mp4"))
                .map_err(|e| e.to_string())?
                .len();
            if bytes < 100 {
                return Err("생성된 영상이 비어 있습니다.".into());
            }
            value["bytes"] = json!(bytes);
            value["outputUrl"] = json!(format!("/api/video/jobs/{}/content", ctx.job_id));
            progress(ctx, "completed", 100)?;
            Ok(value)
        }
        Err(e) => {
            let _ = std::fs::remove_file(dir.join("output.part.mp4"));
            if ctx
                .state
                .store
                .get("job", &ctx.job_id)
                .is_some_and(|j| j["videoState"]["inFlight"] == true)
                && !e.starts_with("needs_review:")
            {
                Err(format!("needs_review: {e}"))
            } else {
                Err(e)
            }
        }
    }
}
#[cfg(test)]
#[path = "video/tests.rs"]
mod tests;
