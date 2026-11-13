use super::*;
use serde_json::json;

#[test]
fn explicit_full_room_preserves_owner_turns() {
    let full=json!({"economyMode":"full","routingMode":"discussion","rounds":5});
    assert_eq!(effective_rounds(&full,"안녕하세요",false,5,false),5);
    assert!(!active(&full));
}
#[test]
fn adaptive_policy_bounds_simple_and_complex_discussion() {
    let room=json!({"economyMode":"adaptive","routingMode":"discussion",
        "economyKernel":{"parameters":{"casualSpeakers":1,
        "mediumSpeakers":2,"deepSpeakers":5,"deepRounds":2}}});
    assert!(active(&room));
    assert_eq!(target_speakers(&room,false,5),1);
    assert_eq!(target_speakers(&room,true,500),5);
    assert_eq!(effective_rounds(&room,"안녕하세요",false,4,false),1);
    assert_eq!(effective_rounds(&room,"분석",false,4,true),2);
    assert_eq!(effective_rounds(&room,"분석",true,4,true),4);
    assert_eq!(effective_rounds(&room,"모두 분석해주세요",false,4,true),4);
}
#[test]
fn invalid_or_missing_algorithm_parameters_do_not_expand_work() {
    let room=json!({"economyMode":"adaptive","routingMode":"discussion",
        "economyKernel":{"parameters":{"deepSpeakers":999999,"deepRounds":0}}});
    assert_eq!(target_speakers(&room,true,300),32);
    assert_eq!(effective_rounds(&room,"분석",false,2,true),1);
}
