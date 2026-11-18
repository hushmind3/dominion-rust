//! Pure, optional KRW-normalized price comparison for stored Money leads.
//! All inputs are owner-entered, never a verified quotation or transaction.
use rusqlite::{params,OptionalExtension};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::path::Path;
fn err(e:impl std::fmt::Display)->String{e.to_string()}
fn numeric(v:&Value,key:&str)->Option<f64>{
    v[key].as_f64().filter(|n|n.is_finite()&&*n>=0.0&&*n<=1e12)
}
fn text<'a>(v:&'a Value,key:&str)->&'a str{
    v[key].as_str().unwrap_or("").trim()
}
fn money(amount:f64,currency:&str,fx:Option<f64>)->Option<f64>{
    let rate=if currency=="KRW"{Some(1.0)}else{fx.filter(|v|v.is_finite()&&*v>0.0&&*v<=1e8)};
    let converted=amount*rate?;
    (converted.is_finite()&&converted<=1e16).then_some(converted)
}
fn round(v:f64)->f64{(v*100.0).round()/100.0}
/// Compare an owner-entered unit/total price against numeric tokens in
/// the *exact saved citation*, not a separate model-generated summary.
/// Quotation membership is not evidence of a transaction or market demand.
fn cited_amount_matches(quote:&str,amount:f64)->bool{
    if !amount.is_finite()||amount<0.0||quote.chars().count()>600{return false;}
    let mut token=String::new();
    let matches=|token:&str|{
        let clean=token.replace(',',"");
        clean.parse::<f64>().ok().is_some_and(|v|
            v.is_finite() && (v-amount).abs()<=0.000001_f64.max(amount*1e-10))
    };
    for ch in quote.chars(){
        if ch.is_ascii_digit()||ch==','||ch=='.' {
            token.push(ch);
        } else if !token.is_empty(){
            if matches(&token){return true;}
            token.clear();
        }
    }
    !token.is_empty()&&matches(&token)
}
fn source_host(value:&Value)->String{
    value["url"].as_str().unwrap_or("")
        .split_once("://").map(|(_,rest)|rest).unwrap_or("")
        .split(|ch:char|ch=='/'||ch=='?'||ch=='#').next().unwrap_or("")
        .trim_start_matches("www.").to_lowercase()
}
/// A complete, comparable assumption set yields a mathematical spread, not
/// proof that a real buyer or vendor offered/accepted either value.
pub fn evaluate(input:&Value)->Value{
    let buyer=&input["buyer"];
    let supplier=&input["supplier"];
    let costs=&input["costs"];
    let mut issues=Vec::<String>::new();
    let valid_kind=|v:&Value|matches!(text(v,"quoteKind"),
        "asking"|"indicative"|"claimed_transaction");
    let valid_currency=|v:&Value| {
        let curr=text(v,"currency");
        curr.len()==3&&curr.bytes().all(|b|b.is_ascii_uppercase())
    };
    let date=text(buyer,"asOf");
    let spec=text(buyer,"spec");
    let unit=text(buyer,"unit");
    if date.is_empty()||date.len()!=10||date.as_bytes().get(4)!=Some(&b'-')
        ||date.as_bytes().get(7)!=Some(&b'-')
        ||date!=text(supplier,"asOf"){
        issues.push("가격의 기준일이 없거나 양쪽 기준일이 다릅니다".into());
    }
    if spec.is_empty()||spec.chars().count()>140||
        !spec.eq_ignore_ascii_case(text(supplier,"spec")){
        issues.push("상품/서비스 규격이 다르거나 미확인입니다".into());
    }
    if unit.is_empty()||unit.chars().count()>40||unit!=text(supplier,"unit"){
        issues.push("수량 단위가 다르거나 미확인입니다".into());
    }
    if !valid_kind(buyer)||!valid_kind(supplier){
        issues.push("호가·희망가격·실거래 주장 종류를 지정해 주세요".into());
    }
    if !valid_currency(buyer)||!valid_currency(supplier){
        issues.push("유효한 3자리 통화 코드를 지정해 주세요".into());
    }
    let buyer_qty=numeric(buyer,"quantity").filter(|v|*v>0.0);
    let supplier_qty=numeric(supplier,"quantity").filter(|v|*v>0.0);
    if buyer_qty.is_none()||supplier_qty.is_none(){
        issues.push("양쪽 견적의 기준 수량이 필요합니다".into());
    }
    let moq=numeric(supplier,"minimumQuantity");
    if let (Some(q),Some(min))=(buyer_qty,moq){
        if q<min{issues.push("최소주문수량이 구매 수량보다 큽니다".into());}
    }
    let fx_date=text(input,"fxAsOf");
    let foreign=[buyer,supplier].iter().any(|v|text(v,"currency")!="KRW");
    if foreign&&(fx_date.is_empty()||fx_date!=date){
        issues.push("환율 시점과 양쪽 가격 기준일이 일치해야 합니다".into());
    }
    let fx_buyer=numeric(buyer,"krwPerUnit");
    let fx_supplier=numeric(supplier,"krwPerUnit");
    let buyer_amount=numeric(buyer,"amount");
    let supplier_amount=numeric(supplier,"amount");
    let sell=buyer_amount.and_then(|v|money(v,text(buyer,"currency"),fx_buyer));
    let buy=supplier_amount.and_then(|v|money(v,text(supplier,"currency"),fx_supplier));
    if sell.is_none()||buy.is_none(){
        issues.push("가격·외화 환율 중 값이 누락됐습니다".into());
    }
    // A missing cost is *unknown*, never silently zero.
    let fee_keys=["shippingKrw","feesKrw","taxKrw","laborKrw",
        "returnsKrw","otherKrw"];
    let fees=fee_keys.iter().map(|key|numeric(costs,key)).collect::<Vec<_>>();
    let missing=fee_keys.iter().zip(fees.iter()).filter_map(|(key,v)|
        v.is_none().then_some(*key)).collect::<Vec<_>>();
    if !missing.is_empty(){
        issues.push(format!("부대비용 미확인: {}",missing.join(", ")));
    }
    let mut gap=Value::Null;
    let mut sell_total=Value::Null;
    let mut procurement_total=Value::Null;
    if issues.is_empty(){
        if let (Some(s),Some(b),Some(bq),Some(sq))=
            (sell,buy,buyer_qty,supplier_qty){
            let resale=s/bq*bq;
            let procurement=b/sq*bq;
            let extra=fees.iter().flatten().sum::<f64>();
            let value=resale-procurement-extra;
            if value.is_finite()&&value.abs()<=1e17{
                gap=json!(round(value));
                sell_total=json!(round(resale));
                procurement_total=json!(round(procurement+extra));
            }else{issues.push("계산 결과가 허용된 수치 범위를 벗어났습니다".into());}
        }
    }
    json!({"status":if issues.is_empty(){
        "comparable_user_input_not_validated_trade"
    }else{"incomplete_or_incomparable"},
        "spreadKrw":gap,"buyerTotalKrw":sell_total,"landedCostKrw":procurement_total,
        "issues":issues,"buyerQuoteKind":text(buyer,"quoteKind"),
        "supplierQuoteKind":text(supplier,"quoteKind"),
        "tradeVerified":false,"fxRateVerified":false,
        "modelCalls":0,
        "explanation":"호가/희망가격/실거래 주장은 입력자의 분류일 뿐 입증된 계약이나 실제 마진이 아닙니다"})
}
fn linked_quote(conn:&rusqlite::Connection,candidate:&Value,side:&Value)->Result<Value,String>{
    let source=text(side,"sourceId");
    let quote=text(side,"quote");
    let requested_revision=text(side,"revisionId");
    if source.is_empty(){
        return Ok(json!({"status":"no_source_reference_user_assumption"}));
    }
    if !candidate["sourceIds"].as_array().into_iter().flatten()
        .any(|id|id.as_str()==Some(source)){
        return Ok(json!({"sourceId":source,
            "status":"source_not_in_original_candidate_evidence"}));
    }
    let raw:Option<String>=conn.query_row(
        "SELECT record FROM money_signals WHERE id=?1",params![source],|r|r.get(0))
        .optional().map_err(err)?;
    let Some(raw)=raw else{
        return Ok(json!({"sourceId":source,"status":"original_source_unavailable"}));
    };
    let current:Value=serde_json::from_str(&raw).map_err(err)?;
    let current_version=super::provenance::prepare(&current);
    let current_id=current_version["revisionId"].as_str().unwrap_or("");
    // A supplied revision is an exact historical proof request, not a hint:
    // never silently fall back to the current source when a pin is unavailable.
    if !requested_revision.is_empty() &&
        (requested_revision.len()!=64 ||
         !requested_revision.bytes().all(|byte|byte.is_ascii_hexdigit())){
        return Ok(json!({"sourceId":source,"requestedRevisionId":requested_revision,
            "currentRevisionId":current_id,"status":"invalid_source_revision_id",
            "tradeVerified":false}));
    }
    let mut selected=current;
    let mut revision_status="current_snapshot";
    if !requested_revision.is_empty(){
        if requested_revision!=current_id{
            let historical:Option<String>=conn.query_row(
                "SELECT record FROM money_signal_revisions
                 WHERE source_id=?1 AND revision_id=?2",
                params![source,requested_revision],|r|r.get(0)
            ).optional().map_err(err)?;
            let Some(historical)=historical else{
                return Ok(json!({"sourceId":source,
                    "requestedRevisionId":requested_revision,
                    "currentRevisionId":current_id,
                    "status":"source_revision_unavailable",
                    "tradeVerified":false}));
            };
            selected=serde_json::from_str(&historical).map_err(err)?;
            revision_status="historical_saved_revision";
        }else{
            revision_status="pinned_current_snapshot";
        }
    }
    let selected_version=super::provenance::prepare(&selected);
    let selected_id=selected_version["revisionId"].as_str().unwrap_or("");
    if !requested_revision.is_empty()&&selected_id!=requested_revision{
        return Ok(json!({"sourceId":source,
            "requestedRevisionId":requested_revision,
            "currentRevisionId":current_id,
            "status":"source_revision_integrity_mismatch",
            "tradeVerified":false}));
    }
    let status=super::provenance::citation_status(&selected,quote);
    let saved=matches!(status,
        "exact_saved_feed_field_not_primary_document"|
        "exact_saved_page_field_not_transaction");
    let amount_aligned=if saved{
        side["amount"].as_f64().filter(|a|a.is_finite()&&*a>=0.0)
            .map(|amount|cited_amount_matches(quote,amount))
    }else{None};
    Ok(json!({"sourceId":source,"revisionId":selected_id,
        "currentRevisionId":current_id,"revisionStatus":revision_status,
        "revisionChangedSinceQuote":selected_id!=current_id,
        "quote":quote.chars().take(600).collect::<String>(),
        "status":status,"sourceHost":source_host(&selected),
        "quotedAmountMatchesInput":amount_aligned,
        "tradeVerified":false}))
}
/// Same domain database and owner audit history; does not start any Run.
pub fn compare_and_store(root:&Path,id:&str,input:&Value)->Result<Value,String>{
    if id.is_empty()||id.len()>180{return Err("invalid candidate ID".into());}
    let mut conn=super::open(root)?;
    let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(err)?;
    let raw:Option<String>=tx.query_row(
        "SELECT record FROM money_candidates WHERE id=?1",params![id],
        |r|r.get(0)).optional().map_err(err)?;
    let Some(raw)=raw else{return Err("candidate not found".into());};
    let mut candidate:Value=serde_json::from_str(&raw).map_err(err)?;
    // Repeated owner input must return the *stored* original receipt. Otherwise
    // collection revisions change the apparent citation proof for the same ID.
    let signature=format!("{id}:{}",input);
    let comparison_id=format!("price:{}",format!("{:x}",Sha256::digest(signature.as_bytes())));
    let existing:Option<String>=tx.query_row(
        "SELECT record FROM money_price_comparisons WHERE id=?1",
        params![comparison_id],|r|r.get(0)).optional().map_err(err)?;
    if let Some(existing)=existing{
        return serde_json::from_str(&existing).map_err(err);
    }
    let buyer_check=linked_quote(&tx,&candidate,&input["buyer"])?;
    let supplier_check=linked_quote(&tx,&candidate,&input["supplier"])?;
    let mut record=evaluate(input);
    record["id"]=json!(comparison_id);
    record["candidateId"]=json!(id);
    record["buyer"]=input["buyer"].clone();
    record["supplier"]=input["supplier"].clone();
    record["costs"]=input["costs"].clone();
    record["fxAsOf"]=input["fxAsOf"].clone();
    // The prices may be mathematically comparable without any archived
    // quote backing them. Keep those two distinct evidence dimensions.
    let amount_support=[&buyer_check,&supplier_check];
    let supported=amount_support.iter().all(|v|
        v["quotedAmountMatchesInput"]==true);
    let any_mismatch=amount_support.iter().any(|v|
        v["quotedAmountMatchesInput"]==false);
    let buyer_host=buyer_check["sourceHost"].as_str().unwrap_or("");
    let supplier_host=supplier_check["sourceHost"].as_str().unwrap_or("");
    let independent=!buyer_host.is_empty()&&!supplier_host.is_empty()
        &&buyer_host!=supplier_host;
    record["priceEvidenceStatus"]=json!(if supported&&independent {
        "independent_saved_quotes_match_entered_amounts_not_verified_sale"
    }else if any_mismatch{
        "entered_price_disagrees_with_saved_quote"
    }else if supported {
        "saved_quotes_match_but_sources_not_independent"
    }else{
        "insufficient_independent_numeric_quote_evidence"
    });
    record["sourceHostIndependent"]=json!(independent);
    record["sourceChecks"]=json!({"buyer":buyer_check,"supplier":supplier_check});
    record["inputStatus"]=json!("user_supplied_not_primary_source_verified");
    record["createdAt"]=json!(
        crate::frontend_state::timestamp(crate::domain::now_ms()));
    tx.execute("INSERT OR IGNORE INTO money_price_comparisons
        (id,candidate_id,created_ms,record) VALUES(?1,?2,?3,?4)",
        params![comparison_id,id,crate::domain::now_ms() as i64,record.to_string()]).map_err(err)?;
    candidate["lastPriceComparisonId"]=json!(comparison_id);
    candidate["lastPriceComparisonStatus"]=record["status"].clone();
    tx.execute("UPDATE money_candidates SET record=?1,updated_ms=?2 WHERE id=?3",
        params![candidate.to_string(),crate::domain::now_ms() as i64,id]).map_err(err)?;
    tx.commit().map_err(err)?;
    Ok(record)
}
pub fn comparisons(root:&Path,id:&str)->Result<Vec<Value>,String>{
    if !root.join("money.sqlite").is_file(){return Ok(vec![]);}
    let conn=super::open(root)?;
    let mut stmt=conn.prepare("SELECT record FROM money_price_comparisons
        WHERE candidate_id=?1 ORDER BY created_ms DESC,id DESC LIMIT 12").map_err(err)?;
    let rows=stmt.query_map(params![id],|r|r.get::<_,String>(0)).map_err(err)?;
    rows.map(|r|r.map_err(err).and_then(|s|
        serde_json::from_str::<Value>(&s).map_err(err))).collect()
}
#[cfg(test)]
mod tests{
 use super::*;
 fn example()->Value{
    json!({"buyer":{"amount":120000.0,"quantity":10.0,"spec":"sensor-v2",
        "unit":"unit","asOf":"2026-11-18","currency":"KRW","quoteKind":"asking"},
        "supplier":{"amount":85000.0,"quantity":10.0,"spec":"sensor-v2",
        "unit":"unit","asOf":"2026-11-18","currency":"KRW","quoteKind":"indicative"},
        "costs":{"shippingKrw":5000,"feesKrw":2000,"taxKrw":0,
            "laborKrw":3000,"returnsKrw":1000,"otherKrw":0}})
 }
 #[test]fn persistent_candidate_comparisons_retain_citation_states(){
    let root=std::env::temp_dir().join(format!("dominion-prices-{}",uuid::Uuid::new_v4()));
    let buyer=json!({"id":"buyer-source","url":"https://buyer.example/request",
        "title":"Buyer wants 10 sensor units","description":"Budget estimate 120000 won",
        "sourceName":"Buyer","contentStatus":"headline_observed"});
    let supplier=json!({"id":"supplier-source","url":"https://supplier.example/quote",
        "title":"Supplier indicative sensor quote","description":"Estimate 85000 won",
        "sourceName":"Supplier","contentStatus":"headline_observed"});
    super::super::save_signals(&root,&[buyer,supplier]).unwrap();
    let candidate=json!({"id":"lead-1","title":"Sensor supply hypothesis",
        "status":"draft","sourceIds":["buyer-source","supplier-source"]});
    super::super::save_report(&root,&[candidate],
        &json!({"candidateIds":["lead-1"]})).unwrap();
    let mut input=example();
    input["buyer"]["sourceId"]=json!("buyer-source");
    input["buyer"]["quote"]=json!("Budget estimate 120000 won");
    input["supplier"]["sourceId"]=json!("supplier-source");
    input["supplier"]["quote"]=json!("Estimate 85000 won");
    let first=compare_and_store(&root,"lead-1",&input).unwrap();
    assert_eq!(first["sourceChecks"]["buyer"]["status"],
        "exact_saved_feed_field_not_primary_document");
    assert_eq!(first["sourceChecks"]["supplier"]["status"],
        "exact_saved_feed_field_not_primary_document");
    assert_eq!(first["sourceChecks"]["buyer"]["quotedAmountMatchesInput"],true);
    assert_eq!(first["sourceChecks"]["supplier"]["quotedAmountMatchesInput"],true);
    assert_eq!(first["sourceHostIndependent"],true);
    assert_eq!(first["priceEvidenceStatus"],
        "independent_saved_quotes_match_entered_amounts_not_verified_sale");
    assert_eq!(first["tradeVerified"],false);
    assert_eq!(compare_and_store(&root,"lead-1",&input).unwrap()["id"],first["id"]);
    assert_eq!(comparisons(&root,"lead-1").unwrap().len(),1);
    let saved=super::super::candidate(&root,"lead-1").unwrap().unwrap();
    assert_eq!(saved["lastPriceComparisonId"],first["id"]);
    std::fs::remove_dir_all(root).unwrap();
 }
 #[test]fn saved_quote_number_matching_refuses_false_price_equivalence(){
    assert!(cited_amount_matches("Price 120,000 KRW per pack",120000.0));
    assert!(cited_amount_matches("Unit price 4.75 USD",4.75));
    assert!(!cited_amount_matches("Price 120000 KRW",12000.0));
    assert!(!cited_amount_matches("Price 120,000 KRW",1200000.0));
    assert!(!cited_amount_matches("No numeric amount supplied",120000.0));
 }
 #[test]fn mismatched_quote_is_not_marked_as_supported_even_if_source_text_exists(){
    let root=std::env::temp_dir().join(format!(
        "dominion-source-price-mismatch-{}",uuid::Uuid::new_v4()));
    let buyer=json!({"id":"buyer-e","url":"https://buyer.example/request",
        "description":"Budget estimate 120000 won","sourceName":"Buyer",
        "contentStatus":"headline_observed"});
    let seller=json!({"id":"seller-e","url":"https://seller.example/quote",
        "description":"Supplier quote 85000 won","sourceName":"Seller",
        "contentStatus":"headline_observed"});
    super::super::save_signals(&root,&[buyer,seller]).unwrap();
    super::super::save_report(&root,&[json!({"id":"lead-mismatch",
        "status":"draft","sourceIds":["buyer-e","seller-e"]})],
        &json!({"candidateIds":["lead-mismatch"]})).unwrap();
    let mut input=example();
    input["buyer"]["sourceId"]=json!("buyer-e");
    input["buyer"]["quote"]=json!("Budget estimate 120000 won");
    input["buyer"]["amount"]=json!(1200000.0);
    input["supplier"]["sourceId"]=json!("seller-e");
    input["supplier"]["quote"]=json!("Supplier quote 85000 won");
    let result=compare_and_store(&root,"lead-mismatch",&input).unwrap();
    assert_eq!(result["sourceChecks"]["buyer"]["status"],
        "exact_saved_feed_field_not_primary_document");
    assert_eq!(result["sourceChecks"]["buyer"]["quotedAmountMatchesInput"],false);
    assert_eq!(result["sourceChecks"]["supplier"]["quotedAmountMatchesInput"],true);
    assert_eq!(result["priceEvidenceStatus"],"entered_price_disagrees_with_saved_quote");
    assert_eq!(result["tradeVerified"],false);
    std::fs::remove_dir_all(root).unwrap();
 }
 #[test]fn comparable_prices_account_for_every_declared_cost(){
    let p=evaluate(&example());
    assert_eq!(p["status"],"comparable_user_input_not_validated_trade");
    assert_eq!(p["spreadKrw"],24000.0);
    assert_eq!(p["landedCostKrw"],96000.0);
    assert_eq!(p["tradeVerified"],false);
 }
 #[test]fn missing_costs_or_spec_do_not_fabricate_margin(){
    let mut q=example();
    q["costs"]["shippingKrw"]=Value::Null;
    assert!(evaluate(&q)["spreadKrw"].is_null());
    q["costs"]["shippingKrw"]=json!(0);
    q["supplier"]["spec"]=json!("not the same sensor");
    assert!(evaluate(&q)["spreadKrw"].is_null());
 }
 #[test]fn fx_conversion_requires_explicit_same_date_rate(){
    let mut q=example();
    q["supplier"]["currency"]=json!("USD");
    q["supplier"]["amount"]=json!(50.0);
    assert!(evaluate(&q)["spreadKrw"].is_null());
    q["supplier"]["krwPerUnit"]=json!(1400.0);
    q["fxAsOf"]=json!("2026-11-18");
    assert_eq!(evaluate(&q)["spreadKrw"],39000.0);
 }
    #[test]fn pinned_historical_quotes_survive_refresh_without_rewriting_old_price_receipts(){
        let root=std::env::temp_dir().join(format!(
            "dominion-price-revisions-{}",uuid::Uuid::new_v4()));
        let source=json!({"id":"buyer-source","url":"https://buyer.example/request",
            "title":"Buyer requests sensors",
            "description":"Budget estimate 120000 won",
            "sourceName":"Buyer","contentStatus":"headline_observed"});
        super::super::save_signals(&root,&[source.clone()]).unwrap();
        let original_revision=super::super::provenance::prepare(&source)["revisionId"]
            .as_str().unwrap().to_owned();
        super::super::save_report(&root,&[json!({"id":"lead-revised",
            "status":"draft","sourceIds":["buyer-source"]})],
            &json!({"candidateIds":["lead-revised"]})).unwrap();
        let mut input=example();
        input["buyer"]["sourceId"]=json!("buyer-source");
        input["buyer"]["quote"]=json!("Budget estimate 120000 won");
        let first=compare_and_store(&root,"lead-revised",&input).unwrap();
        assert_eq!(first["sourceChecks"]["buyer"]["revisionId"],original_revision);
        let mut changed=source.clone();
        changed["description"]=json!("Budget estimate withdrawn");
        super::super::save_signals(&root,&[changed.clone()]).unwrap();
        let current_revision=super::super::provenance::prepare(&changed)["revisionId"]
            .as_str().unwrap().to_owned();
        assert_ne!(original_revision,current_revision);
        // Same input, same ID, same immutable receipt despite the new source.
        assert_eq!(compare_and_store(&root,"lead-revised",&input).unwrap(),first);
        let mut pinned=input.clone();
        pinned["buyer"]["revisionId"]=json!(original_revision);
        pinned["buyer"]["amount"]=json!(121000);
        let historical=compare_and_store(&root,"lead-revised",&pinned).unwrap();
        assert_eq!(historical["sourceChecks"]["buyer"]["status"],
            "exact_saved_feed_field_not_primary_document");
        assert_eq!(historical["sourceChecks"]["buyer"]["revisionStatus"],
            "historical_saved_revision");
        assert_eq!(historical["sourceChecks"]["buyer"]["revisionChangedSinceQuote"],true);
        assert_eq!(historical["sourceChecks"]["buyer"]["currentRevisionId"],current_revision);
        assert_eq!(historical["tradeVerified"],false);
        let mut bad_pin=pinned.clone();
        bad_pin["buyer"]["revisionId"]=json!("f".repeat(64));
        let unknown=compare_and_store(&root,"lead-revised",&bad_pin).unwrap();
        assert_eq!(unknown["sourceChecks"]["buyer"]["status"],
            "source_revision_unavailable");
        let mut unpinned=pinned;
        unpinned["buyer"].as_object_mut().unwrap().remove("revisionId");
        unpinned["buyer"]["amount"]=json!(122000);
        let latest=compare_and_store(&root,"lead-revised",&unpinned).unwrap();
        assert_eq!(latest["sourceChecks"]["buyer"]["status"],
            "quote_not_found_in_saved_source");
        assert_eq!(comparisons(&root,"lead-revised").unwrap().len(),4);
        std::fs::remove_dir_all(root).unwrap();
    }

}
