use super::*;
#[test]
fn control_starts_from_pre_method_revision_and_treated_task(){
    let original=json!({"rsiSourceDir":"/project","rsiBaseCommit":"f".repeat(40),
        "limits":{"tokenBudget":50000,"timeBudgetSeconds":1800}});
    let anchor=json!({"methodOffGeneratorCommit":"a".repeat(40),
        "treatedGenerationId":"B","treatedCampaignId":"original",
        "treatedJobId":"job-B"});
    let treated=json!({"id":"job-B","kind":"rsi_generation","input":{
        "rsiPromptBase":"original task", "rsiObjective":"work on memory",
        "rsiFrozenCases":[{"id":"frozen"}],"rsiRemainingCampaignTokens":40000,
        "providerId":"actual-provider","model":"actual-model",
        "rsiSourceBranch":"dominion-sub4"}});
    let result=scheduled_input(&original,&anchor,&treated).unwrap();
    assert_eq!(result["rsiCausalControlCommit"],"a".repeat(40));
    assert_eq!(result["rsiCausalControlOf"],"job-B");
    assert_eq!(result["prompt"],"original task");
    assert_eq!(result["rsiFrozenCases"],treated["input"]["rsiFrozenCases"]);
    assert_eq!(result["providerId"],"actual-provider");
    assert_eq!(result["limits"]["maxAttempts"],1);
    assert_eq!(result["limits"]["tokenBudget"],40000);
    assert_eq!(result["selectionPolicy"],"automatic");
    assert_eq!(result["rsiAutoMethodControl"],false);
}
#[test]
fn undersized_control_budget_does_not_generate_unfair_comparison(){
    let anchor=json!({"methodOffGeneratorCommit":"a".repeat(40),"treatedJobId":"B"});
    let bad=json!({"id":"B","kind":"rsi_generation","input":{
       "rsiPromptBase":"task","rsiFrozenCases":[{"id":"case"}],
       "rsiRemainingCampaignTokens":500}});
    assert!(scheduled_input(&json!({}),&anchor,&bad).is_none());
}
