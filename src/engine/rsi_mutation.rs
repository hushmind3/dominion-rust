//! A source-driven candidate generator inside DOMINION's native RSI worker.
//! Its output is not an improvement until the frozen paired evaluator accepts it.
use crate::{engine::rsi, state::AppState};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// The writer has already authenticated every durable owner message.
    /// Store that immutable provenance once, instead of re-querying the source
    /// message table for each remembered fact at every subsequent recall.
    /// Legacy facts retain the existing origin lookup for compatibility.
fn materialize_verified_owner_provenance(source: &str) -> Option<String> {
    let guard="|| message[\"metadata\"][\"origin\"].is_object()";
    let creation="        \"status\": \"active\",\n    }));";
    let original="        .filter(|(_, fact)| {\n            !fact[\"sourceMessageId\"]\n                .as_str()\n                .and_then(|id| state.store.message(id))\n                .is_some_and(|source| source[\"metadata\"][\"origin\"].is_object())\n        })";
    let replacement="        .filter(|(_, fact)| {\n            // Ingestion already authenticated local owner records. Legacy\n            // entries without the flag still revalidate against the source.\n            fact[\"verifiedLocalOwner\"]==true || !fact[\"sourceMessageId\"]\n                .as_str()\n                .and_then(|id| state.store.message(id))\n                .is_some_and(|source| source[\"metadata\"][\"origin\"].is_object())\n        })";
    if !source.contains(guard) || source.matches(creation).count()!=1
        || source.matches(original).count()!=1 {return None;}
    let with_fact=source.replacen(creation,
        "        \"status\": \"active\",\n        \"verifiedLocalOwner\": true,\n    }));",1);
    Some(with_fact.replacen(original,replacement,1))
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

/// A third-generation structural optimization: facts originate as immutable
/// owner statements, yet their text terms are reconstructed for every query.
/// Cache the exact terms of the *stored, truncated* text at ingestion. Legacy
/// rows keep the original runtime tokenization path, preserving behavior.
fn cache_invariant_fact_terms(source: &str) -> Option<String> {
    let scorer = "fn query_relevance(query: &str, text: &str, query_tokens: &std::collections::BTreeSet<String>) -> usize {";
    let call = "query_relevance(query, text, &query_tokens)";
    let text_calc = "    let text_tokens = memory::terms(text);\n    score + query_tokens.intersection(&text_tokens).count().min(12)";
    let stored = "        \"text\": text.chars().take(MAX_ENTRY_CHARS).collect::<String>(),";
    let insertion = "    facts.push(json!({\n";
    let context_call = format!("(!text.is_empty()).then_some(({call}, index, fact))");
    if [scorer, call, text_calc, stored, insertion, context_call.as_str()]
        .iter().any(|marker| source.matches(marker).count() != 1)
    {
        return None;
    }
    let mut output = source.replacen(
        insertion,
        "    let stored_text = text.chars().take(MAX_ENTRY_CHARS).collect::<String>();\n    let fact_tokens = memory::terms(&stored_text).into_iter().collect::<Vec<_>>();\n    facts.push(json!({\n",
        1,
    );
    output = output.replacen(stored, "        \"text\": stored_text,\n        \"factTokens\": fact_tokens,", 1);
    output = output.replacen(
        scorer,
        "fn query_relevance(query: &str, text: &str, query_tokens: &std::collections::BTreeSet<String>, cached_terms: Option<&Vec<Value>>) -> usize {",
        1,
    );
    output = output.replacen(
        text_calc,
        "    let matches = if let Some(terms) = cached_terms {\n        terms.iter().filter(|term| term.as_str().is_some_and(|term| query_tokens.contains(term))).count()\n    } else {\n        let text_tokens = memory::terms(text);\n        query_tokens.intersection(&text_tokens).count()\n    };\n    score + matches.min(12)",
        1,
    );
    output = output.replacen(
        &context_call,
        &format!("(!text.is_empty()).then_some((query_relevance(query, text, &query_tokens, fact[\"factTokens\"].as_array()), index, fact))"),
        1,
    );
    Some(output)
}

/// A second-generation optimization is discoverable only after the first
/// source transform changed the scoring call. It learns that a query domain
/// is invariant across all candidate facts, and that no stated domain lookup
/// is required when the requested domain is absent.
fn hoist_invariant_query_domain(source: &str) -> Option<String> {
    let marker = "    let query_tokens = memory::terms(query);\n    let mut ranked = facts\n";
    let scorer = "fn query_relevance(query: &str, text: &str, query_tokens: &std::collections::BTreeSet<String>) -> usize {";
    let call = "query_relevance(query, text, &query_tokens)";
    let old = "    if let (Some(asked), Some(stated)) = (fact_domain(query), fact_domain(text)) {\n        if asked == stated {\n            score += 32;\n        }\n    }";
    if source.matches(marker).count()!=1 || source.matches(scorer).count()!=1
        || source.matches(call).count()!=1 || source.matches(old).count()!=1 {
        return None;
    }
    let mut output=source.replacen(
        marker,
        "    let query_tokens = memory::terms(query);\n    let asked_domain = fact_domain(query);\n    let mut ranked = facts\n",1
    );
    output=output.replacen(
        scorer,
        "fn query_relevance(query: &str, text: &str, query_tokens: &std::collections::BTreeSet<String>, asked_domain: Option<&str>) -> usize {",1
    );
    output=output.replacen(
        old,
        "    if let Some(asked) = asked_domain {\n        if fact_domain(text) == Some(asked) {\n            score += 32;\n        }\n    }",1
    );
    output=output.replacen(
        call,"query_relevance(query, text, &query_tokens, asked_domain)",1
    );
    Some(output)
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
    let (operator,transformed)=if let Some(next)=materialize_verified_owner_provenance(&existing) {
        ("verified_owner_provenance_cache_v1",next)
    }else if let Some(next)=hoist_invariant_query_terms(&existing) {
        ("loop_invariant_term_hoist_v1",next)
    }else if let Some(next)=cache_invariant_fact_terms(&existing) {
        ("immutable_owner_fact_token_cache_v1",next)
    }else if let Some(next)=hoist_invariant_query_domain(&existing) {
        ("loop_invariant_domain_hoist_v2",next)
    }else{
        return Ok(None);
    };
    // The exact same code transformation is deterministic. A rejected
    // source hash/operator pair must not loop forever as a new generation.
    // Keep each repository partition isolated from other users' histories.
    let repository=source.to_string_lossy().to_string();
    let before=digest(&existing);
    let already_measured=state.store.all("rsi_mutation").iter().any(|trial|
        trial["sourceDir"].as_str()==Some(repository.as_str())
            && trial["beforeSha256"].as_str()==Some(before.as_str())
            && trial["operator"].as_str()==Some(operator)
            && trial["evaluation"].is_object());
    if already_measured { return Ok(None); }
    let receipt = json!({
        "id": job_id, "operator": operator, "sourceDir": repository,
        "campaignId": input["rsiCampaignId"],
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
    fn verified_owner_origin_cache_keeps_legacy_validation() {
        let original=include_str!("room_state.rs");
        let first=materialize_verified_owner_provenance(original)
            .expect("authenticated owner provenance is a native optimization");
        assert!(first.contains("\"verifiedLocalOwner\": true"));
        assert!(first.contains("fact[\"verifiedLocalOwner\"]==true || !fact[\"sourceMessageId\"]"));
        assert!(first.contains(".and_then(|id| state.store.message(id))"));
        assert!(materialize_verified_owner_provenance(&first).is_none());
        let second=hoist_invariant_query_terms(&first).expect("next code optimization");
        let third=hoist_invariant_query_domain(&second).expect("third source generation");
        assert_ne!(first,second);
        assert_ne!(second,third);
    }

    #[test]
    fn current_engine_has_two_distinct_source_generations() {
        let original=include_str!("room_state.rs");
        let first=hoist_invariant_query_terms(original).expect("first source generation");
        assert_ne!(first,original);
        let second=hoist_invariant_query_domain(&first).expect("descendant source generation");
        assert_ne!(second,first);
        assert!(second.contains("let asked_domain = fact_domain(query);"));
        assert!(hoist_invariant_query_terms(&second).is_none());
        assert!(hoist_invariant_query_domain(&second).is_none());
    }

    #[test]
    fn recursive_fact_cache_preserves_legacy_fallback_and_original_ranking() {
        let source = include_str!("room_state.rs");
        let hoisted = hoist_invariant_query_terms(source).unwrap();
        let cached = cache_invariant_fact_terms(&hoisted).unwrap();
        assert_ne!(hoisted, cached);
        assert!(cached.contains("\"factTokens\": fact_tokens"));
        assert!(cached.contains("memory::terms(&stored_text)"));
        assert!(cached.contains("fact[\"factTokens\"].as_array()"));
        assert!(cached.contains("let text_tokens = memory::terms(text);"));
        assert!(cached.contains("score + matches.min(12)"));
        assert_eq!(cached.matches("let query_tokens = memory::terms(query);").count(), 1);
        assert!(cache_invariant_fact_terms(&cached).is_none());
        // The existing next-generation domain optimization must still
        // compose with the new immutable fact-token cache.
        let descendant = hoist_invariant_query_domain(&cached).unwrap();
        assert!(descendant.contains("let asked_domain = fact_domain(query);"));
        assert!(hoist_invariant_query_domain(&descendant).is_none());
    }

    #[test]
    fn derives_new_query_domain_hoist_after_first_generation() {
        let source=r#"fn query_relevance(query: &str, text: &str, query_tokens: &std::collections::BTreeSet<String>) -> usize {
    if query.trim().is_empty() { return 0; }
    let mut score = 0usize;
    if let (Some(asked), Some(stated)) = (fact_domain(query), fact_domain(text)) {
        if asked == stated {
            score += 32;
        }
    }
    let text_tokens = memory::terms(text);
    score + query_tokens.intersection(&text_tokens).count().min(12)
}
fn context(facts: &[Value], query: &str) {
    let query_tokens = memory::terms(query);
    let mut ranked = facts
        .iter()
        .filter_map(|(index, fact)| {
            let text = fact["text"].as_str()?;
            Some((query_relevance(query, text, &query_tokens), index, fact))
        });
}
"#;
        let next=hoist_invariant_query_domain(source).unwrap();
        assert_eq!(next.matches("fact_domain(query)").count(),1);
        assert!(next.contains("if let Some(asked) = asked_domain"));
        assert!(next.contains("query_relevance(query, text, &query_tokens, asked_domain)"));
        assert!(hoist_invariant_query_domain(&next).is_none());
    }

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
