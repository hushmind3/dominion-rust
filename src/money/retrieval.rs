//! Indexed, token-free recall for the optional Money tool.
//! Reuses money.sqlite and the native Run engine; never infers demand or sales.
use rusqlite::{params,Connection,OptionalExtension};
use serde_json::Value;
use std::{collections::{HashMap,HashSet},path::Path};
fn err(e:impl std::fmt::Display)->String{e.to_string()}
const MAX_WINDOW:usize=120;
const ROLES:&[&str]=&[
    "buyer_request","shortage","cost_rise","demand",
    "cost_drop","launch","policy"
];
fn text(value:&Value,key:&str,max:usize)->String{
    value[key].as_str().unwrap_or("").chars().take(max).collect()
}
fn is_east_asian(c:char)->bool{
    matches!(c as u32,0xAC00..=0xD7AF|0x1100..=0x11FF|0x3130..=0x318F
        |0x3400..=0x9FFF|0x3040..=0x30FF)
}
/// FTS5 unicode61 alone cannot recall substrings inside Korean/CJK words.
/// Whitespace-separated syllable bigrams are only an index, not linguistic facts.
fn grams(text:&str)->String{
    let mut result=Vec::new();
    let mut segment=Vec::new();
    let mut flush=|segment:&mut Vec<char>,result:&mut Vec<String>|{
        for group in segment.windows(2).take(250){
            result.push(group.iter().collect::<String>());
        }
        segment.clear();
    };
    for c in text.to_lowercase().chars().take(1600){
        if is_east_asian(c){
            segment.push(c);
        }else if !segment.is_empty(){
            flush(&mut segment,&mut result);
        }
        if result.len()>=250{break;}
    }
    flush(&mut segment,&mut result);
    result.truncate(250);
    result.join(" ")
}
fn searchable(row:&Value)->(String,String,String){
    let title=text(row,"title",300);
    let body=format!("{} {} {}",
        text(row,"description",900),text(row,"articleExcerpt",1200),
        row["topics"].as_array().into_iter().flatten()
            .filter_map(Value::as_str).take(8).collect::<Vec<_>>().join(" "));
    let ngrams=grams(&format!("{title} {body}"));
    (title,body,ngrams)
}
pub(super) fn schema(conn:&Connection)->Result<(),String>{
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS money_index_ids(
            rowid INTEGER PRIMARY KEY, id TEXT NOT NULL UNIQUE
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS money_signal_fts USING
            fts5(title,body,ngrams,tokenize='unicode61');
        CREATE TABLE IF NOT EXISTS money_signal_facets(
            signal_id TEXT NOT NULL, facet TEXT NOT NULL,
            PRIMARY KEY(signal_id,facet)
        );
        CREATE INDEX IF NOT EXISTS money_facets_role ON money_signal_facets(facet,signal_id);
    ").map_err(err)
}
pub(super) fn index_signal(conn:&Connection,row:&Value)->Result<(),String>{
    let id=row["id"].as_str().filter(|v|!v.is_empty()).ok_or("signal ID missing")?;
    let created=conn.execute("INSERT OR IGNORE INTO money_index_ids(id) VALUES(?1)",params![id])
        .map_err(err)?;
    if created==0{return Ok(());}
    let rowid:i64=conn.query_row("SELECT rowid FROM money_index_ids WHERE id=?1",
        params![id],|r|r.get(0)).map_err(err)?;
    let (title,body,ngrams)=searchable(row);
    conn.execute("INSERT INTO money_signal_fts(rowid,title,body,ngrams) VALUES(?1,?2,?3,?4)",
        params![rowid,title,body,ngrams]).map_err(err)?;
    for (key,prefix) in [("topics","topic:"),("changeTags","role:")]{
        for v in row[key].as_array().into_iter().flatten()
            .filter_map(Value::as_str).take(12){
            conn.execute("INSERT OR IGNORE INTO money_signal_facets(signal_id,facet) VALUES(?1,?2)",
                params![id,format!("{prefix}{v}")]).map_err(err)?;
        }
    }
    Ok(())
}
/// Old Money installations are indexed on their first explicit search, not
/// on ordinary SDK startup. Small transactions bound peak memory and let
/// partial failures retry from the durable index IDs.
fn backfill(conn:&mut Connection)->Result<(),String>{
    loop{
        let items={
            let mut stmt=conn.prepare("SELECT s.record FROM money_signals s
                WHERE NOT EXISTS(SELECT 1 FROM money_index_ids i WHERE i.id=s.id)
                ORDER BY s.id LIMIT 256").map_err(err)?;
            let rows=stmt.query_map([],|r|r.get::<_,String>(0)).map_err(err)?;
            rows.map(|r|r.map_err(err).and_then(|s|
                serde_json::from_str::<Value>(&s).map_err(err))).collect::<Result<Vec<_>,_>>()?
        };
        if items.is_empty(){return Ok(());}
        let tx=conn.transaction().map_err(err)?;
        for row in &items{index_signal(&tx,row)?;}
        tx.commit().map_err(err)?;
    }
}
fn query_expression(text:&str)->String{
    let mut tokens=Vec::new();
    let mut word=String::new();
    let mut push=|word:&mut String,tokens:&mut Vec<String>|{
        if word.chars().count()>=3{
            let clean=word.chars().take(24).collect::<String>();
            if !tokens.contains(&clean){tokens.push(clean);}
        }
        word.clear();
    };
    for c in text.to_lowercase().chars().take(360){
        if c.is_ascii_alphanumeric(){word.push(c);}
        else {push(&mut word,&mut tokens);}
        if tokens.len()>=6{break;}
    }
    push(&mut word,&mut tokens);
    for gram in grams(text).split_whitespace().take(10){
        if !tokens.iter().any(|v|v==gram){tokens.push(gram.to_string());}
        if tokens.len()>=12{break;}
    }
    tokens.into_iter().take(12).map(|word|format!("\"{word}\""))
        .collect::<Vec<_>>().join(" OR ")
}
fn search_conn(conn:&Connection,query:&str,limit:usize)->Result<Vec<Value>,String>{
    let expression=query_expression(query);
    if expression.is_empty(){return Ok(vec![]);}
    let mut stmt=conn.prepare("SELECT s.record FROM money_signal_fts
        JOIN money_index_ids i ON i.rowid=money_signal_fts.rowid
        JOIN money_signals s ON s.id=i.id
        WHERE money_signal_fts MATCH ?1
        ORDER BY bm25(money_signal_fts,3.0,1.0,0.55),s.captured_ms DESC
        LIMIT ?2").map_err(err)?;
    let rows=stmt.query_map(params![expression,limit.min(40) as i64],
        |r|r.get::<_,String>(0)).map_err(err)?;
    rows.map(|r|r.map_err(err).and_then(|s|serde_json::from_str(&s).map_err(err))).collect()
}
pub fn search(root:&Path,query:&str,limit:usize)->Result<Vec<Value>,String>{
    let mut conn=super::open(root)?;
    backfill(&mut conn)?;
    search_conn(&conn,query,limit)
}
fn recent(conn:&Connection,limit:usize)->Result<Vec<Value>,String>{
    let mut stmt=conn.prepare("SELECT record FROM money_signals
        ORDER BY captured_ms DESC,id LIMIT ?1").map_err(err)?;
    let rows=stmt.query_map(params![limit as i64],|r|r.get::<_,String>(0)).map_err(err)?;
    rows.map(|r|r.map_err(err).and_then(|s|serde_json::from_str(&s).map_err(err))).collect()
}
fn role(conn:&Connection,role:&str,limit:usize)->Result<Vec<Value>,String>{
    let mut stmt=conn.prepare("SELECT s.record FROM money_signals s
        JOIN money_signal_facets f ON f.signal_id=s.id
        WHERE f.facet=?1 ORDER BY s.captured_ms DESC,s.id LIMIT ?2").map_err(err)?;
    let rows=stmt.query_map(params![format!("role:{role}"),limit as i64],
        |r|r.get::<_,String>(0)).map_err(err)?;
    rows.map(|r|r.map_err(err).and_then(|s|serde_json::from_str(&s).map_err(err))).collect()
}
fn host(row:&Value)->String{
    row["url"].as_str().unwrap_or("").split_once("://").map(|(_,r)|r)
        .unwrap_or("").split('/').next().unwrap_or("").trim_start_matches("www.").to_lowercase()
}
/// Cross-role archive retrieval, not a timeline predictor. Caps and
/// publisher diversity prevent recent/viral stories monopolizing inference.
pub fn discovery(root:&Path,limit:usize)->Result<Vec<Value>,String>{
    let mut conn=super::open(root)?;
    backfill(&mut conn)?;
    let recent=recent(&conn,40)?;
    let mut rankings=Vec::<Vec<Value>>::new();
    for kind in ROLES{rankings.push(role(&conn,kind,24)?);}
    // Recall old alternatives or buyer problems semantically related to
    // a new observation without injecting the entire historical corpus.
    let seeds=recent.iter().filter(|row|
        row["changeTags"].as_array().is_some_and(|tags|!tags.is_empty()))
        .take(3).map(|row|text(row,"title",180)).collect::<Vec<_>>();
    for seed in seeds{rankings.push(search_conn(&conn,&seed,18)?);}
    let mut selected=Vec::new();
    let mut seen=HashSet::new();
    let mut publishers=HashMap::<String,usize>::new();
    let max=limit.clamp(1,MAX_WINDOW);
    let mut add=|row:Value,selected:&mut Vec<Value>|{
        let Some(id)=row["id"].as_str()else{return;};
        if seen.contains(id){return;}
        let publisher=host(&row);
        if publishers.get(&publisher).copied().unwrap_or(0)>=16{return;}
        seen.insert(id.to_owned());
        *publishers.entry(publisher).or_default()+=1;
        selected.push(row);
    };
    // Sample all economic roles before filling with the newest material.
    for index in 0..24{
        for channel in &rankings{
            if let Some(row)=channel.get(index){add(row.clone(),&mut selected);}
            if selected.len()>=max{return Ok(selected);}
        }
    }
    for row in recent{
        add(row,&mut selected);
        if selected.len()>=max{break;}
    }
    Ok(selected)
}
#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    use uuid::Uuid;
    fn row(id:&str,title:&str,tags:&[&str],host:&str)->Value{
        json!({"id":id,"title":title,"url":format!("https://{host}/{id}"),
            "sourceName":host,"description":title,"topics":["manufacturing"],
            "changeTags":tags,"verification":"headline_observed_not_paid_demand"})
    }
    #[test] fn finds_old_economic_role_beyond_recent_window_without_model_tokens(){
        let dir=std::env::temp_dir().join(format!("dominion-money-index-{}",Uuid::new_v4()));
        super::super::save_signals(&dir,&[row("historic","병원 입찰 센서 구매요청",&["buyer_request"],"buyer.example")]).unwrap();
        for chunk in 0..5{
            let batch=(0..120).map(|i|row(&format!("recent-{chunk}-{i}"),
                "Unrelated software announcement",&["launch"],&format!("feed{chunk}.example")))
                .collect::<Vec<_>>();
            super::super::save_signals(&dir,&batch).unwrap();
        }
        let found=discovery(&dir,120).unwrap();
        assert!(found.iter().any(|v|v["id"]=="historic"));
        assert!(found.len()<=120);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test] fn korean_bigrams_find_partial_phrase_and_legacy_rows_backfill(){
        let dir=std::env::temp_dir().join(format!("dominion-money-legacy-{}",Uuid::new_v4()));
        let conn=super::super::open(&dir).unwrap();
        let legacy=row("old","반도체 센서 공급망 병목",&["shortage"],"archive.example");
        conn.execute("INSERT INTO money_signals(id,captured_ms,publisher,title,url,record)
            VALUES(?1,1,'archive','title','https://archive.example/old',?2)",
            params!["old",legacy.to_string()]).unwrap();
        drop(conn);
        let result=search(&dir,"센서 공급망",12).unwrap();
        assert!(result.iter().any(|r|r["id"]=="old"));
        assert!(search(&dir,"센서 공급망",12).unwrap().len()<=12);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
