//! Portable, dependency-free decision contract between a durable harness and
//! replaceable agent loop. A policy may choose *when* to ask for a model turn;
//! it has no filesystem, network, database, tool executor or approval authority.
#![forbid(unsafe_code)]

/// Abstract snapshot copied from durable runtime state. Never grants access
/// to the original Run, provider credentials or the concrete tool registry.
#[derive(Debug,Clone,Copy,PartialEq,Eq,Default)]
pub struct TurnSnapshot {
    pub running:bool,
    pub terminal:bool,
    pub approval_pending:bool,
    pub program_ready:bool,
    pub has_queued_tools:bool,
    pub plan_exists:bool,
    pub separate_planning:bool,
    pub steps:usize,
    pub max_steps:usize,
}
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum LoopStep {
    Stop,
    ResolveApproval,
    AdvanceProgram,
    Dispatch,
    FusePlan,
    RequestPlan,
    MaxStepsExceeded,
    ModelTurn,
}
/// A deterministic, side-effect-free scheduling algorithm. Only the runtime
/// may execute effects and change durable Run state.
pub trait AgentLoop:Send+Sync {
    fn name(&self)->&'static str;
    fn next(&self,state:TurnSnapshot)->LoopStep;
}

impl TurnSnapshot {
    /// The runtime enforces this admission gate *after* an interchangeable
    /// agent loop chooses a phase. Custom algorithms cannot bypass approvals,
    /// execute a superseded queued call, or run another model after exhaustion.
    pub fn admit(self,decision:LoopStep)->Result<(),&'static str>{
        let required=if self.terminal||!self.running{
            Some(LoopStep::Stop)
        }else if self.approval_pending {
            Some(LoopStep::ResolveApproval)
        }else if self.program_ready {
            Some(LoopStep::AdvanceProgram)
        }else if self.has_queued_tools {
            Some(LoopStep::Dispatch)
        }else if !self.plan_exists {
            if matches!(decision,LoopStep::FusePlan|LoopStep::RequestPlan){
                return Ok(())
            }
            return Err("uninitialized plan requires an explicit planning phase");
        }else if self.steps>=self.max_steps {
            Some(LoopStep::MaxStepsExceeded)
        }else {
            Some(LoopStep::ModelTurn)
        };
        if Some(decision)==required{Ok(())}
        else{Err("agent loop attempted to bypass a mandatory runtime state barrier")}
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn loop_cannot_model_through_unsettled_or_expired_effect(){
        let b=TurnSnapshot{running:true,approval_pending:true,max_steps:4,..Default::default()};
        assert!(b.admit(LoopStep::ModelTurn).is_err());
        assert!(b.admit(LoopStep::ResolveApproval).is_ok());
        let stale=TurnSnapshot{running:true,has_queued_tools:true,steps:99,
            max_steps:4,..Default::default()};
        assert!(stale.admit(LoopStep::ModelTurn).is_err());
        assert!(stale.admit(LoopStep::Dispatch).is_ok());
        let expired=TurnSnapshot{running:true,plan_exists:true,steps:4,
            max_steps:4,..Default::default()};
        assert!(expired.admit(LoopStep::ModelTurn).is_err());
        assert!(expired.admit(LoopStep::MaxStepsExceeded).is_ok());
    }
    #[test]fn only_approved_phase_is_allowed_for_terminal_or_fresh_task(){
        let terminal=TurnSnapshot{terminal:true,running:true,approval_pending:true,
            ..Default::default()};
        assert!(terminal.admit(LoopStep::ResolveApproval).is_err());
        assert!(terminal.admit(LoopStep::Stop).is_ok());
        let fresh=TurnSnapshot{running:true,max_steps:5,..Default::default()};
        assert!(fresh.admit(LoopStep::ModelTurn).is_err());
        assert!(fresh.admit(LoopStep::FusePlan).is_ok());
        assert!(fresh.admit(LoopStep::RequestPlan).is_ok());
    }
}
