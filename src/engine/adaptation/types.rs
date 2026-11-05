//! Strict contracts shared by the execution, learning and adaptation paths.
//!
//! These are intentionally additive to the existing JSON records. They give
//! the new paths typed boundaries without replacing the React-facing records.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOutcome {
    Succeeded,
    Failed,
    Unknown,
    Cancelled,
    AnsweredUnverified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compliance {
    Followed,
    Violated,
    NotTriggered,
    Unknown,
    NotApplicable,
}

#[allow(dead_code)] // contract enum; consumed by the S2 verification adapter
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckVerdict {
    Pass,
    Fail,
    Unknown,
    NotApplicable,
}

#[allow(dead_code)] // contract enum; consumed by the S2/S3 failure classifier
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    InvalidArguments,
    MissingPrerequisite,
    ToolFailure,
    WrongArtifact,
    MissingEvidence,
    ModelNoncompliance,
    SemanticUncertain,
    PermissionDenied,
    EnvironmentUnavailable,
    OutcomeUnknown,
    BudgetExhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationRegime {
    ProductionObserved,
    SandboxControl,
    SandboxTreatment,
    Canary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelProvenance {
    DeterministicOracle,
    OwnerAdjudication,
    SemanticJudge,
    Unlabeled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Message,
    Operation,
    Artifact,
    Verification,
    OwnerCorrection,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceRef {
    pub kind: EvidenceKind,
    pub id: String,
    #[serde(default)]
    pub revision: Option<String>,
    #[serde(default)]
    pub scope: Value,
    #[serde(default)]
    pub observed_at: Option<String>,
    #[serde(default)]
    pub digest: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskContract {
    pub task_id: String,
    pub revision: String,
    pub source: String,
    pub scope: Value,
    pub intent: String,
    pub family: String,
    #[serde(default)]
    pub mandatory: Vec<Value>,
    #[serde(default)]
    pub bindings: Value,
    #[serde(default)]
    pub permissions: Value,
    #[serde(default)]
    pub budget: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StrategyRevision {
    pub strategy_id: String,
    pub revision: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub applicability: Value,
    pub authority: String,
    #[serde(default)]
    pub procedure: Option<Value>,
    #[serde(default)]
    pub advice: Option<String>,
    #[serde(default)]
    pub checks: Vec<Value>,
    #[serde(default)]
    pub provenance: Vec<EvidenceRef>,
    #[serde(default)]
    pub compatibility: Value,
    #[serde(default)]
    pub scope: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CandidateScore {
    pub arm_id: String,
    pub mean: f64,
    pub uncertainty: f64,
    pub cost: f64,
    pub risk: f64,
    pub probability: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Decision {
    pub id: String,
    pub task_id: String,
    pub slot: String,
    pub context: Value,
    pub candidates: Vec<CandidateScore>,
    pub selected: String,
    pub probability: f64,
    pub policy_revision: String,
    pub rng_seed: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Observation {
    pub decision_id: String,
    pub attempt_id: String,
    pub outcome: TaskOutcome,
    pub compliance: Compliance,
    #[serde(default)]
    pub checks: Vec<Value>,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
    #[serde(default)]
    pub costs: Value,
    pub regime: ObservationRegime,
    pub labels: LabelProvenance,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectionSnapshot {
    pub contract: TaskContract,
    #[serde(default)]
    pub strategies: Vec<StrategyRevision>,
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub context_items: Vec<Value>,
    #[serde(default)]
    pub memory_watermark: String,
    pub snapshot_hash: String,
}

fn non_empty(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if value.chars().count() > 4096 {
        return Err(format!("{field} is too long"));
    }
    Ok(())
}

impl EvidenceRef {
    pub fn validate(&self) -> Result<(), String> {
        non_empty(&self.id, "evidence.id")
    }
}

impl TaskContract {
    pub fn validate(&self) -> Result<(), String> {
        non_empty(&self.task_id, "taskId")?;
        non_empty(&self.revision, "revision")?;
        non_empty(&self.intent, "intent")?;
        non_empty(&self.family, "family")?;
        if self.mandatory.len() > 64 {
            return Err("mandatory checks exceed 64".into());
        }
        Ok(())
    }
}

impl StrategyRevision {
    pub fn validate(&self) -> Result<(), String> {
        non_empty(&self.strategy_id, "strategyId")?;
        non_empty(&self.revision, "revision")?;
        if !matches!(self.authority.as_str(), "standing" | "adaptive") {
            return Err("authority must be standing or adaptive".into());
        }
        if self.checks.len() > 64 || self.provenance.len() > 64 {
            return Err("strategy revision exceeds bounded arrays".into());
        }
        for evidence in &self.provenance {
            evidence.validate()?;
        }
        Ok(())
    }
}

impl Decision {
    pub fn validate(&self) -> Result<(), String> {
        non_empty(&self.id, "decision.id")?;
        non_empty(&self.task_id, "decision.taskId")?;
        non_empty(&self.selected, "decision.selected")?;
        if !(0.0..=1.0).contains(&self.probability) {
            return Err("decision.probability must be in [0,1]".into());
        }
        if self.candidates.len() > 64 {
            return Err("decision candidates exceed 64".into());
        }
        let sum: f64 = self.candidates.iter().map(|item| item.probability).sum();
        if !self.candidates.is_empty() && (sum - 1.0).abs() > 0.001 {
            return Err("candidate probabilities must sum to 1".into());
        }
        Ok(())
    }
}

impl Observation {
    pub fn validate(&self) -> Result<(), String> {
        non_empty(&self.decision_id, "observation.decisionId")?;
        non_empty(&self.attempt_id, "observation.attemptId")?;
        if self.checks.len() > 64 || self.evidence.len() > 128 {
            return Err("observation bounds exceeded".into());
        }
        for evidence in &self.evidence {
            evidence.validate()?;
        }
        Ok(())
    }
}

impl SelectionSnapshot {
    pub fn validate(&self) -> Result<(), String> {
        self.contract.validate()?;
        non_empty(&self.snapshot_hash, "snapshotHash")?;
        if self.strategies.len() > 16 || self.decisions.len() > 16 {
            return Err("selection snapshot bounds exceeded".into());
        }
        for strategy in &self.strategies {
            strategy.validate()?;
        }
        for decision in &self.decisions {
            decision.validate()?;
        }
        Ok(())
    }
}

/// Sort object keys recursively before hashing. Revision hashes must not
/// depend on JSON insertion order.
pub fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            let mut ordered = Map::new();
            for key in keys {
                if let Some(item) = map.get(&key) {
                    ordered.insert(key, canonical_json(item));
                }
            }
            Value::Object(ordered)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_json).collect()),
        other => other.clone(),
    }
}

pub fn revision_hash(value: &Value) -> String {
    let bytes = canonical_json(value).to_string();
    format!("{:x}", Sha256::digest(bytes.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract() -> Value {
        serde_json::json!({
            "taskId": "task-1",
            "revision": "r1",
            "source": "owner",
            "scope": {"kind": "local"},
            "intent": "파일을 만든다",
            "family": "file_transform",
            "mandatory": [],
            "bindings": {},
            "permissions": {},
            "budget": {"maxRounds": 8}
        })
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let mut value = contract();
        value["extra"] = serde_json::json!(true);
        assert!(serde_json::from_value::<TaskContract>(value).is_err());
    }

    #[test]
    fn revision_hash_is_key_order_independent() {
        let left = serde_json::json!({"a": 1, "b": {"x": 2, "y": 3}});
        let right = serde_json::json!({"b": {"y": 3, "x": 2}, "a": 1});
        assert_eq!(revision_hash(&left), revision_hash(&right));
        assert_ne!(revision_hash(&left), revision_hash(&serde_json::json!({"a": 2})));
    }

    #[test]
    fn observation_roundtrips_and_validates() {
        let observation = Observation {
            decision_id: "d1".into(),
            attempt_id: "a1".into(),
            outcome: TaskOutcome::Succeeded,
            compliance: Compliance::Followed,
            checks: vec![],
            evidence: vec![EvidenceRef {
                kind: EvidenceKind::Operation,
                id: "op1".into(),
                revision: None,
                scope: serde_json::json!({"kind":"local"}),
                observed_at: None,
                digest: None,
            }],
            costs: serde_json::json!({"tokens": 10}),
            regime: ObservationRegime::SandboxTreatment,
            labels: LabelProvenance::DeterministicOracle,
        };
        observation.validate().unwrap();
        let text = serde_json::to_string(&observation).unwrap();
        let decoded: Observation = serde_json::from_str(&text).unwrap();
        assert_eq!(decoded.attempt_id, "a1");
    }
}
