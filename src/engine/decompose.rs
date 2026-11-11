//! Deterministic task decomposition.
//!
//! The harness needs an ordered list of sub-goals it can verify without asking
//! a model to grade itself. Splitting is purely lexical and the resulting
//! quality score is independent from whole-task success: it measures token
//! coverage, order preservation and how many explicit clauses were found.
use serde_json::{json, Value};

pub const KERNELS: &[&str] = &["single-step", "clause-split", "numbered-first"];

fn kernel(algorithm: &Value) -> &str {
    algorithm["implementation"]
        .as_str()
        .or_else(|| algorithm["variant"].as_str())
        .filter(|kernel| KERNELS.contains(kernel))
        .unwrap_or("single-step")
}

fn is_boundary(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, ',' | '.' | ';' | ':' | '!' | '?' | ')' | '(' | '"' | '\'')
}

fn is_hangul(ch: char) -> bool {
    matches!(ch, '\u{AC00}'..='\u{D7A3}' | '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}')
}

fn marker_at(text: &str, index: usize, marker: &str) -> bool {
    let Some(rest) = text.get(index..) else {
        return false;
    };
    if !rest.starts_with(marker) {
        return false;
    }
    let before = text[..index].chars().next_back();
    let after = text[index + marker.len()..].chars().next();
    let before_ok = before.is_none_or(is_boundary);
    let after_ok = after.is_none_or(is_boundary);
    before_ok && after_ok
}

/// Korean connective '-고' occurs after the verb rather than as a standalone
/// word. Keep that suffix attached to the preceding step so token coverage
/// and the original instruction survive decomposition. Restrict to common
/// action endings to avoid splitting nouns such as '최고' or '사고'.
fn action_conjunction_at(input: &str, index: usize, current: &str) -> bool {
    let Some(rest) = input.get(index..) else { return false };
    let Some(after) = rest.strip_prefix('고') else { return false };
    if !after.chars().next().is_some_and(char::is_whitespace) {
        return false;
    }
    let Some(previous) = current.chars().last() else { return false };
    matches!(
        previous,
        '읽' | '하' | '쓰' | '받' | '찾' | '풀' | '적' | '보' | '열'
            | '닫' | '넣' | '켜' | '끄' | '내' | '주' | '잡' | '먹'
            | '쌓' | '짚' | '옮' | '타' | '돌' | '맞' | '펴'
    )
}

fn split_markers(input: &str) -> Vec<String> {
    // Suffix forms such as "실행한 뒤" keep the verb stem, so the marker is the
    // standalone word rather than the whole phrase.
    const MARKERS: &[&str] = &[
        "그리고",
        "그 다음",
        "다음으로",
        "다음",
        "나서",
        "뒤",
        "후에",
        "then",
        "after that",
    ];
    let mut pieces = Vec::new();
    let mut current = String::new();
    let mut index = 0usize;
    'scan: while index < input.len() {
        for marker in MARKERS {
            if marker_at(input, index, marker) {
                pieces.push(current.trim().to_string());
                current.clear();
                index += marker.len();
                continue 'scan;
            }
        }
        let Some(ch) = input[index..].chars().next() else {
            break;
        };
        if ch == '고' && action_conjunction_at(input, index, &current) {
            current.push(ch);
            pieces.push(current.trim().to_string());
            current.clear();
            index += ch.len_utf8();
            continue;
        }
        if ch == ';' || ch == '\n' {
            pieces.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(ch);
        }
        index += ch.len_utf8();
    }
    pieces.push(current.trim().to_string());
    pieces
        .into_iter()
        .map(|piece| {
            piece
                .trim_matches(|ch: char| ch.is_whitespace() || matches!(ch, '-' | '*' | '•'))
                .to_string()
        })
        .filter(|piece| !piece.is_empty())
        .collect()
}

fn numbered_lines(input: &str) -> Vec<String> {
    input
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            let bullet = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .or_else(|| trimmed.strip_prefix("• "));
            if let Some(text) = bullet {
                return (!text.trim().is_empty()).then(|| text.trim().to_string());
            }
            let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
            if digits == 0 {
                return None;
            }
            let rest = trimmed[digits..].trim_start();
            let rest = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')'))?;
            let text = rest.trim();
            (!text.is_empty()).then(|| text.to_string())
        })
        .collect()
}

fn tokens(text: &str) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() || is_hangul(ch) {
            current.extend(ch.to_lowercase());
        } else if !current.is_empty() {
            if current.chars().count() >= 2 {
                out.insert(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
    }
    if current.chars().count() >= 2 {
        out.insert(current);
    }
    out
}

fn quality(input: &str, subtasks: &[Value], expected: usize) -> Value {
    let input_tokens = tokens(input);
    let covered = subtasks
        .iter()
        .filter_map(|subtask| subtask["text"].as_str())
        .flat_map(tokens)
        .collect::<std::collections::BTreeSet<_>>();
    let coverage = if input_tokens.is_empty() {
        if subtasks.is_empty() { 0.0 } else { 1.0 }
    } else {
        input_tokens.iter().filter(|token| covered.contains(*token)).count() as f64
            / input_tokens.len() as f64
    };
    let lower = input.to_lowercase();
    let mut last = 0usize;
    let mut ordered = true;
    for subtask in subtasks {
        let Some(text) = subtask["text"].as_str() else {
            continue;
        };
        let needle = text.to_lowercase();
        if let Some(index) = lower[last..].find(&needle) {
            last += index + needle.len();
        } else {
            ordered = false;
            break;
        }
    }
    let order = if ordered { 1.0 } else { 0.6 };
    let count = if expected == 0 {
        1.0
    } else {
        (subtasks.len() as f64 / expected as f64).min(1.0)
    };
    let score = (coverage * order * count).clamp(0.0, 1.0);
    json!({
        "tokens": input_tokens.len(),
        "coverage": coverage,
        "orderPreserved": ordered,
        "expectedSegments": expected,
        "countFactor": count,
        "score": score,
    })
}

/// Splits `input` with the selected kernel. `single-step` always yields one
/// subtask; `clause-split` splits on explicit sequence markers; and
/// `numbered-first` prefers an explicit numbered/bulleted list, then falls
/// back to clause splitting.
pub fn decompose(input: &str, algorithm: &Value) -> Value {
    let kernel = kernel(algorithm);
    let trimmed = input.trim();
    let expected = split_markers(trimmed).len().max(1);
    let parts = match kernel {
        "numbered-first" => {
            let numbered = numbered_lines(trimmed);
            if numbered.len() >= 2 {
                numbered
            } else {
                split_markers(trimmed)
            }
        }
        "clause-split" => split_markers(trimmed),
        _ => {
            if trimmed.is_empty() {
                Vec::new()
            } else {
                vec![trimmed.to_string()]
            }
        }
    };
    let subtasks = parts
        .into_iter()
        .enumerate()
        .map(|(index, text)| json!({"index": index + 1, "text": text}))
        .collect::<Vec<_>>();
    let grade = quality(trimmed, &subtasks, expected);
    json!({
        "kernel": kernel,
        "subtasks": subtasks,
        "quality": grade,
        "qualityScore": grade["score"],
        "revision": algorithm["revision"].clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn algorithm(implementation: &str) -> Value {
        json!({"kind": "task.decomposition", "implementation": implementation})
    }

    #[test]
    fn clause_split_preserves_order_and_coverage() {
        let result = decompose(
            "설정 파일을 읽고 그리고 테스트를 실행한 뒤 결과를 보고해",
            &algorithm("clause-split"),
        );
        let texts = result["subtasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|task| task["text"].as_str())
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert_eq!(texts.len(), 3, "{result}");
        assert!(texts[0].contains("설정 파일"));
        assert!(texts[1].contains("테스트"));
        assert!(texts[2].contains("결과"));
        assert_eq!(result["quality"]["orderPreserved"], true);
        assert!(result["qualityScore"].as_f64().unwrap() >= 0.75, "{result}");
    }

    #[test]
    fn single_step_is_graded_lower_on_multi_clause_requests() {
        let input = "읽고 그리고 실행한 뒤 보고해";
        let single = decompose(input, &algorithm("single-step"));
        let split = decompose(input, &algorithm("clause-split"));
        assert_eq!(single["subtasks"].as_array().unwrap().len(), 1);
        assert_eq!(split["subtasks"].as_array().unwrap().len(), 3);
        assert!(
            split["qualityScore"].as_f64().unwrap() > single["qualityScore"].as_f64().unwrap(),
            "single={} split={}",
            single["qualityScore"],
            split["qualityScore"]
        );
    }

    #[test]
    fn numbered_first_prefers_an_explicit_list() {
        let result = decompose(
            "1. 저장소를 확인한다\n2. 테스트를 실행한다\n3. 결과를 정리한다",
            &algorithm("numbered-first"),
        );
        let texts = result["subtasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|task| task["text"].as_str())
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert_eq!(texts.len(), 3, "{result}");
        assert_eq!(texts[0], "저장소를 확인한다");
        assert_eq!(result["quality"]["orderPreserved"], true);
        assert!(result["qualityScore"].as_f64().unwrap() >= 0.9, "{result}");
    }

    #[test]
    fn korean_unmarked_sequences_become_executable_steps() {
        let cases = [
            ("설정을 읽고 결과를 저장한 다음 검증해", 3),
            ("문서를 읽고 파일을 저장하고 실행 결과를 검증해", 3),
            ("모델을 선택하고 응답을 저장한 다음 결과를 비교해", 3),
            ("서버를 켜고 로그를 보고 결과를 정리해", 3),
            ("원본을 유지하고 변경을 확인해", 2),
            ("로그를 읽고 오류를 찾아보고 수정한 다음 테스트해", 4),
            ("명령을 실행하고 나서 결과를 보고해", 2),
            ("최고 수준의 설계안을 작성해", 1),
            ("교통사고 보고서를 확인해", 1),
        ];
        for (input, expected) in cases {
            let result = decompose(input, &algorithm("numbered-first"));
            let steps = result["subtasks"].as_array().unwrap();
            assert_eq!(steps.len(), expected, "{input}: {result}");
            assert_eq!(result["quality"]["orderPreserved"], true, "{input}");
            assert!(result["qualityScore"].as_f64().unwrap() > 0.65, "{input}: {result}");
        }
    }

    #[test]
    fn empty_input_yields_no_subtasks() {
        let result = decompose("   ", &algorithm("clause-split"));
        assert!(result["subtasks"].as_array().unwrap().is_empty());
        assert_eq!(result["qualityScore"].as_f64().unwrap(), 0.0);
    }

    #[test]
    fn measurement_clause_split_improves_decomposition_quality() {
        let requests = [
            "설정 파일을 읽고 그리고 테스트를 실행한 뒤 결과를 보고해",
            "로그를 확인하고 그리고 오류를 분류한 뒤 수정안을 작성해",
            "저장소를 복제하고 다음으로 의존성을 설치한 뒤 빌드를 실행해",
        ];
        let average = |implementation: &str| {
            let scores = requests
                .iter()
                .map(|request| {
                    decompose(request, &algorithm(implementation))["qualityScore"]
                        .as_f64()
                        .unwrap_or(0.0)
                })
                .collect::<Vec<_>>();
            scores.iter().sum::<f64>() / scores.len() as f64
        };
        let single = average("single-step");
        let split = average("clause-split");
        println!(
            "decomposition measurement: avgQuality single={single:.2} clause-split={split:.2}"
        );
        assert!(split > single, "clause-split did not improve: {single} -> {split}");
        assert!(split >= 0.8, "clause-split quality below target: {split}");
    }
}
