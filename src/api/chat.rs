use crate::engine;
use crate::state::AppState;
use axum::{extract::{State},response::{IntoResponse,Response,sse::{Event,KeepAlive,Sse}},Json};
use serde_json::{Value};
use std::convert::Infallible;
use tokio::sync::mpsc;

pub async fn chat(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let (sender, mut receiver) = mpsc::unbounded_channel::<Value>();
    let task_state = state.clone();
    tokio::spawn(async move {
        engine::chat::run(task_state, body, sender).await;
    });

    let stream = async_stream::stream! {
        while let Some(event) = receiver.recv().await {
            yield Ok::<_, Infallible>(Event::default().data(event.to_string()));
        }
    };

    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

