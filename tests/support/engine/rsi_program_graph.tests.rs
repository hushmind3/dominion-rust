#[cfg(test)]
mod tests {
    use super::*;
    fn frontier()->Vec<Value> {
        vec![json!({"role":"memory","domain":"memory",
                "path":"src/engine/memory.rs","sourceSha256":"a".repeat(64)}),
             json!({"role":"improvement_methods","domain":"improver",
                "path":"src/engine/rsi_program_graph.rs","sourceSha256":"b".repeat(64)}),
             json!({"role":"new_role","domain":"improver",
                "path":"src/engine/new_role.rs","sourceSha256":"c".repeat(64)})]
    }
    fn authored()->Value {
        normalize(&json!({
            "entry":"invent",
            "nodes":[
              {"id":"invent","role":"memory","domain":"memory",
               "path":"src/engine/memory.rs",
               "objective":"Redesign the memory source retrieval strategy",
               "mechanism":"Learn a different algorithm from real retrieval failures"},
              {"id":"new_algorithm","role":"new_role","domain":"improver",
               "path":"src/engine/new_role.rs",
               "objective":"Use the new subsystem to synthesize better search rules",
               "mechanism":"Transfer successful retrieval ideas to a new algorithmic role"},
              {"id":"rewrite_program","role":"improvement_methods","domain":"improver",
               "path":"src/engine/rsi_program_graph.rs",
               "objective":"Rewrite the algorithm for choosing future improvement programs",
               "mechanism":"Use independently measured successor failures to replace graph logic"}
            ],
            "edges":[{"from":"invent","to":"new_algorithm","when":"improved"},
                     {"from":"invent","to":"rewrite_program","when":"neutral"},
                     {"from":"new_algorithm","to":"rewrite_program","when":"adopted"},
                     {"from":"rewrite_program","to":"invent","when":"adopted"}]
        })).unwrap()
    }
    fn parent(id:&str,head:&str,generator:&str,program:Value,plan:Value)->Value {
        json!({"id":id,"jobId":format!("job-{id}"),"candidateId":format!("cand-{id}"),
            "generatorCommit":generator,"acceptedCommit":head,
            "accepted":true,"pairedRevalidated":true,
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary",
                "generatorCommit":generator},
            "evaluation":{"metrics":{"eligible":true,"improvedHoldouts":1}},
            "changedSourcePaths":["src/engine/memory.rs"],
            "acceptedSourceHashes":{"src/engine/memory.rs":"c".repeat(64)},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":generator,"generationJobId":format!("job-{id}"),
                "openProgram":program},
            "openImprovementProgram":plan})
    }
    #[test]
    fn accepted_model_writes_open_graph_that_compiled_descendant_inherits(){
        let p=authored();let f=frontier();let zero="0".repeat(40);let one="1".repeat(40);
        let first=parent("first",&one,&zero,p.clone(),Value::Null);
        let plan=synthesize(&[first.clone()],&f,
            &["memory","improver"],&one);
        assert_eq!(plan["nodeId"],"invent");
        assert_eq!(plan["originGenerationId"],"first");
        assert!(valid(&plan,&[first],&f,&one));
    }
    #[test]
    fn invented_roles_and_conditional_graphs_are_not_a_fixed_stage_list(){
        let p=authored();let f=frontier();
        let zero="0".repeat(40);let one="1".repeat(40);let two="2".repeat(40);
        let first=parent("first",&one,&zero,p,Value::Null);
        let plan=synthesize(&[first.clone()],&f,
            &["memory","improver"],&one);
        let mut child=parent("child",&two,&one,Value::Null,plan.clone());
        child["compiledPlanExecuted"]=json!({"source":"running_compiled_descendant_binary",
            "generatorCommit":one,"openImprovementRevision":plan["revision"],
            "metaRevision":"m"});
        child["metaPolicy"]=json!({"selectionMode":"authored_open_improvement_program",
            "openImprovementRevision":plan["revision"],"revision":"m",
            "target":plan["target"]});
        let history=vec![first,child.clone()];
        let next=synthesize(&history,&f,&["memory","improver"],&two);
        assert_eq!(next["nodeId"],"new_algorithm");
        assert!(valid(&next,&history,&f,&two),
            "a new role target must execute if its current source is indexed");
    }
    #[test]
    fn independently_measured_non_gain_can_rewrite_the_program_not_repeat_improvement(){
        let program=authored();let f=frontier();
        let zero="0".repeat(40);let one="1".repeat(40);let two="2".repeat(40);
        let first=parent("seed",&one,&zero,program,Value::Null);
        let first_plan=synthesize(&[first.clone()],&f,&["memory","improver"],&one);
        let mut observed=parent("neutral",&two,&one,Value::Null,first_plan.clone());
        observed["evaluation"]["metrics"]["improvedHoldouts"]=json!(0);
        observed["compiledPlanExecuted"]=json!({
            "source":"running_compiled_descendant_binary",
            "generatorCommit":one,"metaRevision":"m",
            "openImprovementRevision":first_plan["revision"]});
        observed["metaPolicy"]=json!({
            "selectionMode":"authored_open_improvement_program","revision":"m",
            "openImprovementRevision":first_plan["revision"],
            "target":first_plan["target"]});
        let history=vec![first,observed];
        let next=synthesize(&history,&f,&["memory","improver"],&two);
        assert_eq!(next["nodeId"],"rewrite_program");
        assert_eq!(next["target"]["path"],"src/engine/rsi_program_graph.rs");
        assert!(valid(&next,&history,&f,&two));
    }
    #[test]
    fn stale_rebound_source_is_rejected_even_with_recomputed_revision(){
        let program=authored();let f=frontier();let one="1".repeat(40);
        let first=parent("seed",&one,&"0".repeat(40),program,Value::Null);
        let mut selected=synthesize(&[first.clone()],&f,&["memory","improver"],&one);
        selected["target"]["sourceSha256"]=json!("0".repeat(64));
        let mut unsigned=selected.clone();
        unsigned.as_object_mut().unwrap().remove("revision");
        selected["revision"]=json!(sha(&unsigned));
        assert!(!valid(&selected,&[first],&f,&one));
    }
    #[test]
    fn an_adopted_program_rewrite_counts_only_after_the_new_program_executes(){
        let old=authored();
        let mut new=old.clone();
        // Rewrite not only content but its conditional improvement structure:
        // future branches can evolve rather than follow preset phases.
        new["edges"][0]["to"]=json!("rewrite_program");
        new["revision"]=Value::Null;
        let mut unsigned=new.clone();
        unsigned.as_object_mut().unwrap().remove("revision");
        new["revision"]=json!(sha(&unsigned));
        assert!(valid_authored(&new));
        let f=frontier();let zero="0".repeat(40);let one="1".repeat(40);
        let two="2".repeat(40);let three="3".repeat(40);
        let first=parent("first",&one,&zero,old.clone(),Value::Null);
        let p1=synthesize(&[first.clone()],&f,&["memory","improver"],&one);
        let mut author=parent("rewriter",&two,&one,new.clone(),p1.clone());
        author["metaPolicy"]=json!({
            "selectionMode":"authored_open_improvement_program",
            "revision":"m","openImprovementRevision":p1["revision"],
            "target":p1["target"]
        });
        author["compiledPlanExecuted"]=json!({
            "source":"running_compiled_descendant_binary",
            "generatorCommit":one,"metaRevision":"m",
            "openImprovementRevision":p1["revision"]
        });
        author["changedSourcePaths"]=json!([
            "src/engine/memory.rs","src/engine/rsi_program_graph.rs"]);
        author["acceptedSourceHashes"]["src/engine/rsi_program_graph.rs"]
            =json!("b".repeat(64));
        let pre=vec![first.clone(),author.clone()];
        let p2=synthesize(&pre,&f,&["memory","improver"],&two);
        assert_eq!(p2["programSha256"],new["revision"]);
        assert!(valid(&p2,&pre,&f,&two));
        let mut execution=parent("new-method-exec",&three,&two,Value::Null,p2.clone());
        execution["metaPolicy"]=json!({
            "selectionMode":"authored_open_improvement_program",
            "revision":"q","openImprovementRevision":p2["revision"],
            "target":p2["target"]
        });
        execution["compiledPlanExecuted"]=json!({
            "source":"running_compiled_descendant_binary",
            "generatorCommit":two,"metaRevision":"q",
            "openImprovementRevision":p2["revision"]
        });
        let mut history=vec![first,author,execution];
        assert_eq!(handoff(&history)["executedProgramRewrites"],1);
        history[2]["compiledPlanExecuted"]["openImprovementRevision"]
            =json!("wrong");
        assert_eq!(handoff(&history)["executedProgramRewrites"],0);
    }
    #[test]
    fn quality_learned_from_measured_task_cases_overrules_alphabetic_branch_order(){
        let mut program=authored();
        // Both branches are legal after adoption. A bad mechanism has the
        // alphabetically first ID, but independently measured quality favors
        // the other arm. No static A->B->A selector can fake that feedback.
        program["edges"]=json!([
            {"from":"invent","to":"new_algorithm","when":"adopted"},
            {"from":"invent","to":"rewrite_program","when":"adopted"}
        ]);
        program["nodes"][1]["mechanism"]=json!("stagnant procedure without gains");
        program["nodes"][2]["mechanism"]=json!("repair executable source with measured feedback");
        let mut unsigned=program.clone();
        unsigned.as_object_mut().unwrap().remove("revision");
        program["revision"]=json!(sha(&unsigned));
        let f=frontier();
        let zero="0".repeat(40);let one="1".repeat(40);let two="2".repeat(40);
        let creator=parent("creator",&one,&zero,program.clone(),Value::Null);
        let first_plan=synthesize(&[creator.clone()],&f,
            &["memory","improver"],&one);
        let mut runner=parent("runner",&two,&one,Value::Null,first_plan.clone());
        runner["metaPolicy"]=json!({
            "selectionMode":"authored_open_improvement_program",
            "revision":"m","openImprovementRevision":first_plan["revision"],
            "target":first_plan["target"]
        });
        runner["compiledPlanExecuted"]=json!({
            "source":"running_compiled_descendant_binary",
            "generatorCommit":one,"metaRevision":"m",
            "openImprovementRevision":first_plan["revision"]
        });
        let measured=|id:&str,mechanism:&str,after:bool|{
            let revision="d".repeat(64);
            let role=if mechanism.starts_with("stagnant"){"new_role"}
                else{"improvement_methods"};
            let path=if role=="new_role"{"src/engine/new_role.rs"}
                else{"src/engine/rsi_program_graph.rs"};
            json!({
                "id":id,"candidateId":format!("measured-{id}"),
                "generatorCommit":zero,
                "evaluationProfileSha256":"c".repeat(64),
                "pairedMeasured":true,"accepted":false,
                "changedSourcePaths":[path],
                "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                    "metrics":{"verdict":"measured",
                        "measurements":[{"id":"holdout-case","domain":"tools",
                            "holdout":true,"baselinePass":false,"candidatePass":after}]}},
                "compiledPlanExecuted":{"source":"running_compiled_descendant_binary",
                    "generatorCommit":zero,"metaRevision":"m",
                    "openImprovementRevision":revision.clone()},
                "metaPolicy":{"revision":"m","selectionMode":"authored_open_improvement_program",
                    "target":{"path":path,"role":role},
                    "openImprovementRevision":revision.clone()},
                "openImprovementProgram":{
                    "source":"compiled_descendant_open_improvement_program_v1",
                    "revision":revision,"target":{"path":path,"role":role},
                    "node":{"mechanism":mechanism}}
            })
        };
        // Two independent trials each: one repair strategy works and the
        // competing low-ranked mechanism yields neutral outcomes.
        let prior=vec![
            measured("bad-a","stagnant procedure without gains",false),
            measured("bad-b","stagnant procedure without gains",false),
            measured("good-a","repair executable source with measured feedback",true),
            measured("good-b","repair executable source with measured feedback",true),
            creator,runner];
        let next=synthesize(&prior,&f,&["memory","improver"],&two);
        assert_eq!(next["nodeId"],"rewrite_program",
            "verified task quality must select the stronger evidence-backed method");
        assert_eq!(next["principleLearningSha256"],
            crate::engine::rsi_principle_learning::study(&prior)["sha256"]);
        assert!(valid(&next,&prior,&f,&two));
    }
    #[test]
    fn no_independent_adoption_never_mints_a_next_generation_program(){
        let mut first=parent("first",&"1".repeat(40),&"0".repeat(40),
            authored(),Value::Null);
        first["pairedRevalidated"]=json!(false);
        assert!(synthesize(&[first],&frontier(),
            &["memory","improver"],&"1".repeat(40)).is_null());
    }
}
