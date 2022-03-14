// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_time::MassaTime;
use serde::{Deserialize, Serialize};
use std::fmt::Formatter;

/// stats produced by network module
#[derive(Serialize, Deserialize, Debug)]
pub struct NetworkStats {
    /// in connections count
    pub in_connection_count: u64,
    /// out connections count
    pub out_connection_count: u64,
    /// total known peers count
    pub known_peer_count: u64,
    /// banned node count
    pub banned_peer_count: u64,
    /// active node count
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

/// stats produced by consensus module
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusStats {
    /// start of the time span for stats
    pub start_timespan: MassaTime,
    /// end of the time span for stats
    pub end_timespan: MassaTime,
    /// number of final blocks
    pub final_block_count: u64,
    /// number of final operations
    pub final_operation_count: u64,
    /// number of stale blocks in memory
    pub stale_block_count: u64,
    ///  number of actives cliques
    pub clique_count: u64,
    /// total number of stakers
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

/// stats produced by pool module
#[derive(Serialize, Deserialize, Debug)]
pub struct PoolStats {
    /// number of operations in the pool
    pub operation_count: u64,
    /// number of endorsement in the pool
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
