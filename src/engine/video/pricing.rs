//! Frozen LTX-2.5 paid text-to-video quote and owner-approved spending cap.
//! Price source (2026-11-14 snapshot): https://docs.ltx.io/pricing
//! This is a LIST PRICE estimate, not actual spend; paid provider rates can
//! change without notice. Explicit KRW FX reference is not a live FX feed.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

const FX_REFERENCE_KRW_PER_USD:u64=1336; // 2026-11-13 reference, not live FX
const PRICE_DATE:&str="2026-11-14";
const FX_DATE:&str="2026-11-13";

fn unit_cents(model:&str,quality:&str)->Result<u64,String>{
    match (model,quality){
        ("ltx-2-5-fast","720p")=>Ok(9),
        ("ltx-2-5-fast","1080p")=>Ok(13),
        ("ltx-2-5-fast","1440p")=>Ok(19),
        ("ltx-2-5-fast","4k")=>Ok(30),
        ("ltx-2-5-pro","720p")=>Ok(12),
        ("ltx-2-5-pro","1080p")=>Ok(17),
        ("ltx-2-5-pro","1440p")=>Ok(25),
        ("ltx-2-5-pro","4k")=>Ok(39),
        _=>Err("LTX 모델·해상도의 공개 견적이 없습니다. 유료 작업을 등록하지 않습니다.".into())
    }
}

fn digest(model:&str,quality:&str,durations:&[u64],audio:bool)->String{
    let tuple=json!({"provider":"ltx","model":model,"quality":quality,
        "durations":durations,"audio":audio,"snapshot":PRICE_DATE});
    format!("{:x}",Sha256::digest(tuple.to_string().as_bytes()))
}

/// Price only the actual clips that will be submitted. Retakes must exclude
/// verified reused clips; source jobs remain immutable.
pub(super) fn quote(model:&str,quality:&str,durations:&[u64],audio:bool)->Result<Value,String>{
    if durations.is_empty()||durations.len()>6||durations.iter().any(|s|*s==0||*s>20){
        return Err("유료 생성 길이·장면 수가 유효하지 않습니다.".into())
    }
    let billed_seconds=durations.iter().try_fold(0u64,|acc,s|acc.checked_add(*s))
        .ok_or("유료 영상 길이 계산 초과")?;
    let price=unit_cents(model,quality)?;
    let usd_cents=price.checked_mul(billed_seconds).ok_or("견적 계산 범위 초과")?;
    let reference_krw=usd_cents.checked_mul(FX_REFERENCE_KRW_PER_USD)
        .and_then(|n|n.checked_add(99)).ok_or("원화 환산 범위 초과")?/100;
    Ok(json!({
        "engine":"ltx","model":model,"quality":quality,
        "billableSeconds":billed_seconds,"paidCalls":durations.len(),
        "clipDurations":durations,"unitUsdCentsPerSecond":price,
        "totalUsdCents":usd_cents,"referenceKrw":reference_krw,
        "fxKrwPerUsd":FX_REFERENCE_KRW_PER_USD,"fxAsOf":FX_DATE,
        "pricingAsOf":PRICE_DATE,"pricingUrl":"https://docs.ltx.io/pricing",
        "quoteDigest":digest(model,quality,durations,audio),
        "audio":audio,"estimate":true,"actualBilledUsdCents":null,
        "includesTaxes":false,"includesModelPlanningCalls":false
    }))
}

/// Client-supplied approval is bound to a server-computed quote digest and a
/// money ceiling. Merely saying "I accept N clips" is not enough.
pub(super) fn approved(body:&Value,quote:&Value)->Result<(),String>{
    if body["approvedPaidQuote"]!=true {
        return Err("유료 영상 견적과 원화 환산액을 확인하고 명시적으로 승인하세요.".into())
    }
    let max=body["maxUsdCents"].as_u64().ok_or("유료 생성 승인 금액 상한이 필요합니다.")?;
    if max<quote["totalUsdCents"].as_u64().unwrap_or(u64::MAX) {
        return Err("요금 견적이 승인된 금액을 초과했습니다. 새 견적을 확인하세요.".into())
    }
    if body["quoteDigest"].as_str()!=quote["quoteDigest"].as_str() {
        return Err("영상 모델·길이·해상도 또는 요금표가 변경됐습니다. 새 견적이 필요합니다.".into())
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn official_ltx_25_prices_and_krw_reference_are_integer_exact(){
        let q=quote("ltx-2-5-pro","1080p",&[10,6,6],true).unwrap();
        assert_eq!(q["paidCalls"],3);
        assert_eq!(q["billableSeconds"],22);
        assert_eq!(q["unitUsdCentsPerSecond"],17);
        assert_eq!(q["totalUsdCents"],374);
        assert_eq!(q["referenceKrw"],4997);
        assert!(quote("ltx-2-5-pro","invalid",&[6],true).is_err());
    }
    #[test]
    fn approving_requires_matching_digest_and_sufficient_ceiling(){
        let q=quote("ltx-2-5-fast","720p",&[6,8],false).unwrap();
        assert!(approved(&json!({"approvedPaidQuote":true,
            "maxUsdCents":q["totalUsdCents"],"quoteDigest":q["quoteDigest"]}),&q).is_ok());
        assert!(approved(&json!({"approvedPaidQuote":true,
            "maxUsdCents":125,"quoteDigest":q["quoteDigest"]}),&q).is_err());
        assert!(approved(&json!({"approvedPaidQuote":true,
            "maxUsdCents":9999,"quoteDigest":"stale"}),&q).is_err());
        assert!(approved(&json!({"maxUsdCents":9999,"quoteDigest":q["quoteDigest"]}),&q).is_err());
        assert_ne!(q["quoteDigest"],quote("ltx-2-5-fast","720p",&[6,8],true).unwrap()["quoteDigest"]);
        assert_ne!(q["quoteDigest"],quote("ltx-2-5-fast","720p",&[8,6],false).unwrap()["quoteDigest"]);
    }
}
