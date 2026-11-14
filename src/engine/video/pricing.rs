//! Bounded, deterministic pre-billing estimates for LTX 2.5 text-to-video.
//! This is a dated reference price snapshot, not a provider billing receipt.
//! If LTX changes its published tariff, update PRICE_REVISION and the rate
//! table together; old queued jobs then stop before any additional paid POST.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

const FX_REFERENCE_KRW:u64=1336; // 2026-11-13 reference, not a live exchange rate
const PRICE_REVISION:&str="ltx-2-5-t2v-2026-09-08-v1";
const PRICE_SOURCE:&str="https://docs.ltx.io/pricing";
const PRICE_ARCHIVE:&str="https://archive.vn/2026.09.08-074349/https%3A/docs.ltx.io/pricing";

pub(super) fn quote(render:&Value,seconds:u64,calls:u64)->Result<Value,String>{
    if !(6..=180).contains(&seconds)||!(1..=6).contains(&calls){
        return Err("유료 영상 길이 또는 생성 건수를 확인하세요.".into());
    }
    let model=render["model"].as_str().ok_or("동결 영상 모델이 없습니다.")?;
    let quality=render["quality"].as_str().ok_or("동결 영상 화질이 없습니다.")?;
    let cents:u64=match (model,quality){
        ("ltx-2-5-fast","720p")=>9,
        ("ltx-2-5-fast","1080p")=>13,
        ("ltx-2-5-fast","1440p")=>19,
        ("ltx-2-5-fast","4k")=>30,
        ("ltx-2-5-pro","720p")=>12,
        ("ltx-2-5-pro","1080p")=>17,
        ("ltx-2-5-pro","1440p")=>25,
        ("ltx-2-5-pro","4k")=>39,
        _=>return Err("영상 가격표에 없는 모델·화질입니다. 유료 생성 요청을 중단합니다.".into()),
    };
    let usd_cents=cents.checked_mul(seconds).ok_or("USD 가격 계산 초과")?;
    // The runtime reserves a rounded-up KRW amount BEFORE EACH remote POST.
    // A multi-shot quote must sum precisely those per-clip reservations;
    // otherwise the last paid clip could exceed an owner's exact approved cap.
    let durations=if calls>1 {
        let clips=render["film"]["clips"].as_array()
            .filter(|clips|clips.len()==calls as usize)
            .ok_or("다중 장면 원화 견적에는 동결된 장면 길이가 필요합니다.")?;
        clips.iter().map(|clip|clip["duration"].as_u64()
            .ok_or_else(||"동결 장면 길이를 찾을 수 없습니다.".to_string()))
            .collect::<Result<Vec<_>,_>>()?
    }else{vec![seconds]};
    if durations.iter().try_fold(0u64,|a,n|a.checked_add(*n))!=Some(seconds) {
        return Err("장면 길이 합계와 영상 견적 길이가 다릅니다.".into());
    }
    let mut krw=0u64;
    for duration in durations {
        let clip_cents=cents.checked_mul(duration).ok_or("클립 가격 계산 초과")?;
        let clip_krw=clip_cents.checked_mul(FX_REFERENCE_KRW)
            .and_then(|v|v.checked_add(99)).ok_or("원화 클립 가격 계산 초과")?/100;
        krw=krw.checked_add(clip_krw).ok_or("원화 총액 계산 초과")?;
    }
    let fingerprint=format!("{:x}",Sha256::digest(format!(
        "{PRICE_REVISION}|{model}|{quality}|{seconds}|{calls}|{usd_cents}|{krw}"
    ).as_bytes()));
    Ok(json!({
        "provider":"LTX","model":model,"quality":quality,
        "seconds":seconds,"paidCalls":calls,
        "usdCents":usd_cents,"usdCentsPerSecond":cents,
        "referenceKrw":krw,"referenceFxKrwPerUsd":FX_REFERENCE_KRW,
        "fxReferenceDate":"2026-11-13","pricingAsOf":"2026-09-08",
        "pricingVersion":PRICE_REVISION,"pricingUrl":PRICE_SOURCE,
        "priceArchiveUrl":PRICE_ARCHIVE,"fingerprint":fingerprint,
        "estimated":true,"actualBilledKrw":null,"includesTaxes":false
    }))
}
pub(super) fn approved(quote:&Value,body:&Value)->Result<u64,String>{
    let amount=body["maxApprovedKrw"].as_u64()
        .ok_or("원화 승인 한도가 없습니다. 최신 견적 확인이 필요합니다.")?;
    if body["approvedPaidQuote"]!=true
        ||body["approvedQuoteFingerprint"]!=quote["fingerprint"]
        ||amount<quote["referenceKrw"].as_u64().unwrap_or(u64::MAX){
        return Err("LTX 견적이 변경됐거나 원화 승인 한도가 부족합니다. 최신 견적을 확인하고 다시 승인하세요.".into());
    }
    Ok(amount)
}
pub(super) fn price_version_matches(quote:&Value)->bool{
    quote["pricingVersion"]==PRICE_REVISION
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn estimates_locked_model_duration_and_approved_krw(){
        let render=json!({"model":"ltx-2-5-pro","quality":"1080p"});
        let film=json!({"model":"ltx-2-5-pro","quality":"1080p",
            "film":{"clips":[{"duration":8},{"duration":8}]}});
        let q=quote(&film,16,2).unwrap();
        assert_eq!(q["usdCents"],272);
        assert_eq!(q["referenceKrw"],3634);
        let actual_segments=json!({"model":"ltx-2-5-pro","quality":"1080p",
            "film":{"clips":[{"duration":10},{"duration":6}]}});
        // 10s and 6s reservations each round up: 3635 KRW, not 3634.
        let segmented=quote(&actual_segments,16,2).unwrap();
        assert_eq!(segmented["referenceKrw"],3635);
        let accepted=json!({"approvedPaidQuote":true,
            "approvedQuoteFingerprint":q["fingerprint"],"maxApprovedKrw":3634});
        assert_eq!(approved(&q,&accepted).unwrap(),3634);
        assert!(approved(&q,&json!({"approvedPaidQuote":true,
            "approvedQuoteFingerprint":q["fingerprint"],"maxApprovedKrw":3633})).is_err());
        let changed=json!({"model":"ltx-2-5-pro","quality":"1080p",
            "film":{"clips":[{"duration":10},{"duration":8}]}});
        assert!(approved(&quote(&changed,18,2).unwrap(),&accepted).is_err());
        assert!(quote(&render,16,2).is_err());
        assert!(quote(&json!({"model":"unknown","quality":"1080p"}),8,1).is_err());
    }
}
