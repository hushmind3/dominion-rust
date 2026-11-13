//! A source-driven candidate generator inside DOMINION's native RSI worker.
//! Its output is not an improvement until the frozen paired evaluator accepts it.
use crate::{engine::rsi, state::AppState};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

#[cfg(test)]
#[path = "../../rsi_domain_accumulator.rs"]
mod accumulator;

fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// Replace the default selector's six feature maps and repeated domain scans
/// with one accumulator. Expression and counterexample policies still execute
/// their original interpreters; verified payoff and objective constraints stay.
fn stream_verified_domain_selection(source: &str) -> Option<String> {
    let entry="pub(crate) fn select_domain(algorithm: &Value, history: &[Value], objective: &str) -> String {";
    let features_start=source.find("fn search_features(")?;
    let score_start=source[features_start..].find("\nfn score_domain(")?+features_start+1;
    let score_end=source[score_start..].find("\npub(crate) fn candidate_domains(")?+score_start;
    let features_hash=digest(&source[features_start..score_start-1]);
    // This optimizer implements two specifically verified feature layouts.
    // A descendant may evolve a NEW scoring formula; do not silently bypass
    // that formula merely because some old snippets still appear in it.
    if !matches!(features_hash.as_str(),
        "fee020813338a7a5c3a5c6d4b6767a5854852b5beb7994c173480c60bff92efc"
        |"c50260d36d53cd2641cdf3c483ea14cc8616101a68cfed9c26830b89656da977")
        || digest(&source[score_start..score_end])
            !="a85223433037d93f97c213b8f41d7e9d65b7cf3366f698639dac7452cf5015eb" {
        return None;
    }
    if source.matches(entry).count()!=1 || source.contains("fn fast_failure_domain(")
        || !source.contains("let measured_payoff=if get(\"verifiedOutcomes\")>0.0")
        || !source.contains("get(\"gainPer10kTokens\").min(4.0)*0.30")
        || !source.contains("let mut best = (f64::NEG_INFINITY, candidate_domains(objective)[0]);") {
        return None;
    }
    let dispatch=format!("{entry}\n    let implementation=algorithm[\"implementation\"].as_str()\n        .or_else(||algorithm[\"variant\"].as_str()).unwrap_or(\"failure-clusters\");\n    if !matches!(implementation,\"expression\"|\"counterexample-first\") {{\n        return fast_failure_domain(history,&candidate_domains(objective),\n            |domain|objective_matches(domain,objective));\n    }}");
    let mut next=source.replacen(entry,&dispatch,1);
    next.push('\n');
    next.push_str(include_str!("../../rsi_domain_accumulator.rs"));
    Some(next)
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
/// Stream all measured domain statistics in one pass. This operator is derived
/// from the CURRENT Sub3 search_features implementation, including verified
/// holdout gains. The older Sub2 optimization drops those features and must
/// never overwrite Sub3's richer selection algorithm.
fn stream_verified_policy_statistics(source: &str) -> Option<String> {
    const START: &str =
        "    let rows = history\n        .iter()\n        .filter(|row| row[\"domain\"] == domain)\n        .collect::<Vec<_>>();";
    const END: &str = "    let gain_efficiency=if verified_tokens>0.0 {";
    const UNRESOLVED: &str = "let unresolved = if rows\n        .last()";
    let signature = "fn search_features(history: &[Value], domain: &str, objective: &str) -> BTreeMap<String, f64> {";
    if source.matches(signature).count() != 1 || source.matches(START).count() != 1
        || source.matches(UNRESOLVED).count() != 1 {
        return None;
    }
    let block_start = source.find(START)?;
    let block_end = block_start + source[block_start..].find(END)?;
    let prior = &source[block_start..block_end];
    for required in [
        "let accepted = rows.iter()",
        "let regressions = rows",
        "let inconclusive = rows",
        "let tokens = rows",
        "let useful = rows",
        "let verified=rows.iter()",
        "let verified_outcomes=verified.len()",
        "let holdout_gains=verified.iter()",
        "let verified_tokens=verified.iter()",
        "row[\"newFindings\"]",
        "row[\"evidenceVerified\"]",
    ] {
        if !prior.contains(required) {return None;}
    }
    let replacement = r#"    // Stream each domain's candidate, evidence and cost statistics in one pass.
    // This preserves measured holdout selection while eliminating temporary
    // row/verified Vec allocations and repeated scans of the same history.
    let mut attempts = 0.0_f64;
    let mut accepted = 0.0_f64;
    let mut regressions = 0.0_f64;
    let mut inconclusive = 0.0_f64;
    let mut tokens_total = 0u64;
    let mut useful = 0.0_f64;
    let mut verified_outcomes = 0.0_f64;
    let mut holdout_gains_total = 0u64;
    let mut verified_tokens_total = 0u64;
    let mut last_row: Option<&Value> = None;
    for row in history.iter().filter(|row|row["domain"]==domain) {
        attempts += 1.0;
        if row["accepted"]==true {accepted += 1.0;}
        if row["evaluation"]["verdict"]=="regressed" {regressions += 1.0;}
        if row["evaluation"]["verdict"]=="inconclusive" {inconclusive += 1.0;}
        tokens_total += row["tokens"].as_u64().unwrap_or(0);
        if row["accepted"]==true || row["newFindings"].as_u64().unwrap_or(0)>0 {
            useful += 1.0;
        }
        if row["evidenceVerified"]==true {
            verified_outcomes += 1.0;
            holdout_gains_total += row["verifiedHoldoutGains"].as_u64().unwrap_or(0).min(64);
            verified_tokens_total += row["tokens"].as_u64().unwrap_or(0).max(1000);
        }
        last_row=Some(row);
    }
    let tokens = tokens_total as f64;
    let holdout_gains = holdout_gains_total as f64;
    let verified_tokens = verified_tokens_total as f64;
"#;
    let mut result=String::with_capacity(source.len());
    result.push_str(&source[..block_start]);
    result.push_str(replacement);
    result.push_str(&source[block_end..]);
    result= result.replacen(UNRESOLVED, "let unresolved = if last_row",1);
    (result!=source).then_some(result)
}

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
    Some(transformed.replacen(old_tie,
        "let mut best=(f64::NEG_INFINITY,scores.first().map(|(domain,_)|*domain).unwrap_or(DOMAINS[0]));",1))
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
    // Do not append a native edit to model-written or inherited source. Only
    // create a new candidate when the *whole* disposable tree is pristine.
    let clean=std::process::Command::new("git").current_dir(isolated)
        .args(["status","--porcelain","--untracked-files=normal"])
        .output().map_err(|e|e.to_string())?;
    if !clean.status.success() || !clean.stdout.is_empty() {
        return Ok(None);
    }
    let original = fs::read_to_string(source.join(relative)).map_err(|e| e.to_string())?;
    let existing = fs::read_to_string(isolated.join(relative)).map_err(|e| e.to_string())?;
    if original != existing { return Ok(None); }
    // The exact same code transformation is deterministic. A rejected
    // source hash/operator pair must not loop forever as a new generation.
    // Keep each repository partition isolated from other users' histories.
    let repository=source.to_string_lossy().to_string();
    let before=digest(&existing);
    let trials=state.store.all("rsi_mutation");
    let measured=|operator: &str| trials.iter().any(|trial|
        trial["sourceDir"].as_str()==Some(repository.as_str())
            && trial["beforeSha256"].as_str()==Some(before.as_str())
            && trial["operator"].as_str()==Some(operator)
            && trial["candidateId"].as_str().and_then(|id|state.store.get("rsi_candidate",id))
                .is_some_and(|candidate|
                    candidate["origin"]=="native_inductive_source_mutation"
                    && candidate["nativeMutation"]["beforeSha256"]==before
                    && candidate["nativeMutation"]["afterSha256"]==trial["afterSha256"]
                    && candidate["nativeMutation"]["operator"]==operator
                    && crate::engine::rsi::replayed_pair_metrics(state,&candidate).is_some()));
    let Some((operator,transformed))=choose_transformation(&existing,measured) else {return Ok(None);};
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
    if state.store.put("rsi_mutation", &receipt).is_none() {
        fs::write(isolated.join(relative),existing).map_err(|e|e.to_string())?;
        return Err("RSI 소스 변형 영수증을 저장하지 못했습니다.".into());
    }
    state.store.audit("rsi.inductive_source_mutation", receipt.clone());
    Ok(Some(receipt))
}

/// A measured rejection of one applicable operator must not hide the other
/// source-derived alternatives. No model invocation or acceptance is implied.
fn choose_transformation(source: &str, measured: impl Fn(&str)->bool) -> Option<(&'static str,String)> {
    type Operator = (&'static str, fn(&str)->Option<String>);
    let operators: [Operator;7] = [
        ("streamed_verified_domain_selector_v3",stream_verified_domain_selection),
        ("streamed_verified_improver_statistics_v2",stream_verified_policy_statistics),
        ("streamed_improver_policy_statistics_v1",stream_search_policy_statistics),
        ("verified_owner_provenance_cache_v1",materialize_verified_owner_provenance),
        ("loop_invariant_term_hoist_v1",hoist_invariant_query_terms),
        ("immutable_owner_fact_token_cache_v1",cache_invariant_fact_terms),
        ("loop_invariant_domain_hoist_v2",hoist_invariant_query_domain),
    ];
    operators.into_iter().filter(|(name,_)| !measured(name))
        .find_map(|(name,transform)|transform(source).map(|next|(name,next)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_domain_selector_preserves_verified_payoff_objective_and_ties() {
        use crate::engine::rsi_evolve;
        let mut history=Vec::new();
        for n in 0..80 {
            history.push(json!({"domain":rsi_evolve::DOMAINS[n%6],
                "accepted":n%4==0,"tokens":(n*919)%45000,
                "evidenceVerified":n%3==0,"verifiedHoldoutGains":n%90,
                "evaluation":{"verdict":if n%7==0 {"regressed"}else{"inconclusive"}}}));
            for objective in ["","memory","기억 improver","tools context","algorithm"] {
                let candidates=rsi_evolve::candidate_domains(objective);
                let got=accumulator::fast_failure_domain(&history,&candidates,
                    |domain|rsi_evolve::objective_matches(domain,objective));
                let scores=rsi_evolve::scored_domains(&json!({"implementation":"failure-clusters"}),&history,objective).unwrap();
                let expected=scores.into_iter().fold((f64::NEG_INFINITY,candidates[0]),
                    |best,(domain,score)|if score>best.0 {(score,domain)}else{best}).1;
                assert_eq!(got,expected,"history={n} objective={objective}");
                assert_eq!(rsi_evolve::select_domain(
                    &json!({"implementation":"failure-clusters"}),&history,objective),expected,
                    "the compiled descendant must execute the same verified selector");
            }
        }
        let source=include_str!("../../rsi_evolve.rs");
        let evolved=stream_verified_domain_selection(source).unwrap_or_else(||source.to_string());
        assert!(evolved.contains("return fast_failure_domain(history"));
        assert!(evolved.contains("\"expression\"|\"counterexample-first\""));
        assert!(stream_verified_domain_selection(&evolved).is_none());
        let changed_formula=source.replace("get(\"objectiveMatch\") * 2.0)",
            "get(\"objectiveMatch\") * 2.0 + get(\"unresolved\"))");
        assert_ne!(changed_formula,source);
        assert!(stream_verified_domain_selection(&changed_formula).is_none(),
            "a genuinely evolved policy must not be replaced by an old default formula");
        let changed_features=source.replace("holdout_gains*10_000.0/verified_tokens",
            "holdout_gains*20_000.0/verified_tokens");
        assert!(stream_verified_domain_selection(&changed_features).is_none());
    }
    #[test]
    fn a_rejected_applicable_operator_does_not_starve_other_source_hypotheses() {
        let source=include_str!("../../room_state.rs");
        let first=choose_transformation(source, |_|false).unwrap();
        let second=choose_transformation(source, |name|name==first.0).unwrap();
        assert_ne!(first.0,second.0);
        assert_ne!(first.1,second.1);
        assert!(choose_transformation(source, |_|true).is_none());
    }
    #[test]
    fn current_sub3_improver_streaming_keeps_verified_payoff_features() {
        let source=include_str!("../../rsi_evolve.rs");
        let output=stream_verified_policy_statistics(source)
            .unwrap_or_else(||source.to_string());
        assert!(output.contains("gainPer10kTokens"));
        assert!(output.contains("verifiedHoldoutGains"));
        assert!(output.contains("holdoutGains"));
        assert!(output.contains("let unresolved = if last_row")
            || output.contains("let unresolved = if rows"));
        if output!=source {
            assert!(output.contains("let mut last_row: Option<&Value>"));
            assert!(stream_verified_policy_statistics(&output).is_none());
        }
    }
    #[test]
    fn verified_owner_origin_cache_keeps_legacy_validation() {
        // The compiled descendant may already contain these mutations. A
        // source-evolution regression test must survive actual adoption.
        let original=include_str!("../../room_state.rs");
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
        let original=include_str!("../../room_state.rs");
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
    fn meta_improver_evolves_verified_streamed_statistics_without_dropping_metrics() {
        let source = include_str!("../../rsi_evolve.rs");
        // The first adopted child already carries the streamed optimizer.
        // Descendant CI must verify the preserved behavior instead of
        // requiring the old, unoptimized source to still exist.
        let first_generation=stream_verified_policy_statistics(source);
        let candidate=first_generation.as_deref().unwrap_or(source);
        assert!(first_generation.is_some()
            || source.contains("let mut last_row: Option<&Value> = None"));
        assert!(candidate.contains("let mut last_row: Option<&Value> = None"));
        assert!(candidate.contains("row[\"evidenceVerified\"]==true"));
        assert!(candidate.contains("let holdout_gains = holdout_gains_total as f64;"));
        assert!(candidate.contains("(\"gainPer10kTokens\".to_string(), gain_efficiency)"));
        assert!(candidate.contains("let unresolved = if last_row"));
        assert!(stream_verified_policy_statistics(&candidate).is_none());
    }

    #[test]
    fn recursive_fact_cache_preserves_legacy_fallback_and_original_ranking() {
        let source = include_str!("../../room_state.rs");
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
