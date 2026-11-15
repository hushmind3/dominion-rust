use crate::{domain::{Approval, CreateRun, PublicSettings, ReconcileTool, Run}, engine::Engine, tools};
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    body::Body,
    middleware::{self, Next},
    response::{sse::{Event, KeepAlive, Sse}, IntoResponse, Response},
    routing::{get, post}, Json, Router,
};
use futures_core::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc, time::Duration};
use uuid::Uuid;
use tokio_util::io::ReaderStream;
use tower_http::services::ServeDir;

type ApiError = (StatusCode, Json<Value>);
fn bad(message: impl Into<String>) -> ApiError {
    (StatusCode::BAD_REQUEST, Json(json!({"error":message.into()})))
}
fn not_found() -> ApiError {
    (StatusCode::NOT_FOUND, Json(json!({"error":"execution not found"})))
}
async fn authorize(State(engine): State<Arc<Engine>>, request: axum::extract::Request, next: Next) -> Response {
    if let Some(token) = engine.settings.token.as_ref() {
        let expected = format!("Bearer {token}");
        if request.headers().get(header::AUTHORIZATION).and_then(|h|h.to_str().ok()) != Some(expected.as_str()) && request.headers().get("x-dominion-token").and_then(|h|h.to_str().ok()) != Some(token.as_str()) {
            return (StatusCode::UNAUTHORIZED,Json(json!({"error":"Bearer access token required"}))).into_response();
        }
    }
    next.run(request).await
}
pub fn router(engine: Arc<Engine>) -> Router {
    let api = Router::new()
        .route("/health",get(|| async { Json(json!({"ok":true,"name":"dominion","api":"v1"})) }))
        .route("/config",get(config))
        .route("/memory",get(list_memory).post(save_memory))
        .route("/runs",get(list_runs).post(create_run))
        .route("/runs/from-repository",post(create_repository_run))
        .route("/runs/{id}",get(get_run))
        .route("/runs/{id}/resume",post(resume))
        .route("/runs/{id}/cancel",post(cancel))
        .route("/runs/{id}/approval",post(approve))
        .route("/runs/{id}/directive",post(directive))
        .route("/runs/{id}/reconcile",post(reconcile))
        .route("/runs/{id}/subagents",get(list_subagents).post(create_subagent))
        .route("/runs/{id}/events",get(events))
        .route("/runs/{id}/files",get(list_files))
        .route("/runs/{id}/files/{*path}",get(read_file))
        .route("/groups",get(list_groups).post(create_group))
        .route("/groups/{id}",get(get_group))
        .route("/groups/{id}/messages",post(group_say))
        .route("/groups/{id}/next",post(group_next))
        .route("/videos",get(list_videos).post(create_video))
        .route("/videos/{id}",get(get_video))
        .route("/videos/{id}/render",post(render_video))
        .route("/videos/{id}/scenes/{index}",post(change_scene))
        .route("/videos/{id}/download",get(download_video))
        .route("/improvements",get(list_improvements).post(create_improvement))
        .route("/improvements/{id}",get(get_improvement))
        .route("/improvements/{id}/approve",post(approve_improvement))
        .route("/improvements/{id}/adopt",post(adopt_improvement))
        .route("/improvements/{id}/descendant",post(descendant_improvement))
        .route_layer(middleware::from_fn_with_state(Arc::clone(&engine), authorize));
    // Main's React/Vite build is the canonical UI; Rust serves the built files.
    // The development Vite server proxies API calls to this same router.
    let dist=std::env::var("DOMINION_WEB_DIR").map(std::path::PathBuf::from)
        .unwrap_or_else(|_|std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web/dist"));
    Router::new()
        .nest("/api/v1",api)
        .fallback_service(ServeDir::new(dist).append_index_html_on_directories(true))
        .with_state(engine)
}

#[derive(Deserialize)]
struct MemoryQuery { scope:Option<String>, query:Option<String> }
async fn list_memory(State(engine):State<Arc<Engine>>,Query(q):Query<MemoryQuery>)->Json<Vec<crate::knowledge::MemoryEntry>>{
    Json(engine.knowledge.search(q.scope.as_deref().unwrap_or("global"),q.query.as_deref().unwrap_or(""),20).await)
}
#[derive(Deserialize)]
struct SaveMemory {scope:String,title:String,content:String}
async fn save_memory(State(engine):State<Arc<Engine>>,Json(req):Json<SaveMemory>)
    ->Result<(StatusCode,Json<crate::knowledge::MemoryEntry>),ApiError>{
    let note=engine.knowledge.remember(&req.scope,&req.title,&req.content).await.map_err(bad)?;
    Ok((StatusCode::CREATED,Json(note)))
}

async fn config(State(engine): State<Arc<Engine>>) -> Json<PublicSettings> {
    Json(PublicSettings {
        model:engine.settings.main.name.clone(),
        economy_model:engine.settings.economy.name.clone(),
        configured:!engine.settings.main.api_key.is_empty() || engine.settings.main.base_url.contains("localhost") || engine.settings.main.base_url.contains("127.0.0.1"),
        shell_enabled:engine.settings.shell_enabled,max_steps:engine.settings.max_steps,mcp_server_count:engine.mcp.server_count(),context_max_chars:engine.settings.context_max_chars,
    })
}
async fn list_runs(State(engine): State<Arc<Engine>>) -> Json<Vec<Run>> { Json(engine.store.list().await) }
async fn create_run(State(engine): State<Arc<Engine>>, Json(req): Json<CreateRun>) -> Result<(StatusCode,Json<Run>), ApiError> {
    let run=crate::sdk::Dominion::from_engine(engine).submit(req).await.map_err(bad)?;
    Ok((StatusCode::CREATED,Json(run)))
}
#[derive(Deserialize)]
struct RepositoryRunRequest {
    instruction:String,
    repository:String,
    budget_krw:Option<f64>,
    max_steps:Option<usize>,
}
async fn create_repository_run(State(engine):State<Arc<Engine>>,Json(input):Json<RepositoryRunRequest>)->Result<(StatusCode,Json<Run>),ApiError>{
    let req=CreateRun{instruction:input.instruction,budget_krw:input.budget_krw,max_steps:input.max_steps};
    let run=crate::sdk::Dominion::from_engine(engine).submit_from_repository(req,&input.repository).await.map_err(bad)?;
    Ok((StatusCode::CREATED,Json(run)))
}
async fn get_run(State(engine): State<Arc<Engine>>, Path(id):Path<Uuid>) -> Result<Json<Run>,ApiError> {
    engine.store.get(id).await.map(Json).ok_or_else(not_found)
}
async fn resume(State(engine): State<Arc<Engine>>, Path(id):Path<Uuid>) -> Result<Json<Value>,ApiError> {
    engine.launch(id).await.map_err(bad)?;
    Ok(Json(json!({"ok":true})))
}
async fn cancel(State(engine): State<Arc<Engine>>, Path(id):Path<Uuid>) -> Result<Json<Run>,ApiError> {
    engine.cancel(id).await.map(Json).map_err(bad)
}
async fn approve(State(engine): State<Arc<Engine>>, Path(id):Path<Uuid>, Json(input):Json<Approval>) -> Result<Json<Value>,ApiError> {
    engine.approve(id,input.allow).await.map_err(bad)?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
struct Directive { instruction: String }
async fn directive(State(engine): State<Arc<Engine>>, Path(id):Path<Uuid>, Json(input):Json<Directive>) -> Result<Json<Run>,ApiError> {
    engine.directive(id,input.instruction).await.map(Json).map_err(bad)
}
async fn reconcile(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>,Json(input):Json<ReconcileTool>)->Result<Json<Run>,ApiError>{
    engine.reconcile(id,input).await.map(Json).map_err(bad)
}
async fn list_subagents(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)->Result<Json<Vec<Run>>,ApiError>{
    if engine.store.get(id).await.is_none(){return Err(not_found());}
    let children=engine.store.list().await.into_iter().filter(|run|run.parent_run_id==Some(id)).collect();
    Ok(Json(children))
}
async fn create_subagent(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>,Json(input):Json<CreateRun>)->Result<(StatusCode,Json<Run>),ApiError>{
    let child=crate::sdk::Dominion::from_engine(engine).delegate(id,input).await.map_err(bad)?;
    Ok((StatusCode::CREATED,Json(child)))
}
#[derive(Default,Deserialize)]
struct EventQuery { since: Option<u64> }
async fn events(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>,Query(params):Query<EventQuery>) -> Result<Sse<impl Stream<Item=Result<Event,Infallible>>>,ApiError> {
    if engine.store.get(id).await.is_none() {return Err(not_found());}
    let mut seq = params.since.unwrap_or(0);
    let output = async_stream::stream! {
        loop {
            match engine.store.get(id).await {
                Some(run) => {
                    for record in &run.events {
                        if record.seq <= seq { continue; }
                        seq = record.seq;
                        let event = Event::default().id(record.seq.to_string()).event(&record.kind)
                            .json_data(record).unwrap_or_else(|_|Event::default().event("encoding_error").data("{}"));
                        yield Ok(event);
                    }
                    if run.terminal() { break; }
                }
                None => break,
            }
            tokio::time::sleep(Duration::from_millis(750)).await;
        }
    };
    Ok(Sse::new(output).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}
async fn list_files(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>) -> Result<Json<Value>,ApiError> {
    if engine.store.get(id).await.is_none(){return Err(not_found());}
    let data=tools::execute(&engine.settings,id,&crate::domain::ToolCall{
        id:"api-list".into(),name:"list_files".into(),arguments:json!({"path":""})
    }).await.map_err(bad)?;
    let items:Value=serde_json::from_str(&data).map_err(|e| bad(e.to_string()))?;
    Ok(Json(json!({"files":items})))
}
async fn read_file(State(engine):State<Arc<Engine>>,Path((id,path)):Path<(Uuid,String)>) -> Result<Json<Value>,ApiError> {
    if engine.store.get(id).await.is_none(){return Err(not_found());}
    let content=tools::execute(&engine.settings,id,&crate::domain::ToolCall{
        id:"api-read".into(),name:"read_file".into(),arguments:json!({"path":path})
    }).await.map_err(bad)?;
    Ok(Json(json!({"path":path,"content":content})))
}

async fn list_groups(State(engine): State<Arc<Engine>>) -> Json<Vec<crate::group::Room>> {
    Json(engine.groups.list().await)
}
async fn create_group(State(engine):State<Arc<Engine>>,Json(req):Json<crate::group::NewRoom>) -> Result<(StatusCode,Json<crate::group::Room>),ApiError> {
    let room=engine.groups.create(req).await.map_err(bad)?;
    Ok((StatusCode::CREATED,Json(room)))
}
async fn get_group(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>) -> Result<Json<crate::group::Room>,ApiError> {
    engine.groups.get(id).await.map(Json).ok_or_else(not_found)
}
async fn group_say(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>,Json(input):Json<crate::group::Say>)->Result<Json<crate::group::Room>,ApiError>{
    engine.groups.say(id,input.content).await.map(Json).map_err(bad)
}
async fn group_next(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>) -> Result<Json<crate::group::Room>,ApiError> {
    engine.groups.next(&engine.model,id).await.map(Json).map_err(bad)
}

async fn list_videos(State(engine):State<Arc<Engine>>)->Json<Vec<crate::video::Video>>{
    Json(engine.videos.list().await)
}
async fn create_video(State(engine):State<Arc<Engine>>,Json(input):Json<crate::video::NewVideo>)->Result<(StatusCode,Json<crate::video::Video>),ApiError>{
    let video=engine.videos.create(&engine.model,input).await.map_err(bad)?;
    let started=engine.videos.start(video.id).await.map_err(bad)?;
    Ok((StatusCode::CREATED,Json(started)))
}
async fn get_video(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)->Result<Json<crate::video::Video>,ApiError>{
    engine.videos.get(id).await.map(Json).ok_or_else(not_found)
}
async fn render_video(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)->Result<Json<crate::video::Video>,ApiError>{
    engine.videos.start(id).await.map(Json).map_err(bad)
}
async fn change_scene(State(engine):State<Arc<Engine>>,Path((id,index)):Path<(Uuid,usize)>,Json(input):Json<crate::video::ChangeScene>)->Result<Json<crate::video::Video>,ApiError>{
    engine.videos.change_scene(id,index,input).await.map(Json).map_err(bad)
}
async fn download_video(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)->Result<Response,ApiError>{
    let path=engine.videos.output_path(id).await.map_err(bad)?;
    let file=tokio::fs::File::open(path).await.map_err(|e|bad(e.to_string()))?;
    let body=Body::from_stream(ReaderStream::new(file));
    Ok(([(header::CONTENT_TYPE,"video/mp4"),(header::CONTENT_DISPOSITION,"attachment; filename=dominion.mp4")],body).into_response())
}

async fn list_improvements(State(engine):State<Arc<Engine>>)->Json<Vec<crate::improvement::Improvement>>{
    Json(engine.improvements.list().await)
}
async fn create_improvement(State(engine):State<Arc<Engine>>,Json(input):Json<crate::improvement::NewImprovement>)->Result<(StatusCode,Json<crate::improvement::Improvement>),ApiError>{
    let item=engine.improvements.create(input).await.map_err(bad)?;
    Ok((StatusCode::CREATED,Json(item)))
}
async fn get_improvement(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)->Result<Json<crate::improvement::Improvement>,ApiError>{
    engine.improvements.get(id).await.map(Json).ok_or_else(not_found)
}
async fn approve_improvement(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)->Result<Json<crate::improvement::Improvement>,ApiError>{
    engine.improvements.start(engine.model.clone(),id,engine.settings.shell_enabled).await.map(Json).map_err(bad)
}
async fn adopt_improvement(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)->Result<Json<crate::improvement::Improvement>,ApiError>{
    engine.improvements.adopt(id).await.map(Json).map_err(bad)
}
async fn descendant_improvement(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)->Result<(StatusCode,Json<crate::improvement::Improvement>),ApiError>{
    let result=engine.improvements.descendant(id).await.map_err(bad)?;
    Ok((StatusCode::CREATED,Json(result)))
}
