use crate::{client::DominionClient, error::{DominionError, Result}, types::{ChatEvent, ServerEvent, WatchOptions}};
use async_stream::try_stream;
use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt};
use reqwest::{Response, StatusCode};
use std::{pin::Pin, time::Duration};

pub type ChatStream = Pin<Box<dyn Stream<Item = Result<ChatEvent>> + Send>>;
pub type EventStream = Pin<Box<dyn Stream<Item = Result<ServerEvent>> + Send>>;

/// Chat POST is *never* re-issued: a disconnected client cannot know whether
/// a workspace edit, command or purchase already completed.
pub(crate) fn chat_stream(response: Response) -> Result<ChatStream> {
    let output = try_stream! {
        let mut source = response.bytes_stream().eventsource();
        let mut completed = false;
        while let Some(frame) = source.next().await {
            let frame = frame.map_err(|error| DominionError::Protocol(error.to_string()))?;
            let event: ChatEvent = serde_json::from_str(&frame.data)?;
            if event.kind == "error" {
                let message = event.field("message")
                    .or_else(||event.field("error"))
                    .and_then(|value| value.as_str()).unwrap_or("agent turn failed");
                Err(DominionError::Protocol(message.into()))?;
            }
            let terminal = event.kind == "done";
            yield event;
            if terminal {
                completed = true;
                break;
            }
        }
        if !completed {
            Err(DominionError::Interrupted)?;
        }
    };
    Ok(Box::pin(output))
}

/// Durable GET /events can safely reconnect from the last observed sequence.
/// The caller must persist that cursor after handling side effects. The server
/// replays at most 500 records on a connection; catch-up gaps require snapshot
/// reconciliation by the caller.
pub(crate) fn events_stream(client: DominionClient, options: WatchOptions) -> EventStream {
    let output = try_stream! {
        let mut cursor = options.after;
        let mut reconnects = 0_usize;
        loop {
            match client.open_event_stream(cursor).await {
                Ok(response) => {
                    let mut source = response.bytes_stream().eventsource();
                    while let Some(frame) = source.next().await {
                        let frame = match frame {
                            Ok(frame) => frame,
                            Err(_) => break, // transport disconnected; safe GET cursor replay
                        };
                        let record: ServerEvent = serde_json::from_str(&frame.data)?;
                        if record.seq == 0 {
                            Err(DominionError::Protocol("event sequence must be positive".into()))?;
                        }
                        if !frame.id.is_empty() && frame.id != record.seq.to_string() {
                            Err(DominionError::Protocol("event ID and sequence disagree".into()))?;
                        }
                        if record.seq > cursor {
                            cursor = record.seq;
                            reconnects = 0;
                            yield record;
                        }
                    }
                }
                Err(error) => {
                    let fatal = matches!(&error, DominionError::Http {status, ..}
                        if status.is_client_error() || *status == StatusCode::NOT_IMPLEMENTED)
                        || matches!(&error, DominionError::Configuration(_) | DominionError::Protocol(_));
                    if fatal || !options.reconnect { Err(error)?; }
                }
            }
            if !options.reconnect { break; }
            if options.max_reconnects.is_some_and(|limit| reconnects >= limit) {
                Err(DominionError::Interrupted)?;
            }
            let factor = 1u32 << reconnects.min(5);
            reconnects = reconnects.saturating_add(1);
            let pause = options.retry_delay.saturating_mul(factor)
                .max(Duration::from_millis(20)).min(Duration::from_secs(30));
            tokio::time::sleep(pause).await;
        }
    };
    Box::pin(output)
}
