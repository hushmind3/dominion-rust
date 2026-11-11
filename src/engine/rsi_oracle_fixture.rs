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

fn group_context_probe() -> Value {
    use crate::engine::chat::{context_oracle as continuity, TurnRequest};
    use continuity::compile;
    let root=std::env::temp_dir().join(format!("dominion-group-oracle-{}",uuid::Uuid::new_v4()));
    let config=crate::config::Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
    let state=AppState::new(config.clone(),crate::db::Store::open(&config.db_path()).unwrap(),crate::vault::Vault::open(&config.key_path()));
    state.store.put("session",&json!({"id":"room:r","roomId":"r"}));
    state.store.put("room",&json!({"id":"r","contextBudgetTokens":8000,"agentIds":(0..9).map(|i|format!("agent-{i}")).collect::<Vec<_>>()}));
    state.store.put("assistant_settings",&json!({"id":"global","instructions":"OWNER_COMMON_INSTRUCTION_VERBATIM"}));
    for agent in 0..9 {
        state.store.put("agent",&json!({"id":format!("agent-{agent}"),"name":format!("참가자{agent}"),"systemPrompt":format!("FULL_PERSONA_AGENT_{agent}_VERBATIM")}));
    }
    state.store.put("provider",&json!({"id":"fixture","name":"fixture","baseUrl":"https://fixture.invalid","model":"fixture","kind":"openai","toolMode":"native"}));
    let route_request=TurnRequest::from_value(&json!({"input":"fixture","providerId":"fixture"})).unwrap();
    let provider=continuity::select_provider(&state,&route_request,&json!({"id":"room:r","roomId":"r"})).unwrap();
    let mut rounds=Vec::new();
    for round in 0..3 {
        for index in 0..=usize::from(round>0) {
            state.store.put("memory",&json!({"id":format!("memory-{index}"),"text":format!("MEMORY_{index}_VERBATIM"),"type":"preference",
                "scope":"shared","shareWithRooms":true,"status":"active","importance":0.8+f64::from(round)*0.01,
                "confidence":0.6+f64::from(round)*0.1,"revision":format!("ranking-update-{round}")}));
        }
        let source=format!("owner-{round}");let input=format!("OWNER_ROUND_{round} 기억을 그대로 유지하고 답해.");
        state.store.write_message(&json!({"id":source,"session_id":"room:r","role":"user","content":input,"metadata":{"authorKind":"owner"}})).unwrap();
        for agent in 0..9 {
            let agent_id=format!("agent-{agent}");
            let request=TurnRequest::from_value(&json!({"input":input,"agentId":agent_id,"toolsEnabled":false})).unwrap();
            let compiled=compile(&state,&request,"room:r",&source,&[],&[]).unwrap();
            let mut messages=vec![json!({"role":"system","content":compiled.system.prompt,"_dominion_source":source,
                "_dominion_dependencies":continuity::dependencies(&state,&compiled.system.items)})];
            messages.extend(compiled.history);
            let mut plan=continuity::prepare(&state,&provider,&request,"room:r",&compiled.system.prompt,&json!({})).unwrap();
            let (wire,projection)=continuity::project(&state,&provider,&messages,&[],"room:r",&mut plan);
            rounds.push(json!({"round":round,"agent":agent,"messages":wire,"workingView":plan["workingView"]}));
            let reply=state.store.write_message(&json!({"session_id":"room:r","role":"assistant","agent_id":agent_id,
                "content":format!("CHOICE_AGENT_{agent}_ROUND_{round}"),"metadata":{"replyToMessageId":source,"authorKind":"agent"}})).unwrap();
            continuity::commit(&state,&plan,&reply);
            continuity::commit_projection(&state,&plan,&reply,projection.unwrap());
        }
    }
    let result=json!({"rounds":rounds,"transcriptCount":state.store.messages("room:r").len()});
    drop(state);std::fs::remove_dir_all(root).unwrap();result
}


#[test]
fn group_chat_context_efficiency() {
    let started=Instant::now();
    let observation=group_context_probe();
    let rows=observation["rounds"].as_array().unwrap();
    assert_eq!(rows.len(),27,"nine participants, three turns each");
    assert_eq!(observation["transcriptCount"],30,"original owner and participant messages remain stored");
    let mut checks=0usize;let mut passed=0usize;let mut chars=0usize;
    for row in rows {
        let round=row["round"].as_u64().unwrap();let agent=row["agent"].as_u64().unwrap();
        let messages=row["messages"].as_array().unwrap();let text=json!(messages).to_string();
        chars+=text.chars().count();
        let persona=format!("FULL_PERSONA_AGENT_{agent}_VERBATIM");
        let previous=format!("CHOICE_AGENT_{agent}_ROUND_{}",round.saturating_sub(1));
        let invariants=[
            messages[0]["content"].as_str().unwrap_or("").contains(&persona),
            text.contains("OWNER_COMMON_INSTRUCTION_VERBATIM"),
            text.contains(&format!("OWNER_ROUND_{round}")),
            text.matches("MEMORY_0_VERBATIM").count()==1,
            text.matches("MEMORY_1_VERBATIM").count()==usize::from(round>0),
            round==0 || row["workingView"]["rebaseReason"].is_null(),
            round==0 || messages.iter().any(|message|message["role"]=="assistant" && message["content"]==previous),
        ];
        checks+=invariants.len();passed+=invariants.into_iter().filter(|ok|*ok).count();
    }
    metric(passed as f64/checks as f64,started,json!({"contextChars":chars}));
    assert_eq!(passed,checks,"input savings must retain identity, source, memory and committed self replies");
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
        // The source repair campaign targets Korean action ordering. The
        // frozen evaluator must exercise the target behavior, not unrelated
        // planning cases alone. Vary nouns, verbs, connectors and negatives;
        // a candidate tailored to one prompt phrase cannot pass them all.
        ("설정을 읽고 결과를 저장한 다음 검증해", 3),
        ("설계를 읽고 결과를 저장한 다음 검증해", 3),
        ("코드를 수정하고 나서 테스트를 실행해", 2),
        ("자료를 검색하고 내용을 요약해", 2),
        ("파일을 열고 항목을 정리한 다음 보고서를 내보내", 3),
        ("사고를 보고하고 대책을 적용해", 2),
        ("사고 예방책을 검토해", 1),
        ("최고 품질을 유지해", 1),
        ("보고서와 사고 기록을 검토해", 1),
        ("고객의 최고 평가를 분석해", 1),
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
            hits += usize::from(
                rsi_evolve::select_domain(
                    &json!({"implementation":"failure-clusters","parameters":{}}),
                    history,
                    "",
                ) == *expected,
            );
        }
    }
    let quality = hits as f64 / (scenarios.len() * 5000) as f64;
    assert!(quality >= 0.5);
    metric(quality, started, json!({"discoveryScore":quality}));
}
