//! Evolving experiment selection, separate from frozen evaluation and proof.
//!
//! Fixed-weight models generate source revisions. This typed genome changes
//! WHICH source experiment the recompiled descendant runs next. Only original
//! paired, independently revalidated worker outcomes may modify its genotype.
//! The policy never rewrites/relaxes an evaluator or claims capability proof.
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

#[derive(Clone,Copy,Debug,Serialize,Deserialize,Eq,PartialEq)]
#[serde(rename_all="snake_case")]
enum Arm { Explore, Factorial, Repair }

impl Arm {
    fn name(self)->&'static str {
        match self {Self::Explore=>"explore",Self::Factorial=>"factorial",Self::Repair=>"repair"}
    }
    fn variants()->[Self;3]{[Self::Explore,Self::Factorial,Self::Repair]}
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Genome {
    // These genes affect the REAL ranking and can be inherited, mutated
    // and recombined; not merely labels for a hard-coded rotation.
    payoff_weight:u64,
    novelty_weight:u64,
    cost_weight:u64,
    recovery_weight:u64,
}
impl Default for Genome {
    fn default()->Self{Self{payoff_weight:5,novelty_weight:4,cost_weight:2,recovery_weight:3}}
}
impl Genome {
    fn clamp(&mut self){
        self.payoff_weight=self.payoff_weight.clamp(1,8);
        self.novelty_weight=self.novelty_weight.clamp(1,8);
        self.cost_weight=self.cost_weight.clamp(1,8);
        self.recovery_weight=self.recovery_weight.clamp(1,8);
    }
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields,rename_all="camelCase")]
struct Contract {
    source:String,
    generator_commit:String,
    revision:String,
    parent_revision:Option<String>,
    arm:Arm,
    genome:Genome,
    evidence_job_ids:Vec<String>,
    observed_trials:usize,
    #[serde(default)]
    effect_proven:bool,
}
fn hash(value:&Value)->String{
    format!("{:x}",Sha256::digest(serde_json::to_vec(value).unwrap_or_default()))
}
fn observed(row:&Value)->Option<(Arm,Genome)>{
    if row["candidateOrigin"]!="model_driven_detached_worktree"
        ||row["pairedMeasured"]!=true
        ||(row["accepted"]==true &&row["pairedRevalidated"]!=true)
        ||row["experimentStrategyApplied"]["source"]
            !="running_compiled_descendant_experiment_strategy_v1"
        ||row["experimentStrategyApplied"]["generatorCommit"]!=row["generatorCommit"]
        ||row["experimentStrategyApplied"]["revision"]!=row["experimentStrategy"]["revision"]
        ||row["experimentStrategyApplied"]["arm"]!=row["experimentStrategy"]["arm"]
        ||row["experimentStrategyApplied"]["workerApplied"]!=true {
        return None;
    }
    if validate(&row["experimentStrategy"],row["generatorCommit"].as_str()?).is_err(){
        return None;
    }
    let c:Contract=serde_json::from_value(row["experimentStrategy"].clone()).ok()?;
    if c.source!="compiled_descendant_experiment_strategy_v1"
        ||row["generatorCommit"].as_str()!=Some(c.generator_commit.as_str())
        ||c.effect_proven {return None;}
    Some((c.arm,c.genome))
}
fn score(row:&Value,genome:&Genome)->i64{
    let gain=row["evaluation"]["metrics"]["improvedHoldouts"].as_u64().unwrap_or(0).min(8);
    let regression=row["evaluation"]["metrics"]["regressions"].as_u64().unwrap_or(0).min(8);
    let tokens=row["tokens"].as_u64().unwrap_or(0);
    (genome.payoff_weight as i64)*((gain as i64)*3+(row["accepted"]==true) as i64*4)
        -(genome.recovery_weight as i64)*(regression as i64)*3
        -(genome.cost_weight as i64)*((tokens/10000).min(12) as i64)
}

/// A policy parent can be reused only if its original executed worker
/// produced a frozen paired receipt; unmeasured declarations are ignored.
/// Recombinations are proposals, not proof of the policy's superiority.
fn inherit(history:&[Value])->(Genome,Option<String>){
    let verified=history.iter().filter_map(|r|observed(r).map(|(arm,genome)|{
        let grade=score(r,&genome);
        (r,arm,genome,grade)
    })).collect::<Vec<_>>();
    let Some((last,_,last_genome,_))=verified.last() else{return (Genome::default(),None)};
    let mut next=last_genome.clone();
    if last["accepted"]==true {
        next.payoff_weight+=1;
        next.cost_weight=next.cost_weight.saturating_sub(1);
    }else if last["evaluation"]["metrics"]["regressions"].as_u64().unwrap_or(0)>0{
        next.recovery_weight+=1;
        next.novelty_weight+=1;
    }else{
        next.novelty_weight+=1;
        next.cost_weight+=1;
    }
    // Use the best other independently measured genome as a donor, not an
    // arbitrary user-submitted policy object or a successful file name.
    if let Some((_,_,donor,_))=verified.iter().max_by_key(|x|x.3){
        next.payoff_weight=(next.payoff_weight+donor.payoff_weight).div_ceil(2);
        next.cost_weight=(next.cost_weight+donor.cost_weight).div_ceil(2);
    }
    next.clamp();
    (next,last["experimentStrategy"]["revision"].as_str().map(str::to_string))
}

/// Choose among genuinely executable experiments. This is called in the
/// compiled descendant plan, before any worker is launched. No per-N fixed
/// schedule: successful measured experiments can be exploited; sparse arms
/// get a novelty bonus; repeated regressions increase repair preference.
pub(crate) fn propose(history:&[Value],revision:&str,factorial:bool,repair:bool)->Value{
    let (genome,parent)=inherit(history);
    let allowed=Arm::variants().into_iter().filter(|arm|
        match arm {Arm::Explore=>true,Arm::Factorial=>factorial,Arm::Repair=>repair})
        .collect::<Vec<_>>();
    let mut scored=Vec::new();
    let real=history.iter().filter_map(|r|observed(r).map(|(arm,_)|(r,arm)))
        .collect::<Vec<_>>();
    for arm in allowed {
        let rows=real.iter().filter(|(_,a)|*a==arm).map(|(r,_)|*r)
            .collect::<Vec<_>>();
        let trials=rows.len() as i64;
        let rewards=rows.iter().map(|r|score(r,&genome)).sum::<i64>();
        // Integer arithmetic makes tie-breaking and source receipt hashes
        // independent of architecture-specific floating-point behavior.
        let mean=if trials>0 {rewards*100/trials}else{0};
        let novelty=(genome.novelty_weight as i64)*500/(trials+1);
        let recovery=if arm==Arm::Repair {genome.recovery_weight as i64*100}else{0};
        scored.push((mean+novelty+recovery,arm,trials));
    }
    scored.sort_by(|a,b|b.0.cmp(&a.0).then_with(||a.1.name().cmp(b.1.name())));
    let selected=scored.first().map(|r|r.1).unwrap_or(Arm::Explore);
    let ids=real.iter().rev().take(16).filter_map(|(r,_)|r["jobId"].as_str())
        .map(str::to_string).collect::<Vec<_>>();
    let revision_id=hash(&json!(["rsi.experiment.genome.v1",revision,parent,
        selected,&genome,&ids]));
    let contract=Contract{
        source:"compiled_descendant_experiment_strategy_v1".into(),
        generator_commit:revision.into(),revision:revision_id,
        parent_revision:parent,arm:selected,genome,evidence_job_ids:ids,
        observed_trials:real.len(),effect_proven:false
    };
    serde_json::to_value(contract).unwrap_or(Value::Null)
}

/// Bind the selected arm to its actual executable experiment plan.
pub(crate) fn implementation_matches(
    policy:&Value,factorial:&Value,repair:&Value
)->bool{
    match policy["arm"].as_str(){
        Some("explore")=>factorial.is_null()&&repair.is_null(),
        Some("factorial")=>factorial.is_object()&&repair.is_null(),
        Some("repair")=>repair.is_object()&&factorial.is_null(),
        _=>false
    }
}

/// Validate the stable typed interface at the coordinator/worker boundary.
/// A child is free to evolve its ranking algorithm in source code, but may not
/// send an unrecognized strategy or change the immutable source identity.
pub(crate) fn validate(policy:&Value,revision:&str)->Result<(),String>{
    let contract:Contract=serde_json::from_value(policy.clone())
        .map_err(|e|format!("후손 실험 정책 계약 불일치: {e}"))?;
    if contract.source!="compiled_descendant_experiment_strategy_v1"
        ||contract.generator_commit!=revision
        ||contract.revision.len()!=64
        ||!contract.revision.bytes().all(|x|x.is_ascii_hexdigit())
        ||contract.effect_proven
        ||hash(&json!([
            "rsi.experiment.genome.v1",revision,contract.parent_revision,
            contract.arm,&contract.genome,&contract.evidence_job_ids
        ]))!=contract.revision
        ||contract.evidence_job_ids.len()>16
        ||contract.genome.payoff_weight==0||contract.genome.payoff_weight>8
        ||contract.genome.novelty_weight==0||contract.genome.novelty_weight>8
        ||contract.genome.cost_weight==0||contract.genome.cost_weight>8
        ||contract.genome.recovery_weight==0||contract.genome.recovery_weight>8 {
        return Err("후손 실험 정책 출처·범위·가중치 계약 위반".into());
    }
    if contract.evidence_job_ids.iter().any(|id|id.is_empty())
        ||contract.evidence_job_ids.iter().collect::<std::collections::BTreeSet<_>>().len()
            !=contract.evidence_job_ids.len() {
        return Err("실험 정책 학습에 사용한 작업 영수증 중복·공백".into());
    }
    Ok(())
}
#[cfg(test)]
#[path = "../../../../tests/unit/rsi_meta_experiment_evolution_tests.rs"]
mod tests;
