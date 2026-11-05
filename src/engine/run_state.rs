//! H4 RunCoordinator phase reducer.
//!
//! The execution record's `status` is the durable outcome; `phase` is the
//! lifecycle position used by recovery. Transitions are explicit so a crashed
//! or duplicated run cannot jump from a terminal state back into executing.
use crate::state::AppState;
use serde_json::{json, Value};

pub const QUEUED: &str = "queued";
pub const RUNNING: &str = "running";
pub const WAITING_APPROVAL: &str = "waiting_approval";
pub const VERIFYING: &str = "verifying";
pub const FINALIZING: &str = "finalizing";
pub const SUCCEEDED: &str = "succeeded";
pub const FAILED: &str = "failed";
pub const CANCELLED: &str = "cancelled";
pub const INTERRUPTED: &str = "interrupted";

pub fn is_terminal(phase: &str) -> bool {
    matches!(phase, SUCCEEDED | FAILED | CANCELLED | INTERRUPTED)
}

/// Returns the next phase, or None for an illegal transition. Repeating the
/// current phase is treated as idempotent.
pub fn transition(current: &str, event: &str) -> Option<&'static str> {
    let next = match event {
        "start" => match current {
            QUEUED | RUNNING => RUNNING,
            _ => return None,
        },
        "approval_requested" => match current {
            RUNNING | WAITING_APPROVAL => WAITING_APPROVAL,
            _ => return None,
        },
        "approval_resolved" => match current {
            WAITING_APPROVAL | RUNNING => RUNNING,
            _ => return None,
        },
        "verify" => match current {
            RUNNING | VERIFYING => VERIFYING,
            _ => return None,
        },
        "finalize" => match current {
            RUNNING | VERIFYING | WAITING_APPROVAL | FINALIZING => FINALIZING,
            _ => return None,
        },
        "succeed" => match current {
            FINALIZING | RUNNING | VERIFYING | SUCCEEDED => SUCCEEDED,
            _ => return None,
        },
        "fail" => match current {
            _ if is_terminal(current) => return None,
            _ => FAILED,
        },
        "cancel" => match current {
            _ if current == CANCELLED => CANCELLED,
            _ if is_terminal(current) => return None,
            _ => CANCELLED,
        },
        "interrupt" => match current {
            _ if current == INTERRUPTED => INTERRUPTED,
            _ if is_terminal(current) => return None,
            _ => INTERRUPTED,
        },
        _ => return None,
    };
    Some(next)
}

pub fn phase_for_status(status: &str) -> &'static str {
    match status {
        "completed" => SUCCEEDED,
        "cancelled" => CANCELLED,
        "interrupted" => INTERRUPTED,
        _ => FAILED,
    }
}

/// Applies one event to the durable execution record. An illegal transition is
/// audited and leaves the phase unchanged instead of corrupting recovery.
pub fn apply(state: &AppState, execution_id: &str, event: &str) -> Option<Value> {
    let mut execution = state.store.get("execution", execution_id)?;
    let current = execution["phase"].as_str().unwrap_or(QUEUED).to_string();
    let Some(next) = transition(&current, event) else {
        state.store.audit(
            "run.phase_rejected",
            json!({"executionId": execution_id, "phase": current, "event": event}),
        );
        return Some(execution);
    };
    if next == current {
        return Some(execution);
    }
    execution["phase"] = json!(next);
    execution["phaseUpdatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    let saved = state.store.put("execution", &execution)?;
    state.bus.record("execution", &saved);
    Some(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_cannot_restart_after_a_terminal_phase() {
        let mut phase = QUEUED;
        for event in ["start", "approval_requested", "approval_resolved", "verify", "finalize", "succeed"] {
            phase = transition(phase, event).expect(event);
        }
        assert_eq!(phase, SUCCEEDED);
        for event in ["start", "finalize", "interrupt"] {
            assert!(transition(phase, event).is_none(), "{event}");
        }
    }

    #[test]
    fn crash_recovery_can_interrupt_running_and_waiting_runs() {
        assert_eq!(transition(RUNNING, "interrupt"), Some(INTERRUPTED));
        assert_eq!(
            transition(WAITING_APPROVAL, "interrupt"),
            Some(INTERRUPTED)
        );
        assert_eq!(transition(FINALIZING, "interrupt"), Some(INTERRUPTED));
        assert!(transition(INTERRUPTED, "start").is_none());
    }

    #[test]
    fn final_status_maps_to_one_terminal_phase() {
        assert_eq!(phase_for_status("completed"), SUCCEEDED);
        assert_eq!(phase_for_status("cancelled"), CANCELLED);
        assert_eq!(phase_for_status("interrupted"), INTERRUPTED);
        assert_eq!(phase_for_status("failed"), FAILED);
    }
}
