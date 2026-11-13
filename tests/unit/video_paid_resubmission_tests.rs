use super::*;
use serde_json::json;

#[test]
fn paid_remote_receipts_stop_replay_even_after_download() {
    assert!(!unsafe_to_resubmit(&json!({"videoState":{"inFlight":false,
        "completedClips":0,"taskIds":[]}})));
    assert!(unsafe_to_resubmit(&json!({"videoState":{"inFlight":true,
        "completedClips":0}})));
    assert!(unsafe_to_resubmit(&json!({"videoState":{"inFlight":false,
        "completedClips":1,"taskIds":["receipt"]}})));
    assert!(unsafe_to_resubmit(&json!({"videoState":{"inFlight":false,
        "completedClips":0,"taskIds":["submitted"]}})));
}
