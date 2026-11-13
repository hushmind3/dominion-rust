use super::*;

fn measured(a:bool,b:bool)->Value{
    json!({"evaluation":{"metrics":{"measurements":[
        {"id":"memory-holdout","domain":"memory","holdout":true,"baselinePass":false,"candidatePass":a},
        {"id":"tools-holdout","domain":"tools","holdout":true,"baselinePass":true,"candidatePass":b},
        {"id":"meta-holdout","domain":"improver","holdout":true,"baselinePass":true,"candidatePass":true}
    ]}}})
}
#[test]
fn source_control_requires_same_frozen_cases_and_real_downstream_transition(){
    assert_eq!(contrast(&measured(true,true),&measured(false,true)),Some((1,0,2)));
    assert_eq!(contrast(&measured(false,true),&measured(true,true)),Some((0,1,2)));
    assert_eq!(contrast(&measured(true,true),&measured(true,true)),Some((0,0,2)));
    let mut forged=measured(false,true);
    forged["evaluation"]["metrics"]["measurements"][0]["baselinePass"]=json!(true);
    assert!(contrast(&measured(true,true),&forged).is_none());
    forged=measured(false,true);
    forged["evaluation"]["metrics"]["measurements"][0]["id"]=json!("fake");
    assert!(contrast(&measured(true,true),&forged).is_none());
}
#[test]
fn same_task_allows_different_policy_chosen_files_not_different_prompt_or_budget(){
    let mut control=json!({"input":{
        "rsiFrozenCases":[{"id":"frozen"}],"rsiPromptBase":"same task",
        "rsiObjective":"memory","rsiTargetDomain":"memory",
        "reasoningEffort":"high","rsiSourceBranch":"dominion-sub4",
        "rsiRemainingCampaignTokens":12000,"rsiFocusFiles":["src/engine/memory.rs"]
    }});
    let mut treated=control.clone();
    treated["input"]["rsiFocusFiles"]=json!(["src/engine/room_state.rs"]);
    treated["input"]["rsiRemainingCampaignTokens"]=json!(8000);
    assert!(same_task(&control,&treated));
    control["input"]["rsiRemainingCampaignTokens"]=json!(7000);
    assert!(!same_task(&control,&treated));
    control["input"]["rsiRemainingCampaignTokens"]=json!(12000);
    control["input"]["rsiPromptBase"]=json!("different task");
    assert!(!same_task(&control,&treated));
}

#[test]
fn policy_selected_domain_is_an_observed_outcome_not_a_matching_constraint(){
    let mut pre=json!({"input":{"rsiFrozenCases":[{"id":"frozen"}],
        "rsiPromptBase":"same task","rsiObjective":"improve capability",
        "rsiTargetDomain":"memory","reasoningEffort":"high",
        "rsiSourceBranch":"dominion-sub4",
        "rsiRemainingCampaignTokens":40000}});
    let mut on=pre.clone();
    on["input"]["rsiTargetDomain"]=json!("tools");
    assert!(same_task(&pre,&on),
        "the inherited method is allowed to select a different capability");
    pre["input"]["rsiObjective"]=json!("different task");
    assert!(!same_task(&pre,&on));
}

#[test]
fn preserving_previously_passing_guard_is_not_novel_capability(){
    let treated=measured(true,true);
    let mut control=measured(true,false);
    assert_eq!(contrast(&treated,&control),Some((0,0,2)));
    control["evaluation"]["metrics"]["measurements"][0]["candidatePass"]=json!(false);
    assert_eq!(contrast(&treated,&control),Some((1,0,2)));
}
