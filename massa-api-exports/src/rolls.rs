// Copyright (c) 2022 MASSA LABS <info@massa.net>

use serde::{Deserialize, Serialize};

/// Roll counts
#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub(crate)  struct RollsInfo {
    /// count taken into account for the current cycle
    pub(crate)  active_rolls: u64,
    /// at final blocks
    pub(crate)  final_rolls: u64,
    /// at latest blocks
    pub(crate)  candidate_rolls: u64,
}

impl std::fmt::Display for RollsInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "\tActive rolls: {}", self.active_rolls)?;
        writeln!(f, "\tFinal rolls: {}", self.final_rolls)?;
        writeln!(f, "\tCandidate rolls: {}", self.candidate_rolls)?;
        Ok(())
    }
}
