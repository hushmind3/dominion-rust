// Frozen by the coordinator and compiled into BOTH variants only for evaluation.
// Expectations and measurement code never come from the candidate worktree.
use crate::engine::{decompose, memory, room_state, rsi_evolve};
use crate::{config::Config, db::Store, state::AppState, vault::Vault};
use serde_json::{json, Value};
use std::time::Instant;

fn state() -> (AppState, std::path::PathBuf) {
    let root =
        std::env::temp_dir().join(format!("dominion-frozen-oracle-{}", uuid::Uuid::new_v4()));
    let config = Config {
        port: 0,
        data_dir: root.clone(),
        web_dir: root.join("web"),
        workspace: root.clone(),
    };
    let state = AppState::new(
        config.clone(),
        Store::open(&config.db_path()).unwrap(),
        Vault::open(&config.key_path()),
    );
    (state, root)
}
fn metric(quality: f64, started: Instant, other: Value) {
    let mut values = json!({"quality":quality,"elapsedMs":started.elapsed().as_secs_f64()*1000.0});
    if let Some(object) = other.as_object() {
        for (key, value) in object {
            values[key] = value.clone();
        }
    }
    println!("DOMINION_RSI_METRIC {values}");
}

#[test]
fn memory_ranking() {
    let (state, root) = state();
    let documents = [
        ("배포 담당자는 코볼트다", "배포 담당자"),
        ("회의 시간은 매주 화요일 오전 열 시다", "화요일 회의 시간"),
        (
            "데이터베이스 복구 책임자는 민수다",
            "데이터베이스 복구 책임자",
        ),
        (
            "사용자는 답변을 한국어로 받는 것을 선호한다",
            "답변 언어 선호",
        ),
        (
            "payment retries use exponential backoff",
            "payment retry backoff",
        ),
        (
            "release builds require signed artifacts",
            "release artifact signature",
        ),
        ("오늘의 암호는 SILVER-ORCHID-1188이다", "오늘 암호"),
        ("주간 보고서 마감은 금요일 오후 세 시다", "주간 보고서 마감"),
    ];
    for (index, (text, _)) in documents.iter().enumerate() {
        state.store.put("memory",&json!({"id":format!("answer-{index}"),"text":text,
            "status":"active","scope":"agent","agentId":"a","partitionKey":"local","importance":0.8}));
    }
    for index in 0..96 {
        state.store.put("memory",&json!({"id":format!("distractor-{index}"),
            "text":format!("프로젝트 {index} 잡담·회의·배포·보고서 관련 참고 자료"),
            "status":"active","scope":"agent","agentId":"a","partitionKey":"local","importance":0.5}));
    }
    let scope = memory::MemoryScope::for_session(&state.store, "s", Some("a"));
    let started = Instant::now();
    let mut hits = 0usize;
    for _ in 0..30 {
        for (index, (_, query)) in documents.iter().enumerate() {
            let result = memory::retrieve(&state.store, &scope, query, 5);
            if result
                .first()
                .is_some_and(|r| r["id"] == format!("answer-{index}"))
            {
                hits += 1;
            }
        }
    }
    let quality = hits as f64 / (documents.len() * 30) as f64;
    assert!(
        quality >= 0.5,
        "retrieval must retain minimum useful accuracy"
    );
    metric(quality, started, json!({}));
    drop(state);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn long_owner_recall() {
    let (state, root) = state();
    for index in 0..120 {
        room_state::observe_owner_message(
            &state,
            "r",
            "s",
            &json!({
                "id":format!("fact-{index}"),"role":"user","agent_id":null,
                "content":format!("프로젝트{index}의 출시 코드는 VALUE-{index:04}이다. 기억해."),
                "created_at":format!("2026-01-01 00:{:02}:{:02}",index/60,index%60),
                "metadata":{"authorKind":"owner"}
            }),
        )
        .unwrap();
    }
    let started = Instant::now();
    let mut hits = 0;
    let mut chars = 0;
    let queries = [0, 7, 21, 55, 73, 119];
    for _ in 0..30 {
        for index in queries {
            let text = room_state::context_block_for_query(
                &state,
                "r",
                &format!("프로젝트{index}의 출시 코드는 무엇이지?"),
            )
            .unwrap_or_default();
            hits += usize::from(text.contains(&format!("VALUE-{index:04}")));
            chars += text.chars().count();
        }
    }
    let count = (queries.len() * 30) as f64;
    let quality = hits as f64 / count;
    assert!(
        quality >= 0.5,
        "long-term recall must retain minimum useful accuracy"
    );
    metric(quality, started, json!({"contextChars":chars as f64/count}));
    drop(state);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn planning_quality() {
    let cases = [
        (
            "1. 입력을 읽는다\n2. 결과를 저장한다\n3. 저장된 결과를 검증한다",
            3,
        ),
        ("설정을 읽고 결과를 저장한 다음 검증해", 3),
        ("1) inspect\n2) implement\n3) verify", 3),
        ("파일을 읽어", 1),
    ];
    let started = Instant::now();
    let mut hits = 0;
    for _ in 0..3000 {
        for (input, expected) in cases {
            let result = decompose::decompose(
                input,
                &json!({"implementation":"numbered-first","parameters":{"kernel":"numbered-first"}}),
            );
            hits += usize::from(result["subtasks"].as_array().map(Vec::len) == Some(expected));
        }
    }
    let quality = hits as f64 / (cases.len() * 3000) as f64;
    assert!(quality >= 0.5);
    metric(quality, started, json!({}));
}

#[test]
fn improvement_search() {
    let mut scenarios = vec![(Vec::<Value>::new(), "memory")];
    let mut visited = Vec::new();
    for (domain, next) in [
        ("memory", "tools"),
        ("tools", "context"),
        ("context", "learning"),
        ("learning", "ui"),
        ("ui", "improver"),
    ] {
        visited.push(json!({"domain":domain,"accepted":false,"tokens":10_000}));
        scenarios.push((visited.clone(), next));
    }
    let started = Instant::now();
    let mut hits = 0;
    for _ in 0..5000 {
        for (history, expected) in &scenarios {
            hits += usize::from(rsi_evolve::choose_domain(history) == *expected);
        }
    }
    let quality = hits as f64 / (scenarios.len() * 5000) as f64;
    assert!(quality >= 0.5);
    metric(quality, started, json!({"discoveryScore":quality}));
}
