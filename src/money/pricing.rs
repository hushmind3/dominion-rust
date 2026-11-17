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
    let item:Value=serde_json::from_str(&raw).map_err(err)?;
    let status=super::provenance::citation_status(&item,quote);
    let version=super::provenance::prepare(&item);
    Ok(json!({"sourceId":source,"revisionId":version["revisionId"],
        "quote":quote.chars().take(400).collect::<String>(),
        "status":status,"tradeVerified":false}))
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
    let buyer_check=linked_quote(&tx,&candidate,&input["buyer"])?;
    let supplier_check=linked_quote(&tx,&candidate,&input["supplier"])?;
    let signature=format!("{id}:{}",input);
    let comparison_id=format!("price:{}",format!("{:x}",Sha256::digest(signature.as_bytes())));
    let mut record=evaluate(input);
    record["id"]=json!(comparison_id);
    record["candidateId"]=json!(id);
    record["buyer"]=input["buyer"].clone();
    record["supplier"]=input["supplier"].clone();
    record["costs"]=input["costs"].clone();
    record["fxAsOf"]=input["fxAsOf"].clone();
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
}
