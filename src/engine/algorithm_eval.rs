//! Deterministic algorithm evaluation.
//!
//! These fixtures need no model call. They measure whether a kernel changes a
//! concrete behavior: retrieval inclusion, decomposition quality or conflict
//! resolution accuracy. Results describe a candidate; they do not activate it.
use crate::engine::{debate, decompose, memory};
use crate::state::AppState;
use serde_json::{json, Value};
use std::path::Path;

const EVAL_VERSION: &str = "algorithm-eval-v1";

fn algorithm(kind: &str, implementation: &str, parameters: Value) -> Value {
    json!({
        "id": format!("eval-{kind}-{implementation}"),
        "kind": kind,
        "implementation": implementation,
        "variant": implementation,
        "revision": implementation,
        "status": "active",
        "source": "evaluation",
        "parameters": parameters,
    })
}

fn memory_store(rerank: &str, importance: &str) -> crate::db::Store {
    let store = crate::db::Store::open(Path::new(":memory:")).expect("in-memory store");
    store.put("algorithm", &algorithm("memory.rerank", rerank, json!({})));
    store.put(
        "algorithm",
        &algorithm("memory.importance", importance, json!({})),
    );
    store
}

fn scope() -> memory::MemoryScope {
    memory::MemoryScope {
        session_id: "evaluation".into(),
        agent_id: Some("evaluation-agent".into()),
        partition_key: "evaluation".into(),
    }
}

fn rerank_corpus(store: &crate::db::Store, scope: &memory::MemoryScope) {
    for (id, text) in [
        ("rerank-1-gold", "database migration rollback runbook notes archive"),
        ("rerank-1-short", "database migration"),
        ("rerank-2-gold", "token expiration audit trail notes archive"),
        ("rerank-2-short", "token expiration"),
        ("rerank-3-gold", "cache invalidation policy notes archive log"),
        ("rerank-3-short", "cache invalidation"),
        ("rerank-f1", "database backup"),
        ("rerank-f2", "token rotation"),
        ("rerank-f3", "cache warmup"),
        ("rerank-f4", "migration report"),
        ("rerank-f5", "expiration warning"),
    ] {
        store.put(
            "memory",
            &json!({
                "id": id,
                "text": text,
                "type": "lesson",
                "importance": 0.5,
                "scope": "agent",
                "agentId": "evaluation-agent",
                "partitionKey": scope.partition_key,
                "status": "active",
                "learnedAt": "2026-01-01T00:00:00Z",
                "lastUsedAt": "2026-01-01T00:00:00Z",
            }),
        );
    }
}

fn memory_rerank() -> Value {
    let queries = [
        ("rollback database migration", "rerank-1-gold"),
        ("audit token expiration", "rerank-2-gold"),
        ("policy cache invalidation", "rerank-3-gold"),
    ];
    let kernels = ["none", "bm25", "bm25-importance"];
    let mut rows = Vec::new();
    for kernel in kernels {
        let store = memory_store(kernel, "fixed");
        let scope = scope();
        rerank_corpus(&store, &scope);
        let hits = queries
            .iter()
            .filter(|(query, gold)| {
                memory::retrieve(&store, &scope, query, 3)
                    .first()
                    .and_then(|record| record["id"].as_str())
                    == Some(*gold)
            })
            .count();
        rows.push(json!({
            "kernel": kernel,
            "metric": "top1_accuracy",
            "value": hits as f64 / queries.len() as f64,
            "hits": hits,
            "total": queries.len(),
        }));
    }
    dimension(
        "memory.rerank",
        "기억 재랭킹",
        "top1_accuracy",
        &rows,
        "none",
    )
}

fn importance_corpus(store: &crate::db::Store, scope: &memory::MemoryScope) {
    // Gold and distractor have the same query terms and length; only evidence
    // and recency differ, so the importance kernel is the only variable.
    for (id, text, evidence, last_used) in [
        ("z-importance-1-gold", "deployment checklist gamma", 8, "2020-01-01T00:00:00Z"),
        ("a-importance-1-distractor", "deployment checklist delta", 0, "2026-11-01T00:00:00Z"),
        ("z-importance-2-gold", "release rollback gamma", 8, "2020-01-01T00:00:00Z"),
        ("a-importance-2-distractor", "release rollback delta", 0, "2026-11-01T00:00:00Z"),
        ("z-importance-3-gold", "cache eviction gamma", 8, "2020-01-01T00:00:00Z"),
        ("a-importance-3-distractor", "cache eviction delta", 0, "2026-11-01T00:00:00Z"),
    ] {
        store.put(
            "memory",
            &json!({
                "id": id,
                "text": text,
                "type": "lesson",
                "importance": 0.5,
                "scope": "agent",
                "agentId": "evaluation-agent",
                "partitionKey": scope.partition_key,
                "status": "active",
                "sourceMessageIds": (0..evidence).map(|index| format!("{id}-m{index}")).collect::<Vec<_>>(),
                "evidenceMessageIds": (0..evidence).map(|index| format!("{id}-m{index}")).collect::<Vec<_>>(),
                "createdAt": "2026-01-01T00:00:00Z",
                "learnedAt": "2026-01-01T00:00:00Z",
                "lastUsedAt": last_used,
            }),
        );
    }
}

fn memory_importance() -> Value {
    let queries = [
        ("deployment checklist", "z-importance-1-gold"),
        ("release rollback", "z-importance-2-gold"),
        ("cache eviction", "z-importance-3-gold"),
    ];
    let kernels = ["recency-uses", "fixed", "evidence-weighted"];
    let mut rows = Vec::new();
    for kernel in kernels {
        let store = memory_store("none", kernel);
        let scope = scope();
        importance_corpus(&store, &scope);
        let hits = queries
            .iter()
            .filter(|(query, gold)| {
                memory::retrieve(&store, &scope, query, 3)
                    .first()
                    .and_then(|record| record["id"].as_str())
                    == Some(*gold)
            })
            .count();
        rows.push(json!({
            "kernel": kernel,
            "metric": "top1_accuracy",
            "value": hits as f64 / queries.len() as f64,
            "hits": hits,
            "total": queries.len(),
        }));
    }
    dimension(
        "memory.importance",
        "기억 중요도",
        "top1_accuracy",
        &rows,
        "recency-uses",
    )
}

fn decomposition() -> Value {
    let requests = [
        "설정 파일을 읽고 그리고 테스트를 실행한 뒤 결과를 보고해",
        "로그를 확인하고 그리고 오류를 분류한 뒤 수정안을 작성해",
        "저장소를 복제하고 다음으로 의존성을 설치한 뒤 빌드를 실행해",
        "1. 저장소를 확인한다\n2. 테스트를 실행한다\n3. 결과를 정리한다",
    ];
    let kernels = ["single-step", "clause-split", "numbered-first"];
    let mut rows = Vec::new();
    for kernel in kernels {
        let selector = json!({"implementation": kernel, "variant": kernel});
        let scores = requests
            .iter()
            .map(|input| {
                decompose::decompose(input, &selector)["qualityScore"]
                    .as_f64()
                    .unwrap_or(0.0)
            })
            .collect::<Vec<_>>();
        let average = scores.iter().sum::<f64>() / scores.len() as f64;
        rows.push(json!({
            "kernel": kernel,
            "metric": "mean_quality",
            "value": average,
            "perRequest": scores,
        }));
    }
    dimension(
        "task.decomposition",
        "작업 분해",
        "mean_quality",
        &rows,
        "single-step",
    )
}

fn conflict_case(
    owner: &str,
    outputs: Vec<Value>,
    expected: &str,
) -> (String, Vec<Value>, String) {
    (owner.to_string(), outputs, expected.to_string())
}

fn conflict_output(agent: &str, content: &str, evidence: u64) -> Value {
    json!({"agentId": agent, "content": content, "evidenceCount": evidence})
}

fn multiagent_conflict() -> Value {
    let cases = vec![
        conflict_case(
            "테스트해",
            vec![
                conflict_output("a", "테스트 실패", 0),
                conflict_output("b", "테스트 완료", 2),
            ],
            "b",
        ),
        conflict_case(
            "테스트해",
            vec![
                conflict_output("a", "테스트 완료", 0),
                conflict_output("b", "테스트 실패", 3),
            ],
            "b",
        ),
        conflict_case(
            "아직 실패 상태야",
            vec![
                conflict_output("a", "테스트 완료", 0),
                conflict_output("b", "테스트 실패", 0),
            ],
            "b",
        ),
        conflict_case(
            "테스트해",
            vec![
                conflict_output("a", "테스트 성공", 5),
                conflict_output("b", "테스트 실패", 0),
            ],
            "b",
        ),
        conflict_case(
            "테스트해",
            vec![
                conflict_output("a", "테스트 실패", 0),
                conflict_output("b", "테스트 성공", 0),
            ],
            "b",
        ),
    ];
    let kernels = ["evidence-first", "owner-authority", "latest-speaker"];
    let mut rows = Vec::new();
    for kernel in kernels {
        let correct = cases
            .iter()
            .filter(|(owner, outputs, expected)| {
                debate::resolve(owner, outputs, kernel)["winner"]["agentId"].as_str()
                    == Some(expected.as_str())
            })
            .count();
        rows.push(json!({
            "kernel": kernel,
            "metric": "accuracy",
            "value": correct as f64 / cases.len() as f64,
            "correct": correct,
            "total": cases.len(),
        }));
    }
    dimension(
        "multiagent.conflict",
        "멀티에이전트 충돌 해결",
        "accuracy",
        &rows,
        "latest-speaker",
    )
}

fn dimension(kind: &str, label: &str, metric: &str, rows: &[Value], baseline: &str) -> Value {
    let mut winner = rows
        .first()
        .and_then(|row| row["kernel"].as_str())
        .unwrap_or(baseline)
        .to_string();
    let mut winner_value = rows
        .first()
        .and_then(|row| row["value"].as_f64())
        .unwrap_or(0.0);
    // Ties keep the earlier kernel: a more complex kernel must be strictly
    // better to displace the simpler one.
    for row in rows {
        let value = row["value"].as_f64().unwrap_or(0.0);
        if value > winner_value + 1e-9 {
            winner = row["kernel"].as_str().unwrap_or(baseline).to_string();
            winner_value = value;
        }
    }
    let baseline_value = rows
        .iter()
        .find(|row| row["kernel"].as_str() == Some(baseline))
        .and_then(|row| row["value"].as_f64())
        .unwrap_or(0.0);
    let tied = rows
        .iter()
        .filter(|row| {
            (row["value"].as_f64().unwrap_or(0.0) - winner_value).abs() <= 1e-9
        })
        .filter_map(|row| row["kernel"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    json!({
        "kind": kind,
        "label": label,
        "metric": metric,
        "baseline": baseline,
        "baselineValue": baseline_value,
        "winner": winner,
        "winnerValue": winner_value,
        "tied": tied,
        "improvement": winner_value - baseline_value,
        "kernels": rows,
    })
}

/// Runs every deterministic evaluation and returns the comparison table.
/// This function never calls a model and never activates a kernel.
pub fn run(_state: &AppState) -> Value {
    let dimensions = vec![
        memory_rerank(),
        memory_importance(),
        decomposition(),
        multiagent_conflict(),
    ];
    let improved = dimensions
        .iter()
        .filter(|dimension| dimension["improvement"].as_f64().unwrap_or(0.0) > 0.0)
        .count();
    json!({
        "version": EVAL_VERSION,
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "deterministic": true,
        "modelCalls": 0,
        "dimensions": dimensions,
        "improvedDimensions": improved,
        "totalDimensions": 4,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "dominion-algorithm-eval-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    #[test]
    fn deterministic_evaluation_identifies_the_expected_winners() {
        let (state, dir) = state();
        let report = run(&state);
        assert_eq!(report["modelCalls"], 0);
        let dimension = |kind: &str| {
            report["dimensions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["kind"] == kind)
                .unwrap()
                .clone()
        };
        assert_eq!(dimension("memory.rerank")["winner"], "bm25");
        let decomposition = dimension("task.decomposition");
        assert_eq!(decomposition["winner"], "clause-split");
        assert!(decomposition["tied"]
            .as_array()
            .unwrap()
            .iter()
            .any(|kernel| kernel == "numbered-first"));
        assert_eq!(
            dimension("multiagent.conflict")["winner"],
            "owner-authority"
        );
        let importance = dimension("memory.importance");
        assert!(
            importance["winnerValue"].as_f64().unwrap()
                > importance["baselineValue"].as_f64().unwrap(),
            "{importance}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
