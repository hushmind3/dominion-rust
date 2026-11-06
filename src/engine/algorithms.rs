//! Algorithm space. The harness owns execution, observation, tools,
//! checkpointing and safety. Algorithms are versioned, selectable pieces the
//! harness calls by kind. Keeping them in the store makes addition, replacement
//! and generated candidates independent from engine code.
use crate::state::AppState;
use serde_json::{json, Value};

pub const KINDS: &[&str] = &[
    "adaptation.selection",
    "learning.applicability",
    "strategy.selection",
    "program.repair",
    "context.selection",
    "verification.predicate",
    "tool.selection",
    "tool.discovery",
    "tool.combination",
    "tool.arguments",
    "task.decomposition",
    "execution.budget",
    "memory.retrieval",
    "memory.rerank",
    "memory.importance",
    "memory.forgetting",
    "memory.compaction",
    "memory.compression",
    "learning.counterexample",
    "recovery.strategy",
    "multiagent.sharing",
    "multiagent.roles",
    "multiagent.conflict",
    "execution.ordering",
    "execution.parallelism",
    "selfimprove.search",
];

/// Concrete kernels the Rust harness can execute. Registering a name that is
/// not here is rejected: an algorithm record must point at real behavior.
pub fn executable_kernels(kind: &str) -> &'static [&'static str] {
    match kind {
        "adaptation.selection" => &["balanced", "discrimination-first", "progress-first", "probe-first", "expression"],
        "learning.applicability" => &["conservative", "probe-unknown"],
        "strategy.selection" => &["estimated-softmax", "exploit-only", "expression"],
        "program.repair" => &["completed-preserving", "append-only"],
        "context.selection" => &["failure-first"],
        "verification.predicate" => &["three-valued", "strict"],
        "tool.selection" => &["read-before-write", "block-write-without-read", "none"],
        "tool.discovery" => &["all", "core", "query-relevant"],
        "tool.combination" => &["sequential", "parallel-reads"],
        "tool.arguments" => &["none", "normalize-paths"],
        "task.decomposition" => &["single-step", "clause-split", "numbered-first"],
        "execution.budget" => &["standard", "extended", "lean"],
        "memory.retrieval" => &["lexical", "failure-first", "balanced"],
        "memory.rerank" => &["none", "bm25", "bm25-importance"],
        "memory.importance" => &["recency-uses", "fixed", "evidence-weighted"],
        "memory.forgetting" => &["decay", "never"],
        "memory.compaction" => &["exact", "never"],
        "memory.compression" => &["extractive", "none"],
        "learning.counterexample" => &["narrow", "hold", "disable"],
        "recovery.strategy" => &["fingerprint", "block-identical", "allow-all"],
        "multiagent.sharing" => &["two-agents", "three-agents", "never"],
        "multiagent.roles" => &["discussion", "round-robin", "mentioned"],
        "multiagent.conflict" => &["evidence-first", "owner-authority", "latest-speaker"],
        "execution.ordering" => &["as-written", "read-first"],
        "execution.parallelism" => &["concurrent", "sequential"],
        "selfimprove.search" => &["failure-clusters", "counterexample-first"],
        _ => &[],
    }
}

pub fn kernel_exists(kind: &str, implementation: &str) -> bool {
    executable_kernels(kind)
        .iter()
        .any(|kernel| *kernel == implementation)
}

pub fn kind_label(kind: &str) -> &str {
    match kind {
        "adaptation.selection" => "적응 개입 선택",
        "learning.applicability" => "경험 적용 조건",
        "strategy.selection" => "전략 선택",
        "program.repair" => "프로그램 수리",
        "context.selection" => "컨텍스트 선택",
        "verification.predicate" => "검증 predicate",
        "tool.selection" => "도구 선택",
        "tool.discovery" => "도구 탐색",
        "tool.combination" => "도구 조합",
        "tool.arguments" => "도구 인자 정규화",
        "task.decomposition" => "작업 분해",
        "execution.budget" => "실행 예산",
        "memory.retrieval" => "기억 검색",
        "memory.rerank" => "기억 재랭킹",
        "memory.importance" => "기억 중요도 평가",
        "memory.forgetting" => "기억 망각",
        "memory.compaction" => "기억 중복 압축",
        "memory.compression" => "기억 요약 압축",
        "learning.counterexample" => "반례 처리",
        "recovery.strategy" => "복구 전략",
        "multiagent.sharing" => "에이전트 지식 공유",
        "multiagent.roles" => "에이전트 역할·발언 방식",
        "multiagent.conflict" => "멀티에이전트 충돌 해결",
        "execution.ordering" => "도구 실행 순서",
        "execution.parallelism" => "병렬 실행",
        "selfimprove.search" => "개선 후보 탐색",
        _ => "기타",
    }
}

/// Safe arithmetic expression over named candidate features. This lets a new
/// selection rule be added as an executable algorithm record without a Rust
/// rebuild, while the harness still owns the candidate features.
pub fn eval_expression(
    expression: &str,
    variables: &std::collections::BTreeMap<String, f64>,
) -> Result<f64, String> {
    struct Parser<'a> {
        chars: Vec<char>,
        index: usize,
        variables: &'a std::collections::BTreeMap<String, f64>,
    }
    impl<'a> Parser<'a> {
        fn skip(&mut self) {
            while self.chars.get(self.index).is_some_and(|ch| ch.is_whitespace()) {
                self.index += 1;
            }
        }
        fn peek(&mut self) -> Option<char> {
            self.skip();
            self.chars.get(self.index).copied()
        }
        fn next(&mut self) -> Option<char> {
            self.skip();
            let value = self.chars.get(self.index).copied();
            if value.is_some() {
                self.index += 1;
            }
            value
        }
        fn expr(&mut self) -> Result<f64, String> {
            let mut value = self.term()?;
            while let Some(op @ ('+' | '-')) = self.peek() {
                self.next();
                let rhs = self.term()?;
                value = if op == '+' { value + rhs } else { value - rhs };
            }
            Ok(value)
        }
        fn term(&mut self) -> Result<f64, String> {
            let mut value = self.factor()?;
            while let Some(op @ ('*' | '/')) = self.peek() {
                self.next();
                let rhs = self.factor()?;
                if op == '*' {
                    value *= rhs;
                } else {
                    if rhs.abs() < f64::EPSILON {
                        return Err("division by zero".into());
                    }
                    value /= rhs;
                }
            }
            Ok(value)
        }
        fn factor(&mut self) -> Result<f64, String> {
            match self.next() {
                Some('+') => self.factor(),
                Some('-') => Ok(-self.factor()?),
                Some('(') => {
                    let value = self.expr()?;
                    if self.next() != Some(')') {
                        return Err("missing closing parenthesis".into());
                    }
                    Ok(value)
                }
                Some(ch) if ch.is_ascii_digit() || ch == '.' => {
                    let mut text = ch.to_string();
                    while let Some(next) = self.peek() {
                        if next.is_ascii_digit() || next == '.' {
                            text.push(next);
                            self.next();
                        } else {
                            break;
                        }
                    }
                    text.parse::<f64>().map_err(|_| "invalid number".into())
                }
                Some(ch) if ch.is_alphabetic() || ch == '_' => {
                    let mut name = ch.to_string();
                    while let Some(next) = self.peek() {
                        if next.is_alphanumeric() || next == '_' {
                            name.push(next);
                            self.next();
                        } else {
                            break;
                        }
                    }
                    self.variables
                        .get(&name)
                        .copied()
                        .ok_or_else(|| format!("unknown variable {name}"))
                }
                other => Err(format!("unexpected token {other:?}")),
            }
        }
    }
    if expression.trim().is_empty() {
        return Err("expression is empty".into());
    }
    let mut parser = Parser {
        chars: expression.chars().collect(),
        index: 0,
        variables,
    };
    let value = parser.expr()?;
    if parser.peek().is_some() {
        return Err("trailing tokens in expression".into());
    }
    Ok(value)
}

fn builtin(
    kind: &str,
    name: &str,
    variant: &str,
    description: &str,
    parameters: Value,
) -> Value {
    let id = format!(
        "algorithm-{}",
        crate::engine::memory::stable_id(&["builtin", kind, name])
    );
    json!({
        "id": id,
        "kind": kind,
        "affects": kind,
        "name": name,
        "variant": variant,
        "implementation": variant,
        "description": description,
        "parameters": parameters,
        "source": "builtin",
        "status": "available",
        "revision": crate::engine::memory::stable_id(&[kind, name, variant]),
        "createdAt": chrono::Utc::now().to_rfc3339(),
    })
}

pub fn builtins() -> Vec<Value> {
    vec![
        builtin(
            "adaptation.selection",
            "balanced",
            "balanced",
            "진전·구별·비용·불확실성을 함께 고려한다.",
            json!({"discriminationWeight": 1.0, "progressWeight": 1.0, "costWeight": 1.0, "uncertaintyWeight": 0.5}),
        ),
        builtin(
            "adaptation.selection",
            "discrimination-first",
            "discrimination-first",
            "원인이 불명확할 때 가설을 구별하는 probe를 우선한다.",
            json!({"discriminationWeight": 3.0, "progressWeight": 0.3, "costWeight": 1.0, "uncertaintyWeight": 0.2}),
        ),
        builtin(
            "adaptation.selection",
            "progress-first",
            "progress-first",
            "원인이 좁혀졌을 때 실제 문제 해결 가능성이 큰 개입을 우선한다.",
            json!({"discriminationWeight": 0.3, "progressWeight": 3.0, "costWeight": 0.8, "uncertaintyWeight": 0.5}),
        ),
        builtin(
            "adaptation.selection",
            "probe-first",
            "probe-first",
            "낮은 비용의 관측으로 원인을 먼저 구별한다.",
            json!({"discriminationWeight": 2.5, "progressWeight": 0.4, "costWeight": 1.5, "uncertaintyWeight": 0.2}),
        ),
        builtin(
            "learning.applicability",
            "conservative",
            "conservative",
            "Unknown 조건은 적용하지 않고 관측을 요구한다.",
            json!({"unknown": "hold"}),
        ),
        builtin(
            "learning.applicability",
            "probe-unknown",
            "probe-unknown",
            "Unknown 조건은 작은 probe로 확인한 뒤 적용한다.",
            json!({"unknown": "probe"}),
        ),
        builtin(
            "strategy.selection",
            "estimated-softmax",
            "estimated-softmax",
            "검증 성공/실패·불확실성·비용으로 순위를 정하고 softmax 확률을 기록한다.",
            json!({"uncertaintyPenalty": 0.3, "riskPenalty": 0.5, "exploration": 1}),
        ),
        builtin(
            "strategy.selection",
            "exploit-only",
            "exploit-only",
            "관측 표본이 충분한 전략만 선택한다.",
            json!({"uncertaintyPenalty": 0.6, "exploration": 0}),
        ),
        builtin(
            "program.repair",
            "completed-preserving",
            "completed-preserving",
            "완료된 외부 행동을 보존하고 미완료 노드만 교체한다.",
            json!({"maxRevisions": 2, "preserveCompleted": true}),
        ),
        builtin(
            "program.repair",
            "append-only",
            "append-only",
            "기존 pending node를 교체하지 않고 새 node만 추가한다.",
            json!({"maxRevisions": 2, "preserveCompleted": true, "mode": "append"}),
        ),
        builtin(
            "context.selection",
            "failure-first",
            "failure-first",
            "같은 실패 유형의 사례와 조건부 방법을 먼저 넣는다.",
            json!({"failureBoost": 2.0, "methodLimit": 3}),
        ),
        builtin(
            "verification.predicate",
            "three-valued",
            "three-valued",
            "True/False/Unknown으로 평가하고 Unknown을 실패로 바꾸지 않는다.",
            json!({"unknown": "hold"}),
        ),
        builtin(
            "verification.predicate",
            "strict",
            "strict",
            "Unknown을 충족 실패로 처리한다. 기본은 three-valued다.",
            json!({"unknown": "fail"}),
        ),
        builtin(
            "memory.rerank",
            "none",
            "none",
            "기존 검색 점수를 그대로 사용한다.",
            json!({}),
        ),
        builtin(
            "memory.rerank",
            "bm25",
            "bm25",
            "BM25 문서 빈도·길이 정규화로 재랭킹한다.",
            json!({"k1": 1.2, "b": 0.75}),
        ),
        builtin(
            "memory.rerank",
            "bm25-importance",
            "bm25-importance",
            "BM25에 저장 중요도와 사용 신호를 더한다.",
            json!({"k1": 1.2, "b": 0.75}),
        ),
        builtin(
            "memory.compression",
            "extractive",
            "extractive",
            "오래된 낮은 중요도 기억을 하나의 요약 기억으로 압축하고 원문은 보관한다.",
            json!({"maxAgeDays": 30, "maxImportance": 0.35, "minGroup": 2}),
        ),
        builtin(
            "memory.compression",
            "none",
            "none",
            "자동 요약 압축을 하지 않는다.",
            json!({}),
        ),
        builtin(
            "memory.importance",
            "recency-uses",
            "recency-uses",
            "최근 사용과 사용 횟수를 중요도에 반영한다.",
            json!({"usesWeight": 0.15, "recencyDays": 7}),
        ),
        builtin(
            "memory.importance",
            "fixed",
            "fixed",
            "저장된 중요도만 사용한다.",
            json!({}),
        ),
        builtin(
            "memory.importance",
            "evidence-weighted",
            "evidence-weighted",
            "사용 횟수와 근거 수를 함께 반영한다.",
            json!({"usesWeight": 0.1, "evidenceWeight": 0.2}),
        ),
        builtin(
            "memory.forgetting",
            "decay",
            "decay",
            "오래 사용되지 않은 낮은 중요도 기억을 보관 처리한다.",
            json!({"minImportance": 0.3, "maxAgeDays": 30}),
        ),
        builtin(
            "memory.forgetting",
            "never",
            "never",
            "자동 망각을 하지 않는다.",
            json!({}),
        ),
        builtin(
            "memory.compaction",
            "exact",
            "exact",
            "같은 영역과 작성자의 중복 기억만 보관 처리하고 원문 근거를 유지한다.",
            json!({}),
        ),
        builtin(
            "memory.compaction",
            "never",
            "never",
            "자동 중복 압축을 하지 않는다.",
            json!({}),
        ),
        builtin(
            "tool.discovery",
            "all",
            "all",
            "모든 도구 정의를 제공한다.",
            json!({}),
        ),
        builtin(
            "tool.discovery",
            "core",
            "core",
            "핵심 도구만 제공해 작은 컨텍스트를 보호한다.",
            json!({"tools": ["workspace_list","workspace_read","workspace_write","shell_exec","web_search","memory_store","memory_search","delegate_task","task_status","task_result"]}),
        ),
        builtin(
            "tool.discovery",
            "query-relevant",
            "query-relevant",
            "질문과 관련된 도구를 우선하고 핵심 도구는 유지한다.",
            json!({"extra": 8}),
        ),
        builtin(
            "tool.combination",
            "sequential",
            "sequential",
            "도구 호출을 순서대로 실행한다.",
            json!({}),
        ),
        builtin(
            "tool.combination",
            "parallel-reads",
            "parallel-reads",
            "같은 라운드의 독립 읽기 도구를 병렬 실행한다.",
            json!({}),
        ),
        builtin(
            "tool.arguments",
            "none",
            "none",
            "도구 인자를 수정하지 않는다.",
            json!({}),
        ),
        builtin(
            "tool.arguments",
            "normalize-paths",
            "normalize-paths",
            "경로 인자의 공백·중복 슬래시·선행 ./를 정규화한다.",
            json!({}),
        ),
        builtin(
            "task.decomposition",
            "single-step",
            "single-step",
            "요청 전체를 하나의 작업으로 취급한다.",
            json!({}),
        ),
        builtin(
            "task.decomposition",
            "clause-split",
            "clause-split",
            "명시적 연결어·구분자로 요청을 순서 있는 하위 작업으로 나눈다.",
            json!({}),
        ),
        builtin(
            "task.decomposition",
            "numbered-first",
            "numbered-first",
            "명시적 번호/목록이 있으면 그 순서를 우선 사용한다.",
            json!({}),
        ),
        builtin(
            "execution.budget",
            "standard",
            "standard",
            "기본 라운드/도구 예산.",
            json!({"maxRounds": 8, "maxTools": 40, "maxElapsedSecs": 600}),
        ),
        builtin(
            "execution.budget",
            "extended",
            "extended",
            "복잡한 작업을 위한 확장 예산.",
            json!({"maxRounds": 12, "maxTools": 80, "maxElapsedSecs": 900}),
        ),
        builtin(
            "execution.budget",
            "lean",
            "lean",
            "짧은 작업을 위한 축소 예산.",
            json!({"maxRounds": 6, "maxTools": 20, "maxElapsedSecs": 300}),
        ),
        builtin(
            "memory.retrieval",
            "lexical",
            "lexical",
            "어휘 중첩과 실패 가중치로 경험을 검색한다.",
            json!({"failureBoost": 2.0}),
        ),
        builtin(
            "memory.retrieval",
            "failure-first",
            "failure-first",
            "실패 경험을 더 강하게 우선한다.",
            json!({"failureBoost": 4.0}),
        ),
        builtin(
            "memory.retrieval",
            "balanced",
            "balanced",
            "한국어 띄어쓰기 차이와 문서 희소도를 반영하는 모델 없는 경험 검색 재정렬.",
            json!({"failureBoost": 2.0}),
        ),
        builtin(
            "learning.counterexample",
            "narrow",
            "narrow",
            "반례가 나오면 적용 조건을 좁힌다.",
            json!({"mode": "narrow"}),
        ),
        builtin(
            "learning.counterexample",
            "hold",
            "hold",
            "반례를 설명할 관측이 없으면 방법을 보류한다.",
            json!({"mode": "hold"}),
        ),
        builtin(
            "recovery.strategy",
            "fingerprint",
            "fingerprint",
            "실패 시 환경 fingerprint가 바뀐 경우에만 같은 호출을 허용한다.",
            json!({"mode": "fingerprint"}),
        ),
        builtin(
            "recovery.strategy",
            "allow-all",
            "allow-all",
            "실패 후 재시도를 모델 판단에 맡긴다.",
            json!({"mode": "allow-all"}),
        ),
        builtin(
            "multiagent.sharing",
            "two-agents",
            "two-agents",
            "서로 다른 두 에이전트가 지지한 방법만 공유한다.",
            json!({"minAgents": 2}),
        ),
        builtin(
            "multiagent.sharing",
            "three-agents",
            "three-agents",
            "세 에이전트가 지지해야 공유한다.",
            json!({"minAgents": 3}),
        ),
        builtin(
            "multiagent.roles",
            "discussion",
            "discussion",
            "라운드마다 참가자가 토론하듯 발언한다.",
            json!({}),
        ),
        builtin(
            "multiagent.roles",
            "round-robin",
            "round-robin",
            "참가자를 순환 순서로 발언시킨다.",
            json!({}),
        ),
        builtin(
            "multiagent.roles",
            "mentioned",
            "mentioned",
            "멘션된 참가자 중심으로 발언시킨다.",
            json!({}),
        ),
        builtin(
            "multiagent.conflict",
            "evidence-first",
            "evidence-first",
            "도구 증거가 많은 참가자의 주장을 우선한다.",
            json!({}),
        ),
        builtin(
            "multiagent.conflict",
            "owner-authority",
            "owner-authority",
            "주인 메시지의 명시적 상태를 우선한다.",
            json!({}),
        ),
        builtin(
            "multiagent.conflict",
            "latest-speaker",
            "latest-speaker",
            "마지막 발언자의 주장을 우선한다.",
            json!({}),
        ),
        builtin(
            "execution.ordering",
            "as-written",
            "as-written",
            "모델이 제시한 도구 호출 순서를 유지한다.",
            json!({}),
        ),
        builtin(
            "execution.ordering",
            "read-first",
            "read-first",
            "같은 라운드에서 읽기/목록 도구를 쓰기보다 먼저 실행한다.",
            json!({}),
        ),
        builtin(
            "execution.parallelism",
            "concurrent",
            "concurrent",
            "독립 작업을 동시에 실행한다.",
            json!({"mode": "concurrent"}),
        ),
        builtin(
            "execution.parallelism",
            "sequential",
            "sequential",
            "독립 작업도 순차 실행한다.",
            json!({"mode": "sequential"}),
        ),
        builtin(
            "selfimprove.search",
            "failure-clusters",
            "failure-clusters",
            "실패 유형 묶음에서 개선 후보를 생성한다.",
            json!({"order": "frequency"}),
        ),
        builtin(
            "tool.selection",
            "read-before-write",
            "read-before-write",
            "쓰기 전에 읽기/목록 도구로 대상을 확인한다.",
            json!({"probeTools": ["workspace_list", "workspace_read"]}),
        ),
    ]
}

pub fn seed(state: &AppState) -> Vec<Value> {
    let mut saved = Vec::new();
    for mut algorithm in builtins() {
        // The first builtin of each kind is the explicit default. Later
        // activation never silently promotes another kind or another tool.
        let kind = algorithm["kind"].as_str().unwrap_or("").to_string();
        let records = state.store.all("algorithm");
        let kind_exists = records
            .iter()
            .any(|item| item["kind"].as_str() == Some(kind.as_str()));
        let has_active = records.iter().any(|item| {
            item["kind"].as_str() == Some(kind.as_str())
                && item["status"].as_str() == Some("active")
        });
        if !kind_exists && !has_active {
            algorithm["status"] = json!("active");
            algorithm["activatedAt"] = json!(chrono::Utc::now().to_rfc3339());
        }
        let id = algorithm["id"].as_str().unwrap_or("").to_string();
        if state.store.get("algorithm", &id).is_none() {
            if let Some(record) = state.store.put("algorithm", &algorithm) {
                state.bus.record("algorithm", &record);
                saved.push(record);
            }
        } else if let Some(mut existing) = state.store.get("algorithm", &id) {
            // Upgrade records created before executable-kernel metadata.
            let mut changed = false;
            if existing["implementation"].is_null() {
                existing["implementation"] = existing["variant"].clone();
                changed = true;
            }
            if existing["affects"].is_null() {
                existing["affects"] = existing["kind"].clone();
                changed = true;
            }
            if changed {
                if let Some(record) = state.store.put("algorithm", &existing) {
                    state.bus.record("algorithm", &record);
                }
            }
        }
    }
    saved
}

pub fn list(state: &AppState) -> Value {
    seed(state);
    let mut items = state.store.all("algorithm");
    items.sort_by(|a, b| {
        a["kind"]
            .as_str()
            .unwrap_or("")
            .cmp(b["kind"].as_str().unwrap_or(""))
            .then_with(|| {
                a["name"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b["name"].as_str().unwrap_or(""))
            })
    });
    let kinds = KINDS
        .iter()
        .map(|kind| {
            let rows = items
                .iter()
                .filter(|item| item["kind"].as_str() == Some(*kind))
                .cloned()
                .collect::<Vec<_>>();
            json!({
                "kind": kind,
                "label": kind_label(kind),
                "kernels": executable_kernels(kind),
                "active": rows.iter().find(|item| item["status"].as_str() == Some("active")).cloned(),
                "algorithms": rows,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "kinds": kinds,
        // Algorithm activation is per kind. Self-improvement and the tool
        // gateway are harness capabilities, not algorithm kinds, so changing
        // one algorithm cannot disable them or leak into another tool.
        "isolation": {
            "activationScope": "kind",
            "selfImprovementIndependent": true,
            "toolGatewayIndependent": true,
        },
    })
}

pub fn active(state: &AppState, kind: &str) -> Option<Value> {
    seed(state);
    state
        .store
        .all("algorithm")
        .into_iter()
        .find(|item| {
            item["kind"].as_str() == Some(kind)
                && item["status"].as_str() == Some("active")
        })
}

/// The executable kernel chosen for a kind. `variant` is display metadata and
/// must never diverge from the kernel the harness actually runs.
pub fn variant(state: &AppState, kind: &str) -> String {
    active(state, kind)
        .and_then(|item| {
            item["implementation"]
                .as_str()
                .or_else(|| item["variant"].as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| match kind {
            "strategy.selection" => "estimated-softmax".into(),
            "program.repair" => "completed-preserving".into(),
            "tool.selection" => "read-before-write".into(),
            "verification.predicate" => "three-valued".into(),
            "learning.applicability" => "conservative".into(),
            _ => "balanced".into(),
        })
}

fn agent_of(_state: &AppState, agent_id: Option<&str>) -> Option<String> {
    // A turn without an agent still needs a stable scope. Use the shared
    // "global" statistics instead of silently picking another bot.
    Some(
        agent_id
            .filter(|value| !value.is_empty())
            .unwrap_or("global")
            .to_string(),
    )
}

fn stat_id(agent: &str, kind: &str, revision: &str) -> String {
    format!(
        "algorithm-stat-{}",
        crate::engine::memory::stable_id(&[agent, kind, revision])
    )
}

/// UCB1 over the agent's own outcomes. The model is irrelevant; what improves
/// is which judgment kernel this bot has actually had success with.
pub fn choose_for_agent(
    state: &AppState,
    kind: &str,
    agent_id: Option<&str>,
    execution_id: Option<&str>,
) -> Option<Value> {
    seed(state);
    let agent = agent_of(state, agent_id)?;
    let candidates = state
        .store
        .all("algorithm")
        .into_iter()
        .filter(|algorithm| {
            algorithm["kind"].as_str() == Some(kind)
                // Builtins are trusted executable baselines. Generated/user
                // candidates cannot enter live UCB exploration merely by
                // being registered as "available": they require validation
                // or explicit activation by the owner.
                && (matches!(algorithm["status"].as_str(),
                        Some("active" | "validated"))
                    || (algorithm["status"].as_str() == Some("available")
                        && algorithm["source"].as_str() == Some("builtin")))
                && algorithm["implementation"]
                    .as_str()
                    .is_some_and(|implementation| kernel_exists(kind, implementation))
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return None;
    }
    let all_stats = state.store.all("algorithm_stat");
    let stats = all_stats
        .iter()
        .filter(|stat| {
            stat["agentId"].as_str() == Some(agent.as_str())
                && stat["kind"].as_str() == Some(kind)
        })
        .cloned()
        .collect::<Vec<_>>();
    let global_stats = all_stats
        .iter()
        .filter(|stat| {
            stat["agentId"].as_str() == Some("global")
                && stat["kind"].as_str() == Some(kind)
        })
        .cloned()
        .collect::<Vec<_>>();
    let total = stats
        .iter()
        .map(|stat| stat["uses"].as_u64().unwrap_or(0))
        .sum::<u64>() as f64;
    let active_revision = active(state, kind)
        .and_then(|algorithm| algorithm["revision"].as_str().map(str::to_string));
    let chosen = candidates
        .into_iter()
        .max_by(|a, b| {
            let score = |algorithm: &Value| {
                let revision = algorithm["revision"].as_str().unwrap_or("");
                let stat = stats
                    .iter()
                    .find(|stat| stat["revision"].as_str() == Some(revision))
                    .or_else(|| {
                        global_stats
                            .iter()
                            .find(|stat| stat["revision"].as_str() == Some(revision))
                    });
                let uses = stat
                    .and_then(|stat| stat["uses"].as_u64())
                    .unwrap_or(0) as f64;
                let successes = stat
                    .and_then(|stat| stat["successes"].as_u64())
                    .unwrap_or(0) as f64;
                let rate = if uses > 0.0 { successes / uses } else { 0.0 };
                let exploration = if uses > 0.0 {
                    (2.0 * (total + 1.0).ln() / uses).sqrt()
                } else {
                    1.5
                };
                let active_bonus = if active_revision
                    .as_deref()
                    == algorithm["revision"].as_str()
                {
                    0.05
                } else {
                    0.0
                };
                rate + exploration + active_bonus
            };
            score(a)
                .partial_cmp(&score(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;
    if let Some(execution_id) = execution_id {
        let usage_id = format!(
            "algorithm-usage-{}",
            crate::engine::memory::stable_id(&[execution_id, kind])
        );
        if state.store.get("algorithm_usage", &usage_id).is_none() {
            if let Some(saved) = state.store.put(
                "algorithm_usage",
                &json!({
                    "id": usage_id,
                    "executionId": execution_id,
                    "agentId": agent,
                    "kind": kind,
                    "revision": chosen["revision"],
                    "status": "pending",
                    "createdAt": chrono::Utc::now().to_rfc3339(),
                }),
            ) {
                state.bus.record("algorithm_usage", &saved);
            }
        }
    }
    Some(chosen)
}

pub fn variant_for_agent(
    state: &AppState,
    kind: &str,
    agent_id: Option<&str>,
) -> String {
    choose_for_agent(state, kind, agent_id, None)
        .and_then(|algorithm| {
            algorithm["implementation"]
                .as_str()
                .or_else(|| algorithm["variant"].as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| variant(state, kind))
}

/// Records the observable outcome of a specific algorithm decision. The final
/// task outcome is only a fallback; this signal distinguishes which domain
/// actually succeeded or failed.
pub fn mark_signal(state: &AppState, execution_id: &str, kind: &str, ok: bool) {
    let usage_id = format!(
        "algorithm-usage-{}",
        crate::engine::memory::stable_id(&[execution_id, kind])
    );
    let Some(mut usage) = state.store.get("algorithm_usage", &usage_id) else {
        return;
    };
    if !usage["signal"].is_object() {
        usage["signal"] = json!({"kind": kind, "successes": 0, "failures": 0});
    }
    if ok {
        usage["signal"]["successes"] =
            json!(usage["signal"]["successes"].as_u64().unwrap_or(0) + 1);
    } else {
        usage["signal"]["failures"] =
            json!(usage["signal"]["failures"].as_u64().unwrap_or(0) + 1);
    }
    usage["signalUpdatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    if let Some(saved) = state.store.put("algorithm_usage", &usage) {
        state.bus.record("algorithm_usage", &saved);
    }
}

/// Records one domain outcome for a concrete kernel. Used by harness paths that
/// do not have a normal execution record (for example a room-level debate
/// resolution) but still need the same per-agent and global statistics.
pub fn record_domain_outcome(
    state: &AppState,
    agent_id: Option<&str>,
    kind: &str,
    implementation: &str,
    ok: bool,
) {
    seed(state);
    let agent = agent_of(state, agent_id).unwrap_or_else(|| "global".into());
    let Some(revision) = state
        .store
        .all("algorithm")
        .into_iter()
        .find(|algorithm| {
            algorithm["kind"].as_str() == Some(kind)
                && algorithm["implementation"].as_str() == Some(implementation)
                && algorithm["status"].as_str() != Some("disabled")
        })
        .and_then(|algorithm| algorithm["revision"].as_str().map(str::to_string))
    else {
        return;
    };
    apply_stat(state, &agent, kind, &revision, ok);
}

fn apply_stat(state: &AppState, agent: &str, kind: &str, revision: &str, success: bool) {
    let id = stat_id(agent, kind, revision);
    let mut stat = state.store.get("algorithm_stat", &id).unwrap_or_else(|| {
        json!({
            "id": id,
            "agentId": agent,
            "kind": kind,
            "revision": revision,
            "uses": 0,
            "successes": 0,
            "failures": 0,
            "createdAt": chrono::Utc::now().to_rfc3339(),
        })
    });
    stat["uses"] = json!(stat["uses"].as_u64().unwrap_or(0) + 1);
    if success {
        stat["successes"] = json!(stat["successes"].as_u64().unwrap_or(0) + 1);
    } else {
        stat["failures"] = json!(stat["failures"].as_u64().unwrap_or(0) + 1);
    }
    stat["successRate"] = json!(
        stat["successes"].as_u64().unwrap_or(0) as f64
            / stat["uses"].as_u64().unwrap_or(1).max(1) as f64
    );
    stat["lastUsedAt"] = json!(chrono::Utc::now().to_rfc3339());
    if let Some(saved) = state.store.put("algorithm_stat", &stat) {
        state.bus.record("algorithm_stat", &saved);
    }
    // Collective statistics: a new bot starts from what the group has already
    // learned instead of a blank slate. A global agent already updated this
    // record above; do not count it twice.
    if agent == "global" {
        return;
    }
    let global_id = stat_id("global", kind, revision);
    let mut global = state.store.get("algorithm_stat", &global_id).unwrap_or_else(|| {
        json!({
            "id": global_id,
            "agentId": "global",
            "kind": kind,
            "revision": revision,
            "uses": 0,
            "successes": 0,
            "failures": 0,
            "createdAt": chrono::Utc::now().to_rfc3339(),
        })
    });
    global["uses"] = json!(global["uses"].as_u64().unwrap_or(0) + 1);
    if success {
        global["successes"] = json!(global["successes"].as_u64().unwrap_or(0) + 1);
    } else {
        global["failures"] = json!(global["failures"].as_u64().unwrap_or(0) + 1);
    }
    global["successRate"] = json!(
        global["successes"].as_u64().unwrap_or(0) as f64
            / global["uses"].as_u64().unwrap_or(1).max(1) as f64
    );
    global["lastUsedAt"] = json!(chrono::Utc::now().to_rfc3339());
    if let Some(saved) = state.store.put("algorithm_stat", &global) {
        state.bus.record("algorithm_stat", &saved);
    }
}

/// Applies the execution outcome to every algorithm used during that run.
pub fn finalize_execution(state: &AppState, execution: &Value) {
    let Some(execution_id) = execution["id"].as_str() else {
        return;
    };
    // A deterministic shortcut made no model decision, so it carries no
    // evidence for which judgment algorithm works better.
    if execution["deterministicShortcut"].is_object() {
        for mut usage in state.store.all("algorithm_usage") {
            if usage["executionId"].as_str() == Some(execution_id)
                && usage["status"].as_str() == Some("pending")
            {
                usage["status"] = json!("excluded");
                usage["reason"] = json!("deterministic shortcut: no model decision");
                state.store.put("algorithm_usage", &usage);
            }
        }
        return;
    }
    let task_success = execution["taskOutcome"].as_str() == Some("succeeded");
    for mut usage in state.store.all("algorithm_usage") {
        if usage["executionId"].as_str() != Some(execution_id)
            || usage["status"].as_str() != Some("pending")
        {
            continue;
        }
        let (Some(agent), Some(kind), Some(revision)) = (
            usage["agentId"].as_str(),
            usage["kind"].as_str(),
            usage["revision"].as_str(),
        ) else {
            continue;
        };
        // Domain signals win over the whole-task fallback. Multiple events in
        // one run are aggregated, not overwritten by the last one.
        let success = if usage["signal"].is_object() {
            let successes = usage["signal"]["successes"].as_u64().unwrap_or(0);
            let failures = usage["signal"]["failures"].as_u64().unwrap_or(0);
            successes > 0 && failures == 0 || successes > failures
        } else {
            task_success
        };
        apply_stat(state, agent, kind, revision, success);
        usage["status"] = json!("completed");
        usage["success"] = json!(success);
        state.store.put("algorithm_usage", &usage);
    }
}

/// A compact per-agent view of which judgment kernels have worked.
pub fn agent_context_block(state: &AppState, agent_id: &str) -> Option<(String, Vec<Value>)> {
    let stats = state
        .store
        .all("algorithm_stat")
        .into_iter()
        .filter(|stat| {
            stat["agentId"].as_str() == Some(agent_id)
                || stat["agentId"].as_str() == Some("global")
        })
        .collect::<Vec<_>>();
    if stats.is_empty() {
        return None;
    }
    let mut best = std::collections::BTreeMap::new();
    for stat in &stats {
        let kind = stat["kind"].as_str().unwrap_or("");
        let rate = stat["successRate"].as_f64().unwrap_or(0.0);
        let uses = stat["uses"].as_u64().unwrap_or(0);
        let entry = best.entry(kind.to_string()).or_insert((0.0, 0_u64, String::new()));
        if (rate, uses) > (entry.0, entry.1) {
            *entry = (rate, uses, stat["revision"].as_str().unwrap_or("").to_string());
        }
    }
    if best.is_empty() {
        return None;
    }
    let mut block = String::from("Algorithm performance for this bot:");
    let mut items = Vec::new();
    for (kind, (rate, uses, revision)) in best {
        block.push_str(&format!(
            "\n- {kind}: {:.0}% success over {uses} uses",
            rate * 100.0
        ));
        items.push(json!({
            "kind": "algorithm_stat",
            "algorithmKind": kind,
            "revision": revision,
            "uses": uses,
            "successRate": rate,
        }));
    }
    Some((block, items))
}

pub fn register(state: &AppState, body: &Value) -> Result<Value, String> {
    let kind = body["kind"]
        .as_str()
        .filter(|value| KINDS.contains(value))
        .ok_or("unknown algorithm kind")?;
    let name = body["name"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or("algorithm name is required")?;
    let implementation = body["implementation"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(name);
    if !kernel_exists(kind, implementation) {
        return Err(format!(
            "algorithm kind {kind} has no executable kernel {implementation}; available: {:?}",
            executable_kernels(kind)
        ));
    }
    if implementation == "expression" {
        let expression = body["parameters"]["expression"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .ok_or("expression kernel requires parameters.expression")?;
        let mut sample = std::collections::BTreeMap::new();
        for name in [
            "discrimination", "progress", "cost", "uncertainty", "historical",
            "unresolved", "precondition", "benefit", "similarity", "risk",
        ] {
            sample.insert(name.to_string(), 1.0);
        }
        eval_expression(expression, &sample)?;
    }
    let variant = body["variant"].as_str().unwrap_or(implementation);
    let parameters = body["parameters"]
        .as_object()
        .cloned()
        .map(Value::Object)
        .unwrap_or_else(|| json!({}));
    let id = format!(
        "algorithm-{}",
        crate::engine::memory::stable_id(&[kind, name, variant, &parameters.to_string()])
    );
    if let Some(existing) = state.store.get("algorithm", &id) {
        return Ok(existing);
    }
    let record = json!({
        "id": id,
        "kind": kind,
        "affects": kind,
        "name": name,
        "variant": variant,
        "implementation": implementation,
        "description": body["description"].as_str().unwrap_or(""),
        "parameters": parameters,
        "source": body["source"].as_str().unwrap_or("user"),
        "status": "available",
        "revision": crate::engine::memory::stable_id(&[kind, name, variant, &parameters.to_string()]),
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    state
        .store
        .put("algorithm", &record)
        .ok_or_else(|| "algorithm could not be stored".into())
        .map(|saved| {
            state.bus.record("algorithm", &saved);
            state.store.audit(
                "algorithm.registered",
                json!({"id": saved["id"], "kind": kind, "name": name}),
            );
            saved
        })
}

pub fn activate(state: &AppState, id: &str) -> Result<Value, String> {
    let Some(algorithm) = state.store.get("algorithm", id) else {
        return Err("algorithm not found".into());
    };
    let kind = algorithm["kind"].as_str().unwrap_or("").to_string();
    let mut records = Vec::new();
    for mut other in state.store.all("algorithm") {
        if other["kind"].as_str() == Some(kind.as_str())
            && other["status"].as_str() == Some("active")
            && other["id"].as_str() != Some(id)
        {
            other["status"] = json!("available");
            other["deactivatedAt"] = json!(chrono::Utc::now().to_rfc3339());
            records.push((other["id"].as_str().unwrap_or("").to_string(), other));
        }
    }
    let mut activated = algorithm;
    activated["status"] = json!("active");
    activated["activatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    records.push((id.to_string(), activated.clone()));
    let mut transaction = Vec::new();
    for (rid, value) in records {
        let _ = rid;
        transaction.push(("algorithm".to_string(), value));
    }
    state.store.transaction(&transaction)?;
    for (_, value) in &transaction {
        state.bus.record("algorithm", value);
    }
    state.store.audit(
        "algorithm.activated",
        json!({
            "id": id,
            "kind": kind,
            "revision": activated["revision"],
            "activationScope": "kind",
            "selfImprovementIndependent": true,
            "toolGatewayIndependent": true,
        }),
    );
    Ok(activated)
}

pub fn disable(state: &AppState, id: &str) -> Option<Value> {
    let mut algorithm = state.store.get("algorithm", id)?;
    algorithm["status"] = json!("disabled");
    let saved = state.store.put("algorithm", &algorithm)?;
    state.bus.record("algorithm", &saved);
    Some(saved)
}

/// Generates new algorithm candidates from failure evidence. The candidates
/// are real parameterizations of existing harness interfaces; anything that
/// would require a new engine primitive stays a typed proposal.
pub fn generate(state: &AppState, body: &Value) -> Value {
    let kind = body["kind"].as_str().unwrap_or("adaptation.selection");
    let evidence = body["evidence"].as_array().cloned().unwrap_or_default();
    let evidence_text = evidence
        .iter()
        .map(|item| item.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let base = format!(
        "generated-{}",
        crate::engine::memory::stable_id(&[kind, &evidence_text])
    );
    let lower = evidence_text.to_lowercase();
    let timeout_heavy = lower.matches("timeout").count() + lower.matches("시간").count();
    let permission_heavy =
        lower.matches("permission").count() + lower.matches("권한").count();
    let not_found_heavy = lower.matches("not_found").count() + lower.matches("not found").count();
    let templates: Vec<Value> = match kind {
        "adaptation.selection" => {
            // Evidence changes the starting kernel: repeated timeouts/unknown
            // causes favour a cheap probe; permission failures favour a
            // discrimination-focused search; otherwise progress wins.
            let (kernel, description, discrimination, progress) = if timeout_heavy
                > permission_heavy.max(not_found_heavy)
            {
                ("probe-first", "시간 초과/불명확 원인 증거가 많아 저비용 probe를 먼저 둔다.", 2.8, 0.4)
            } else if permission_heavy > not_found_heavy {
                (
                    "discrimination-first",
                    "권한 실패 증거가 많아 원인을 구별하는 개입을 우선한다.",
                    3.0,
                    0.5,
                )
            } else {
                (
                    "progress-first",
                    "파일/경로 실패 증거가 많아 실제 해결 개입을 우선한다.",
                    0.4,
                    2.5,
                )
            };
            vec![
                json!({"name": format!("{base}-{kernel}"), "variant": kernel, "implementation": kernel,
                    "description": description,
                    "parameters": {"discriminationWeight": discrimination, "progressWeight": progress, "costWeight": 1.2}}),
                json!({"name": format!("{base}-{kernel}-explore"), "variant": kernel, "implementation": kernel,
                    "description": "증거 기반 후보의 탐색 비중을 높인 변형",
                    "parameters": {"discriminationWeight": discrimination, "progressWeight": progress, "costWeight": 1.8}}),
            ]
        }
        "strategy.selection" => vec![
            json!({"name": format!("{base}-exploit-only"), "variant": "exploit-only", "implementation": "exploit-only",
                "description": "관측 표본이 충분한 전략만 선택한다.",
                "parameters": {"uncertaintyPenalty": 0.4, "costWeight": 1.5, "exploration": 0}}),
            json!({"name": format!("{base}-estimated"), "variant": "estimated-softmax", "implementation": "estimated-softmax",
                "description": "불확실성을 반영해 탐색과 활용을 함께 사용한다.",
                "parameters": {"uncertaintyPenalty": 0.6, "costWeight": 0.8, "exploration": 1}}),
        ],
        _ => {
            let kernel = executable_kernels(kind)
                .first()
                .copied()
                .unwrap_or("");
            if kernel.is_empty() {
                Vec::new()
            } else {
                vec![json!({
                    "name": format!("{base}-{kernel}"),
                    "variant": kernel,
                    "implementation": kernel,
                    "description": "실패 증거에서 생성된 실행 커널 후보",
                    "parameters": {},
                })]
            }
        }
    };
    let mut created = Vec::new();
    for template in templates {
        let body = json!({
            "kind": kind,
            "name": template["name"],
            "variant": template["variant"],
            "implementation": template["implementation"].as_str().unwrap_or(template["variant"].as_str().unwrap_or("")),
            "description": template["description"],
            "parameters": template["parameters"],
            "source": "generated",
        });
        if let Ok(record) = register(state, &body) {
            created.push(record);
        }
    }
    json!({"kind": kind, "created": created, "evidence": evidence})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-alg-{}", uuid::Uuid::new_v4()));
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
    fn only_executable_kernels_register_and_activation_is_kind_isolated() {
        let (state, dir) = state();
        seed(&state);
        let adaptation_before = active(&state, "adaptation.selection").unwrap();
        let strategy_before = active(&state, "strategy.selection").unwrap();
        assert!(register(
            &state,
            &json!({"kind":"adaptation.selection","name":"fake","implementation":"does-not-exist"})
        )
        .is_err());
        let record = register(
            &state,
            &json!({
                "kind":"adaptation.selection",
                "name":"cheap-probe",
                "implementation":"probe-first",
                "parameters":{"discriminationWeight":2.0}
            }),
        )
        .unwrap();
        activate(&state, record["id"].as_str().unwrap()).unwrap();
        assert_eq!(
            active(&state, "adaptation.selection").unwrap()["id"],
            record["id"]
        );
        assert_ne!(
            active(&state, "adaptation.selection").unwrap()["id"],
            adaptation_before["id"]
        );
        assert_eq!(
            active(&state, "strategy.selection").unwrap()["id"],
            strategy_before["id"],
            "activation leaked into another kind"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unverified_generated_algorithms_never_enter_live_ucb_until_promoted() {
        let (state, dir) = state();
        seed(&state);
        let candidate = register(&state, &json!({
            "kind":"memory.importance", "name":"unsafe-generated-candidate",
            "implementation":"fixed", "source":"generated",
            "parameters":{"usesWeight":999.0}
        })).unwrap();
        assert_eq!(candidate["status"], "available");
        for mut item in state.store.all("algorithm") {
            if item["kind"] == "memory.importance"
                && item["id"] != candidate["id"]
            {
                item["status"] = json!("disabled");
                state.store.put("algorithm", &item);
            }
        }
        assert!(choose_for_agent(
            &state, "memory.importance", Some("agent-a"), None
        ).is_none(), "unreviewed candidate reached production");
        let activated = activate(&state, candidate["id"].as_str().unwrap()).unwrap();
        assert_eq!(choose_for_agent(
            &state, "memory.importance", Some("agent-a"), None
        ).unwrap()["id"], activated["id"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn generation_uses_evidence_and_returns_executable_kernels() {
        let (state, dir) = state();
        let generated = generate(
            &state,
            &json!({
                "kind":"adaptation.selection",
                "evidence":["timeout timeout permission denied"]
            }),
        );
        let created = generated["created"].as_array().unwrap();
        assert!(!created.is_empty());
        for algorithm in created {
            let kind = algorithm["kind"].as_str().unwrap();
            let implementation = algorithm["implementation"].as_str().unwrap();
            assert!(kernel_exists(kind, implementation), "{algorithm}");
        }
        assert!(created
            .iter()
            .any(|algorithm| algorithm["implementation"] == "probe-first"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn expression_kernel_adds_new_selection_logic_without_rebuild() {
        let (state, dir) = state();
        let mut variables = std::collections::BTreeMap::new();
        variables.insert("discrimination".to_string(), 2.0);
        variables.insert("cost".to_string(), 0.5);
        assert_eq!(
            eval_expression("discrimination * 2 - cost", &variables).unwrap(),
            3.5
        );
        let record = register(
            &state,
            &json!({
                "kind":"adaptation.selection",
                "name":"custom-formula",
                "implementation":"expression",
                "parameters":{"expression":"discrimination * 2 - cost"}
            }),
        )
        .unwrap();
        assert_eq!(record["implementation"], "expression");
        assert!(register(
            &state,
            &json!({
                "kind":"adaptation.selection",
                "name":"broken-formula",
                "implementation":"expression",
                "parameters":{"expression":"discrimination +"}
            })
        )
        .is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn execution_reads_implementation_not_display_variant() {
        let (state, dir) = state();
        seed(&state);
        let record = register(
            &state,
            &json!({
                "kind":"tool.selection",
                "name":"display-only-name",
                "variant":"display-only-name",
                "implementation":"block-write-without-read",
                "parameters":{}
            }),
        )
        .unwrap();
        activate(&state, record["id"].as_str().unwrap()).unwrap();
        assert_eq!(variant(&state, "tool.selection"), "block-write-without-read");
        assert_ne!(variant(&state, "tool.selection"), "display-only-name");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn agent_algorithm_outcomes_become_local_and_global_statistics() {
        let (state, dir) = state();
        seed(&state);
        let chosen = choose_for_agent(
            &state, "adaptation.selection", Some("agent-a"), Some("exec-1"),
        )
        .expect("algorithm");
        assert!(state.store.all("algorithm_usage").iter().any(|usage| {
            usage["executionId"] == "exec-1"
                && usage["agentId"] == "agent-a"
                && usage["revision"] == chosen["revision"]
        }));
        finalize_execution(
            &state,
            &json!({"id":"exec-1","agentId":"agent-a","taskOutcome":"succeeded"}),
        );
        let stats = state.store.all("algorithm_stat");
        let local = stats.iter().find(|stat| {
            stat["agentId"] == "agent-a"
                && stat["revision"] == chosen["revision"]
        }).expect("local stat");
        assert_eq!(local["uses"], 1);
        assert_eq!(local["successes"], 1);
        let global = stats.iter().find(|stat| {
            stat["agentId"] == "global"
                && stat["revision"] == chosen["revision"]
        }).expect("global stat");
        assert_eq!(global["uses"], 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_domain_signal_overrides_whole_task_success() {
        let (state, dir) = state();
        seed(&state);
        let chosen = choose_for_agent(
            &state, "verification.predicate", Some("agent-a"), Some("exec-signal"),
        )
        .expect("algorithm");
        mark_signal(&state, "exec-signal", "verification.predicate", false);
        finalize_execution(
            &state,
            &json!({"id":"exec-signal","agentId":"agent-a","taskOutcome":"succeeded"}),
        );
        let stat = state
            .store
            .all("algorithm_stat")
            .into_iter()
            .find(|stat| {
                stat["agentId"] == "agent-a"
                    && stat["revision"] == chosen["revision"]
            })
            .unwrap();
        assert_eq!(stat["successes"], 0, "{stat}");
        assert_eq!(stat["failures"], 1, "{stat}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn global_execution_is_counted_once_and_signals_aggregate() {
        let (state, dir) = state();
        seed(&state);
        let chosen = choose_for_agent(
            &state, "adaptation.selection", None, Some("exec-global"),
        )
        .expect("algorithm");
        mark_signal(&state, "exec-global", "adaptation.selection", false);
        mark_signal(&state, "exec-global", "adaptation.selection", false);
        mark_signal(&state, "exec-global", "adaptation.selection", true);
        finalize_execution(
            &state,
            &json!({"id":"exec-global","taskOutcome":"succeeded"}),
        );
        let stats = state
            .store
            .all("algorithm_stat")
            .into_iter()
            .filter(|stat| stat["revision"] == chosen["revision"])
            .collect::<Vec<_>>();
        assert_eq!(stats.len(), 1, "global was counted twice: {stats:?}");
        assert_eq!(stats[0]["uses"], 1, "{stats:?}");
        assert_eq!(stats[0]["successes"], 0, "last signal overwrote failures");
        assert_eq!(stats[0]["failures"], 1, "{stats:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn active_optimizer_is_found_by_status_not_a_fixed_id() {
        let (state, dir) = state();
        state.store.put(
            "optimizer_policy",
            &json!({
                "id":"optimizer-custom",
                "revision":"optimizer-custom-v2",
                "status":"active",
                "failureClustering":"custom"
            }),
        );
        let active = crate::engine::policy::active_optimizer(&state);
        assert_eq!(active["revision"], "optimizer-custom-v2");
        let _ = std::fs::remove_dir_all(dir);
    }
}
