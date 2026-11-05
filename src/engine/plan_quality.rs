//! H9 semantic planning metric.
//!
//! File success alone cannot tell whether a planning policy produced a useful
//! plan. This scorer is deliberately deterministic and model-agnostic: it
//! measures structure (ordered steps, verification) and relevance to the
//! request (token/path overlap). It is a heuristic, not a learned evaluator.
use serde_json::{json, Value};

pub const METRIC_VERSION: &str = "plan_quality_v1";
pub const EVALUATOR_ID: &str = "plan-quality-main";
pub const EVALUATOR_VERSION: u32 = 1;
/// Independent evaluator with a different implementation, used by the outer
/// gate to detect evaluator-specific wins.
pub const INDEPENDENT_METRIC_VERSION: &str = "plan_quality_independent_v1";
pub const INDEPENDENT_EVALUATOR_ID: &str = "plan-quality-independent";
pub const INDEPENDENT_EVALUATOR_VERSION: u32 = 1;
/// Minimum semantic delta required before a planning policy can activate.
pub const MIN_SEMANTIC_DELTA: f64 = 0.05;

fn tokens(text: &str) -> std::collections::HashSet<String> {
    text.to_lowercase()
        .split(|ch: char| !(ch.is_alphanumeric() || ch == '_' || ch == '-' || ch == '.'))
        .filter(|token| token.chars().count() >= 2)
        .map(str::to_string)
        .collect()
}

fn step_count(plan: &str) -> usize {
    plan.lines()
        .filter(|line| {
            let line = line.trim_start();
            line.starts_with("- ")
                || line.starts_with("* ")
                || line.starts_with("• ")
                || line
                    .split_once(['.', ')'])
                    .is_some_and(|(prefix, rest)| {
                        !prefix.is_empty()
                            && prefix.chars().all(|ch| ch.is_ascii_digit())
                            && rest.starts_with(' ')
                    })
        })
        .count()
}

fn has_verification(plan: &str) -> bool {
    let lower = plan.to_lowercase();
    ["검증", "확인", "verify", "check", "test", "assert"]
        .iter()
        .any(|word| lower.contains(word))
}

pub fn score(plan: &str, request: &str) -> Value {
    let trimmed = plan.trim();
    let chars = trimmed.chars().count();
    if chars == 0 {
        return json!({
            "metric": METRIC_VERSION,
            "evaluator": {"id": EVALUATOR_ID, "version": EVALUATOR_VERSION},
            "total": 0.0, "structure": 0.0,
            "relevance": 0.0, "steps": 0, "hasVerification": false, "chars": 0
        });
    }
    let steps = step_count(trimmed);
    let verification = has_verification(trimmed);
    let mut structure: f64 = match steps {
        0 => 0.0,
        1 => 0.5,
        2..=6 => 1.0,
        7..=8 => 0.7,
        _ => 0.4,
    };
    if verification {
        structure = (structure + 0.2).min(1.0);
    }
    if chars < 20 {
        structure *= 0.5;
    } else if chars > 2_000 {
        structure *= 0.8;
    }
    let request_tokens = tokens(request);
    let plan_tokens = tokens(trimmed);
    let overlap = request_tokens
        .iter()
        .filter(|token| plan_tokens.contains(*token))
        .count();
    let mut relevance = if request_tokens.is_empty() {
        0.0
    } else {
        overlap as f64 / request_tokens.len() as f64
    };
    // Concrete artifact/path mentions from the request are strong signals.
    let paths = request_tokens
        .iter()
        .filter(|token| token.contains('.') && token.chars().any(|ch| ch.is_alphabetic()))
        .collect::<Vec<_>>();
    if !paths.is_empty() {
        let hit = paths
            .iter()
            .filter(|path| plan_tokens.contains(**path))
            .count();
        relevance = (relevance + 0.5 * (hit as f64 / paths.len() as f64)).min(1.0);
    }
    let total = (0.55 * structure + 0.45 * relevance).clamp(0.0, 1.0);
    json!({
        "metric": METRIC_VERSION,
        "evaluator": {"id": EVALUATOR_ID, "version": EVALUATOR_VERSION},
        "total": (total * 10_000.0).round() / 10_000.0,
        "structure": (structure * 10_000.0).round() / 10_000.0,
        "relevance": (relevance * 10_000.0).round() / 10_000.0,
        "steps": steps,
        "hasVerification": verification,
        "chars": chars,
    })
}

fn bigrams(text: &str) -> std::collections::HashSet<(char, char)> {
    let normalized = text
        .to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect::<Vec<_>>();
    normalized
        .windows(2)
        .map(|pair| (pair[0], pair[1]))
        .collect()
}

/// Independent evaluator: character-bigram overlap instead of word tokens and
/// sentence/line structure instead of numbered steps. It is intentionally a
/// different algorithm so the outer gate can detect a metric-specific win.
pub fn score_independent(plan: &str, request: &str) -> Value {
    let trimmed = plan.trim();
    if trimmed.is_empty() {
        return json!({
            "metric": INDEPENDENT_METRIC_VERSION,
            "evaluator": {"id": INDEPENDENT_EVALUATOR_ID, "version": INDEPENDENT_EVALUATOR_VERSION},
            "total": 0.0, "structure": 0.0, "relevance": 0.0,
            "steps": 0, "hasVerification": false, "chars": 0
        });
    }
    let request_grams = bigrams(request);
    let plan_grams = bigrams(trimmed);
    let relevance = if request_grams.is_empty() {
        0.0
    } else {
        (request_grams
            .intersection(&plan_grams)
            .count() as f64
            / request_grams.len() as f64)
            .min(1.0)
    };
    let non_empty_lines = trimmed
        .lines()
        .filter(|line| line.trim().chars().count() >= 2)
        .count();
    let verification = has_verification(trimmed);
    let sequencing = ["그리고", "다음", "then", "after", "먼저"]
        .iter()
        .any(|word| trimmed.to_lowercase().contains(word));
    let mut structure: f64 = match non_empty_lines {
        0 => 0.0,
        1 => 0.4,
        2..=8 => 0.9,
        _ => 0.6,
    };
    if verification {
        structure = (structure + 0.15).min(1.0);
    }
    if sequencing {
        structure = (structure + 0.1).min(1.0);
    }
    if trimmed.chars().count() < 20 {
        structure *= 0.5;
    }
    let total = (0.5 * structure + 0.5 * relevance).clamp(0.0, 1.0);
    json!({
        "metric": INDEPENDENT_METRIC_VERSION,
        "evaluator": {"id": INDEPENDENT_EVALUATOR_ID, "version": INDEPENDENT_EVALUATOR_VERSION},
        "total": (total * 10_000.0).round() / 10_000.0,
        "structure": (structure * 10_000.0).round() / 10_000.0,
        "relevance": (relevance * 10_000.0).round() / 10_000.0,
        "steps": non_empty_lines,
        "hasVerification": verification,
        "chars": trimmed.chars().count(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUEST: &str = "bench/harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.";

    #[test]
    fn a_structured_relevant_plan_scores_high() {
        let plan = "1. bench/harness_report.txt 파일을 만든다\n2. 내용을 REPORT-OK로 저장한다\n3. 파일 내용을 검증한다";
        let score = score(plan, REQUEST);
        assert!(score["total"].as_f64().unwrap() > 0.7, "{score}");
        assert_eq!(score["steps"], 3);
        assert_eq!(score["hasVerification"], true);
    }

    #[test]
    fn empty_or_irrelevant_plans_score_low() {
        assert_eq!(score("", REQUEST)["total"], 0.0);
        let vague = score("1. 작업을 진행한다\n2. 완료한다", REQUEST);
        assert!(vague["total"].as_f64().unwrap() < 0.45, "{vague}");
        let relevant = score(
            "1. bench/harness_report.txt 파일을 만든다\n2. REPORT-OK 내용을 저장한다\n3. 검증한다",
            REQUEST,
        );
        assert!(
            relevant["total"].as_f64().unwrap() > vague["total"].as_f64().unwrap()
        );
    }

    #[test]
    fn the_independent_evaluator_shares_the_direction_but_not_the_algorithm() {
        let good = score_independent(
            "1. bench/harness_report.txt 파일을 만든다\n2. REPORT-OK 내용을 저장한다\n3. 검증한다",
            REQUEST,
        );
        let empty = score_independent("", REQUEST);
        let vague = score_independent("1. 작업을 진행한다\n2. 완료한다", REQUEST);
        assert_eq!(empty["total"], 0.0);
        assert!(good["total"].as_f64().unwrap() > 0.5, "{good}");
        assert!(
            good["total"].as_f64().unwrap() > vague["total"].as_f64().unwrap()
        );
        assert_eq!(good["metric"], INDEPENDENT_METRIC_VERSION);
        assert_eq!(good["evaluator"]["id"], INDEPENDENT_EVALUATOR_ID);
        assert_eq!(
            good["evaluator"]["version"],
            INDEPENDENT_EVALUATOR_VERSION
        );
        assert_ne!(good["metric"], METRIC_VERSION);
    }

    #[test]
    fn scoring_is_deterministic_and_bounded() {
        let plan = "1. bench/harness_report.txt를 생성한다\n2. 검증한다";
        let first = score(plan, REQUEST);
        let second = score(plan, REQUEST);
        assert_eq!(first, second);
        let total = first["total"].as_f64().unwrap();
        assert!((0.0..=1.0).contains(&total));
        assert_eq!(first["metric"], METRIC_VERSION);
    }
}
