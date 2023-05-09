// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::node::NodeId;
use massa_models::stats::{ConsensusStats, ExecutionStats, NetworkStats};
use massa_models::{config::CompactConfig, slot::Slot, version::Version};
use massa_time::MassaTime;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::IpAddr;

/// node status
#[derive(Debug, Deserialize, Serialize)]
pub(crate)  struct NodeStatus {
    /// our node id
    pub(crate)  node_id: NodeId,
    /// optional node ip
    pub(crate)  node_ip: Option<IpAddr>,
    /// node version
    pub(crate)  version: Version,
    /// now
    pub(crate)  current_time: MassaTime,
    /// current cycle
    pub(crate)  current_cycle: u64,
    /// current cycle starting timestamp
    pub(crate)  current_cycle_time: MassaTime,
    /// next cycle starting timestamp
    pub(crate)  next_cycle_time: MassaTime,
    /// connected nodes (node id, ip address, true if the connection is outgoing, false if incoming)
    pub(crate)  connected_nodes: BTreeMap<NodeId, (IpAddr, bool)>,
    /// latest slot, none if now is before genesis timestamp
    pub(crate)  last_slot: Option<Slot>,
    /// next slot
    pub(crate)  next_slot: Slot,
    /// consensus stats
    pub(crate)  consensus_stats: ConsensusStats,
    /// pool stats (operation count and endorsement count)
    pub(crate)  pool_stats: (usize, usize),
    /// network stats
    pub(crate)  network_stats: NetworkStats,
    /// execution stats
    pub(crate)  execution_stats: ExecutionStats,
    /// compact configuration
    pub(crate)  config: CompactConfig,
}

impl std::fmt::Display for NodeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Node's ID: {}", self.node_id)?;
        if self.node_ip.is_some() {
            writeln!(f, "Node's IP: {}", self.node_ip.unwrap())?;
        } else {
            writeln!(f, "No routable IP set")?;
        }
        writeln!(f)?;

        writeln!(f, "Version: {}", self.version)?;
        writeln!(f, "Config:\n{}", self.config)?;
        writeln!(f)?;

        writeln!(f, "Current time: {}", self.current_time.to_utc_string())?;
        writeln!(f, "Current cycle: {}", self.current_cycle)?;
        if self.last_slot.is_some() {
            writeln!(f, "Last slot: {}", self.last_slot.unwrap())?;
        }
        writeln!(f, "Next slot: {}", self.next_slot)?;
        writeln!(f)?;

        writeln!(f, "{}", self.consensus_stats)?;

        writeln!(f, "Pool stats:")?;
        writeln!(f, "\tOperations count: {}", self.pool_stats.0)?;
        writeln!(f, "\tEndorsements count: {}", self.pool_stats.1)?;
        writeln!(f)?;

        writeln!(f, "{}", self.network_stats)?;

        writeln!(f, "{}", self.execution_stats)?;

        writeln!(f, "Connected nodes:")?;
        for (node_id, (ip_addr, is_outgoing)) in &self.connected_nodes {
            writeln!(
                f,
                "Node's ID: {} / IP address: {} / {} connection",
                node_id,
                ip_addr,
                if *is_outgoing { "Out" } else { "In" }
            )?
        }
        Ok(())
    }
}
