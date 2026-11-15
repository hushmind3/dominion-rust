use crate::{domain::{Approval, CreateRun, PublicSettings, Run}, engine::Engine, tools};
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{sse::{Event, KeepAlive, Sse}, Html, IntoResponse, Response},
    routing::{get, post}, Json, Router,
};
use futures_core::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc, time::Duration};
use uuid::Uuid;

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
        if request.headers().get(header::AUTHORIZATION).and_then(|h|h.to_str().ok()) != Some(expected.as_str()) {
            return (StatusCode::UNAUTHORIZED,Json(json!({"error":"Bearer access token required"}))).into_response();
        }
    }
    next.run(request).await
}
pub fn router(engine: Arc<Engine>) -> Router {
    let api = Router::new()
        .route("/health",get(|| async { Json(json!({"ok":true,"name":"dominion","api":"v1"})) }))
        .route("/config",get(config))
        .route("/runs",get(list_runs).post(create_run))
        .route("/runs/{id}",get(get_run))
        .route("/runs/{id}/resume",post(resume))
        .route("/runs/{id}/cancel",post(cancel))
        .route("/runs/{id}/approval",post(approve))
        .route("/runs/{id}/directive",post(directive))
        .route("/runs/{id}/events",get(events))
        .route("/runs/{id}/files",get(list_files))
        .route("/runs/{id}/files/{*path}",get(read_file))
        .route_layer(middleware::from_fn_with_state(Arc::clone(&engine), authorize));
    Router::new()
        .nest("/api/v1",api)
        .route("/",get(|| async { Html(include_str!("../web/index.html")) }))
        .route("/app.js",get(|| async { ([(header::CONTENT_TYPE,"text/javascript; charset=utf-8")], include_str!("../web/app.js")) }))
        .with_state(engine)
}
async fn config(State(engine): State<Arc<Engine>>) -> Json<PublicSettings> {
    Json(PublicSettings {
        model:engine.settings.main.name.clone(),
        economy_model:engine.settings.economy.name.clone(),
        configured:!engine.settings.main.api_key.is_empty() || engine.settings.main.base_url.contains("localhost") || engine.settings.main.base_url.contains("127.0.0.1"),
        shell_enabled:engine.settings.shell_enabled,max_steps:engine.settings.max_steps,
    })
}
async fn list_runs(State(engine): State<Arc<Engine>>) -> Json<Vec<Run>> { Json(engine.store.list().await) }
async fn create_run(State(engine): State<Arc<Engine>>, Json(req): Json<CreateRun>) -> Result<(StatusCode,Json<Run>), ApiError> {
    let run = Run::new(req,engine.settings.max_steps).map_err(bad)?;
    engine.store.put(run.clone()).await.map_err(bad)?;
    engine.launch(run.id).await.map_err(bad)?;
    Ok((StatusCode::CREATED,Json(engine.store.get(run.id).await.unwrap_or(run))))
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
#[derive(Default,Deserialize)]
struct EventQuery { since: Option<u64> }
async fn events(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>,Query(params):Query<EventQuery>) -> Result<Sse<impl Stream<Item=Result<Event,Infallible>>>,ApiError> {
    if engine.store.get(id).await.is_none() {return Err(not_found());}
    let mut seq = params.since.unwrap_or(0);
    let output = async_stream::stream! {
        loop {
            match engine.store.get(id).await {
                Some(run) => {
                    for record in run.events.iter().filter(|event|event.seq > seq) {
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
    let items:Value=serde_json::from_str(&data).map_err(bad)?;
    Ok(Json(json!({"files":items})))
}
async fn read_file(State(engine):State<Arc<Engine>>,Path((id,path)):Path<(Uuid,String)>) -> Result<Json<Value>,ApiError> {
    if engine.store.get(id).await.is_none(){return Err(not_found());}
    let content=tools::execute(&engine.settings,id,&crate::domain::ToolCall{
        id:"api-read".into(),name:"read_file".into(),arguments:json!({"path":path})
    }).await.map_err(bad)?;
    Ok(Json(json!({"path":path,"content":content})))
}
