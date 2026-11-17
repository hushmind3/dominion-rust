//! Money intelligence extraction and provenance-labelled causal association graph.
//! No model calls: these operators enrich every market event and every result
//! before SQLite ingestion. Relations are hypotheses/lexical observations,
//! NEVER proof of a causal effect or a paying customer.
use serde_json::{json,Value};
use sha2::{Sha256,Digest};

const TOPICS:&[(&str,&[&str])]=&[
    ("ai",&["artificial intelligence","llm","agent","qwen","deepseek","ai ","ai-","인공지능","人工智能","大模型"]),
    ("hardware",&["chip","gpu","sensor","robot","battery","semiconductor","hardware","반도체","센서","로봇","배터리","芯片","机器人","传感器"]),
    ("manufacturing",&["factory","manufacturing","production","supply chain","공장","제조","생산","공급망","制造","工厂"]),
    ("software",&["open source","github","api","saas","software","오픈소스","소프트웨어","开源","软件"]),
    ("energy",&["electricity","solar","power grid","hydrogen","energy","전력","태양광","전기료","에너지","新能源","电力"]),
    ("logistics",&["shipping","delivery","port","freight","logistics","물류","배송","해운","운송","物流","运输"]),
    ("commerce",&["retail","shopping","ecommerce","export","import","소매","쇼핑","수출","수입","电商","进出口"]),
    ("finance",&["fintech","bank","payments","insurance","credit","금융","결제","보험","대출","银行","支付"]),
    ("health",&["medical","hospital","healthcare","diagnosis","의료","병원","건강","진단","医疗","医院"]),
    ("education",&["education","learning","training","tutoring","교육","학습","시험","教育","学习"]),
    ("construction",&["building","construction","architecture","housing","건설","건축","설비","建筑","施工"]),
    ("agriculture",&["food","agriculture","farming","crop","농업","식품","재배","农业","食品"])
];
const CHANGES:&[(&str,&[&str])]=&[
    // These are only lexical mentions, not proof a tender remains open.
    ("buyer_request",&["procurement notice","request for proposal",
        "request for quotation","request for tender","invitation to bid",
        "purchase order","tender announcement","rfp","rfq",
        "采购意向","采购公告","招标","招標","入札","調達","발주","입찰","견적요청","구매요청"]),
    ("cost_drop",&["cheaper","cost cuts","price cut","plung","discount","가격 하락","원가 절감","저렴","降价","降本"]),
    ("cost_rise",&["price hike","rising costs","inflation","expensive","가격 인상","비용 상승","涨价","成本上涨"]),
    ("shortage",&["shortage","supply crunch","backlog","bottleneck","품귀","부족","공급난","短缺","缺货"]),
    ("launch",&["launch","release","open source","new model","debut","출시","공개","발표","发布","开源"]),
    ("policy",&["regulation","ban ","tariff","lawsuit","compliance","관세","법안","규제","금지","监管","关税"]),
    ("demand",&["demand surges","orders rise","waitlist","demand growth","수요 급증","주문 증가","需求增长","订单上涨"])
];
fn digest(raw:&str)->String{format!("{:x}",Sha256::digest(raw.as_bytes()))}
fn bound(value:&Value,max:usize)->String{
    value.as_str().unwrap_or("").trim().chars().take(max).collect()
}
fn has(text:&str,words:&[&str])->bool{
    words.iter().any(|word|text.contains(word))
}
pub fn sector_node_id(text:&str)->String{node_id("sector",text)}
fn node_id(kind:&str,text:&str)->String{
    format!("{kind}:{}",digest(&text.trim().to_lowercase()))
}
fn relation(from_kind:&str,from:&str,to_kind:&str,to:&str,predicate:&str,
    verification:&str,source:&str)->Option<Value>{
    if from.trim().is_empty()||to.trim().is_empty(){return None;}
    let from=from.trim().chars().take(150).collect::<String>();
    let to=to.trim().chars().take(150).collect::<String>();
    let from_id=node_id(from_kind,&from);
    let to_id=node_id(to_kind,&to);
    let id=format!("edge:{}",digest(&format!("{from_id}:{predicate}:{to_id}:{source}")));
    Some(json!({"id":id,"from":from_id,"to":to_id,"fromLabel":from,
        "toLabel":to,"predicate":predicate,"verification":verification,
        "sourceId":source,"createdAt":crate::frontend_state::timestamp(crate::domain::now_ms())}))
}

/// Lexical observations are not trusted economic measurements. In particular,
/// a news mention spike must never be relabelled as real customer demand.
pub fn signal_facets(row:&Value)->Value{
    let text=format!("{} {} {}",bound(&row["title"],350),bound(&row["description"],700),
        bound(&row["articleExcerpt"],1100)).to_lowercase();
    let topics=TOPICS.iter().filter_map(|(tag,terms)|has(&text,terms).then_some(*tag))
        .take(8).collect::<Vec<_>>();
    let changes=CHANGES.iter().filter_map(|(tag,terms)|has(&text,terms).then_some(*tag))
        .take(4).collect::<Vec<_>>();
    json!({"topics":topics,"changeTags":changes,"classificationMethod":"lexical_observation",
        "demandValidated":false,"priceChangeValidated":false})
}
pub fn enrich_signal(mut row:Value)->Value{
    let facets=signal_facets(&row);
    row["topics"]=facets["topics"].clone();
    row["changeTags"]=facets["changeTags"].clone();
    row["classificationMethod"]=facets["classificationMethod"].clone();
    row
}
pub fn signal_edges(row:&Value)->Vec<Value>{
    let signal_id=bound(&row["id"],100);
    if signal_id.is_empty(){return vec![]}
    let mut result=Vec::new();
    for topic in row["topics"].as_array().into_iter().flatten().take(8){
        if let Some(tag)=topic.as_str(){
            if let Some(edge)=relation("source",&signal_id,"sector",tag,
                "mentions_sector","lexical_observation",&signal_id){
                result.push(edge);
            }
        }
    }
    for change in row["changeTags"].as_array().into_iter().flatten().take(4){
        if let Some(tag)=change.as_str(){
            if let Some(edge)=relation("source",&signal_id,"change",tag,
                "mentions_change","lexical_observation",&signal_id){
                result.push(edge);
            }
        }
    }
    result
}

/// Model-generated relations are explicitly unverified and reference the
/// candidate that produced them. Restricted predicates prevent arbitrary
/// relations from being silently treated as facts.
pub fn candidate_edges(candidate:&Value)->Vec<Value>{
    let id=bound(&candidate["id"],160);
    let mut out=Vec::new();
    if let Some(edge)=relation("product",&bound(&candidate["offer"],160),
        "customer",&bound(&candidate["customer"],160),"offered_to","model_hypothesis",&id){
        out.push(edge);
    }
    for link in candidate["links"].as_array().into_iter().flatten().take(20){
        let predicate=link["relation"].as_str().unwrap_or("");
        if !matches!(predicate,"enables"|"substitutes"|"sold_to"|"reduces_cost_of"
            |"distributed_via"|"requires"|"competes_with"|"addresses"|"powered_by"){
            continue;
        }
        let from=bound(&link["from"],150);
        let to=bound(&link["to"],150);
        if let Some(edge)=relation("concept",&from,"concept",&to,predicate,
            "model_hypothesis",&id){
            out.push(edge);
        }
    }
    out
}
/// A source event linking disparate sectors is an inquiry cue, not a verified
/// connection. A single article covering two domains can seed the next query.
pub fn cross_domain_cues(rows:&[Value],max:usize)->Vec<Value>{
    let mut cues=Vec::new();
    for item in rows{
        let tags=item["topics"].as_array().into_iter().flatten()
            .filter_map(Value::as_str).take(8).collect::<Vec<_>>();
        if tags.len()<2{continue;}
        for i in 0..tags.len(){
            for j in i+1..tags.len(){
                let (a,b)=(tags[i],tags[j]);
                if cues.iter().any(|c:&Value|c["first"]==a && c["second"]==b
                    ||c["first"]==b && c["second"]==a){continue;}
                cues.push(json!({"first":a,"second":b,
                    "sourceUrl":item["url"],"sourceId":item["id"],
                    "signal":item["title"],"status":"observed_co_mention_not_causality"}));
                if cues.len()>=max{return cues;}
            }
        }
    }
    cues
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn procurement_wording_is_lexical_not_paid_demand(){
        let sample=json!({"id":"buyer1","title":"2026년 의료시설 입찰 공고",
            "description":"구매요청 자료 공개"});
        let tagged=signal_facets(&sample);
        assert!(tagged["changeTags"].as_array().unwrap().contains(&json!("buyer_request")));
        assert_eq!(tagged["demandValidated"],false);
        assert_eq!(tagged["priceChangeValidated"],false);
    }
    #[test] fn bilingual_signal_detection_does_not_fake_demand(){
        let sample=json!({"id":"s1","title":"中国机器人芯片降价",
            "description":"한국 센서 로봇 원가 절감"});
        let result=enrich_signal(sample);
        assert!(result["topics"].as_array().unwrap().contains(&json!("hardware")));
        assert!(result["changeTags"].as_array().unwrap().contains(&json!("cost_drop")));
        assert!(result["priceChangeValidated"].is_null());
        assert!(signal_facets(&result)["demandValidated"]==false);
        assert!(!signal_edges(&result).is_empty());
    }
    #[test] fn only_allowlisted_model_edges_and_source_provenance(){
        let c=json!({"id":"candidate-1","offer":"센서 모듈","customer":"건설 현장",
            "links":[{"from":"센서","to":"계측기","relation":"substitutes"},
            {"from":"확인 안 된 것","to":"사실","relation":"proven_fact"}]});
        let edges=candidate_edges(&c);
        assert_eq!(edges.len(),2);
        assert!(edges.iter().all(|e|e["verification"]=="model_hypothesis"));
        assert_eq!(edges[1]["predicate"],"substitutes");
    }
    #[test] fn cross_sector_cues_are_unique_and_bounded(){
        let items=vec![enrich_signal(json!({"id":"s1",
            "url":"https://example.org/one","title":"AI robot sensors and hospital software"}))];
        assert!(!cross_domain_cues(&items,10).is_empty());
        assert_eq!(cross_domain_cues(&items,1).len(),1);
    }
}
