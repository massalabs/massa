// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::amount::Amount;
use massa_models::ledger::LedgerData;

use serde::{Deserialize, Serialize};

/// Current balance ledger info
#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub(crate)  struct LedgerInfo {
    /// final data
    pub(crate)  final_ledger_info: LedgerData,
    /// latest data
    pub(crate)  candidate_ledger_info: LedgerData,
    /// locked balance, for example balance due to a roll sell
    pub(crate)  locked_balance: Amount,
}

impl std::fmt::Display for LedgerInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\tFinal balance: {}", self.final_ledger_info.balance)?;
        writeln!(
            f,
            "\tCandidate balance: {}",
            self.candidate_ledger_info.balance
        )?;
        writeln!(f, "\tLocked balance: {}", self.locked_balance)?;
        Ok(())
    }
}
