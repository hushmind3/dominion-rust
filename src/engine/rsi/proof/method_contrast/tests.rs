use super::*;
#[test]
fn neither_name_nor_meta_label_is_an_executed_method() {
    assert!(!actual_method_use(&json!({"methodSnapshot":{"methodPath":"source.rs"},
        "methodActions":[],"methodExecutions":[]})));
    assert!(!actual_method_use(&json!({"methodActions":[
        {"decision":null,"inducedProgramDecision":null}]})));
}
#[test]
fn only_recorded_tool_or_verified_procedure_can_satisfy_execution_part() {
    assert!(actual_method_use(&json!({"methodExecutions":[{
        "result":{"verified":true,"followed":true}}]})));
    assert!(actual_method_use(&json!({"methodActions":[{
        "executionId":"run-1","inducedProgramDecision":{"chosenTool":"workspace_replace_range"}}]})));
    assert!(!actual_method_use(&json!({"methodExecutions":[{
        "result":{"verified":true,"followed":false}}]})));
}
