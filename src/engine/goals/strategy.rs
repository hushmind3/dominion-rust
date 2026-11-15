//! Goal review admission is a policy module, not a provider-specific SDK.
//! The owner chooses the strategy. The Rust verifier always remains final.
use serde_json::{json,Value};

#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub(crate) enum ReviewPolicy {
    /// Quality-first default: independent reviewer on every completed turn.
    Always,
    /// Cost-sensitive option: do not pay for a model reviewer while the
    /// deterministic acceptance gate still has hard unmet criteria.
    AcceptanceReady,
}
impl ReviewPolicy {
    pub(crate) fn parse(value:Option<&Value>)->Result<Self,String>{
        match value {
            None=>Ok(Self::Always),
            Some(v) if v=="always"=>Ok(Self::Always),
            Some(v) if v=="acceptance_ready"=>Ok(Self::AcceptanceReady),
            _=>Err("reviewPolicy는 always 또는 acceptance_ready여야 합니다.".into()),
        }
    }
    pub(crate) fn as_str(self)->&'static str{
        match self {Self::Always=>"always",Self::AcceptanceReady=>"acceptance_ready"}
    }
    pub(crate) fn should_review(self, deterministic_gaps:&[String])->bool{
        matches!(self,Self::Always)||deterministic_gaps.is_empty()
    }
}

/// Public contract is shared by the goal API, both SDKs and the React goal
/// launch menu. The server stays authoritative if these clients are older.
pub fn catalog()->Value{
    json!({
        "defaultReviewPolicy":"always",
        "reviewPolicies":[
            {"id":"always","label":"매 단계 독립 검토","description":"검증자 모델이 모든 실행 단계를 살펴보고 부족한 점을 찾습니다."},
            {"id":"acceptance_ready","label":"검증 조건 충족 시 모델 검토","description":"Rust의 결정적 검증 조건이 미충족이면 모델 검토 호출을 건너뛰고 수정·재계획합니다. 최종 완료에는 독립 검토가 필수입니다."}
        ],
        "ownerSteeringPriority":true,
        "finalAcceptanceOwner":"dominion_rust",
        "sideEffectReplay":"human_review_required"
    })
}
