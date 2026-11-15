//! Extracted isolated regression tests for src/engine/procedure.rs.
use super::*;

    fn procedure() -> Value {
        json!({
            "type": "procedure",
            "appliesTo": { "paths": ["report.txt"] },
            "before": [
                { "when": { "tool": "workspace_write", "path": "report.txt" },
                  "require": { "tool": "workspace_write", "path": "audit.txt" } }
            ],
            "after": [
                { "kind": "file_contains", "path": "audit.txt", "text": "report.txt" },
                { "kind": "file_equals", "path": "report.txt", "text": "ok" }
            ]
        })
    }

    fn strategy() -> Value {
        json!({
            "id": "s1",
            "verification": normalize(&procedure()),
        })
    }

    fn operation(tool: &str, path: &str) -> Value {
        json!({
            "id": format!("{tool}:{path}"),
            "tool": tool,
            "arguments": { "path": path, "content": path },
            "status": "completed",
        })
    }

    #[test]
    fn applies_only_to_intended_tasks() {
        let strategy = strategy();
        assert!(applies(&strategy, "report.txt를 만들어", None, None));
        assert!(!applies(&strategy, "다른 파일을 만들어", None, None));
    }

    #[test]
    fn blocks_target_before_prerequisite() {
        let strategy = strategy();
        assert!(missing_requirement(
            &strategy,
            "workspace_write",
            &json!({"path": "report.txt", "content": "ok"}),
            &[],
        )
        .is_some());
        assert!(missing_requirement(
            &strategy,
            "workspace_write",
            &json!({"path": "report.txt", "content": "ok"}),
            &[operation("workspace_write", "audit.txt")],
        )
        .is_none());
    }

    #[test]
    fn evaluates_order_and_postconditions() {
        let dir = std::env::temp_dir().join(format!("dominion-proc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("audit.txt"), "report.txt").unwrap();
        std::fs::write(dir.join("report.txt"), "ok\n").unwrap();
        let operations = vec![
            operation("workspace_write", "audit.txt"),
            operation("workspace_write", "report.txt"),
        ];
        let result = evaluate(&strategy(), &operations, &dir);
        assert_eq!(result["verified"], true);
        assert_eq!(result["followed"], true);
        let reversed = vec![
            operation("workspace_write", "report.txt"),
            operation("workspace_write", "audit.txt"),
        ];
        let result = evaluate(&strategy(), &reversed, &dir);
        assert_eq!(result["followed"], false);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn exact_paths_do_not_match_suffixes_or_commands() {
        let condition = json!({"tool":"workspace_write","path":"report.txt"});
        assert!(operation_matches(
            &json!({"tool":"workspace_write","status":"completed","arguments":{"path":"report.txt","content":"ok"}}),
            &condition
        ));
        assert!(!operation_matches(
            &json!({"tool":"workspace_write","status":"completed","arguments":{"path":"report.txt.bak","content":"ok"}}),
            &condition
        ));
        assert!(!operation_matches(
            &json!({"tool":"shell_exec","status":"completed","arguments":{"command":"echo report.txt"}}),
            &condition
        ));
    }

    #[test]
    fn symlink_and_oversized_files_are_rejected() {
        let dir = std::env::temp_dir().join(format!("dominion-proc-{}", uuid::Uuid::new_v4()));
        let outside = std::env::temp_dir().join(format!("dominion-out-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&outside, "outside").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, dir.join("link.txt")).unwrap();
        assert!(matches!(
            read_workspace_file(&dir, "link.txt"),
            FileRead::Unknown
        ));
        let big = "x".repeat((MAX_PROCEDURE_FILE_BYTES + 1) as usize);
        std::fs::write(dir.join("big.txt"), big).unwrap();
        assert!(matches!(
            read_workspace_file(&dir, "big.txt"),
            FileRead::Unknown
        ));
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_file(outside);
    }

    #[test]
    fn one_invalid_condition_rejects_the_whole_revision() {
        let mut value = procedure();
        value["before"][0]["when"] = json!({"path": "report.txt"});
        let normalized = normalize(&value);
        assert_eq!(normalized["type"], "invalid");
        assert!(normalized["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("before[0].when"));
    }

    #[test]
    fn empty_execution_is_not_triggered_not_compliant() {
        let dir = std::env::temp_dir().join(format!("dominion-proc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let result = evaluate(&strategy(), &[], &dir);
        assert_eq!(result["triggered"], false);
        assert_eq!(result["followed"], false);
        assert_eq!(result["status"], "not_triggered");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ready_nodes_bind_only_verified_values() {
        let strategy = json!({
            "id":"harness",
            "revision":"r1",
            "verification": normalize(&json!({
                "type":"procedure",
                "appliesTo":{"paths":["out.txt"]},
                "bindings":{"literal":"bound"},
                "nodes":[
                    {"id":"prep","tool":"workspace_write","arguments":{"path":"audit.txt","content":"ok"}},
                    {"id":"write","tool":"workspace_write","arguments":{"path":"$prep.path","content":"$literal"}}
                ],
                "edges":[{"from":"prep","to":"write"}]
            }))
        });
        let mut results = std::collections::HashMap::new();
        let (id, tool, _arguments) =
            ready_intent(&strategy, &[], "out.txt", &results).expect("first node");
        assert_eq!(id, "prep");
        assert_eq!(tool, "workspace_write");
        results.insert(
            "prep".into(),
            json!({"path":"audit.txt","written":true}),
        );
        let (id, _, arguments) =
            ready_intent(
                &strategy,
                &[json!({
                    "origin":"procedure","procedureNodeId":"prep","status":"completed",
                    "procedureStrategyId":"harness","procedureRevision":"r1"
                })],
                "out.txt",
                &results,
            )
                .expect("second node");
        assert_eq!(id, "write");
        assert_eq!(arguments["path"], "audit.txt");
        assert_eq!(arguments["content"], "bound");
    }

    #[test]
    fn completed_operation_with_error_is_not_success() {
        let strategy = json!({
            "id":"harness",
            "revision":"r1",
            "verification": normalize(&json!({
                "type":"procedure",
                "nodes":[
                    {"id":"prep","tool":"workspace_write","arguments":{"path":"audit.txt","content":"ok"}},
                    {"id":"write","tool":"workspace_write","arguments":{"path":"out.txt","content":"ok"}}
                ],
                "edges":[{"from":"prep","to":"write"}]
            }))
        });
        let (id, _, _) = ready_intent(
            &strategy,
            &[json!({
                "origin":"procedure","procedureNodeId":"prep","status":"completed",
                "procedureStrategyId":"harness","procedureRevision":"r1",
                "result":{"error":"Denied by the user"}
            })],
            "out.txt",
            &std::collections::HashMap::new(),
        )
        .expect("errored prerequisite must be retried");
        assert_eq!(id, "prep");
    }

    #[test]
    fn failed_prerequisite_is_not_completed() {
        let strategy = json!({
            "id":"harness",
            "revision":"r1",
            "verification": normalize(&json!({
                "type":"procedure",
                "nodes":[
                    {"id":"prep","tool":"workspace_write","arguments":{"path":"audit.txt","content":"ok"}},
                    {"id":"write","tool":"workspace_write","arguments":{"path":"out.txt","content":"ok"}}
                ],
                "edges":[{"from":"prep","to":"write"}]
            }))
        });
        let mut results = std::collections::HashMap::new();
        results.insert("prep".into(), json!({"path":"audit.txt"}));
        let (id, _, _) = ready_intent(
            &strategy,
            &[json!({
                "origin":"procedure","procedureNodeId":"prep","status":"failed",
                "result":{"error":"denied"}
            })],
            "out.txt",
            &results,
        )
        .expect("failed prerequisite must be retried");
        assert_eq!(id, "prep");
    }

    #[test]
    fn unresolved_binding_does_not_produce_an_intent() {
        let strategy = json!({
            "id":"harness",
            "verification": normalize(&json!({
                "type":"procedure",
                "nodes":[{"id":"write","tool":"workspace_write","arguments":{"path":"out.txt","content":"$missing"}}]
            }))
        });
        assert!(ready_intent(
            &strategy,
            &[],
            "out.txt",
            &std::collections::HashMap::new()
        )
        .is_none());
    }

    #[test]
    fn dag_cycles_unknowns_and_contradictions_are_rejected() {
        let valid = json!({
            "type":"procedure",
            "appliesTo":{"paths":["out.txt"]},
            "nodes":[
                {"id":"prep","tool":"workspace_write","arguments":{"path":"audit.txt","content":"ok"}},
                {"id":"write","tool":"workspace_write","arguments":{"path":"out.txt","content":"ok"}}
            ],
            "edges":[{"from":"prep","to":"write"}],
            "bindings":{}
        });
        assert_eq!(normalize(&valid)["type"], "procedure");
        let mut cycle = valid.clone();
        cycle["edges"] = json!([{"from":"prep","to":"write"},{"from":"write","to":"prep"}]);
        assert_eq!(normalize(&cycle)["type"], "invalid");
        let mut self_edge = valid.clone();
        self_edge["edges"] = json!([{"from":"prep","to":"prep"}]);
        assert_eq!(normalize(&self_edge)["type"], "invalid");
        let mut unknown = valid.clone();
        unknown["edges"] = json!([{"from":"prep","to":"missing"}]);
        assert_eq!(normalize(&unknown)["type"], "invalid");
        let mut unsupported = valid.clone();
        unsupported["nodes"][0]["tool"] = json!("not_a_real_tool");
        assert_eq!(normalize(&unsupported)["type"], "invalid");
        let mut binding = valid.clone();
        binding["nodes"][0]["arguments"]["content"] = json!("$missing");
        assert_eq!(normalize(&binding)["type"], "invalid");
        let mut contradiction = valid.clone();
        contradiction["after"] = json!([
            {"kind":"file_equals","path":"out.txt","text":"a"},
            {"kind":"file_equals","path":"out.txt","text":"b"}
        ]);
        assert_eq!(normalize(&contradiction)["type"], "invalid");
    }

    #[test]
    fn repeated_target_requires_a_fresh_prerequisite() {
        let strategy = strategy();
        let audit = operation("workspace_write", "audit.txt");
        let report = operation("workspace_write", "report.txt");
        assert!(missing_requirement(
            &strategy,
            "workspace_write",
            &json!({"path":"report.txt","content":"ok"}),
            &[audit.clone(), report.clone()],
        )
        .is_some());
        let mut second_audit = operation("workspace_write", "audit.txt");
        second_audit["id"] = json!("audit-2");
        let second_report = json!({
            "id":"report-2","tool":"workspace_write","status":"completed",
            "arguments":{"path":"report.txt","content":"ok"}
        });
        let result = evaluate(
            &strategy,
            &[audit, report, second_audit, second_report],
            Path::new("."),
        );
        assert_eq!(result["followed"], false);
    }

    #[test]
    fn operations_sort_by_monotonic_sequence() {
        let mut operations = vec![
            json!({"id":"late","opSeq":2,"startedAt":"2026-01-01T00:00:02Z"}),
            json!({"id":"early","opSeq":1,"startedAt":"2026-01-01T00:00:01Z"}),
        ];
        sort_operations(&mut operations);
        assert_eq!(operations[0]["id"], "early");
    }

    #[test]
    fn unreadable_artifacts_are_unknown() {
        let dir = std::env::temp_dir().join(format!("dominion-proc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("dir.txt")).unwrap();
        let strategy = json!({
            "id":"unknown","verification":normalize(&json!({
                "type":"procedure","appliesTo":{"paths":["dir.txt"]},
                "after":[{"kind":"file_equals","path":"dir.txt","text":"x"}]
            }))
        });
        let result = evaluate(&strategy, &[], &dir);
        assert_eq!(result["status"], "unknown");
        assert_eq!(result["followed"], false);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unverified_strategy_is_never_counted() {
        let strategy = json!({"id":"u","verification":{"type":"unverified"}});
        assert!(!applies(&strategy, "report.txt", None, None));
        let result = evaluate(&strategy, &[], Path::new("."));
        assert_eq!(result["verified"], false);
    }
