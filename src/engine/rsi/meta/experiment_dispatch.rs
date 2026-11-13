//! Evolvable selection of a concrete experiment execution scope.
//! This is authored and compiled by the descendant, but the independent
//! worker and supervisor verify its sealed source/experiment identities.
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Dispatch {
    source:String,
    generator_commit:String,
    protocol_revision:String,
    operator_id:String,
    experiment:String,
    target_path:String,
    allowed_paths:Option<Vec<String>>,
    needs_method_disabled_control:bool,
    contrast_status:String,
    revision:String,
}
fn hash(v:&Value)->String {
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn sha(value:&str,size:usize)->bool {
    value.len()==size && value.bytes().all(|b|b.is_ascii_hexdigit())
}
/// Called by the CURRENT compiled intelligence. No model or supervisor state
/// is accessed here; changing this algorithm is itself a source improvement.
pub(crate) fn dispatch(plan:&Value)->Result<Value,String> {
    let compiled=&plan["metaEvolutionProgram"];
    if compiled.is_null(){return Ok(Value::Null);}
    if compiled["source"]!="compiled_descendant_recursive_evolution_protocol_v1" {
        return Err("실행 정책의 채택된 메타 프로그램 출처 없음".into());
    }
    let kind=compiled["operator"]["experiment"].as_str().ok_or("실험 종류 없음")?;
    let target=compiled["target"]["path"].as_str()
        .filter(|p|crate::engine::rsi::safe_path(p))
        .ok_or("실험 소스 대상 없음")?;
    let focus=plan["focusFiles"].as_array().ok_or("실험 목표 목록 없음")?;
    if !focus.iter().any(|p|p.as_str()==Some(target)) {
        return Err("실험 변경 대상이 실제 작업 계획에서 제외됐습니다.".into());
    }
    let (allowed,contrast,control):(Option<Vec<String>>,&str,bool)=match kind {
        "paired_source"=>(None,"normal_independent_frozen_source_pair",false),
        "isolated_source"=>(Some(vec![target.into()]),
            "one_source_role_with_frozen_pair",false),
        "meta_policy_rewrite"=>{
            if plan["domain"]!="improver"
                || !(target.starts_with("src/engine/rsi/meta/")
                    || target=="src/engine/rsi_principle_learning.rs"
                    || target=="src/engine/rsi_program_graph.rs") {
                return Err("메타 정책 교정은 실제 개선 알고리즘 소스에만 적용됩니다.".into());
            }
            (Some(vec![target.into()]),"isolated_meta_policy_source_revision",false)
        },
        "method_control"=>{
            if plan["methodTransfer"]["methodId"].as_str().is_none()
                || plan["methodTransfer"]["domain"]!=plan["domain"] {
                return Err("개선법 대조 실험은 채택된 실행 절차와 동일 대상 분야가 필요합니다.".into());
            }
            (Some(vec![target.into()]),"paired_real_method_enabled_disabled_workers",true)
        },
        "factorial_source"=>{
            // A single factor is isolated here; source adoption alone does not
            // imply that A/B/A+B factorial comparisons have all run.
            (Some(vec![target.into()]),"isolated_factor_pending_independent_interaction",false)
        },
        "cross_component"=>{
            let mut selected=vec![target.to_string()];
            for row in plan["coDesignPortfolio"]["focus"].as_array().into_iter().flatten() {
                if let Some(path)=row["path"].as_str().filter(|p|
                    crate::engine::rsi::safe_path(p)
                    && focus.iter().any(|f|f.as_str()==Some(p))) {
                    if !selected.iter().any(|p|p==path){selected.push(path.to_owned());}
                }
                if selected.len()>=8 {break;}
            }
            if selected.len()<2 {
                return Err("교차 구성요소 실험에는 실제 소스 파일 2개 이상의 연결이 필요합니다.".into());
            }
            (Some(selected),"joint_source_observation_no_isolated_causal_credit",false)
        },
        _=>return Err("정의되지 않은 메타 실험 전략".into())
    };
    let revision=compiled["revision"].as_str().filter(|r|sha(r,64))
        .ok_or("선택된 메타 프로그램 해시 없음")?;
    let operator=compiled["operatorId"].as_str().filter(|id|!id.is_empty())
        .ok_or("선택된 메타 연산자 없음")?;
    let mut outcome=Dispatch{
        source:"compiled_meta_experiment_dispatch_v1".into(),
        generator_commit:plan["generatorCommit"].as_str()
            .ok_or("실험 실행 원본 버전 없음")?.into(),
        protocol_revision:revision.into(),operator_id:operator.into(),
        experiment:kind.into(),target_path:target.into(),allowed_paths:allowed,
        needs_method_disabled_control:control,contrast_status:contrast.into(),
        revision:String::new()
    };
    let mut data=serde_json::to_value(&outcome).map_err(|e|e.to_string())?;
    if let Some(obj)=data.as_object_mut(){obj.remove("revision");}
    outcome.revision=hash(&data);
    serde_json::to_value(outcome).map_err(|e|e.to_string())
}
