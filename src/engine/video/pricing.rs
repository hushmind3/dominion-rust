//! LTX V2 text-to-video pricing snapshot (official price table, 2026-11-14).
//! This is a preflight reference estimate, not a payment-provider invoice.
//! The 2026-11-13 close of 1 USD = 1,335 KRW is explicitly frozen and shown
//! to the owner. A newer rate/tariff requires a code-table revision.
use serde_json::{json,Value};

pub(super) const PRICE_VERSION:&str="ltx-2-5-text-2026-11-14";
pub(super) const FX_KRW_PER_USD:u64=1335;
pub(super) const FX_AS_OF:&str="2026-11-13";

fn cents_per_second(model:&str,quality:&str)->Result<u64,String>{
    match (model,quality) {
        ("ltx-2-5-fast","720p")=>Ok(9),
        ("ltx-2-5-fast","1080p")=>Ok(13),
        ("ltx-2-5-fast","1440p")=>Ok(19),
        ("ltx-2-5-fast","4k")=>Ok(30),
        ("ltx-2-5-pro","720p")=>Ok(12),
        ("ltx-2-5-pro","1080p")=>Ok(17),
        ("ltx-2-5-pro","1440p")=>Ok(25),
        ("ltx-2-5-pro","4k")=>Ok(39),
        _=>Err("LTX 영상 모델 또는 화질별 참조 요금을 찾지 못했습니다.".into())
    }
}
pub(super) fn quote_durations(model:&str,quality:&str,parts:&[u64])
    ->Result<Value,String>{
    if parts.is_empty()||parts.len()>6||parts.iter().any(|s| *s<6||*s>20||s%2!=0) {
        return Err("견적 산출에 사용할 영상 길이가 올바르지 않습니다.".into());
    }
    let cents=cents_per_second(model,quality)?;
    let mut total=0u64;
    let clips=parts.iter().enumerate().map(|(index,seconds)|{
        let billed_cents=seconds.saturating_mul(cents);
        let krw=billed_cents.saturating_mul(FX_KRW_PER_USD).div_ceil(100);
        total=total.saturating_add(krw);
        json!({"index":index,"seconds":seconds,"estimatedKrw":krw})
    }).collect::<Vec<_>>();
    Ok(json!({
        "engine":"ltx","model":model,"quality":quality,
        "priceVersion":PRICE_VERSION,
        "currency":"KRW","estimatedKrw":total,
        "paidCallCount":parts.len(),"clips":clips,
        "fxKrwPerUsd":FX_KRW_PER_USD,"fxAsOf":FX_AS_OF,
        "priceAsOf":"2026-11-14",
        "source":"https://docs.ltx.io/pricing/",
        "estimatedOnly":true,
        "includesTax":false,
        "actualBilledKrw":null
    }))
}
pub(super) fn quote(cfg:&Value,input:&Value)->Result<Value,String>{
    let model=input["ltxModel"].as_str().or(cfg["ltxModel"].as_str()).unwrap_or("ltx-2-5-fast");
    let quality=input["ltxQuality"].as_str().or(cfg["ltxQuality"].as_str()).unwrap_or("1080p");
    let aspect=input["aspectRatio"].as_str().or(cfg["aspectRatio"].as_str()).unwrap_or("9:16");
    super::ltx::resolution(aspect,quality)?;
    let total=input["duration"].as_u64().or(cfg["duration"].as_u64())
        .ok_or("견적에 영상 길이가 필요합니다.")?;
    let parts=if input["ltxFilm"]==true {
        let max=super::film::max_clip(model,quality,24)?;
        super::film::split_duration(total,max)?
    }else{
        if !super::ltx::valid_duration(model,quality,total){
            return Err("LTX 단일 영상의 길이를 확인하세요.".into());
        }
        vec![total]
    };
    quote_durations(model,quality,&parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pro_fast_quality_and_frozen_reference_fx_are_exact() {
        let pro=quote_durations("ltx-2-5-pro","1080p",&[10,6]).unwrap();
        assert_eq!(pro["estimatedKrw"],(170*1335+99)/100+(102*1335+99)/100);
        assert_eq!(pro["paidCallCount"],2);
        assert_eq!(pro["fxAsOf"],"2026-11-13");
        let fast=quote_durations("ltx-2-5-fast","720p",&[20]).unwrap();
        assert_eq!(fast["estimatedKrw"],(180*1335+99)/100);
        assert!(quote_durations("ltx-2-5-pro","unknown",&[6]).is_err());
    }
    #[test]
    fn quote_rejects_single_shot_mislabeled_as_film(){
        let film=json!({"duration":10,"ltxFilm":true,
            "ltxModel":"ltx-2-5-fast","ltxQuality":"1080p",
            "aspectRatio":"16:9"});
        assert!(quote(&json!({}),&film).is_err());
        let single=json!({"duration":8,"ltxModel":"ltx-2-5-fast",
            "ltxQuality":"1080p","aspectRatio":"16:9"});
        assert_eq!(quote(&json!({}),&single).unwrap()["paidCallCount"],1);
    }
}