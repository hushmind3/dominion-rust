//! One budget ledger for model rounds, tool proposals and executed tools.
use serde_json::{json, Value};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct BudgetLedger {
    max_rounds: usize,
    max_tools: usize,
    max_proposals: usize,
    max_elapsed: Duration,
    rounds: usize,
    tools: usize,
    proposals: usize,
    started: Instant,
}

impl BudgetLedger {
    pub fn new(max_rounds: usize, max_tools: usize, max_elapsed: Duration) -> Self {
        Self {
            max_rounds,
            max_tools,
            // Blocked proposals must not silently consume executed-tool slots,
            // but they still need a bounded budget.
            max_proposals: max_tools.saturating_mul(2),
            max_elapsed,
            rounds: 0,
            tools: 0,
            proposals: 0,
            started: Instant::now(),
        }
    }

    pub fn charge_round(&mut self) -> bool {
        if self.rounds >= self.max_rounds {
            return false;
        }
        self.rounds += 1;
        true
    }

    pub fn charge_proposal(&mut self) -> bool {
        if self.proposals >= self.max_proposals {
            return false;
        }
        self.proposals += 1;
        true
    }

    pub fn charge_tool(&mut self) -> bool {
        if self.tools >= self.max_tools {
            return false;
        }
        self.tools += 1;
        true
    }

    /// A fresh ledger for one parallel branch. Counters start at zero so the
    /// parent can merge only the usage of this branch.
    pub fn branch(&self) -> Self {
        Self {
            max_rounds: self.max_rounds.saturating_sub(self.rounds),
            max_tools: self.max_tools.saturating_sub(self.tools),
            max_proposals: self.max_proposals.saturating_sub(self.proposals),
            max_elapsed: self.max_elapsed.saturating_sub(self.started.elapsed()),
            rounds: 0,
            tools: 0,
            proposals: 0,
            started: Instant::now(),
        }
    }

    /// Adds the usage of a completed branch back into this ledger.
    pub fn merge_usage(&mut self, branch: &Self) {
        self.rounds = self.rounds.saturating_add(branch.rounds);
        self.tools = self.tools.saturating_add(branch.tools);
        self.proposals = self.proposals.saturating_add(branch.proposals);
    }

    pub fn can_execute_tool(&self) -> bool {
        self.tools < self.max_tools
    }

    pub fn expired(&self) -> bool {
        self.started.elapsed() > self.max_elapsed
    }

    pub fn snapshot(&self) -> Value {
        json!({
            "rounds": self.rounds,
            "maxRounds": self.max_rounds,
            "tools": self.tools,
            "maxTools": self.max_tools,
            "proposals": self.proposals,
            "maxProposals": self.max_proposals,
            "elapsedMs": self.started.elapsed().as_millis() as u64,
            "maxElapsedMs": self.max_elapsed.as_millis() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_bounds_rounds_tools_and_proposals() {
        let mut ledger = BudgetLedger::new(2, 1, Duration::from_secs(60));
        assert!(ledger.charge_round());
        assert!(ledger.charge_round());
        assert!(!ledger.charge_round());
        assert!(ledger.charge_tool());
        assert!(!ledger.charge_tool());
        assert!(!ledger.can_execute_tool());
        assert!(ledger.charge_proposal());
        assert!(ledger.charge_proposal());
        assert!(!ledger.charge_proposal());
    }

    #[test]
    fn ledger_snapshot_exposes_limits() {
        let ledger = BudgetLedger::new(8, 40, Duration::from_secs(600));
        let snapshot = ledger.snapshot();
        assert_eq!(snapshot["maxRounds"], 8);
        assert_eq!(snapshot["maxTools"], 40);
        assert_eq!(snapshot["maxProposals"], 80);
    }
}
