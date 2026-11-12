//! Frozen independent local model-weight evaluator.
//!
//! The editable descendant is free to invent arbitrary LoRA optimizers and
//! improve all intelligence roles; it cannot rewrite this holdout assessor,
//! read frozen expected labels through the tool, or claim that weight training
//! alone improves general intelligence. This file is excluded by safe_path.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::{BTreeMap,BTreeSet},fs,path::Path};

fn label(text:&str)->bool{
    !text.is_empty()&&text.len()<=110
        && text.bytes().all(|b|b.is_ascii_alphanumeric()
            ||b==b'_'||b==b'-')
}
fn case_data(train:&Path,holdout:&Path)->Result<Vec<Value>,String>{
    if train==holdout{
        return Err("학습과 평가 데이터가 동일합니다.".into());
    }
    let source=fs::read_to_string(train).map_err(|e|e.to_string())?;
    let mut trained=BTreeSet::new();
    for (index,line) in source.lines().enumerate(){
        if index>=4000{return Err("학습 사례 수 초과".into());}
        let row:Value=serde_json::from_str(line)
            .map_err(|e|e.to_string())?;
        if row["split"]!="train"||row["approvedForTraining"]!=true{
            return Err("학습 승인되지 않은 예시".into());
        }
        let prompt=row["prompt"].as_str()
            .filter(|s|!s.is_empty()&&s.len()<=8192)
            .ok_or("학습 입력이 유효하지 않습니다.")?;
        trained.insert(prompt.to_owned());
    }
    let content=fs::read_to_string(holdout).map_err(|e|e.to_string())?;
    let mut seen=BTreeSet::new();
    let mut domains=BTreeSet::new();
    let mut cases=Vec::new();
    for line in content.lines(){
        if cases.len()>=64{return Err("동결 평가 사례 한도 초과".into());}
        let row:Value=serde_json::from_str(line)
            .map_err(|e|e.to_string())?;
        let id=row["id"].as_str().filter(|s|label(s))
            .ok_or("동결 평가 ID가 유효하지 않습니다.")?;
        let prompt=row["prompt"].as_str()
            .filter(|s|!s.is_empty()&&s.len()<=8192)
            .ok_or("동결 평가 입력 오류")?;
        let expected=row["expected"].as_str()
            .filter(|s|!s.is_empty()&&s.len()<=4096)
            .ok_or("동결 평가 정답 오류")?;
        let domain=row["domain"].as_str().filter(|s|label(s))
            .ok_or("동결 평가 영역 오류")?;
        if row["split"]!="holdout"||!seen.insert(id.to_string())
            ||trained.contains(prompt)||expected.is_empty(){
            return Err("학습·동결 평가 누출 또는 중복 사례".into());
        }
        domains.insert(domain.to_string());
        cases.push(row);
    }
    if cases.len()<4||domains.len()<2{
        return Err("독립 평가는 최소 4개 사례·2개 지능 영역이 필요합니다.".into());
    }
    Ok(cases)
}
/// Produce prompt-only examples. Hidden labels never leave the frozen oracle.
pub(crate) fn blind_prompts(train:&Path,holdout:&Path)->Result<Vec<Value>,String>{
    case_data(train,holdout).map(|cases|cases.into_iter().map(|row|
        json!({"id":row["id"],"prompt":row["prompt"]})).collect())
}
/// Only the immutable assessor computes successes, regressions, and adoption.
pub(crate) fn assess(train:&Path,holdout:&Path,output:&Value)
    ->Result<Value,String>{
    let cases=case_data(train,holdout)?;
    assess_cases(&cases,output)
}
fn assess_cases(cases:&[Value],output:&Value)->Result<Value,String>{
    if output["source"]!="separate_blind_base_vs_lora_inference_v1"{
        return Err("인증된 기준·후보 추론 쌍이 아닙니다.".into());
    }
    let measured=output["predictions"].as_array()
        .filter(|rows|rows.len()==cases.len())
        .ok_or("동결 평가 응답 수 불일치")?;
    let mut received=BTreeMap::new();
    for item in measured{
        let id=item["id"].as_str().ok_or("동결 응답 ID 오류")?;
        if !item["baseline"].is_string()
            ||!item["candidate"].is_string()
            ||received.insert(id.to_owned(),item).is_some(){
            return Err("동결 응답이 없거나 중복됐습니다.".into());
        }
    }
    let mut baseline=0u64;let mut candidate=0u64;
    let mut improved=0u64;let mut regressions=0u64;
    let mut domains=BTreeSet::new();
    let mut gained_domains=BTreeSet::new();
    for case in cases{
        let domain=case["domain"].as_str().unwrap_or("");
        let expected=case["expected"].as_str().unwrap_or("").trim();
        let Some(response)=received.get(case["id"].as_str().unwrap_or("")) else{
            return Err("동결 모델 응답이 누락됐습니다.".into());
        };
        let before=response["baseline"].as_str().unwrap_or("").trim()==expected;
        let after=response["candidate"].as_str().unwrap_or("").trim()==expected;
        domains.insert(domain.to_owned());
        if before{baseline+=1;}
        if after{candidate+=1;}
        if !before&&after {improved+=1;gained_domains.insert(domain.to_owned());}
        if before&&!after{regressions+=1;}
    }
    let eligible=improved>=2&&regressions==0
        &&gained_domains.len()>=2&&candidate>baseline;
    let mut receipt=json!({
        "source":"immutable_rust_blind_adapter_assessment_v1",
        "pairedCases":cases.len(),"domains":domains.len(),
        "baselineCorrect":baseline,"candidateCorrect":candidate,
        "improvedCases":improved,"regressions":regressions,
        "improvedDomains":gained_domains.len(),
        "eligible":eligible,
        "baselineWasReevaluated":true,
        "trainingOnlyGainExcluded":true,
        "claim":"frozen_local_holdout_not_unseen_world_generalization"
    });
    receipt["sha256"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&receipt).unwrap_or_default())));
    Ok(receipt)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn case(id:&str,domain:&str)->Value{
        json!({"id":id,"domain":domain,"split":"holdout",
            "prompt":format!("prompt-{id}"),"expected":"yes"})
    }
    #[test]
    fn only_independent_cross_domain_nonregressing_adapter_can_be_adopted(){
        let cases=vec![case("a","memory"),case("b","models"),
            case("c","memory"),case("d","models")];
        let valid=json!({"source":"separate_blind_base_vs_lora_inference_v1",
            "predictions":[{"id":"a","baseline":"no","candidate":"yes"},
                {"id":"b","baseline":"no","candidate":"yes"},
                {"id":"c","baseline":"yes","candidate":"yes"},
                {"id":"d","baseline":"yes","candidate":"yes"}]});
        assert_eq!(assess_cases(&cases,&valid).unwrap()["eligible"],true);
        let mut regress=valid.clone();
        regress["predictions"][3]["candidate"]=json!("no");
        assert_eq!(assess_cases(&cases,&regress).unwrap()["eligible"],false);
        regress=valid;
        regress["predictions"][2]["id"]=json!("a");
        assert!(assess_cases(&cases,&regress).is_err());
    }
    #[test]
    fn labels_are_not_transferred_with_blind_prompts(){
        let root=std::env::temp_dir().join(format!(
            "sub1-frozen-adapter-oracle-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let train=root.join("train.jsonl");
        let held=root.join("held.jsonl");
        fs::write(&train,json!({"prompt":"train-prompt","completion":"yes",
            "split":"train","approvedForTraining":true}).to_string()+"\n").unwrap();
        let rows=[case("a","memory"),case("b","models"),
            case("c","memory"),case("d","models")];
        fs::write(&held,rows.iter().map(Value::to_string)
            .collect::<Vec<_>>().join("\n")+"\n").unwrap();
        let prompts=blind_prompts(&train,&held).unwrap();
        assert!(prompts.iter().all(|row|row["expected"].is_null()
            &&row["prompt"].is_string()));
        fs::remove_dir_all(root).unwrap();
    }
}
