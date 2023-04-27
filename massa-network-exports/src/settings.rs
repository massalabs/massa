// Copyright (c) 2022 MASSA LABS <info@massa.net>

use enum_map::EnumMap;
use massa_time::MassaTime;
use serde::Deserialize;
use std::net::{IpAddr, SocketAddr};

use crate::peers::PeerType;

/// Network configuration
#[derive(Debug, Deserialize, Clone)]
pub struct NetworkConfig {
    /// Where to listen for communications.
    pub bind: SocketAddr,
    /// Our own IP if it is routable, else None.
    pub routable_ip: Option<IpAddr>,
    /// Protocol port
    pub protocol_port: u16,
    /// Time interval spent waiting for a response from a peer.
    /// In milliseconds
    pub connect_timeout: MassaTime,
    /// `Network_worker` will try to connect to available peers every `wakeup_interval`.
    /// In milliseconds
    pub wakeup_interval: MassaTime,
    /// Path to the file containing initial peers.
    pub initial_peers_file: std::path::PathBuf,
    /// Path to the file containing known peers.
    pub peers_file: std::path::PathBuf,
    /// Path to the file containing our keypair
    pub keypair_file: std::path::PathBuf,
    /// Configuration for `PeerType` connections
    pub peer_types_config: EnumMap<PeerType, PeerTypeConnectionConfig>,
    /// Limit on the number of in connections per ip.
    pub max_in_connections_per_ip: usize,
    /// Limit on the number of idle peers we remember.
    pub max_idle_peers: usize,
    /// Limit on the number of banned peers we remember.
    pub max_banned_peers: usize,
    /// Peer database is dumped every `peers_file_dump_interval` in milliseconds
    pub peers_file_dump_interval: MassaTime,
    /// After `message_timeout` milliseconds we are no longer waiting on handshake message
    pub message_timeout: MassaTime,
    /// Every `ask_peer_list_interval` in milliseconds we ask every one for its advertisable peers list.
    pub ask_peer_list_interval: MassaTime,
    /// Max wait time for sending a Node event.
    pub max_send_wait_node_event: MassaTime,
    /// Max wait time for sending a Network event.
    pub max_send_wait_network_event: MassaTime,
    /// Time after which we forget a node
    pub ban_timeout: MassaTime,
    /// Timeout Duration when we send a `PeerList` in handshake
    pub peer_list_send_timeout: MassaTime,
    /// Max number of in connection overflowed managed by the handshake that send a list of peers
    pub max_in_connection_overflow: usize,
    /// Max operations per message in the network to avoid sending to big data packet.
    pub max_operations_per_message: u32,
    /// Read limitation for a connection in bytes per seconds
    pub max_bytes_read: f64,
    /// Write limitation for a connection in bytes per seconds
    pub max_bytes_write: f64,
    /// Max number ids in ask blocks message
    pub max_ask_blocks: u32,
    /// Max operations per block
    pub max_operations_per_block: u32,
    /// Thread count
    pub thread_count: u8,
    /// Endorsement count
    pub endorsement_count: u32,
    /// Max peer advertise length
    pub max_peer_advertise_length: u32,
    /// Max endorsements per message
    pub max_endorsements_per_message: u32,
    /// Max message size
    pub max_message_size: u32,
    /// Maximum length of a datastore value
    pub max_datastore_value_length: u64,
    /// Maximum entry in an operation datastore
    pub max_op_datastore_entry_count: u64,
    /// Maximum length of an operation datastore key
    pub max_op_datastore_key_length: u8,
    /// Maximum length of an operation datastore value
    pub max_op_datastore_value_length: u64,
    /// Maximum length function name in call SC
    pub max_function_name_length: u16,
    /// Maximum size of parameters in call SC
    pub max_parameters_size: u32,
    /// Controller channel size
    pub controller_channel_size: usize,
    /// Event channel size
    pub event_channel_size: usize,
    /// Node command channel size
    pub node_command_channel_size: usize,
    /// Node event channel size
    pub node_event_channel_size: usize,
    /// last start period, used in message deserialization
    pub last_start_period: u64,
    /// max denunciations in block header
    pub max_denunciations_per_block_header: u32,
}

/// Connection configuration for a peer type
/// Limit the current connections for a given peer type as a whole
#[derive(Debug, Deserialize, Clone, Default)]
pub struct PeerTypeConnectionConfig {
    /// max number of incoming connection
    pub max_in_connections: usize,
    /// target number of outgoing connections
    pub target_out_connections: usize,
    /// max number of on going outgoing connection attempt
    pub max_out_attempts: usize,
}

/// setting tests
#[cfg(feature = "testing")]
pub mod tests {
    use crate::NetworkConfig;
    use crate::{test_exports::tools::get_temp_keypair_file, PeerType};
    use enum_map::enum_map;
    use massa_models::config::{
        ENDORSEMENT_COUNT, MAX_ADVERTISE_LENGTH, MAX_ASK_BLOCKS_PER_MESSAGE,
        MAX_DATASTORE_VALUE_LENGTH, MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
        MAX_ENDORSEMENTS_PER_MESSAGE, MAX_FUNCTION_NAME_LENGTH, MAX_MESSAGE_SIZE,
        MAX_OPERATIONS_PER_MESSAGE, MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        MAX_OPERATION_DATASTORE_KEY_LENGTH, MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        MAX_PARAMETERS_SIZE, NETWORK_CONTROLLER_CHANNEL_SIZE, NETWORK_EVENT_CHANNEL_SIZE,
        NETWORK_NODE_COMMAND_CHANNEL_SIZE, NETWORK_NODE_EVENT_CHANNEL_SIZE, THREAD_COUNT,
    };
    use massa_time::MassaTime;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use super::PeerTypeConnectionConfig;

    impl Default for NetworkConfig {
        fn default() -> Self {
            let peer_types_config = enum_map! {
                PeerType::Bootstrap => PeerTypeConnectionConfig {
                    target_out_connections: 1,
                    max_out_attempts: 1,
                    max_in_connections: 1,
                },
                PeerType::WhiteListed => PeerTypeConnectionConfig {
                    target_out_connections: 2,
                    max_out_attempts: 2,
                    max_in_connections: 3,
                },
                PeerType::Standard => PeerTypeConnectionConfig {
                    target_out_connections: 10,
                    max_out_attempts: 15,
                    max_in_connections: 5,
                }
            };
            NetworkConfig {
                bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
                routable_ip: Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
                protocol_port: 0,
                connect_timeout: MassaTime::from_millis(180_000),
                wakeup_interval: MassaTime::from_millis(10_000),
                peers_file: std::path::PathBuf::new(),
                max_in_connections_per_ip: 2,
                max_idle_peers: 3,
                max_banned_peers: 3,
                peers_file_dump_interval: MassaTime::from_millis(10_000),
                message_timeout: MassaTime::from_millis(5000u64),
                ask_peer_list_interval: MassaTime::from_millis(50000u64),
                keypair_file: std::path::PathBuf::new(),
                max_send_wait_node_event: MassaTime::from_millis(100),
                max_send_wait_network_event: MassaTime::from_millis(100),
                ban_timeout: MassaTime::from_millis(100_000_000),
                initial_peers_file: std::path::PathBuf::new(),
                peer_list_send_timeout: MassaTime::from_millis(500),
                max_in_connection_overflow: 2,
                peer_types_config,
                max_operations_per_message: MAX_OPERATIONS_PER_MESSAGE,
                max_bytes_read: std::f64::INFINITY,
                max_bytes_write: std::f64::INFINITY,
                max_ask_blocks: MAX_ASK_BLOCKS_PER_MESSAGE,
                endorsement_count: ENDORSEMENT_COUNT,
                max_endorsements_per_message: MAX_ENDORSEMENTS_PER_MESSAGE,
                max_operations_per_block: MAX_OPERATIONS_PER_MESSAGE,
                max_peer_advertise_length: MAX_ADVERTISE_LENGTH,
                thread_count: THREAD_COUNT,
                max_message_size: MAX_MESSAGE_SIZE,
                max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
                max_op_datastore_entry_count: MAX_OPERATION_DATASTORE_ENTRY_COUNT,
                max_op_datastore_key_length: MAX_OPERATION_DATASTORE_KEY_LENGTH,
                max_op_datastore_value_length: MAX_OPERATION_DATASTORE_VALUE_LENGTH,
                max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
                max_parameters_size: MAX_PARAMETERS_SIZE,
                controller_channel_size: NETWORK_CONTROLLER_CHANNEL_SIZE,
                event_channel_size: NETWORK_EVENT_CHANNEL_SIZE,
                node_command_channel_size: NETWORK_NODE_COMMAND_CHANNEL_SIZE,
                node_event_channel_size: NETWORK_NODE_EVENT_CHANNEL_SIZE,
                last_start_period: 0,
                max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            }
        }
    }

    impl NetworkConfig {
        /// default network settings from port and peer file path
        pub fn scenarios_default(port: u16, peers_file: &std::path::Path) -> Self {
            let peer_types_config = enum_map! {
                PeerType::Bootstrap => PeerTypeConnectionConfig {
                    target_out_connections: 1,
                    max_out_attempts: 1,
                    max_in_connections: 1,
                },
                PeerType::WhiteListed => PeerTypeConnectionConfig {
                    target_out_connections: 2,
                    max_out_attempts: 2,
                    max_in_connections: 3,
                },
                PeerType::Standard => PeerTypeConnectionConfig {
                    target_out_connections: 10,
                    max_out_attempts: 15,
                    max_in_connections: 5,
                }
            };
            let bind = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port);
            let routable_ip = Some(IpAddr::V4(Ipv4Addr::new(200, 200, 200, 200)));
            Self {
                bind,
                routable_ip,
                protocol_port: port,
                connect_timeout: MassaTime::from_millis(3000),
                peers_file: peers_file.to_path_buf(),
                wakeup_interval: MassaTime::from_millis(3000),
                max_in_connections_per_ip: 100,
                max_idle_peers: 100,
                max_banned_peers: 100,
                peers_file_dump_interval: MassaTime::from_millis(30000),
                message_timeout: MassaTime::from_millis(5000u64),
                ask_peer_list_interval: MassaTime::from_millis(50000u64),
                keypair_file: get_temp_keypair_file().path().to_path_buf(),
                max_send_wait_node_event: MassaTime::from_millis(100),
                max_send_wait_network_event: MassaTime::from_millis(100),
                ban_timeout: MassaTime::from_millis(100_000_000),
                initial_peers_file: peers_file.to_path_buf(),
                peer_list_send_timeout: MassaTime::from_millis(50),
                max_in_connection_overflow: 10,
                peer_types_config,
                max_operations_per_message: MAX_OPERATIONS_PER_MESSAGE,
                max_bytes_read: std::f64::INFINITY,
                max_bytes_write: std::f64::INFINITY,
                max_ask_blocks: 10,
                endorsement_count: 8,
                max_endorsements_per_message: MAX_ENDORSEMENTS_PER_MESSAGE,
                max_operations_per_block: MAX_OPERATIONS_PER_MESSAGE,
                max_peer_advertise_length: 128,
                thread_count: THREAD_COUNT,
                max_message_size: MAX_MESSAGE_SIZE,
                max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
                max_op_datastore_entry_count: MAX_OPERATION_DATASTORE_ENTRY_COUNT,
                max_op_datastore_key_length: MAX_OPERATION_DATASTORE_KEY_LENGTH,
                max_op_datastore_value_length: MAX_OPERATION_DATASTORE_VALUE_LENGTH,
                max_function_name_length: MAX_FUNCTION_NAME_LENGTH,
                max_parameters_size: MAX_PARAMETERS_SIZE,
                controller_channel_size: NETWORK_CONTROLLER_CHANNEL_SIZE,
                event_channel_size: NETWORK_EVENT_CHANNEL_SIZE,
                node_command_channel_size: NETWORK_NODE_COMMAND_CHANNEL_SIZE,
                node_event_channel_size: NETWORK_NODE_EVENT_CHANNEL_SIZE,
                last_start_period: 0,
                max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            }
        }
    }
}
