//! Bounded public feed collection ported from the Money branch. Fetch URLs
//! are operator-defined constants; no model/user-provided HTTP targets are read.
use serde_json::{json,Value};
use sha2::{Sha256,Digest};
use std::path::Path;
use std::time::Duration;
use tokio::sync::Mutex;
static COLLECT_LOCK:Mutex<()> = Mutex::const_new(());
const SOURCES:&[(&str,&str)] = &[
    ("HN new technology","https://hnrss.org/newest?points=25"),
    ("BBC business","https://feeds.bbci.co.uk/news/business/rss.xml"),
    ("TechCrunch","https://techcrunch.com/feed/"),
    ("MIT Technology Review","https://www.technologyreview.com/feed/"),
    ("South China Morning Post","https://www.scmp.com/rss/36/feed"),
    ("Yonhap economy","https://www.yna.co.kr/rss/economy.xml"),
    ("Yonhap industry","https://www.yna.co.kr/rss/industry.xml"),
    ("Yonhap market","https://www.yna.co.kr/rss/market.xml"),
    ("Bloomberg technology","https://feeds.bloomberg.com/technology/news.rss"),
    ("Reuters business via Google News","https://news.google.com/rss/search?q=when:24h+site:reuters.com/business&hl=en-US&gl=US&ceid=US:en"),
    ("Manufacturing supply change","https://news.google.com/rss/search?q=manufacturing+robotics+cost+price+when:7d&hl=en-US&gl=US&ceid=US:en"),
    ("Korea procurement and buyer signals","https://news.google.com/rss/search?q=%EA%B5%AC%EB%A7%A4+%EB%B0%9C%EC%A3%BC+%EC%9E%85%EC%B0%B0+when:7d&hl=ko&gl=KR&ceid=KR:ko"),
    ("Korean customer complaints and requests","https://news.google.com/rss/search?q=%EA%B3%A0%EA%B0%9D+%EB%B6%88%EB%A7%8C+%EA%B0%9C%EC%84%A0+%EC%9A%94%EC%B2%AD+when:7d&hl=ko&gl=KR&ceid=KR:ko"),
    ("Chinese buyer procurement intentions","https://news.google.com/rss/search?q=%E9%87%87%E8%B4%AD%E6%84%8F%E5%90%91+%E6%8B%9B%E6%A0%87+when:7d&hl=zh-CN&gl=CN&ceid=CN:zh-Hans"),
    ("Chinese manufacturing supply prices","https://news.google.com/rss/search?q=%E5%B7%A5%E5%8E%82+%E9%99%8D%E6%9C%AC+%E4%BE%9B%E5%BA%94%E9%93%BE+when:7d&hl=zh-CN&gl=CN&ceid=CN:zh-Hans"),
    ("Japanese buyer requests and tenders","https://news.google.com/rss/search?q=%E5%85%AC%E5%8B%9F+%E8%AA%BF%E9%81%94+%E5%85%A5%E6%9C%AD+when:7d&hl=ja&gl=JP&ceid=JP:ja"),
    ("EU public buyer tenders","https://news.google.com/rss/search?q=public+buyer+tender+RFP+when:7d&hl=en-GB&gl=GB&ceid=GB:en"),
    ("Global vendor and service requests","https://news.google.com/rss/search?q=%22request+for+proposals%22+%22service%22+when:7d&hl=en-US&gl=US&ceid=US:en"),
    ("Ask HN customer problems","https://hnrss.org/ask?points=10"),
    ("Show HN new supplier tools","https://hnrss.org/show?points=10"),
    ("Korea public tender notices","https://news.google.com/rss/search?q=%EC%9E%85%EC%B0%B0%EA%B3%B5%EA%B3%A0+when:3d&hl=ko&gl=KR&ceid=KR:ko"),
    ("Global RFQ and invitation to tender","https://news.google.com/rss/search?q=%22request+for+quotation%22+OR+%22invitation+to+tender%22+when:7d&hl=en-US&gl=US&ceid=US:en"),
    ("Contract awards and buyer decisions","https://news.google.com/rss/search?q=%22contract+award%22+OR+%22supplier+selected%22+when:7d&hl=en-US&gl=US&ceid=US:en"),
];
const MAX_ITEMS_PER_SOURCE:usize=18;
const SCRAPES_PER_CYCLE:usize=4;

fn digest(input:&str)->String{format!("{:x}",Sha256::digest(input.as_bytes()))}
fn clean_text(value:&str,max:usize)->String{
    let value=value.replace("<![CDATA[","").replace("]]>","")
        .replace("&amp;","&").replace("&quot;","\"")
        .replace("&apos;","'").replace("&lt;","<")
        .replace("&gt;",">").replace("&#39;","'");
    let mut out=String::new();
    let mut in_tag=false;
    for ch in value.chars(){
        match ch {
            '<'=>in_tag=true,
            '>'=>in_tag=false,
            c if !in_tag=>out.push(c),
            _=>{},
        }
        if out.chars().count()>max{break}
    }
    out.trim().chars().take(max).collect()
}
fn element<'a>(text:&'a str,tag:&str)->Option<&'a str>{
    let open=format!("<{tag}");
    let begin=text.find(&open)?;
    let after=&text[begin+open.len()..];
    if !after.starts_with(|c:char|c==' '||c=='\t'||c=='>'||c=='\n'){
        return None;
    }
    let gt=after.find('>')?;
    let close=format!("</{tag}>");
    let end=after[gt+1..].find(&close)?;
    Some(&after[gt+1..gt+1+end])
}
fn link(item:&str)->String{
    if let Some(link)=element(item,"link"){
        let text=clean_text(link,2000);
        if text.starts_with("https://")||text.starts_with("http://"){return text}
    }
    if let Some(start)=item.find("<link "){
        let tag=&item[start..item[start..].find('>').map(|n|start+n).unwrap_or(item.len())];
        for quote in ['"', '\'']{
            let attr=format!("href={quote}");
            if let Some(start)=tag.find(&attr){
                let rest=&tag[start+attr.len()..];
                if let Some(end)=rest.find(quote){return clean_text(&rest[..end],2000);}
            }
        }
    }
    String::new()
}
fn entries(feed:&str)->Vec<&str>{
    let mut output=Vec::new();
    let (begin,end)=if feed.contains("<item") {("<item","</item>")}else{("<entry","</entry>")};
    let mut rest=feed;
    while let Some(start)=rest.find(begin){
        let item=&rest[start..];
        let Some(end_idx)=item.find(end)else{break;};
        output.push(&item[..end_idx+end.len()]);
        if output.len()>=MAX_ITEMS_PER_SOURCE{break;}
        rest=&item[end_idx+end.len()..];
    }
    output
}
fn parse(feed:&str,source:&str)->Vec<Value>{
    let timestamp=crate::frontend_state::timestamp(crate::domain::now_ms());
    entries(feed).into_iter().filter_map(|item|{
        let title=element(item,"title").map(|x|clean_text(x,250)).unwrap_or_default();
        let url=link(item);
        if title.chars().count()<8||!(url.starts_with("https://")){
            return None;
        }
        let desc=element(item,"description").or_else(||element(item,"summary"))
            .map(|x|clean_text(x,500)).unwrap_or_default();
        let published=element(item,"pubDate").or_else(||element(item,"published"))
            .or_else(||element(item,"updated")).map(|x|clean_text(x,100)).unwrap_or_default();
        Some(super::intelligence::enrich_signal(json!({"id":digest(&url),"title":title,"url":url,"sourceName":source,
            "description":desc,"publishedAt":published,
            "capturedAt":timestamp,"contentStatus":"headline_observed",
            "articleExcerpt":"","verification":"unverified_market_observation"})))
    }).collect()
}

fn normalized_title(title:&str)->String{
    title.to_lowercase().chars().filter(|c|c.is_alphanumeric()).collect()
}

/// The same story arrives from several feeds with different URLs. Drop the
/// repeats before they reach the corpus so tokens are never spent on them.
fn dedupe_titles(signals:Vec<Value>,history:&[Value])->Vec<Value>{
    let mut seen=history.iter().filter_map(|s|s["title"].as_str())
        .map(normalized_title).collect::<std::collections::HashSet<_>>();
    signals.into_iter().filter(|s|s["title"].as_str()
        .map(|title|seen.insert(normalized_title(title))).unwrap_or(false)).collect()
}

fn needs_article_fetch(item:&Value,stored:Option<&Value>)->bool{
    if item["url"].as_str().is_none_or(|url|!url.starts_with("https://")&&!url.starts_with("http://")){
        return false;
    }
    // Headline-only records still need a real source page.
    !stored.is_some_and(|row|row["articleExcerpt"].as_str()
        .is_some_and(|text|text.chars().count()>100))
}

/// Scan three market feeds per cycle without model calls or expanding the
/// Rust agent tool surface. Persist original source URLs and extracted facets.
pub async fn collect(root:&Path,force:bool)->Result<Value,String>{
    let _lock=COLLECT_LOCK.lock().await;
    let bucket=crate::domain::now_ms()/1_800_000;
    if !force {
        let last=super::state(root,"scan")?;
        if last.as_ref().and_then(|x|x["bucket"].as_u64())==Some(bucket){
            return Ok(last.unwrap());
        }
    }
    let count=SOURCES.len();
    let start=(bucket as usize)%count;
    let selected=(0..3).map(|i|SOURCES[(start+i)%count]).collect::<Vec<_>>();
    let client=reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Dominion Money Evidence Collector/0.1")
        .build().map_err(|e|e.to_string())?;
    let mut all=Vec::new();
    let mut reports=Vec::new();
    for (name,url) in selected{
        // Only compiled-in HTTPS feeds can be requested; redirects are not
        // followed, so feed responses cannot redirect to internal networks.
        match client.get(url).send().await {
            Ok(mut response) if response.status().is_success()=>{
                let mut bytes=Vec::new();
                let mut over_limit=false;
                while let Some(chunk)=response.chunk().await.map_err(|e|e.to_string())?{
                    if bytes.len()+chunk.len()>1_000_000{over_limit=true;break}
                    bytes.extend_from_slice(&chunk);
                }
                if over_limit{
                    reports.push(json!({"name":name,"error":"feed larger than 1 MB"}));
                    continue;
                }
                let body=String::from_utf8_lossy(&bytes);
                let items=parse(&body,name);
                reports.push(json!({"name":name,"found":items.len()}));
                all.extend(items);
            },
            Ok(res)=>reports.push(json!({"name":name,"error":format!("HTTP {}",res.status())})),
            Err(err)=>reports.push(json!({"name":name,"error":err.to_string().chars().take(150).collect::<String>()}))
        }
    }
    let old=super::signals(root,180)?;
    let deduplicated=dedupe_titles(all,&old);
    let found=deduplicated.len();
    let added=super::save_signals(root,&deduplicated)?;
    let report=json!({"bucket":bucket,"found":found,"new":added,
        "sources":reports,"capturedAt":crate::frontend_state::timestamp(crate::domain::now_ms()),
        "modelCalls":0});
    // Failed networks must not be recorded as a successful scan interval.
    if reports.iter().any(|x|x["found"].is_number()){
        super::set_state(root,"scan",&report)?;
        Ok(report)
    }else{
        Err(format!("All public feeds failed: {}",json!(reports)))
    }
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]fn extracts_source_title_and_https_url(){
    let feed=r#"<rss><item><title>Buyer seeks factory automation</title><link>https://example.org/tender</link><description>Request for proposal</description></item></rss>"#;
    let found=parse(feed,"public");
    assert_eq!(found.len(),1);
    assert_eq!(found[0]["url"],"https://example.org/tender");
    assert_eq!(found[0]["changeTags"][0],"buyer_request");
 }
 #[test]fn source_rotation_includes_multiple_languages(){
    for needle in ["Chinese buyer","Japanese buyer","Korea procurement","Global RFQ"]{
        assert!(SOURCES.iter().any(|(name,_)|name.contains(needle)));
    }
 }
}
