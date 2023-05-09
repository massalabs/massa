//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::slot::Slot;
use massa_time::MassaTime;
use serde::{Deserialize, Serialize};
use std::fmt::Formatter;

/// execution statistics
#[derive(Serialize, Deserialize, Debug)]
pub struct ExecutionStats {
    /// time window start
    pub(crate) time_window_start: MassaTime,
    /// time window end
    pub(crate) time_window_end: MassaTime,
    /// number of final blocks in the time window
    pub(crate) final_block_count: usize,
    /// number of final executed operations in the time window
    pub(crate) final_executed_operations_count: usize,
    /// active execution cursor slot
    pub(crate) active_cursor: Slot,
}

impl std::fmt::Display for ExecutionStats {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Execution stats:")?;
        writeln!(
            f,
            "\tStart stats timespan time: {}",
            self.time_window_start.to_utc_string()
        )?;
        writeln!(
            f,
            "\tEnd stats timespan time: {}",
            self.time_window_end.to_utc_string()
        )?;
        writeln!(
            f,
            "\tFinal executed block count: {}",
            self.final_block_count
        )?;
        writeln!(
            f,
            "\tFinal executed operation count: {}",
            self.final_executed_operations_count
        )?;
        writeln!(f, "\tActive cursor: {}", self.active_cursor)?;
        Ok(())
    }
}

/// stats produced by network module
#[derive(Serialize, Deserialize, Debug)]
pub struct NetworkStats {
    /// in connections count
    pub(crate) in_connection_count: u64,
    /// out connections count
    pub(crate) out_connection_count: u64,
    /// total known peers count
    pub(crate) known_peer_count: u64,
    /// banned node count
    pub(crate) banned_peer_count: u64,
    /// active node count
    pub(crate) active_node_count: u64,
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
    pub(crate) start_timespan: MassaTime,
    /// end of the time span for stats
    pub(crate) end_timespan: MassaTime,
    /// number of final blocks
    pub(crate) final_block_count: u64,
    /// number of stale blocks in memory
    pub(crate) stale_block_count: u64,
    ///  number of actives cliques
    pub(crate) clique_count: u64,
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
        writeln!(f, "\tClique count: {}", self.clique_count)?;
        Ok(())
    }
}

/// stats produced by pool module
#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct PoolStats {
    /// number of operations in the pool
    pub(crate) operation_count: u64,
    /// number of endorsement in the pool
    pub(crate) endorsement_count: u64,
}

impl std::fmt::Display for PoolStats {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Pool stats:")?;
        writeln!(f, "\tOperations: {}", self.operation_count)?;
        writeln!(f, "\tEndorsements: {}", self.endorsement_count)?;
        Ok(())
    }
}
