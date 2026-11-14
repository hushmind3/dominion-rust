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
    let quality_ready = video::quality_available(&state);
    let reachable = comfy_reachable(cfg["comfyUrl"].as_str().unwrap_or("")).await;
    let comfy = reachable && cfg["comfyWorkflow"].is_object();
    let omni = cfg["hasVideoKey"] == true;
    let ltx_ready = cfg["hasLtxKey"] == true;
    let workflows = workflows_from_config(&cfg);
    let jobs = state.store.all("job").iter()
        .filter(|job| job["kind"] == "video")
        .map(video::public_job).collect::<Vec<_>>();
    Json(json!({
        "config": cfg,
        "runtime": "rust",
        "available": executable.is_some() || comfy || omni || ltx_ready,
        "native": {"available": executable.is_some()},
        "ffmpeg": {"available": executable.is_some(), "path": executable},
        "quality":{"available":quality_ready,"method":"ffprobe","visualSemanticReview":false},
        "comfy": {"reachable": reachable, "available": comfy},
        "omni": {"available": omni, "model":cfg["omniModel"], "targetDurationSeconds":8},
        "ltx": {"available":ltx_ready,"models":["ltx-2-5-fast","ltx-2-5-pro"],"audio":true},
        "engines": [
            {"id":"native","name":"자막 카드 영상","available":executable.is_some(),"reason":if executable.is_some(){""}else{"FFmpeg 실행 파일 필요"}},
            {"id":"comfyui","name":"ComfyUI","available":comfy,"reason":if comfy{""}else{"서버와 API 워크플로 설정 필요"}},
            {"id":"omni","name":"Google Gemini Omni Flash · 음성 포함 고화질","available":omni,"reason":if omni{""}else{"Gemini API 키 등록 필요"}},
            {"id":"ltx","name":"LTX 2.5 · 다중 장면/음성 제작","available":ltx_ready,"reason":if ltx_ready{""}else{"LTX API 키 등록 필요"}}
        ],
        "jobs": jobs,
        "models":if ltx_ready {json!([{"id":"ltx-2-5-fast"},{"id":"ltx-2-5-pro"}])}else{json!([])},
        "modelsReady": ltx_ready,
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
        "omniModel",
        "omniResolution",
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
    if !matches!(cfg["engine"].as_str(),Some("native"|"comfyui"|"omni"|"ltx")) {
        return error(StatusCode::BAD_REQUEST,"영상 엔진 선택 오류");
    }
    if !matches!(cfg["omniModel"].as_str(),Some("gemini-omni-1.2-flash"|"gemini-omni-1.1-flash"))
        || !matches!(cfg["omniResolution"].as_str(),Some("360p"|"720p"|"1080p"|"4k")) {
        return error(StatusCode::BAD_REQUEST,"Omni 모델 또는 출력 화질 오류");
    }
    let new_key=body["videoApiKey"].as_str().map(str::trim).filter(|v|!v.is_empty());
    if new_key.is_some_and(|key| key.len()>512 || key.chars().any(char::is_whitespace)) {
        return error(StatusCode::BAD_REQUEST,"Gemini API 키 형식을 확인하세요.");
    }
    // Keep LTX and Gemini credentials in distinct encrypted secrets.
    // The browser never receives either credential from GET /api/video/config.
    let ltx_key=body["ltxApiKey"].as_str().map(str::trim).filter(|value|!value.is_empty());
    if ltx_key.is_some_and(|value|value.len()>512||value.chars().any(char::is_whitespace)){
        return error(StatusCode::BAD_REQUEST,"LTX API 키 형식을 확인하세요.");
    }
    if state.store.put("video_config", &cfg).is_none() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "영상 설정을 저장할 수 없습니다.");
    }
    if body["removeLtxApiKey"]==true {state.store.remove_secret("video:ltx-api");}
    else if let Some(key)=ltx_key {state.save_secret("video:ltx-api",key);}
    if body["removeVideoApiKey"]==true {state.store.remove_secret("video:api");}
    else if let Some(key)=new_key {state.save_secret("video:api",key);}
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
pub async fn price_quote(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    match video::paid_quote(&state,&body){
        Ok(quote)=>Json(quote).into_response(),
        Err(error)=>(StatusCode::BAD_REQUEST,Json(json!({"error":error}))).into_response(),
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
/// Read-only, output-digest-bound manifest for multi-shot film retakes.
pub async fn film_scenes(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match video::film_scenes(&state,&id){
        Ok(value)=>Json(value).into_response(),
        Err(error)=>self::error(StatusCode::BAD_REQUEST,error)
    }
}
/// Owner-confirmed retake creates one idempotent new job; never retries an unknown paid POST.
pub async fn retake_scene(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response{
    match video::enqueue_retake(&state,&id,&body){
        Ok(value)=>(StatusCode::ACCEPTED,Json(value)).into_response(),
        Err(error)=>self::error(StatusCode::BAD_REQUEST,error)
    }
}
/// Preview one generated scene before deciding which paid clip to retake.
/// Only local, bounded regular MP4 files under the UUID-owned job directory.
pub async fn scene_content(State(state):State<AppState>,Path((id,index)):Path<(String,usize)>,
    request:Request<Body>)->Response{
    let manifest=match video::film_scenes(&state,&id){
        Ok(value)=>value,
        Err(error)=>return self::error(StatusCode::NOT_FOUND,error)
    };
    if index>=6 || manifest["clips"][index]["isAvailable"]!=true{
        return self::error(StatusCode::NOT_FOUND,"재생 가능한 영상 장면이 없습니다.");
    }
    let root=match video::directory(&state,&id).and_then(|path|
        path.canonicalize().map_err(|_|"영상 디렉터리가 없습니다.".to_string())){
        Ok(root)=>root,
        Err(error)=>return self::error(StatusCode::NOT_FOUND,error)
    };
    let original=root.join(format!("clip-{index:02}")).join("output.mp4");
    if !std::fs::symlink_metadata(&original).is_ok_and(|meta|
        meta.file_type().is_file() && meta.len()>=1024 && meta.len()<=256*1024*1024){
        return self::error(StatusCode::NOT_FOUND,"영상 장면을 확인할 수 없습니다.");
    }
    let target=match original.canonicalize(){
        Ok(path) if path.starts_with(&root)=>path,
        _=>return self::error(StatusCode::FORBIDDEN,"영상 디렉터리 밖의 파일은 제공하지 않습니다.")
    };
    match tower_http::services::ServeFile::new(target).try_call(request).await{
        Ok(response)=>{
            let mut response=response.into_response();
            response.headers_mut().insert("x-content-type-options","nosniff".parse().unwrap());
            response.headers_mut().insert("cache-control","no-store".parse().unwrap());
            response
        }
        Err(error)=>self::error(StatusCode::INTERNAL_SERVER_ERROR,error.to_string())
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
    if comfy {
        items.push(json!({
            "id": "comfyui",
            "name": cfg["comfyWorkflowName"].as_str().unwrap_or("ComfyUI"),
            "kind": "comfyui",
            "available": true,
            "hint": "연결된 ComfyUI 워크플로로 생성합니다.",
        }));
    }
    items.push(json!({
        "id":"omni", "name":"Google Gemini Omni Flash (고화질·사운드)",
        "kind":"omni","available":cfg["hasVideoKey"]==true,
        "hint":"Gemini API 키 하나로 고화질 영상 직접 생성. 유료 API 호출입니다."
    }));
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
            {"id":"omni","name":"Gemini Omni Flash","kind":"omni","available":cfg["hasVideoKey"]==true},
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
mod tests {
    use super::*;
    fn state() -> AppState {
        let dir = std::env::temp_dir().join(format!("dominion-video-api-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir,
        };
        AppState::new(
            cfg.clone(),
            crate::db::Store::open(&cfg.db_path()).unwrap(),
            crate::vault::Vault::open(&cfg.key_path()),
        )
    }
    #[test]
    fn enqueue_is_durable_and_never_publishes_implicitly() {
        let s = state();
        let result = video::enqueue(
            &s,
            &json!({"brief":"테스트 영상","engine":"native","duration":3,"publish":true}),
        );
        if video::ffmpeg(&s).is_some() {
            let j = result.unwrap();
            let raw = s.store.get("job", j["id"].as_str().unwrap()).unwrap();
            assert_eq!(raw["input"]["publish"], false);
            assert_eq!(raw["status"], "queued");
            assert!(outputs(&s).is_empty());
        }
        assert!(video::enqueue(&s, &json!({"brief":"x","duration":900})).is_err());
        std::fs::remove_dir_all(s.config.data_dir.clone()).unwrap();
    }
    #[tokio::test]
    async fn native_pipeline_produces_a_real_playable_container() {
        let s = state();
        if video::ffmpeg(&s).is_none() {
            return;
        }
        let j = video::enqueue(
            &s,
            &json!({"brief":"실제 영상 테스트. 한글 프레임 확인","duration":3}),
        )
        .unwrap();
        let id = j["id"].as_str().unwrap().to_string();
        let job = s.store.get("job", &id).unwrap();
        let ctx = crate::engine::jobs::JobContext::new(s.clone(), id.clone());
        let result = video::run(&ctx, &job).await.unwrap();
        assert!(result["bytes"].as_u64().unwrap() > 1000);
        let data = std::fs::read(video::directory(&s, &id).unwrap().join("output.mp4")).unwrap();
        assert_eq!(&data[4..8], b"ftyp");
        assert!(
            video::output_path(&s, &id).is_err(),
            "unfinished job must not be served"
        );
        std::fs::remove_dir_all(s.config.data_dir.clone()).unwrap();
    }
}
