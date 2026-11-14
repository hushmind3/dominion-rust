//! # Dominion Rust SDK
//!
//! Native asynchronous Rust client for the **independent Dominion Rust agent
//! runtime**. Supports durable goals, sessions, reviews/approvals, evidence,
//! chat SSE and resumable **read-only** event subscriptions.
//!
//! This is a Rust-native SDK crate, not a Python/TypeScript wrapper and not a
//! vendor agent runtime. Dominion's separate Rust service remains responsible
//! for model selection, tools, permissions, durable execution, and RSI.
//!
//! ## Example
//!
//! ```no_run
//! use dominion_sdk::{DominionClient, GoalCreate, WaitOptions};
//!
//! # async fn example() -> dominion_sdk::Result<()> {
//! let sdk = DominionClient::new("http://127.0.0.1:4317")?;
//! let goal = sdk.create_goal(GoalCreate::new("Find and fix the regression")
//!     .max_iterations(16)).await?;
//! let final_goal = sdk.wait_goal(&goal.id, WaitOptions::default()).await?;
//! println!("{}: {}", final_goal.id, final_goal.status);
//! # Ok(()) }
//! ```

mod client;
mod error;
mod streaming;
mod types;

pub use client::{DominionClient, DominionThread, GoalHandle, ThreadResult};
pub use error::{DominionError, Result};
pub use streaming::{ChatStream, EventStream};
pub use types::{
    ChatEvent, ChatRequest, Goal, GoalCreate, Job, Message, ModelSelection,
    ServerEvent, Session, SessionCreate, WaitOptions, WatchOptions,
};
