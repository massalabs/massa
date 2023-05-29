// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::node::NodeId;
use massa_models::stats::{ConsensusStats, ExecutionStats, NetworkStats};
use massa_models::{config::CompactConfig, slot::Slot, version::Version};
use massa_time::MassaTime;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::IpAddr;

/// node status
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NodeStatus {
    /// our node id
    pub node_id: NodeId,
    /// optional node ip
    pub node_ip: Option<IpAddr>,
    /// node version
    pub version: Version,
    /// now
    pub current_time: MassaTime,
    /// current cycle
    pub current_cycle: u64,
    /// current cycle starting timestamp
    pub current_cycle_time: MassaTime,
    /// next cycle starting timestamp
    pub next_cycle_time: MassaTime,
    /// connected nodes (node id, ip address, true if the connection is outgoing, false if incoming)
    pub connected_nodes: BTreeMap<NodeId, (IpAddr, bool)>,
    /// latest slot, none if now is before genesis timestamp
    pub last_slot: Option<Slot>,
    /// next slot
    pub next_slot: Slot,
    /// consensus stats
    pub consensus_stats: ConsensusStats,
    /// pool stats (operation count and endorsement count)
    pub pool_stats: (usize, usize),
    /// network stats
    pub network_stats: NetworkStats,
    /// execution stats
    pub execution_stats: ExecutionStats,
    /// compact configuration
    pub config: CompactConfig,
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
