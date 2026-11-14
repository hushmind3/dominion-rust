use crate::{engine::video, state::AppState};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::time::Duration;
fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({"error":message.into()}))).into_response()
}
async fn comfy_reachable(url: &str) -> bool {
    if url.is_empty() {
        return false;
    }
    reqwest::Client::new()
        .get(format!("{}/system_stats", url.trim_end_matches('/')))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .is_ok_and(|r| r.status().is_success())
}
pub fn outputs(state: &AppState) -> Vec<Value> {
    state
        .store
        .all("job")
        .iter()
        .filter(|j| j["kind"] == "video" && j["status"] == "completed")
        .map(video::public_job)
        .collect()
}
pub async fn status(State(state): State<AppState>) -> Json<Value> {
    let cfg = video::config(&state);
    let executable = video::ffmpeg(&state);
    let reachable = comfy_reachable(cfg["comfyUrl"].as_str().unwrap_or("")).await;
    let comfy = reachable && cfg["comfyWorkflow"].is_object();
    let workflows = workflows_from_config(&cfg);
    let runway = cfg["hasRunwayKey"] == true;
    let ltx = cfg["hasLtxKey"] == true;
    let jobs = state.store.all("job").iter()
        .filter(|job| job["kind"] == "video")
        .map(video::public_job).collect::<Vec<_>>();
    Json(json!({
        "config": cfg,
        "runtime": "rust",
        "available": executable.is_some() || comfy || runway || ltx,
        "native": {"available": executable.is_some()},
        "ffmpeg": {"available": executable.is_some(), "path": executable},
        "comfy": {"reachable": reachable, "available": comfy},
        "runway": {"available":runway,"recipe":"2026-06","audio":true,"durations":[5,10,15],
            "qualities":["720p","1080p"],"ratios":["9:16","16:9","1:1"],"shotModes":["auto","custom"]},
        "ltx": {"available":ltx,"models":["ltx-2-5-pro","ltx-2-5-fast"],
            "qualities":["720p","1080p","1440p","4k"],"audio":true,"apiVersion":"v2"},
        "engines": [
            {"id":"native","name":"자막 카드 영상","available":executable.is_some(),"reason":if executable.is_some(){""}else{"FFmpeg 실행 파일 필요"}},
            {"id":"comfyui","name":"ComfyUI","available":comfy,"reason":if comfy{""}else{"서버와 API 워크플로 설정 필요"}},
            {"id":"runway","name":"Runway 멀티샷 · 음성 포함","available":runway,"reason":if runway{""}else{"Runway API 키 등록 필요"}},
            {"id":"ltx","name":"LTX-2.5 · 최대 4K 영상·오디오","available":ltx,"reason":if ltx{""}else{"LTX API 키 등록 필요"}}
        ],
        "jobs": jobs,
        "models": [],
        "modelsReady": runway || ltx || comfy,
        "workflows": workflows,
        "installed": reachable
    }))
}
pub async fn get_config(State(state): State<AppState>) -> Json<Value> {
    Json(video::config(&state))
}
pub async fn save_config(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let mut cfg = video::config(&state);
    for key in [
        "engine",
        "duration",
        "aspectRatio",
        "comfyUrl",
        "comfyWorkflow",
        "comfyWorkflowName",
        "workflowDir",
        "outputDir",
        "ffmpegPath",
        "providerId",
        "model",
        "useModel",
        "runwayQuality",
        "videoQuality",
        "videoModel",
    ] {
        if let Some(v) = body.get(key) {
            cfg[key] = v.clone();
        }
    }
    if let Some(url) = cfg["comfyUrl"].as_str().filter(|s| !s.is_empty()) {
        if !reqwest::Url::parse(url).is_ok_and(|u| {
            matches!(u.scheme(), "http" | "https")
                && u.username().is_empty()
                && u.password().is_none()
        }) {
            return error(
                StatusCode::BAD_REQUEST,
                "ComfyUI HTTP(S) 주소를 확인하세요.",
            );
        }
    }
    if !cfg["comfyWorkflow"].is_null()
        && !cfg["comfyWorkflow"].as_object().is_some_and(|nodes| {
            !nodes.is_empty() && nodes.values().all(|node| {
                node["class_type"].is_string() && node["inputs"].is_object()
            })
        }) {
        return error(
            StatusCode::BAD_REQUEST,
            "API 형식 워크플로 JSON 객체가 필요합니다.",
        );
    }
    if cfg["comfyWorkflow"].to_string().len() > 1024 * 1024 {
        return error(
            StatusCode::BAD_REQUEST,
            "워크플로는 1MiB 이하로 저장하세요.",
        );
    }
    if body["clearRunwayKey"] == true {
        state.store.remove_secret("video:runway");
    } else if let Some(key) = body["runwayApiKey"].as_str() {
        if key.len() > 300 {
            return error(StatusCode::BAD_REQUEST, "Runway API 키가 너무 깁니다.");
        }
        if !key.trim().is_empty() {
            state.save_secret("video:runway", key.trim());
        }
    }
    if body["clearLtxKey"] == true {
        state.store.remove_secret("video:ltx:api");
    } else if let Some(key) = body["ltxApiKey"].as_str() {
        if key.len() > 300 {
            return error(StatusCode::BAD_REQUEST, "LTX API 키가 너무 깁니다.");
        }
        if !key.trim().is_empty() {
            state.save_secret("video:ltx:api", key.trim());
        }
    }
    if state.store.put("video_config", &cfg).is_none() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "영상 설정을 저장할 수 없습니다.");
    }
    state.bus.record("video_config", &json!({"id":"default"}));
    Json(video::config(&state)).into_response()
}
pub async fn list_jobs(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state
        .store
        .all("job")
        .iter()
        .filter(|j| matches!(j["kind"].as_str(), Some("video" | "youtube_upload")))
        .map(video::public_job)
        .collect::<Vec<_>>()))
}
/// Pure, uncharged quote. The same calculation is repeated at job admission.
pub async fn quote_job(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    match video::paid_quote(&state,&body) {
        Ok(quote)=>Json(quote).into_response(),
        Err(message)=>error(StatusCode::BAD_REQUEST,message),
    }
}
pub async fn create_job(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    match video::enqueue(&state, &body) {
        Ok(j) => (StatusCode::ACCEPTED, Json(j)).into_response(),
        Err(e) => error(StatusCode::BAD_REQUEST, e),
    }
}
pub async fn job(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state
        .store
        .get("job", &id)
        .filter(|j| matches!(j["kind"].as_str(), Some("video" | "youtube_upload")))
    {
        Some(j) => Json(video::public_job(&j)).into_response(),
        None => error(StatusCode::NOT_FOUND, "영상 작업을 찾을 수 없습니다."),
    }
}
pub async fn upload(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    match video::enqueue_upload(&state, &id, &body) {
        Ok(j) => (StatusCode::ACCEPTED, Json(j)).into_response(),
        Err(e) => error(StatusCode::BAD_REQUEST, e),
    }
}
pub async fn content(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request<Body>,
) -> Response {
    let path = match video::output_path(&state, &id) {
        Ok(p) => p,
        Err(e) => return error(StatusCode::NOT_FOUND, e),
    };
    match tower_http::services::ServeFile::new(path)
        .try_call(request)
        .await
    {
        Ok(response) => {
            let mut response = response.into_response();
            response
                .headers_mut()
                .insert("x-content-type-options", "nosniff".parse().unwrap());
            response
        }
        Err(_) => error(StatusCode::NOT_FOUND, "영상 파일을 열 수 없습니다."),
    }
}
pub async fn outputs_handler(State(state): State<AppState>) -> Json<Value> {
    Json(json!(outputs(&state)))
}
pub async fn locations(State(state): State<AppState>) -> Json<Value> {
    Json(json!({"outputs":[state.config.data_dir.join("video")],"comfy":[],"workflows":[]}))
}
fn workflows_from_config(cfg: &Value) -> Vec<Value> {
    if !cfg["comfyWorkflow"].is_object() {
        return Vec::new();
    }
    vec![json!({
        "id": "default",
        "name": cfg["comfyWorkflowName"].as_str().unwrap_or("ComfyUI"),
        "bytes": cfg["comfyWorkflow"].to_string().len(),
    })]
}

pub async fn workflows(State(state): State<AppState>) -> Json<Value> {
    Json(json!(workflows_from_config(&video::config(&state))))
}

pub async fn models(State(state): State<AppState>) -> Json<Value> {
    let cfg = video::config(&state);
    let native = video::ffmpeg(&state).is_some();
    let reachable = comfy_reachable(cfg["comfyUrl"].as_str().unwrap_or("")).await;
    let comfy = reachable && cfg["comfyWorkflow"].is_object();
    let mut items = vec![json!({
        "id": "native",
        "name": "DOMINION native (ffmpeg)",
        "kind": "native",
        "available": native,
        "hint": if native { "로컬 ffmpeg으로 장면과 자막을 합성합니다." } else { "ffmpeg를 찾을 수 없습니다. 설정에서 경로를 지정하세요." },
    })];
    if cfg["hasRunwayKey"] == true {
        items.push(json!({"id":"runway","name":"Runway Multi-Shot (2026-06)",
            "kind":"runway","available":true,"hint":"자동 구성 또는 3~5개 샷, 720p·1080p·오디오를 지원합니다."}));
    }
    if cfg["hasLtxKey"] == true {
        items.push(json!({"id":"ltx","name":"LTX-2.5 (V2)",
            "kind":"ltx","available":true,"hint":"Fast/Pro, 최대 4K·오디오, 클라우드 자동 생성"}));
    }
    if comfy {
        items.push(json!({
            "id": "comfyui",
            "name": cfg["comfyWorkflowName"].as_str().unwrap_or("ComfyUI"),
            "kind": "comfyui",
            "available": true,
            "hint": "연결된 ComfyUI 워크플로로 생성합니다.",
        }));
    }
    Json(json!(items))
}

pub async fn catalog(State(state): State<AppState>) -> Json<Value> {
    let cfg = video::config(&state);
    let native = video::ffmpeg(&state).is_some();
    let reachable = comfy_reachable(cfg["comfyUrl"].as_str().unwrap_or("")).await;
    let comfy = reachable && cfg["comfyWorkflow"].is_object();
    Json(json!({
        "models": [
            {"id":"native","name":"DOMINION native (ffmpeg)","kind":"native","available":native},
            {"id":"comfyui","name":cfg["comfyWorkflowName"].as_str().unwrap_or("ComfyUI"),"kind":"comfyui","available":comfy},
            {"id":"runway","name":"Runway Multi-Shot 2026-06","kind":"runway","available":cfg["hasRunwayKey"] == true},
            {"id":"ltx","name":"LTX-2.5 V2","kind":"ltx","available":cfg["hasLtxKey"] == true},
        ],
        "workflows": workflows_from_config(&cfg),
        "engine": cfg["engine"],
        "comfyUrl": cfg["comfyUrl"],
        "comfyReachable": reachable,
    }))
}

pub async fn files(State(state): State<AppState>) -> Json<Value> {
    Json(json!(outputs(&state)))
}

pub async fn reveal(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let path = match video::output_path(&state, &id) {
        Ok(path) => path,
        Err(message) => return error(StatusCode::NOT_FOUND, message),
    };
    #[cfg(target_os = "macos")]
    let launched = tokio::process::Command::new("/usr/bin/open").arg("-R").arg(&path).status().await;
    #[cfg(target_os = "windows")]
    let launched = tokio::process::Command::new("explorer.exe")
        .arg(path.parent().expect("output directory")).status().await;
    #[cfg(all(unix, not(target_os = "macos")))]
    let launched = tokio::process::Command::new("xdg-open")
        .arg(path.parent().expect("output directory")).status().await;
    #[cfg(not(any(unix, windows)))]
    let launched: std::io::Result<std::process::ExitStatus> =
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported));

    match launched {
        Ok(status) if status.success() => Json(json!({"opened":true})).into_response(),
        _ => error(StatusCode::SERVICE_UNAVAILABLE, "영상 폴더를 열 수 없습니다."),
    }
}

pub async fn not_available() -> Response {
    error(
        StatusCode::NOT_IMPLEMENTED,
        "이 작업은 지원하지 않습니다. 영상 작업 만들기를 사용하세요.",
    )
}
#[cfg(test)]
#[path = "video/tests.rs"]
mod tests;

pub async fn scene_diagnostics(
    State(state):State<AppState>,Path(id):Path<String>,
)->Response{
    match video::film_scenes(&state,&id){
        Ok(value)=>Json(value).into_response(),
        Err(message)=>error(StatusCode::BAD_REQUEST,message),
    }
}
pub async fn retake_scene(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>,
)->Response{
    match video::enqueue_retake(&state,&id,&body){
        Ok(value)=>Json(value).into_response(),
        Err(message)=>error(StatusCode::BAD_REQUEST,message),
    }
}
