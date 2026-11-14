use reqwest::StatusCode;
use thiserror::Error;

/// The SDK distinguishes transport failures, rejected engine operations,
/// invalid wire data, and interrupted streams. It never silently replays writes.
#[derive(Debug, Error)]
pub enum DominionError {
    #[error("native Dominion runtime error: {0}")]
    Runtime(String),
    #[error("Dominion I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid Dominion SDK configuration: {0}")]
    Configuration(String),
    #[error("Dominion HTTP transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Dominion returned HTTP {status}: {message}")]
    Http { status: StatusCode, message: String },
    #[error("Dominion wire JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Dominion event stream protocol error: {0}")]
    Protocol(String),
    #[error("Dominion stream ended without a terminal event; never automatically replay side effects")]
    Interrupted,
    #[error("Timed out waiting for {resource}; the remote work was not cancelled")]
    WaitTimeout { resource: String },
}

pub type Result<T> = std::result::Result<T, DominionError>;
