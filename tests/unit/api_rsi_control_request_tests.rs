use super::*;
#[test]
fn only_real_approved_control_modifies_the_input(){
    let mut input=json!({"prompt":"original","rsiObjective":"memory"});
    apply_input(&mut input,&Value::Null,&Value::Null);
    assert_eq!(input["prompt"],"original");
    assert!(input["rsiCausalControlCommit"].is_null());
    let anchor=json!({"treatedJobId":"job-b","treatedGenerationId":"gen-b",
        "treatedCampaignId":"campaign-b","methodOffGeneratorCommit":"a".repeat(40)});
    let treated=json!({"rsiPromptBase":"frozen B","rsiObjective":"memory",
        "rsiFrozenCases":[{"id":"holdout"}]});
    apply_input(&mut input,&anchor,&treated);
    assert_eq!(input["rsiCausalControlOf"],"job-b");
    assert_eq!(input["rsiFrozenCases"],treated["rsiFrozenCases"]);
    assert_eq!(input["prompt"],"frozen B");
}
