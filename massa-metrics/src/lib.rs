//! this library is used to collect metrics from the node and expose them to the prometheus server
//!
//! the metrics are collected from the node and from the survey
//! the survey is a separate thread that is used to collect metrics from the network (active connections)
//!

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, RwLock},
    thread::JoinHandle,
    time::Duration,
};

use lazy_static::lazy_static;
use prometheus::{register_int_gauge, Gauge, IntCounter, IntGauge};
use tokio::sync::oneshot::Sender;
use tracing::warn;

// #[cfg(not(feature = "testing"))]
mod server;

lazy_static! {
    // use lazy_static for these metrics because they are used in storage which implement default
    static ref OPERATIONS_COUNTER: IntGauge = register_int_gauge!(
        "operations_storage_counter",
        "operations storage counter len"
    )
    .unwrap();
    static ref BLOCKS_COUNTER: IntGauge =
        register_int_gauge!("blocks_storage_counter", "blocks storage counter len").unwrap();
    static ref ENDORSEMENTS_COUNTER: IntGauge =
        register_int_gauge!("endorsements_storage_counter", "endorsements storage counter len").unwrap();
}

pub fn set_blocks_counter(val: usize) {
    BLOCKS_COUNTER.set(val as i64);
}

pub fn set_endorsements_counter(val: usize) {
    ENDORSEMENTS_COUNTER.set(val as i64);
}

pub fn set_operations_counter(val: usize) {
    OPERATIONS_COUNTER.set(val as i64);
}

#[derive(Default)]
pub struct MetricsStopper {
    pub(crate) stopper: Option<Sender<()>>,
    pub(crate) stop_handle: Option<JoinHandle<()>>,
}

impl MetricsStopper {
    pub fn stop(&mut self) {
        if let Some(stopper) = self.stopper.take() {
            if stopper.send(()).is_err() {
                warn!("failed to send stop signal to metrics server");
            }

            if let Some(handle) = self.stop_handle.take() {
                if let Err(_e) = handle.join() {
                    warn!("failed to join metrics server thread");
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct MassaMetrics {
    /// enable metrics
    enabled: bool,

    /// number of processors
    process_available_processors: IntGauge,

    /// consensus period for each thread
    /// index 0 = thread 0 ...
    consensus_vec: Vec<Gauge>,

    /// number of stakers
    stakers: IntGauge,
    /// number of rolls
    rolls: IntGauge,

    /// number of elements in the active_history of execution
    active_history: IntGauge,

    /// number of operations in the operation pool
    operations_pool: IntGauge,
    /// number of endorsements in the endorsement pool
    endorsements_pool: IntGauge,
    /// number of elements in the denunciation pool
    denunciations_pool: IntGauge,

    // number of autonomous SCs messages in pool
    async_message_pool_size: IntGauge,

    // number of autonomous SC messages executed as final
    sc_messages_final: IntCounter,

    /// number of times our node (re-)bootstrapped
    bootstrap_counter: IntCounter,
    /// number of times we successfully bootstrapped someone
    bootstrap_peers_success: IntCounter,
    /// number of times we failed/refused to bootstrap someone
    bootstrap_peers_failed: IntCounter,

    /// number of times we successfully tested someone
    protocol_tester_success: IntCounter,
    /// number of times we failed to test someone
    protocol_tester_failed: IntCounter,

    /// know peers in protocol
    protocol_known_peers: IntGauge,
    /// banned peers in protocol
    protocol_banned_peers: IntGauge,

    /// executed final slot
    executed_final_slot: IntCounter,
    /// executed final slot with block (not miss)
    executed_final_slot_with_block: IntCounter,

    /// total bytes receive by peernet manager
    peernet_total_bytes_received: IntCounter,
    /// total bytes sent by peernet manager
    peernet_total_bytes_sent: IntCounter,

    /// total block in graph
    block_graph_counter: IntCounter,
    /// total time to add block to graph
    block_graph_ms: IntCounter,

    /// active in connections peer
    active_in_connections: IntGauge,
    /// active out connections peer
    active_out_connections: IntGauge,

    /// counter of operations for final slot
    operations_final_counter: IntCounter,

    // block_cache
    block_cache_checked_headers_size: IntGauge,
    block_cache_blocks_known_by_peer: IntGauge,

    // Operation cache
    operation_cache_checked_operations: IntGauge,
    operation_cache_checked_operations_prefix: IntGauge,
    operation_cache_ops_know_by_peer: IntGauge,

    // Consensus state
    consensus_state_active_index: IntGauge,
    consensus_state_active_index_without_ops: IntGauge,
    consensus_state_incoming_index: IntGauge,
    consensus_state_discarded_index: IntGauge,
    consensus_state_block_statuses: IntGauge,

    // endorsement cache
    endorsement_cache_checked_endorsements: IntGauge,
    endorsement_cache_known_by_peer: IntGauge,

    // cursor
    active_cursor_thread: IntGauge,
    active_cursor_period: IntGauge,

    final_cursor_thread: IntGauge,
    final_cursor_period: IntGauge,

    // peer bandwidth (bytes sent, bytes received)
    peers_bandwidth: Arc<RwLock<HashMap<String, (IntCounter, IntCounter)>>>,

    pub tick_delay: Duration,
}

impl MassaMetrics {
    #[allow(unused_variables)]
    #[allow(unused_mut)]
    pub fn new(
        enabled: bool,
        addr: SocketAddr,
        nb_thread: u8,
        tick_delay: Duration,
    ) -> (Self, MetricsStopper) {
        let mut consensus_vec = vec![];
        for i in 0..nb_thread {
            let gauge = Gauge::new(
                format!("consensus_thread_{}", i),
                "consensus thread actual period",
            )
            .expect("Failed to create gauge");
            #[cfg(not(feature = "testing"))]
            {
                let _ = prometheus::register(Box::new(gauge.clone()));
            }

            consensus_vec.push(gauge);
        }

        // set available processors
        let process_available_processors =
            IntGauge::new("process_available_processors", "number of processors")
                .expect("Failed to create available_processors counter");

        // stakers
        let stakers = IntGauge::new("stakers", "number of stakers").unwrap();
        let rolls = IntGauge::new("rolls", "number of rolls").unwrap();

        let executed_final_slot =
            IntCounter::new("executed_final_slot", "number of executed final slot").unwrap();
        let executed_final_slot_with_block = IntCounter::new(
            "executed_final_slot_with_block",
            "number of executed final slot with block (not miss)",
        )
        .unwrap();

        let protocol_tester_success = IntCounter::new(
            "protocol_tester_success",
            "number of times we successfully tested someone",
        )
        .unwrap();
        let protocol_tester_failed = IntCounter::new(
            "protocol_tester_failed",
            "number of times we failed to test someone",
        )
        .unwrap();

        // pool
        let operations_pool = IntGauge::new(
            "operations_pool",
            "number of operations in the operation pool",
        )
        .unwrap();
        let endorsements_pool = IntGauge::new(
            "endorsements_pool",
            "number of endorsements in the endorsement pool",
        )
        .unwrap();
        let denunciations_pool = IntGauge::new(
            "denunciations_pool",
            "number of elements in the denunciation pool",
        )
        .unwrap();

        let async_message_pool_size = IntGauge::new(
            "async_message_pool_size",
            "number of autonomous SCs messages in pool",
        )
        .unwrap();

        let sc_messages_final = IntCounter::new(
            "sc_messages_final",
            "number of autonomous SC messages executed as final",
        )
        .unwrap();

        let bootstrap_counter = IntCounter::new(
            "bootstrap_counter",
            "number of times our node (re-)bootstrapped",
        )
        .unwrap();
        let bootstrap_success = IntCounter::new(
            "bootstrap_peers_success",
            "number of times we successfully bootstrapped someone",
        )
        .unwrap();
        let bootstrap_failed = IntCounter::new(
            "bootstrap_peers_failed",
            "number of times we failed/refused to bootstrap someone",
        )
        .unwrap();

        let active_history = IntGauge::new(
            "active_history",
            "number of elements in the active_history of execution",
        )
        .unwrap();

        let know_peers =
            IntGauge::new("protocol_known_peers", "number of known peers in protocol").unwrap();
        let banned_peers = IntGauge::new(
            "protocol_banned_peers",
            "number of banned peers in protocol",
        )
        .unwrap();

        // active cursor
        let active_cursor_thread =
            IntGauge::new("active_cursor_thread", "execution active cursor thread").unwrap();
        let active_cursor_period =
            IntGauge::new("active_cursor_period", "execution active cursor period").unwrap();

        // final cursor
        let final_cursor_thread =
            IntGauge::new("final_cursor_thread", "execution final cursor thread").unwrap();
        let final_cursor_period =
            IntGauge::new("final_cursor_period", "execution final cursor period").unwrap();

        // active connections IN
        let active_in_connections =
            IntGauge::new("active_in_connections", "active connections IN len").unwrap();

        // active connections OUT
        let active_out_connections =
            IntGauge::new("active_out_connections", "active connections OUT len").unwrap();

        // block cache
        let block_cache_checked_headers_size = IntGauge::new(
            "block_cache_checked_headers_size",
            "size of BlockCache checked_headers",
        )
        .unwrap();

        let block_cache_blocks_known_by_peer = IntGauge::new(
            "block_cache_blocks_known_by_peer_size",
            "size of BlockCache blocks_known_by_peer",
        )
        .unwrap();

        // operation cache
        let operation_cache_checked_operations = IntGauge::new(
            "operation_cache_checked_operations",
            "size of OperationCache checked_operations",
        )
        .unwrap();

        let operation_cache_checked_operations_prefix = IntGauge::new(
            "operation_cache_checked_operations_prefix",
            "size of OperationCache checked_operations_prefix",
        )
        .unwrap();

        let operation_cache_ops_know_by_peer = IntGauge::new(
            "operation_cache_ops_know_by_peer",
            "size of OperationCache operation_cache_ops_know_by_peer",
        )
        .unwrap();

        // consensus state from tick.rs
        let consensus_state_active_index = IntGauge::new(
            "consensus_state_active_index",
            "consensus state active index size",
        )
        .unwrap();

        let consensus_state_active_index_without_ops = IntGauge::new(
            "consensus_state_active_index_without_ops",
            "consensus state active index without ops size",
        )
        .unwrap();

        let consensus_state_incoming_index = IntGauge::new(
            "consensus_state_incoming_index",
            "consensus state incoming index size",
        )
        .unwrap();

        let consensus_state_discarded_index = IntGauge::new(
            "consensus_state_discarded_index",
            "consensus state discarded index size",
        )
        .unwrap();

        let consensus_state_block_statuses = IntGauge::new(
            "consensus_state_block_statuses",
            "consensus state block statuses size",
        )
        .unwrap();

        let endorsement_cache_checked_endorsements = IntGauge::new(
            "endorsement_cache_checked_endorsements",
            "endorsement cache checked endorsements size",
        )
        .unwrap();

        let endorsement_cache_known_by_peer = IntGauge::new(
            "endorsement_cache_known_by_peer",
            "endorsement cache know by peer size",
        )
        .unwrap();

        let block_graph_counter =
            IntCounter::new("block_slot_graph_counter", "total block in graph").unwrap();
        let block_graph_ms = IntCounter::new(
            "block_slot_graph_ms",
            "sum of delta in ms between block inclusion in graph and block slot",
        )
        .unwrap();

        let peernet_total_bytes_received = IntCounter::new(
            "peernet_total_bytes_received",
            "total byte received by peernet",
        )
        .unwrap();

        let peernet_total_bytes_sent =
            IntCounter::new("peernet_total_bytes_sent", "total byte sent by peernet").unwrap();

        let operations_final_counter =
            IntCounter::new("operations_final_counter", "total final operations").unwrap();

        let mut stopper = MetricsStopper::default();

        if enabled {
            #[cfg(not(feature = "testing"))]
            {
                let _ = prometheus::register(Box::new(final_cursor_thread.clone()));
                let _ = prometheus::register(Box::new(final_cursor_period.clone()));
                let _ = prometheus::register(Box::new(active_cursor_thread.clone()));
                let _ = prometheus::register(Box::new(active_cursor_period.clone()));
                let _ = prometheus::register(Box::new(active_out_connections.clone()));
                let _ = prometheus::register(Box::new(block_cache_blocks_known_by_peer.clone()));
                let _ = prometheus::register(Box::new(block_cache_checked_headers_size.clone()));
                let _ = prometheus::register(Box::new(operation_cache_checked_operations.clone()));
                let _ = prometheus::register(Box::new(active_in_connections.clone()));
                let _ = prometheus::register(Box::new(operation_cache_ops_know_by_peer.clone()));
                let _ = prometheus::register(Box::new(consensus_state_active_index.clone()));
                let _ = prometheus::register(Box::new(
                    consensus_state_active_index_without_ops.clone(),
                ));
                let _ = prometheus::register(Box::new(consensus_state_incoming_index.clone()));
                let _ = prometheus::register(Box::new(consensus_state_discarded_index.clone()));
                let _ = prometheus::register(Box::new(consensus_state_block_statuses.clone()));
                let _ = prometheus::register(Box::new(
                    operation_cache_checked_operations_prefix.clone(),
                ));
                let _ =
                    prometheus::register(Box::new(endorsement_cache_checked_endorsements.clone()));
                let _ = prometheus::register(Box::new(endorsement_cache_known_by_peer.clone()));
                let _ = prometheus::register(Box::new(block_graph_counter.clone()));
                let _ = prometheus::register(Box::new(block_graph_ms.clone()));
                let _ = prometheus::register(Box::new(peernet_total_bytes_received.clone()));
                let _ = prometheus::register(Box::new(peernet_total_bytes_sent.clone()));
                let _ = prometheus::register(Box::new(operations_final_counter.clone()));
                let _ = prometheus::register(Box::new(stakers.clone()));
                let _ = prometheus::register(Box::new(rolls.clone()));
                let _ = prometheus::register(Box::new(know_peers.clone()));
                let _ = prometheus::register(Box::new(banned_peers.clone()));
                let _ = prometheus::register(Box::new(executed_final_slot.clone()));
                let _ = prometheus::register(Box::new(executed_final_slot_with_block.clone()));
                let _ = prometheus::register(Box::new(active_history.clone()));
                let _ = prometheus::register(Box::new(bootstrap_counter.clone()));
                let _ = prometheus::register(Box::new(bootstrap_success.clone()));
                let _ = prometheus::register(Box::new(bootstrap_failed.clone()));
                let _ = prometheus::register(Box::new(process_available_processors.clone()));
                let _ = prometheus::register(Box::new(operations_pool.clone()));
                let _ = prometheus::register(Box::new(endorsements_pool.clone()));
                let _ = prometheus::register(Box::new(denunciations_pool.clone()));
                let _ = prometheus::register(Box::new(protocol_tester_success.clone()));
                let _ = prometheus::register(Box::new(protocol_tester_failed.clone()));
                let _ = prometheus::register(Box::new(sc_messages_final.clone()));
                let _ = prometheus::register(Box::new(async_message_pool_size.clone()));

                stopper = server::bind_metrics(addr);
            }
        }

        (
            MassaMetrics {
                enabled,
                process_available_processors,
                consensus_vec,
                stakers,
                rolls,
                active_history,
                operations_pool,
                endorsements_pool,
                denunciations_pool,
                async_message_pool_size,
                sc_messages_final,
                bootstrap_counter,
                bootstrap_peers_success: bootstrap_success,
                bootstrap_peers_failed: bootstrap_failed,
                protocol_tester_success,
                protocol_tester_failed,
                protocol_known_peers: know_peers,
                protocol_banned_peers: banned_peers,
                executed_final_slot,
                executed_final_slot_with_block,
                peernet_total_bytes_received,
                peernet_total_bytes_sent,
                block_graph_counter,
                block_graph_ms,
                active_in_connections,
                active_out_connections,
                operations_final_counter,
                block_cache_checked_headers_size,
                block_cache_blocks_known_by_peer,
                operation_cache_checked_operations,
                operation_cache_checked_operations_prefix,
                operation_cache_ops_know_by_peer,
                consensus_state_active_index,
                consensus_state_active_index_without_ops,
                consensus_state_incoming_index,
                consensus_state_discarded_index,
                consensus_state_block_statuses,
                endorsement_cache_checked_endorsements,
                endorsement_cache_known_by_peer,
                // blocks_counter,
                // endorsements_counter,
                // operations_counter,
                active_cursor_thread,
                active_cursor_period,
                final_cursor_thread,
                final_cursor_period,
                peers_bandwidth: Arc::new(RwLock::new(HashMap::new())),
                tick_delay,
            },
            stopper,
        )
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn get_metrics_for_survey_thread(&self) -> (i64, i64, u64, u64) {
        (
            self.active_in_connections.clone().get(),
            self.active_out_connections.clone().get(),
            self.peernet_total_bytes_sent.clone().get(),
            self.peernet_total_bytes_received.clone().get(),
        )
    }

    pub fn set_active_connections(&self, in_connections: usize, out_connections: usize) {
        self.active_in_connections.set(in_connections as i64);
        self.active_out_connections.set(out_connections as i64);
    }

    pub fn set_active_cursor(&self, period: u64, thread: u8) {
        self.active_cursor_thread.set(thread as i64);
        self.active_cursor_period.set(period as i64);
    }

    pub fn set_final_cursor(&self, period: u64, thread: u8) {
        self.final_cursor_thread.set(thread as i64);
        self.final_cursor_period.set(period as i64);
    }

    pub fn set_consensus_period(&self, thread: usize, period: u64) {
        if let Some(g) = self.consensus_vec.get(thread) {
            g.set(period as f64);
        }
    }

    pub fn set_consensus_state(
        &self,
        active_index: usize,
        incoming_index: usize,
        discarded_index: usize,
        block_statuses: usize,
        active_index_without_ops: usize,
    ) {
        self.consensus_state_active_index.set(active_index as i64);
        self.consensus_state_incoming_index
            .set(incoming_index as i64);
        self.consensus_state_discarded_index
            .set(discarded_index as i64);
        self.consensus_state_block_statuses
            .set(block_statuses as i64);
        self.consensus_state_active_index_without_ops
            .set(active_index_without_ops as i64);
    }

    pub fn set_block_cache_metrics(&self, checked_header_size: usize, blocks_known_by_peer: usize) {
        self.block_cache_checked_headers_size
            .set(checked_header_size as i64);
        self.block_cache_blocks_known_by_peer
            .set(blocks_known_by_peer as i64);
    }

    pub fn set_operations_cache_metrics(
        &self,
        checked_operations: usize,
        checked_operations_prefix: usize,
        ops_know_by_peer: usize,
    ) {
        self.operation_cache_checked_operations
            .set(checked_operations as i64);
        self.operation_cache_checked_operations_prefix
            .set(checked_operations_prefix as i64);
        self.operation_cache_ops_know_by_peer
            .set(ops_know_by_peer as i64);
    }

    pub fn set_endorsements_cache_metrics(
        &self,
        checked_endorsements: usize,
        known_by_peer: usize,
    ) {
        self.endorsement_cache_checked_endorsements
            .set(checked_endorsements as i64);
        self.endorsement_cache_known_by_peer
            .set(known_by_peer as i64);
    }

    pub fn inc_block_graph_ms(&self, diff: u64) {
        self.block_graph_ms.inc_by(diff);
    }

    pub fn inc_block_graph_counter(&self) {
        self.block_graph_counter.inc();
    }

    pub fn set_peernet_total_bytes_received(&self, new_value: u64) {
        let diff = new_value.saturating_sub(self.peernet_total_bytes_received.get());
        self.peernet_total_bytes_received.inc_by(diff);
    }

    pub fn set_peernet_total_bytes_sent(&self, new_value: u64) {
        let diff = new_value.saturating_sub(self.peernet_total_bytes_sent.get());
        self.peernet_total_bytes_sent.inc_by(diff);
    }

    pub fn inc_operations_final_counter(&self, diff: u64) {
        self.operations_final_counter.inc_by(diff);
    }

    pub fn set_known_peers(&self, nb: usize) {
        self.protocol_known_peers.set(nb as i64);
    }

    pub fn set_banned_peers(&self, nb: usize) {
        self.protocol_banned_peers.set(nb as i64);
    }

    pub fn inc_executed_final_slot(&self) {
        self.executed_final_slot.inc();
    }

    pub fn inc_executed_final_slot_with_block(&self) {
        self.executed_final_slot_with_block.inc();
    }

    pub fn set_active_history(&self, nb: usize) {
        self.active_history.set(nb as i64);
    }

    pub fn inc_bootstrap_counter(&self) {
        self.bootstrap_counter.inc();
    }

    pub fn inc_bootstrap_peers_success(&self) {
        self.bootstrap_peers_success.inc();
    }

    pub fn inc_bootstrap_peers_failed(&self) {
        self.bootstrap_peers_failed.inc();
    }

    pub fn set_operations_pool(&self, nb: usize) {
        self.operations_pool.set(nb as i64);
    }

    pub fn set_endorsements_pool(&self, nb: usize) {
        self.endorsements_pool.set(nb as i64);
    }

    pub fn set_denunciations_pool(&self, nb: usize) {
        self.denunciations_pool.set(nb as i64);
    }

    pub fn inc_protocol_tester_success(&self) {
        self.protocol_tester_success.inc();
    }

    pub fn inc_protocol_tester_failed(&self) {
        self.protocol_tester_failed.inc();
    }

    pub fn set_stakers(&self, nb: usize) {
        self.stakers.set(nb as i64);
    }

    pub fn set_rolls(&self, nb: usize) {
        self.rolls.set(nb as i64);
    }

    pub fn inc_sc_messages_final_by(&self, diff: usize) {
        self.sc_messages_final.inc_by(diff as u64);
    }

    pub fn set_async_message_pool_size(&self, nb: usize) {
        self.async_message_pool_size.set(nb as i64);
    }

    pub fn set_available_processors(&self, nb: usize) {
        self.process_available_processors.set(nb as i64);
    }

    /// Update the bandwidth metrics for all peers
    /// HashMap<peer_id, (tx, rx)>
    pub fn update_peers_tx_rx(&self, data: HashMap<String, (u64, u64)>) {
        if self.enabled {
            let mut write = self.peers_bandwidth.write().unwrap();

            // metrics of peers that are not in the data HashMap are removed
            let missing_peer: Vec<String> = write
                .keys()
                .filter(|key| !data.contains_key(key.as_str()))
                .cloned()
                .collect();

            for key in missing_peer {
                // remove peer and unregister metrics
                if let Some((tx, rx)) = write.remove(&key) {
                    if let Err(e) = prometheus::unregister(Box::new(tx)) {
                        warn!("Failed to unregister tx metricfor peer {} : {}", key, e);
                    }

                    if let Err(e) = prometheus::unregister(Box::new(rx)) {
                        warn!("Failed to unregister rx metric for peer {} : {}", key, e);
                    }
                }
            }

            for (k, (tx_peernet, rx_peernet)) in data {
                if let Some((tx_metric, rx_metric)) = write.get_mut(&k) {
                    // peer metrics exist
                    // update tx and rx

                    let to_add = tx_peernet.saturating_sub(tx_metric.get());
                    tx_metric.inc_by(to_add);

                    let to_add = rx_peernet.saturating_sub(rx_metric.get());
                    rx_metric.inc_by(to_add);
                } else {
                    // peer metrics does not exist
                    let label_rx = format!("peer_total_bytes_receive_{}", k);
                    let label_tx = format!("peer_total_bytes_sent_{}", k);

                    let peer_total_bytes_receive =
                        IntCounter::new(label_rx, "total byte received by the peer").unwrap();

                    let peer_total_bytes_sent =
                        IntCounter::new(label_tx, "total byte sent by the peer").unwrap();

                    peer_total_bytes_sent.inc_by(tx_peernet);
                    peer_total_bytes_receive.inc_by(rx_peernet);

                    let _ = prometheus::register(Box::new(peer_total_bytes_receive.clone()));
                    let _ = prometheus::register(Box::new(peer_total_bytes_sent.clone()));

                    write.insert(k, (peer_total_bytes_sent, peer_total_bytes_receive));
                }
            }
        }
    }
}
