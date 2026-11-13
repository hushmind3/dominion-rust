use super::*;

#[test]
fn video_state_preserves_paid_receipts_and_frozen_scenes_between_stages() {
    let old=json!({"videoState":{
        "scenes":["same lead","second beat"],
        "shotPlans":[[{"prompt":"shot 1","duration":15}]],
        "taskIds":["remote-task-id"],
        "inFlight":true,
        "remoteTaskId":"remote-task-id",
        "completedClips":0
    }});
    let mut update=json!({"phase":"rendering","videoState":{
        "inFlight":false,"remoteTaskId":null,"completedClips":1
    }});
    merge_video_state(&old,&mut update);
    assert_eq!(update["videoState"]["scenes"],old["videoState"]["scenes"]);
    assert_eq!(update["videoState"]["shotPlans"],old["videoState"]["shotPlans"]);
    assert_eq!(update["videoState"]["taskIds"],old["videoState"]["taskIds"]);
    assert_eq!(update["videoState"]["inFlight"],false);
    assert!(update["videoState"]["remoteTaskId"].is_null());
    assert_eq!(update["videoState"]["completedClips"],1);
    assert!(unsafe_to_resubmit(&json!(update)));
}

#[test]
fn video_state_does_not_rewrite_unrelated_checkpoint_fields() {
    let old=json!({"videoState":{"taskIds":["paid"],"inFlight":false}});
    let mut update=json!({"progress":99});
    merge_video_state(&old,&mut update);
    assert!(update.get("videoState").is_none());
    assert_eq!(update["progress"],99);
}
