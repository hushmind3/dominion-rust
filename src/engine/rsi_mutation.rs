//! A source-driven candidate generator inside DOMINION's native RSI worker.
//! Its output is not an improvement until the frozen paired evaluator accepts it.
use crate::{engine::rsi, state::AppState};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// Discover an iterator scoring function which repeatedly tokenizes an
/// invariant query. Hoist its allocation once, preserving the original
/// intersection, domain bonus, ordering, and tie-breaking expressions.
fn hoist_invariant_query_terms(source: &str) -> Option<String> {
    let term_line = "    let query_tokens = memory::terms(query);\n";
    let ranking_anchor = "    let mut ranked = facts\n";
    if source.matches(term_line).count() != 1 || source.matches(ranking_anchor).count() != 1 {
        return None;
    }
    for line in source.lines() {
        let signature = line.trim();
        if !signature.starts_with("fn ")
            || !signature.ends_with("(query: &str, text: &str) -> usize {") {
            continue;
        }
        let name = signature.strip_prefix("fn ")?.split('(').next()?;
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let call = format!("{name}(query, text)");
        let function = format!("fn {name}(query: &str, text: &str) -> usize {{");
        if source.matches(&call).count() != 1 || source.matches(&function).count() != 1 {
            continue;
        }
        let begin = source.find(&function)?;
        let end = begin + source[begin..].find("\n}\n")?;
        if !source[begin..end].contains(term_line)
            || !source[begin..end].contains("query_tokens.intersection(&text_tokens)") {
            continue;
        }
        let caller = &source[..source.find(&call)?];
        if !caller.contains(ranking_anchor)
            || !caller[caller.rfind(ranking_anchor)?..].contains(".filter_map(") {
            continue;
        }
        let replacement = format!(
            "fn {name}(query: &str, text: &str, query_tokens: &std::collections::BTreeSet<String>) -> usize {{"
        );
        let mut output = source.replacen(&function, &replacement, 1);
        output = output.replacen(term_line, "", 1);
        output = output.replacen(
            ranking_anchor,
            "    let query_tokens = memory::terms(query);\n    let mut ranked = facts\n", 1
        );
        output = output.replacen(&call, &format!("{name}(query, text, &query_tokens)"), 1);
        return Some(output);
    }
    None
}

/// One inductive change, isolated in the disposable git worktree. If the
/// model or inherited descendant already changed bytes, do not mix authors.
pub(crate) fn induce(
    state: &AppState, job_id: &str, input: &Value, source: &Path, isolated: &Path,
) -> Result<Option<Value>, String> {
    if input["rsiCampaignId"].as_str().is_none() { return Ok(None); }
    let Some(files) = input["rsiFocusFiles"].as_array().filter(|v| v.len() == 1) else {
        return Ok(None);
    };
    let Some(relative) = files[0].as_str().filter(|path| rsi::safe_path(path)) else {
        return Ok(None);
    };
    if input["rsiFailedPatchRecovery"]["candidateId"].is_string() { return Ok(None); }
    let original = fs::read_to_string(source.join(relative)).map_err(|e| e.to_string())?;
    let existing = fs::read_to_string(isolated.join(relative)).map_err(|e| e.to_string())?;
    if original != existing { return Ok(None); }
    let Some(transformed) = hoist_invariant_query_terms(&existing) else { return Ok(None); };
    let receipt = json!({
        "id": job_id, "operator": "loop_invariant_term_hoist_v1",
        "origin": "native_inductive_source_mutation",
        "path": relative, "beforeSha256": digest(&existing),
        "afterSha256": digest(&transformed),
        "sourceChanged": true, "qualityGain": Value::Null,
        "proof": "pending_independent_compile_and_paired_holdout",
    });
    fs::write(isolated.join(relative), transformed).map_err(|e| e.to_string())?;
    state.store.put("rsi_mutation", &receipt)
        .ok_or("RSI 소스 변형 영수증을 저장하지 못했습니다.")?;
    state.store.audit("rsi.inductive_source_mutation", receipt.clone());
    Ok(Some(receipt))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hoists_repeated_query_tokenization_without_changing_scoring() {
        let source = r#"fn relevance(query: &str, text: &str) -> usize {
    let query_tokens = memory::terms(query);
    let text_tokens = memory::terms(text);
    score + query_tokens.intersection(&text_tokens).count().min(12)
}
fn choose(facts: &[Value], query: &str) {
    let mut ranked = facts
        .iter()
        .filter_map(|(index, fact)| {
            let text = fact["text"].as_str()?;
            Some((relevance(query, text), index, fact))
        });
}
"#;
        let result = hoist_invariant_query_terms(source).unwrap();
        assert_eq!(result.matches("memory::terms(query)").count(), 1);
        assert!(result.contains("relevance(query, text, &query_tokens)"));
        assert!(result.contains("let query_tokens = memory::terms(query);\n    let mut ranked"));
        assert!(hoist_invariant_query_terms(&result).is_none());
        assert!(hoist_invariant_query_terms("fn unrelated() {}").is_none());
    }
}
