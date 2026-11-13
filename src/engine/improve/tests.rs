//! Tests isolated from the production improve.rs source.
use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-improve-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = AppState::new(config, store, vault);
        state
            .store
            .put("session", &json!({"id":"s","agentId":"a"}));
        (state, dir)
    }

    #[test]
    fn only_real_experiments_promote_or_roll_back_a_strategy() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let base = json!({
            "id":"strategy-x","kind":"strategy","scope":"agent","agentId":"a",
            "partitionKey":scope.partition_key,"status":"adopted","scoreVersion":2,
            "evidenceGrade":"E1_observed","text":"run audit before report",
            "uses":5,"successes":5,"failures":0,"appliedExecutionIds":[]
        });
        state.store.put("improvement", &base);

        // Underpowered or inconclusive samples never promote.
        let small = json!({"experimentId":"exp-1","verdict":"improved","delta":0.4,"independentUnits":3,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, "strategy-x", &small).unwrap();
        assert_eq!(record["status"], "adopted");
        assert_eq!(record["evidenceGrade"], "E1_observed");
        assert_eq!(record["requiresExperiment"], true);
        assert_eq!(record["lastExperiment"]["experimentId"], "exp-1");

        let improved = json!({"experimentId":"exp-2","verdict":"improved","delta":0.3,"independentUnits":8,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, "strategy-x", &improved).unwrap();
        assert_eq!(record["status"], "validated");
        assert_eq!(record["evidenceGrade"], "E2");
        assert_eq!(record["validatedBy"], "isolated_experiment");
        assert_eq!(record["requiresExperiment"], false);
        assert!(record["experimentRevision"].as_str().is_some_and(|value| !value.is_empty()));

        let regressed = json!({"experimentId":"exp-3","verdict":"regressed","delta":-0.2,"independentUnits":8,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, "strategy-x", &regressed).unwrap();
        assert_eq!(record["status"], "rolled_back");
        assert_eq!(record["evidenceGrade"], "E2");
        assert!(record["rolledBackAt"].is_string());

        // The runner's conservative bound names a supported effect
        // `preliminary_support`; it is still an experiment promotion.
        state.store.put(
            "improvement",
            &json!({
                "id":"strategy-y","kind":"strategy","scope":"agent","agentId":"a",
                "partitionKey":scope.partition_key,"status":"adopted","scoreVersion":2,
                "evidenceGrade":"E1_observed","text":"verify output","uses":5,"successes":5,
                "failures":0,"appliedExecutionIds":[]
            }),
        );
        let supported = json!({"experimentId":"exp-4","verdict":"preliminary_support","delta":0.9,"independentUnits":10,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, "strategy-y", &supported).unwrap();
        assert_eq!(record["status"], "validated");
        assert_eq!(record["evidenceGrade"], "E2");
        assert_eq!(record["validatedBy"], "isolated_experiment");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn observed_strategies_get_one_durable_experiment_request() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        state.store.put(
            "improvement",
            &json!({
                "id":"needs-exp","kind":"strategy","scope":"agent","agentId":"a",
                "partitionKey":scope.partition_key,"status":"validated","scoreVersion":2,
                "evidenceGrade":"E1_observed","requiresExperiment":true,
                "revision":"rev-1","text":"check first","uses":6,"successes":6,
                "failures":0,"appliedExecutionIds":[]
            }),
        );
        let created = schedule_experiments(&state);
        assert_eq!(created.len(), 1);
        assert_eq!(created[0]["status"], "queued");
        assert_eq!(created[0]["minIndependentUnits"], MIN_EXPERIMENT_UNITS);
        let stored = state.store.get("improvement", "needs-exp").unwrap();
        assert_eq!(stored["experimentRequestId"], created[0]["id"]);
        // Second scheduling pass is idempotent.
        assert!(schedule_experiments(&state).is_empty());

        let experiment = json!({"experimentId":"exp-req","verdict":"preliminary_support","delta":0.4,"independentUnits":10,"oracleVersion":"v1"});
        apply_experiment_result(&state, "needs-exp", &experiment).unwrap();
        let requests = state.store.all("experiment_request");
        assert_eq!(requests[0]["status"], "completed");
        assert_eq!(requests[0]["experimentId"], "exp-req");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn canary_exposure_is_deterministic_and_partial() {
        let mut included = None;
        let mut excluded = None;
        for index in 0..200 {
            let session = format!("session-{index}");
            let hit = in_canary_bucket(&session, "strategy-1");
            if hit && included.is_none() {
                included = Some(session.clone());
            }
            if !hit && excluded.is_none() {
                excluded = Some(session.clone());
            }
            assert_eq!(hit, in_canary_bucket(&session, "strategy-1"));
        }
        assert!(included.is_some() && excluded.is_some());
        // A different strategy gets an independent bucket.
        let hits = (0..200)
            .filter(|index| in_canary_bucket(&format!("session-{index}"), "strategy-2"))
            .count();
        assert!((5..80).contains(&hits), "unexpected canary share: {hits}");
    }

    #[test]
    fn a_promoted_strategy_starts_in_canary_and_activates_after_clean_use() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        state.store.put(
            "improvement",
            &json!({
                "id":"canary-1","kind":"strategy","scope":"agent","agentId":"a",
                "partitionKey":scope.partition_key,"status":"adopted","scoreVersion":2,
                "evidenceGrade":"E1_observed","text":"verify every output","uses":5,
                "successes":5,"failures":0,"appliedExecutionIds":[]
            }),
        );
        let experiment = json!({"experimentId":"exp-canary","verdict":"preliminary_support","delta":0.5,"independentUnits":10,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, "canary-1", &experiment).unwrap();
        assert_eq!(record["rollout"], "canary");
        assert_eq!(record["canaryTurns"], 0);

        for index in 0..CANARY_SUCCESSES {
            let execution = json!({
                "id":format!("exec-canary-{index}"),"sessionId":"s","agentId":"a",
                "taskOutcome":"succeeded","appliedImprovementIds":["canary-1"],
                "strategyCompliance":[{
                    "strategyId":"canary-1","applicable":true,
                    "result":{"verified":true,"triggered":true,"followed":true}
                }]
            });
            let updates = execution_updates(&state, &execution).unwrap();
            let updated = updates.iter().find(|item| item["id"] == "canary-1").unwrap();
            state.store.put("improvement", updated);
        }
        let stored = state.store.get("improvement", "canary-1").unwrap();
        assert_eq!(stored["status"], "validated");
        assert_eq!(stored["rollout"], "active");
        assert_eq!(stored["requiresExperiment"], false);
        assert!(stored["canaryCompletedAt"].is_string());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn canary_failure_rolls_back_before_full_exposure() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        state.store.put(
            "improvement",
            &json!({
                "id":"canary-2","kind":"strategy","scope":"agent","agentId":"a",
                "partitionKey":scope.partition_key,"status":"adopted","scoreVersion":2,
                "evidenceGrade":"E1_observed","text":"use the safe path","uses":5,
                "successes":5,"failures":0,"appliedExecutionIds":[],
                "rollout":"canary","canaryTurns":1
            }),
        );
        let execution = json!({
            "id":"exec-canary-fail","sessionId":"s","agentId":"a",
            "taskOutcome":"failed","appliedImprovementIds":["canary-2"],
            "strategyCompliance":[{
                "strategyId":"canary-2","applicable":true,
                "result":{"verified":true,"triggered":true,"followed":true}
            }]
        });
        let updates = execution_updates(&state, &execution).unwrap();
        let updated = updates.iter().find(|item| item["id"] == "canary-2").unwrap();
        assert_eq!(updated["status"], "rolled_back");
        assert_eq!(updated["rollout"], "halted");
        assert!(updated["reason"].as_str().is_some_and(|text| text.contains("canary")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn production_observation_is_e1_not_experimental_e2() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        state.store.put(
            "improvement",
            &json!({
                "id":"imp-observed","kind":"strategy","scope":"agent","agentId":"a",
                "partitionKey":scope.partition_key,"status":"adopted",
                "scoreVersion":2,"evidenceGrade":"E0","text":"check outputs",
                "baselineSuccess":0.5,"uses":4,"successes":4,"failures":0,
                "appliedExecutionIds":[]
            }),
        );
        let execution = json!({
            "id":"exec-observed","sessionId":"s","agentId":"a",
            "taskOutcome":"succeeded","appliedImprovementIds":["imp-observed"],
            "strategyCompliance":[{
                "strategyId":"imp-observed","applicable":true,
                "result":{"verified":true,"triggered":true,"followed":true}
            }]
        });
        let updates = execution_updates(&state, &execution).unwrap();
        let updated = updates
            .iter()
            .find(|item| item["id"] == "imp-observed")
            .expect("observation update");
        assert_eq!(updated["uses"], 5);
        assert_eq!(updated["status"], "validated");
        assert_eq!(updated["evidenceGrade"], "E1_observed");
        assert_eq!(updated["validatedBy"], "production_observation");
        assert_eq!(updated["requiresExperiment"], true);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tool_policy_requires_failure_statistics_and_names_the_tool() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        state.store.put(
            "tool_stat",
            &json!({"id":"shell_exec","attempts":4,"successes":1,"failures":3}),
        );
        let adopted = propose(
            &state,
            &scope,
            "tool_policy",
            "shell_exec 대신 workspace_write를 먼저 사용한다",
            "셸 실패가 반복됨",
            &["m1".into()],
            Some("shell_exec"),
            "shared",
        )
        .unwrap();
        assert_eq!(adopted["status"], "adopted");
        assert!(context_block(&state, &scope)
            .unwrap()
            .contains("workspace_write"));

        let rejected = propose(
            &state,
            &scope,
            "tool_policy",
            "다른 도구를 써라",
            "근거",
            &["m2".into()],
            Some("shell_exec"),
            "shared",
        )
        .unwrap();
        assert_eq!(rejected["status"], "rejected");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn probation_rolls_back_a_regressing_improvement() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let adopted = propose(
            &state,
            &scope,
            "strategy",
            "파일 작업은 workspace_write로 먼저 시도한다",
            "실패 후 성공한 절차를 다음 턴에 재사용한다",
            &["m1".into()],
            None,
            "shared",
        )
        .unwrap();
        assert_eq!(adopted["status"], "adopted");
        for index in 0..5 {
            let execution = json!({"id":format!("e{index}"),"sessionId":"s","agentId":"a","status":"failed","toolCalls":1,"taskOutcome":"failed","appliedImprovementIds":[adopted["id"]],
                "strategyCompliance":[{"strategyId":adopted["id"],"applicable":true,
                    "result":{"verified":true,"triggered":true,"followed":false}}]});
            state.store.put("execution", &execution);
            record_execution(&state, &execution);
        }
        let stored = state
            .store
            .get("improvement", adopted["id"].as_str().unwrap())
            .unwrap();
        assert_eq!(stored["status"], "rolled_back");
        assert!(context_block(&state, &scope).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn shared_and_agent_scopes_are_separate_and_harness_is_proposal_only() {
        let (state, dir) = state();
        state
            .store
            .put("session", &json!({"id":"t","agentId":"b"}));
        let scope_a = MemoryScope::for_session(&state.store, "s", Some("a"));
        let scope_b = MemoryScope::for_session(&state.store, "t", Some("b"));
        propose(
            &state,
            &scope_a,
            "strategy",
            "A만 쓰는 전략",
            "A 에이전트의 실행에서 확인된 근거",
            &["m1".into()],
            None,
            "agent",
        )
        .unwrap();
        assert!(context_block(&state, &scope_b).is_none());
        let harness = propose(
            &state,
            &scope_a,
            "harness",
            "도구 루프에 재시도 예산을 추가한다",
            "반복 실패",
            &["m1".into()],
            None,
            "shared",
        )
        .unwrap();
        assert_eq!(harness["status"], "proposed");
        assert!(!context_block(&state, &scope_a)
            .unwrap_or_default()
            .contains("재시도 예산"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn validated_improvements_are_injected_after_probation() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let record = propose(
            &state,
            &scope,
            "strategy",
            "검증된 파일 절차를 재사용한다",
            "실패 후 성공한 절차를 다음 턴에 재사용한다",
            &["m1".into()],
            None,
            "shared",
        )
        .unwrap();
        for index in 0..5 {
            let execution = json!({"id":format!("ok{index}"),"sessionId":"s","agentId":"a","status":"completed","toolCalls":1,"taskOutcome":"succeeded","appliedImprovementIds":[record["id"]],
                "strategyCompliance":[{"strategyId":record["id"],"applicable":true,
                    "result":{"verified":true,"triggered":true,"followed":true}}]});
            state.store.put("execution", &execution);
            record_execution(&state, &execution);
        }
        let stored = state
            .store
            .get("improvement", record["id"].as_str().unwrap())
            .unwrap();
        assert_eq!(stored["status"], "validated");
        assert!(context_block(&state, &scope)
            .unwrap_or_default()
            .contains("검증된 파일 절차"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn validated_strategy_wins_over_a_similar_duplicate() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let first = propose(
            &state,
            &scope,
            "strategy",
            "파일 작업은 workspace_write로 먼저 시도한다",
            "실패 후 성공한 절차를 다음 턴에 재사용한다",
            &["m1".into()],
            None,
            "shared",
        )
        .unwrap();
        for index in 0..5 {
            let execution = json!({"id":format!("v{index}"),"sessionId":"s","agentId":"a","status":"completed","toolCalls":1,"taskOutcome":"succeeded","appliedImprovementIds":[first["id"]],
                "strategyCompliance":[{"strategyId":first["id"],"applicable":true,
                    "result":{"verified":true,"triggered":true,"followed":true}}]});
            state.store.put("execution", &execution);
            record_execution(&state, &execution);
        }
        let duplicate = propose(
            &state,
            &scope,
            "strategy",
            "파일 작업은 workspace_write를 먼저 시도한다",
            "같은 절차가 다시 관찰되어 중복 생성을 막는다",
            &["m2".into()],
            None,
            "shared",
        )
        .unwrap();
        assert_eq!(duplicate["id"], first["id"]);
        assert_eq!(duplicate["status"], "validated");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn completed_without_tool_work_does_not_validate_a_strategy() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let record = propose(
            &state,
            &scope,
            "strategy",
            "도구 없이 완료 선언하지 않는다",
            "도구 실행이 없으면 전략 검증으로 집계하지 않는다",
            &["m1".into()],
            None,
            "shared",
        )
        .unwrap();
        for index in 0..5 {
            let execution = json!({
                "id":format!("bare{index}"),"sessionId":"s","agentId":"a",
                "status":"completed","toolCalls":0,
                "appliedImprovementIds":[record["id"]]
            });
            state.store.put("execution", &execution);
            record_execution(&state, &execution);
        }
        let stored = state
            .store
            .get("improvement", record["id"].as_str().unwrap())
            .unwrap();
        assert_eq!(stored["status"], "adopted");
        assert_eq!(stored["uses"], 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn global_strategies_cross_partitions() {
        let (state, dir) = state();
        state.store.put(
            "session",
            &json!({
                "id":"d","agentId":"b",
                "channelContext":{"transport":"discord","connectionId":"bot","guildId":"g","channelId":"c"}
            }),
        );
        let local = MemoryScope::for_session(&state.store, "s", Some("a"));
        let discord = MemoryScope::for_session(&state.store, "d", Some("b"));
        let record = propose(
            &state,
            &local,
            "strategy",
            "전역 파일 절차를 재사용한다",
            "채널과 무관하게 적용할 절차",
            &["m1".into()],
            None,
            "global",
        )
        .unwrap();
        assert_eq!(record["scope"], "global");
        assert!(context_block(&state, &discord)
            .unwrap_or_default()
            .contains("전역 파일 절차"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_same_execution_is_counted_once() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let record = propose(
            &state,
            &scope,
            "strategy",
            "같은 실행을 두 번 집계하지 않는다",
            "finalization 재시도에도 카운터가 한 번만 증가해야 한다",
            &["m1".into()],
            None,
            "shared",
        )
        .unwrap();
        let execution = json!({
            "id":"exec-once","sessionId":"s","agentId":"a","status":"completed",
            "toolCalls":1,"taskOutcome":"succeeded","appliedImprovementIds":[record["id"]],
            "strategyCompliance":[{
                "strategyId":record["id"],"applicable":true,
                "result":{"verified":true,"triggered":true,"followed":true}
            }]
        });
        record_execution(&state, &execution);
        record_execution(&state, &execution);
        let stored = state
            .store
            .get("improvement", record["id"].as_str().unwrap())
            .unwrap();
        assert_eq!(stored["uses"], 1);
        assert_eq!(stored["successes"], 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn revisions_are_immutable_snapshots() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let record = propose(
            &state,
            &scope,
            "strategy",
            "revision 스냅샷을 남긴다",
            "검증 정의가 바뀌어도 이전 revision을 보존한다",
            &["m1".into()],
            None,
            "shared",
        )
        .unwrap();
        let first = record["revision"].as_str().unwrap().to_string();
        assert_eq!(record["scoreVersion"], 2);
        let updated = attach_verification(
            &state,
            record["id"].as_str().unwrap(),
            &json!({"type":"procedure","after":[{"kind":"file_exists","path":"a.txt"}]}),
        )
        .unwrap();
        assert_ne!(updated["revision"], first);
        assert_eq!(updated["previousRevision"], first);
        let old = state
            .store
            .get("improvement_revision", &first)
            .unwrap();
        assert_eq!(old["revision"], first);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn task_outcome_gates_strategy_success() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let record = propose(
            &state,
            &scope,
            "strategy",
            "taskOutcome으로 전략 성과를 판정한다",
            "completed와 검증된 성공을 분리한다",
            &["m1".into()],
            None,
            "shared",
        )
        .unwrap();
        for (id, outcome, followed) in [
            ("e1", "succeeded", true),
            ("e2", "succeeded", false),
            ("e3", "answered_unverified", true),
            ("e4", "failed", true),
        ] {
            let execution = json!({
                "id":id,"sessionId":"s","agentId":"a","status":"completed",
                "taskOutcome":outcome,"toolCalls":1,
                "appliedImprovementIds":[record["id"]],
                "strategyCompliance":[{
                    "strategyId":record["id"],"applicable":true,
                    "result":{"verified":true,"triggered":true,"followed":followed}
                }]
            });
            record_execution(&state, &execution);
        }
        let stored = state
            .store
            .get("improvement", record["id"].as_str().unwrap())
            .unwrap();
        assert_eq!(stored["uses"], 3);
        assert_eq!(stored["successes"], 1);
        assert_eq!(stored["failures"], 2);
        let _ = std::fs::remove_dir_all(dir);
    }
