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

/// Improve the improver itself. A search-policy evaluation currently
/// materializes the same domain's history as a Vec, then traverses it again
/// for every feature, and re-parses eligible domains for the tie breaker.
/// Synthesize one streaming accumulator, retaining the original scoring,
/// objective scope, and ordering semantics. The frozen improver holdout is
/// still responsible for proving any execution-time gain.
fn stream_search_policy_statistics(source: &str) -> Option<String> {
    let first = "fn search_features(history: &[Value], domain: &str, objective: &str) -> BTreeMap<String, f64> {";
    let after = "\n}\n\nfn score_domain(";
    let start = source.find(first)?;
    if source.matches(first).count() != 1 { return None; }
    let end = start + source[start..].find(after)? + 2;
    let original = &source[start..end];
    let old_rows = "    let rows = history\n        .iter()\n        .filter(|row| row[\"domain\"] == domain)\n        .collect::<Vec<_>>();";
    if !original.contains(old_rows)
        || !original.contains("rows.iter().filter(|row| row[\"accepted\"] == true)")
        || !original.contains("rows\n        .last()")
    {
        return None;
    }
    let old_tie = "let mut best=(f64::NEG_INFINITY,eligible_domains(objective)[0]);";
    if source.matches(old_tie).count() != 1 { return None; }
    let optimized = r#"fn search_features(history: &[Value], domain: &str, objective: &str) -> BTreeMap<String, f64> {
    // One witnessed generation produces all features for its domain in one
    // traversal; no temporary Vec or repeated history scans.
    let mut attempts = 0.0_f64;
    let mut accepted = 0.0_f64;
    let mut regressions = 0.0_f64;
    let mut inconclusive = 0.0_f64;
    let mut tokens = 0u64;
    let mut useful = 0.0_f64;
    let mut last_row: Option<&Value> = None;
    for row in history.iter().filter(|row|row["domain"]==domain) {
        attempts += 1.0;
        accepted += (row["accepted"]==true) as u8 as f64;
        regressions += (row["evaluation"]["verdict"]=="regressed") as u8 as f64;
        inconclusive += (row["evaluation"]["verdict"]=="inconclusive") as u8 as f64;
        tokens += row["tokens"].as_u64().unwrap_or(0);
        useful += (row["accepted"]==true
            || row["candidateId"].is_string()
            || row["newFindings"].as_u64().unwrap_or(0)>0) as u8 as f64;
        last_row=Some(row);
    }
    let barren_rate=if attempts==0.0 {0.0}else{1.0-useful/attempts};
    let success_rate=if attempts==0.0 {0.0}else{accepted/attempts};
    let token_cost=if attempts==0.0 {0.0}else{(tokens as f64/attempts)/10_000.0};
    let objective_match=if objective_matches(domain,objective){1.0}else{0.0};
    let unresolved=if last_row.is_some_and(|row|
        row["candidateId"].is_null() && row["accepted"]!=true) {1.0}else{0.0};
    let exploration=if attempts==0.0 {3.0}else{
        (2.0*((history.len() as f64)+1.0).ln()/attempts).sqrt().min(3.0)
    };
    BTreeMap::from([
        ("attempts".to_string(),attempts),
        ("accepted".to_string(),accepted),
        ("regressions".to_string(),regressions),
        ("inconclusive".to_string(),inconclusive),
        ("novelty".to_string(),if attempts==0.0 {1.0}else{0.0}),
        ("successRate".to_string(),success_rate),
        ("usefulGenerations".to_string(),useful),
        ("barrenRate".to_string(),barren_rate),
        ("tokenCost".to_string(),token_cost),
        ("objectiveMatch".to_string(),objective_match),
        ("unresolved".to_string(),unresolved),
        ("exploration".to_string(),exploration),
    ])
}"#;
    let mut transformed = String::with_capacity(source.len());
    transformed.push_str(&source[..start]);
    transformed.push_str(optimized);
    transformed.push_str(&source[end..]);
    let transformed=transformed.replacen(old_tie,
        "let mut best=(f64::NEG_INFINITY,scores.first().map(|(domain,_)|*domain).unwrap_or(DOMAINS[0]));",1);
    // Fast-path the observed default policy, not an unverified new formula:
    // preserve exact failure-cluster scores, domain tie order and objective
    // constraints while computing all six domains in ONE streaming pass.
    // Expression and counterexample-first policies retain their full feature
    // interpreter. This separation is inspired by measured-harness agent
    // optimization (Agent Lightning) and verified experiment-tree selection.
    let entry="pub(crate) fn select_domain(algorithm: &Value, history: &[Value], objective: &str) -> String {";
    if transformed.matches(entry).count()!=1 {return None;}
    let fast=r#"fn fast_failure_cluster_domain(history:&[Value],objective:&str)->String {
    let eligible=eligible_domains(objective);
    let mut attempts=[0.0_f64;6];
    let mut accepted=[0.0_f64;6];
    let mut useful=[0.0_f64;6];
    let mut tokens=[0u64;6];
    // All candidate domains learn from the SAME witnessed generation trace.
    // No per-domain Vec, map allocation or repeated scan of history.
    for row in history {
        let Some(domain)=row["domain"].as_str() else {continue};
        if !eligible.contains(&domain) {continue;}
        let Some(index)=DOMAINS.iter().position(|value|*value==domain) else {continue};
        attempts[index]+=1.0;
        if row["accepted"]==true {accepted[index]+=1.0;}
        if row["accepted"]==true || row["candidateId"].is_string()
            || row["newFindings"].as_u64().unwrap_or(0)>0 {
            useful[index]+=1.0;
        }
        tokens[index]+=row["tokens"].as_u64().unwrap_or(0);
    }
    let mut best=(f64::NEG_INFINITY,eligible.first().copied().unwrap_or(DOMAINS[0]));
    let log_n=((history.len() as f64)+1.0).ln();
    for domain in eligible {
        let Some(index)=DOMAINS.iter().position(|value|*value==domain) else {continue};
        let tries=attempts[index];
        let posterior=(accepted[index]+0.5)/(tries+2.0);
        let utility=useful[index]/(tries+1.0);
        let cost=if tries==0.0 {0.0}else{(tokens[index] as f64/tries)/10_000.0};
        let barren=if tries==0.0 {0.0}else{1.0-useful[index]/tries};
        let penalty=barren*cost/(1.0+cost);
        let exploration=if tries==0.0 {3.0}
            else {(2.0*log_n/tries).sqrt().min(3.0)};
        let match_bonus=if objective_matches(domain,objective){2.0}else{0.0};
        let score=3.0*posterior/(1.0+cost)+0.25*utility
            +exploration-penalty+match_bonus;
        if score>best.0 {best=(score,domain);}
    }
    best.1.to_string()
}

pub(crate) fn select_domain(algorithm: &Value, history: &[Value], objective: &str) -> String {
    let implementation=algorithm["implementation"].as_str()
        .or_else(||algorithm["variant"].as_str()).unwrap_or("failure-clusters");
    if implementation!="expression" && implementation!="counterexample-first" {
        return fast_failure_cluster_domain(history,objective);
    }"#;
    Some(transformed.replacen(entry,fast,1))
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
    let old = "    if let (Some(asked), Some(stated)) = (fact_domain(query), fact_domain(text)) {\n        if asked == stated {\n            score += 32;\n        }\n    }";
    if source.matches(marker).count()!=1 || source.matches(old).count()!=1 {
        return None;
    }
    // Compose independently discovered source optimizations. The same
    // domain hoist works on both the plain token hoist and its descendant
    // with immutable fact-token caching; it never discards a prior change.
    let candidates = [
        (
            "fn query_relevance(query: &str, text: &str, query_tokens: &std::collections::BTreeSet<String>) -> usize {",
            "query_relevance(query, text, &query_tokens)",
            "fn query_relevance(query: &str, text: &str, query_tokens: &std::collections::BTreeSet<String>, asked_domain: Option<&str>) -> usize {",
            "query_relevance(query, text, &query_tokens, asked_domain)",
        ),
        (
            "fn query_relevance(query: &str, text: &str, query_tokens: &std::collections::BTreeSet<String>, cached_terms: Option<&Vec<Value>>) -> usize {",
            "query_relevance(query, text, &query_tokens, fact[\"factTokens\"].as_array())",
            "fn query_relevance(query: &str, text: &str, query_tokens: &std::collections::BTreeSet<String>, cached_terms: Option<&Vec<Value>>, asked_domain: Option<&str>) -> usize {",
            "query_relevance(query, text, &query_tokens, fact[\"factTokens\"].as_array(), asked_domain)",
        ),
    ];
    let (scorer, call, new_scorer, new_call) = candidates.into_iter()
        .find(|(scorer, call, _, _)| source.matches(scorer).count()==1
            && source.matches(call).count()==1)?;
    let mut output=source.replacen(
        marker,
        "    let query_tokens = memory::terms(query);\n    let asked_domain = fact_domain(query);\n    let mut ranked = facts\n",1
    );
    output=output.replacen(scorer,new_scorer,1);
    output=output.replacen(
        old,
        "    if let Some(asked) = asked_domain {\n        if fact_domain(text) == Some(asked) {\n            score += 32;\n        }\n    }",1
    );
    output=output.replacen(call,new_call,1);
    Some(output)
}

/// Respect an explicitly chosen source. When autonomous source selection is
/// enabled, the measured domain (rather than a stale search excerpt) selects
/// an executable induction target. The worker can still use its normal model
/// path if this known source shape no longer matches a mutation operator.
fn inductive_target_path(input: &Value) -> Option<&str> {
    let selected = input["rsiFocusFiles"].as_array()
        .filter(|files| files.len()==1)
        .and_then(|files|files[0].as_str());
    if input["rsiAutoFocus"] != true {
        return selected;
    }
    match input["rsiTargetDomain"].as_str() {
        Some("memory") => Some("src/engine/room_state.rs"),
        Some("improver") => Some("src/engine/rsi_evolve.rs"),
        _ => selected,
    }
}

/// One inductive change, isolated in the disposable git worktree. If the
/// model or inherited descendant already changed bytes, do not mix authors.
pub(crate) fn induce(
    state: &AppState, job_id: &str, input: &Value, source: &Path, isolated: &Path,
) -> Result<Option<Value>, String> {
    if input["rsiCampaignId"].as_str().is_none() { return Ok(None); }
    let Some(relative) = inductive_target_path(input).filter(|path|
        rsi::safe_path(path) && source.join(path).is_file() && isolated.join(path).is_file()
    ) else { return Ok(None); };
    if input["rsiFailedPatchRecovery"]["candidateId"].is_string() { return Ok(None); }
    let original = fs::read_to_string(source.join(relative)).map_err(|e| e.to_string())?;
    let existing = fs::read_to_string(isolated.join(relative)).map_err(|e| e.to_string())?;
    if original != existing { return Ok(None); }
    let (operator,transformed)=if let Some(next)=stream_search_policy_statistics(&existing) {
        ("streamed_improver_policy_statistics_v1",next)
    }else if let Some(next)=materialize_verified_owner_provenance(&existing) {
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
        // The compiled descendant may already contain these mutations. A
        // source-evolution regression test must survive actual adoption.
        let original=include_str!("room_state.rs");
        let first=materialize_verified_owner_provenance(original)
            .unwrap_or_else(||original.to_string());
        assert!(first.contains("\"verifiedLocalOwner\": true"));
        assert!(first.contains("fact[\"verifiedLocalOwner\"]==true || !fact[\"sourceMessageId\"]"));
        assert!(first.contains(".and_then(|id| state.store.message(id))"));
        assert!(materialize_verified_owner_provenance(&first).is_none());
        let second=hoist_invariant_query_terms(&first)
            .unwrap_or_else(||first.clone());
        assert!(second.contains("let query_tokens = memory::terms(query);\n    let mut ranked = facts"));
        let third=cache_invariant_fact_terms(&second)
            .unwrap_or_else(||second.clone());
        assert!(third.contains("\"factTokens\": fact_tokens"));
        let fourth=hoist_invariant_query_domain(&third)
            .unwrap_or_else(||third.clone());
        assert!(fourth.contains("let asked_domain = fact_domain(query);"));
        assert!(hoist_invariant_query_domain(&fourth).is_none());
    }

    #[test]
    fn current_engine_has_two_distinct_source_generations() {
        let original=include_str!("room_state.rs");
        let first=hoist_invariant_query_terms(original)
            .unwrap_or_else(||original.to_string());
        assert!(first.contains("let query_tokens = memory::terms(query);\n    let mut ranked = facts"));
        let second=hoist_invariant_query_domain(&first)
            .unwrap_or_else(||first.clone());
        assert!(second.contains("let asked_domain = fact_domain(query);"));
        assert!(hoist_invariant_query_terms(&second).is_none());
        assert!(hoist_invariant_query_domain(&second).is_none());
    }

    #[test]
    fn autonomous_induction_follows_measured_domain_but_respects_owner_focus() {
        let mut request = json!({"rsiAutoFocus":true,
            "rsiTargetDomain":"memory","rsiFocusFiles":["src/engine/other.rs"]});
        assert_eq!(inductive_target_path(&request),Some("src/engine/room_state.rs"));
        request["rsiTargetDomain"]=json!("improver");
        assert_eq!(inductive_target_path(&request),Some("src/engine/rsi_evolve.rs"));
        request["rsiTargetDomain"]=json!("tools");
        assert_eq!(inductive_target_path(&request),Some("src/engine/other.rs"));
        request["rsiAutoFocus"]=json!(false);
        assert_eq!(inductive_target_path(&request),Some("src/engine/other.rs"));
        request["rsiFocusFiles"]=json!([]);
        assert_eq!(inductive_target_path(&request),None);
    }

    #[test]
    fn meta_improver_evolves_streamed_policy_statistics_without_changing_rules() {
        let source = include_str!("rsi_evolve.rs");
        let candidate = stream_search_policy_statistics(source)
            .unwrap_or_else(||source.to_string());
        assert!(candidate.contains("let mut last_row: Option<&Value> = None"));
        assert!(candidate.contains("fn fast_failure_cluster_domain("));
        assert!(candidate.contains("return fast_failure_cluster_domain(history,objective);"));
        assert!(candidate.contains("implementation!=\"expression\""));
        assert!(candidate.contains("let mut attempts=[0.0_f64;6]"));
        assert!(candidate.contains("(row[\"accepted\"]==true) as u8 as f64"));
        assert!(candidate.contains("scores.first().map(|(domain,_)|*domain)"));
        assert!(candidate.contains("(\"exploration\".to_string(),exploration)"));
        assert!(stream_search_policy_statistics(&candidate).is_none());
    }

    #[test]
    fn recursive_fact_cache_preserves_legacy_fallback_and_original_ranking() {
        let source = include_str!("room_state.rs");
        let hoisted = hoist_invariant_query_terms(source)
            .unwrap_or_else(||source.to_string());
        let cached = cache_invariant_fact_terms(&hoisted)
            .unwrap_or_else(||hoisted.clone());
        assert!(cached.contains("\"factTokens\": fact_tokens"));
        assert!(cached.contains("memory::terms(&stored_text)"));
        assert!(cached.contains("fact[\"factTokens\"].as_array()"));
        assert!(cached.contains("let text_tokens = memory::terms(text);"));
        assert!(cached.contains("score + matches.min(12)"));
        assert_eq!(cached.matches("let query_tokens = memory::terms(query);").count(), 1);
        assert!(cache_invariant_fact_terms(&cached).is_none());
        let descendant = hoist_invariant_query_domain(&cached)
            .unwrap_or_else(||cached.clone());
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
