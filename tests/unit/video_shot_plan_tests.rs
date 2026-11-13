use super::*;
use serde_json::Value;

#[test]
fn all_paid_clip_lengths_have_exact_multishot_sums() {
    for seconds in 3..=15 {
        let rows = shots("same red bicycle, evening", "ride across a bridge", seconds, 0, 3);
        assert!((1..=3).contains(&rows.len()));
        assert!(rows.iter().all(|row| row["duration"].as_u64().unwrap_or(0) >= 1));
        assert_eq!(rows.iter().map(|row| row["duration"].as_u64().unwrap()).sum::<u64>(), seconds);
        assert!(rows.iter().all(|row| row["prompt"].as_str().unwrap().chars().count() <= 512));
        let payload=request("kling3.0_pro", "1920:1080", seconds, true, &rows);
        if rows.len() > 1 {
            assert!(payload.get("promptText").is_none());
            assert_eq!(payload["providerSettings"]["multiPrompt"], Value::Array(rows));
        } else {
            assert!(payload["promptText"].is_string());
            assert!(payload.get("providerSettings").is_none());
        }
    }
}

#[test]
fn shot_plan_repeats_identity_without_variable_request_overhead() {
    let rows=shots("A scarlet train in snow", "Train enters alpine tunnel", 15, 1, 4);
    assert_eq!(rows.len(),3);
    for row in rows {
        let text=row["prompt"].as_str().unwrap();
        assert!(text.contains("scarlet train"));
        assert!(text.contains("alpine tunnel"));
        assert!(text.contains("Film 2/4"));
    }
}
