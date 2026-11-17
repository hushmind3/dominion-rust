//! Versioned source observations inside the optional Money SQLite corpus.
//! Headline agreement is a lead to investigate, never independent verification.
use rusqlite::{params,Connection,OptionalExtension};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::path::Path;

fn hash(raw:&str)->String{format!("{:x}",Sha256::digest(raw.as_bytes()))}
fn err(e:impl std::fmt::Display)->String{e.to_string()}
fn story_key(title:&str)->String{
    let normalized=title.to_lowercase().chars()
        .filter(|c|c.is_alphanumeric()).take(280).collect::<String>();
    if normalized.is_empty(){String::new()}else{hash(&normalized)}
}
pub fn prepare(row:&Value)->Value{
    let mut item=row.clone();
    // Capture timestamps and model-derived rankings must not invent a new
    // revision when the actual source text is unchanged.
    let original=json!({"title":row["title"],"description":row["description"],
        "articleExcerpt":row["articleExcerpt"],"feedOriginal":row["feedOriginal"],
        "feedOriginalSha256":row["feedOriginalSha256"],
        "publishedAt":row["publishedAt"],"sourceName":row["sourceName"],
        "contentStatus":row["contentStatus"]});
    item["revisionId"]=json!(hash(&original.to_string()));
    item["storyKey"]=json!(story_key(row["title"].as_str().unwrap_or("")));
    item
}
pub(super) fn schema(conn:&Connection)->Result<(),String>{
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS money_signal_revisions(
            source_id TEXT NOT NULL,
            revision_id TEXT NOT NULL,
            observed_ms INTEGER NOT NULL,
            record TEXT NOT NULL,
            PRIMARY KEY(source_id,revision_id)
        );
        CREATE INDEX IF NOT EXISTS money_revisions_recent
          ON money_signal_revisions(source_id,observed_ms DESC);
        CREATE TABLE IF NOT EXISTS money_signal_stories(
            source_id TEXT NOT NULL,
            revision_id TEXT NOT NULL,
            story_key TEXT NOT NULL,
            observed_ms INTEGER NOT NULL,
            PRIMARY KEY(source_id,revision_id)
        );
        CREATE INDEX IF NOT EXISTS money_stories_matching
          ON money_signal_stories(story_key,source_id);
    ").map_err(err)
}
pub(super) fn record(conn:&Connection,row:&Value,observed_ms:u64)->Result<(),String>{
    let id=row["id"].as_str().ok_or("source ID missing")?;
    let revision=row["revisionId"].as_str().ok_or("revision ID missing")?;
    let observed=observed_ms as i64;
    conn.execute("INSERT OR IGNORE INTO money_signal_revisions
        (source_id,revision_id,observed_ms,record) VALUES(?1,?2,?3,?4)",
        params![id,revision,observed,row.to_string()]).map_err(err)?;
    let story=row["storyKey"].as_str().unwrap_or("");
    if !story.is_empty(){
        conn.execute("INSERT OR IGNORE INTO money_signal_stories
            (source_id,revision_id,story_key,observed_ms) VALUES(?1,?2,?3,?4)",
            params![id,revision,story,observed]).map_err(err)?;
    }
    Ok(())
}
/// Deterministic equality against an actual saved field, not against the
/// linked article or a claimed contract. Unmatched quotations stay unverified.
pub fn citation_status(source:&Value,quote:&str)->&'static str{
    let quote=quote.trim();
    if quote.is_empty()||quote.chars().count()>600{return "missing_or_unbounded_quote";}
    if ["title","description","articleExcerpt"].iter().any(|key|
        source[*key].as_str().is_some_and(|text|text.contains(quote))){
        if source["contentStatus"]=="page_fetched"{
            "exact_saved_page_field_not_transaction"
        }else{"exact_saved_feed_field_not_primary_document"}
    }else{"quote_not_found_in_saved_source"}
}
/// Read-only lineage and equivalent-title observations. Legacy sources are
/// returned as a single unversioned snapshot; the lookup never rewrites data.
pub fn history(root:&Path,id:&str)->Result<Value,String>{
    if id.len()>180||id.is_empty(){return Err("invalid source identifier".into());}
    if !root.join("money.sqlite").is_file(){return Err("source not found".into());}
    let conn=Connection::open_with_flags(root.join("money.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(err)?;
    let raw:Option<String>=conn.query_row(
        "SELECT record FROM money_signals WHERE id=?1",params![id],|r|r.get(0)
    ).optional().map_err(err)?;
    let Some(raw)=raw else{return Err("source not found".into());};
    let latest:Value=serde_json::from_str(&raw).map_err(err)?;
    let mut stmt=conn.prepare("SELECT record FROM money_signal_revisions
        WHERE source_id=?1 ORDER BY observed_ms DESC,revision_id DESC LIMIT 16").map_err(err)?;
    let rows=stmt.query_map(params![id],|r|r.get::<_,String>(0)).map_err(err)?;
    let mut versions=rows.map(|r|r.map_err(err).and_then(|s|
        serde_json::from_str::<Value>(&s).map_err(err))).collect::<Result<Vec<_>,_>>()?;
    let legacy=versions.is_empty();
    if legacy{versions.push(prepare(&latest));}
    let mut related_stmt=conn.prepare("SELECT DISTINCT s.source_id,t.record
        FROM money_signal_stories me
        JOIN money_signal_stories s ON s.story_key=me.story_key
          AND s.source_id<>me.source_id
        JOIN money_signals t ON t.id=s.source_id
        WHERE me.source_id=?1
        ORDER BY s.observed_ms DESC LIMIT 12").map_err(err)?;
    let related=related_stmt.query_map(params![id],|r|r.get::<_,String>(1))
        .map_err(err)?
        .map(|r|r.map_err(err).and_then(|s|
            serde_json::from_str::<Value>(&s).map_err(err)))
        .collect::<Result<Vec<_>,_>>()?;
    Ok(json!({"sourceId":id,"current":latest,"revisions":versions,
        "legacyRevisionUnavailable":legacy,"relatedSameTitle":related,
        "relationStatus":"same_normalized_title_not_independent_confirmation",
        "commercialStatus":"no_purchase_or_price_verified_by_matching_headlines"}))
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test] fn revisions_ignore_collection_timestamp_but_keep_actual_content_change(){
    let a=json!({"id":"source","title":"Factory buyer","description":"First quote",
        "capturedAt":"2026-11-01"});
    let b=json!({"id":"source","title":"Factory buyer","description":"First quote",
        "capturedAt":"2026-11-02"});
    let c=json!({"id":"source","title":"Factory buyer","description":"Updated quote",
        "capturedAt":"2026-11-03"});
    assert_eq!(prepare(&a)["revisionId"],prepare(&b)["revisionId"]);
    assert_ne!(prepare(&a)["revisionId"],prepare(&c)["revisionId"]);
 }
 #[test] fn citations_match_saved_text_not_claimed_transactions(){
    let source=json!({"title":"Buyer seeks proposals",
        "description":"Indicative quote 4,000 KRW","contentStatus":"headline_observed"});
    assert_eq!(citation_status(&source,"Buyer seeks proposals"),
        "exact_saved_feed_field_not_primary_document");
    assert_eq!(citation_status(&source,"confirmed payment"),
        "quote_not_found_in_saved_source");
 }
}
