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
        "selfimprove.search" => &["failure-clusters", "counterexample-first", "expression"],
        _ => &[],
    }
}

pub fn kernel_exists(kind: &str, implementation: &str) -> bool {
    executable_kernels(kind)
        .iter()
        .any(|kernel| *kernel == implementation)
}

/// Features available to a model-authored `selfimprove.search` expression.
/// The search coordinator fills exactly these names before evaluating a domain.
pub const SELFIMPROVE_FEATURES: &[&str] = &[
    "attempts",
    "accepted",
    "regressions",
    "inconclusive",
    "novelty",
    "successRate",
    "usefulGenerations",
    "barrenRate",
    "tokenCost",
    "objectiveMatch",
    "unresolved",
    "exploration",
];

fn expression_sample(kind: &str) -> std::collections::BTreeMap<String, f64> {
    let names: &[&str] = if kind == "selfimprove.target" {
        SELFIMPROVE_TARGET_FEATURES
    } else if kind == "selfimprove.search" {
        SELFIMPROVE_FEATURES
    } else {
        &[
            "discrimination",
            "progress",
            "cost",
            "uncertainty",
            "historical",
            "unresolved",
            "precondition",
            "benefit",
            "similarity",
            "risk",
        ]
    };
    names
        .iter()
        .map(|name| (name.to_string(), 1.0))
        .collect()
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

/// Features supplied by the real indexed source selector to an AI-authored
/// targeting formula. Unlike a hardcoded path table, these are measured from
/// the current checkout and previous generations.
pub const SELFIMPROVE_TARGET_FEATURES: &[&str] = &[
    "pathEvidence", "symbolEvidence", "summaryEvidence", "bodyEvidence", "productiveEvidence",
    "domainEvidence", "repeatPenalty", "novelty", "sourceCost",
];

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
    // Generated arithmetic must be bounded, finite and free of stack-exhaustion
    // inputs. It cannot call arbitrary Rust functions or external tools.
    if expression.chars().count()>512 {
        return Err("expression exceeds 512 characters".into());
    }
    let mut nesting=0usize;
    let mut depth=0usize;
    for ch in expression.chars() {
        if ch=='(' {
            nesting+=1;
            depth=depth.max(nesting);
            if depth>32 {return Err("expression nesting exceeds 32".into());}
        } else if ch==')' {
            nesting=nesting.saturating_sub(1);
        }
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
    if !value.is_finite() {
        return Err("expression produced a non-finite score".into());
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


// Source-level responsibilities split; historical public kernel entry points are preserved.
#[path = "algorithms/builtins.rs"]
mod builtins;
use builtins::*;
pub use builtins::{builtins};
#[path = "algorithms/registry.rs"]
mod registry;
use registry::*;
pub use registry::{seed,list,active,variant};
#[path = "algorithms/feedback.rs"]
mod feedback;
use feedback::*;
pub use feedback::{choose_for_agent,variant_for_agent,mark_signal,mark_unrated,record_domain_outcome,finalize_execution,rebuild_stats_from_usage};
#[path = "algorithms/management.rs"]
mod management;
use management::*;
pub use management::{agent_context_block,register,activate,activate_checked,disable,generate};

#[cfg(test)]
#[path = "algorithms/tests.rs"]
mod tests;
