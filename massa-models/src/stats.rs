// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_time::MassaTime;
use serde::{Deserialize, Serialize};
use std::fmt::Formatter;

#[derive(Serialize, Deserialize, Debug)]
pub struct NetworkStats {
    pub in_connection_count: u64,
    pub out_connection_count: u64,
    pub known_peer_count: u64,
    pub banned_peer_count: u64,
    pub active_node_count: u64,
}

impl std::fmt::Display for NetworkStats {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Network stats:")?;
        writeln!(f, "\tIn connections: {}", self.in_connection_count)?;
        writeln!(f, "\tOut connections: {}", self.out_connection_count)?;
        writeln!(f, "\tKnown peers: {}", self.known_peer_count)?;
        writeln!(f, "\tBanned peers: {}", self.banned_peer_count)?;
        writeln!(f, "\tActive nodes: {}", self.active_node_count)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusStats {
    pub start_timespan: MassaTime,
    pub end_timespan: MassaTime,
    pub final_block_count: u64,
    pub final_operation_count: u64,
    pub stale_block_count: u64,
    pub clique_count: u64,
    pub staker_count: u64,
}

impl std::fmt::Display for ConsensusStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Consensus stats:")?;
        writeln!(
            f,
            "\tStart stats timespan time: {}",
            self.start_timespan.to_utc_string()
        )?;
        writeln!(
            f,
            "\tEnd stats timespan time: {}",
            self.end_timespan.to_utc_string()
        )?;
        writeln!(f, "\tFinal block count: {}", self.final_block_count)?;
        writeln!(f, "\tStale block count: {}", self.stale_block_count)?;
        writeln!(f, "\tFinal operation count: {}", self.final_operation_count)?;
        writeln!(f, "\tClique count: {}", self.clique_count)?;
        writeln!(f, "\tStaker count: {}", self.staker_count)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PoolStats {
    pub operation_count: u64,
    pub endorsement_count: u64,
}

impl std::fmt::Display for PoolStats {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Pool stats:")?;
        writeln!(f, "\tOperations: {}", self.operation_count)?;
        writeln!(f, "\tEndorsements: {}", self.endorsement_count)?;
        Ok(())
    }
}
