//! DEFAULT VALUES USED TO INITIALIZE DIVERS CONFIGURATIONS STRUCTURES
//!
//!
//! # Default hard-coded
//!
//! Each crates may contains a `settings.rs` or a `config.rs` the `Default`
//! implementation of each object take the default Values from the following
//! file.
//!
//! These values are the hard-coded values that make sens to never be modified
//! by a user. Generally, this values are passed with dependency injection in a `cfg`
//! parameter for each worker, that is convenient for unit tests.
//!
//! A parallel file with the same constant definitions exist for the testing case.
//! (`default_testing.rs`) But as for the current file you shouldn't modify it.
use std::str::FromStr;

use crate::{amount::Amount, serialization::u32_be_bytes_min_length, version::Version};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use num::rational::Ratio;

/// IMPORTANNT TODO: should be removed after the bootstrap messages refacto
pub const SIGNATURE_DESER_SIZE: usize = 64 + 1;

/// Limit on the number of peers we advertise to others.
pub const MAX_ADVERTISE_LENGTH: u32 = 10000;
/// Maximum message length in bytes
pub const MAX_MESSAGE_SIZE: u32 = 1048576000;
/// Max number of operations per message
pub const MAX_OPERATIONS_PER_MESSAGE: u32 = 1024;
/// Length of the handshake random signature
pub const HANDSHAKE_RANDOMNESS_SIZE_BYTES: usize = 32;

/// Consensus static parameters (defined by protocol used)
/// Changing one of the following values is considered as a breaking change
/// Values differ in `test` flavor building for faster CI and simpler scenarios
pub const CHANNEL_SIZE: usize = 1024;

lazy_static::lazy_static! {
    /// Time in milliseconds when the blockclique started.
    /// In sandbox mode, the value depends on starting time and on the --restart-from-snapshot-at-period argument in CLI,
    /// so that the network starts or restarts 10 seconds after launch
    pub static ref GENESIS_TIMESTAMP: MassaTime = if cfg!(feature = "sandbox") {
        std::env::var("GENESIS_TIMESTAMP").map(|timestamp| MassaTime::from_millis(timestamp.parse::<u64>().unwrap())).unwrap_or_else(|_|
            MassaTime::now()
                .saturating_sub(
                    T0.checked_mul(get_period_from_args()).unwrap()
                )
                .saturating_add(MassaTime::from_millis(1000 * 10)
            )
        )
    } else {
        MassaTime::from_millis(1705312800000) // Monday, January 15, 2024 10:00:00 AM UTC
    };

    /// TESTNET: time when the blockclique is ended.
    pub static ref END_TIMESTAMP: Option<MassaTime> =
    #[allow(clippy::if_same_then_else)]
    if cfg!(feature = "sandbox") {
        None
    } else {
        None
    };
    /// `KeyPair` to sign genesis blocks.
    pub static ref GENESIS_KEY: KeyPair = KeyPair::from_str("S1UxdCJv5ckDK8z87E5Jq5fEfSVLi2cTHgtpfZy7iURs3KpPns8")
        .unwrap();
    /// number of cycle misses (strictly) above which stakers are deactivated
    pub static ref POS_MISS_RATE_DEACTIVATION_THRESHOLD: Ratio<u64> = Ratio::new(7, 10);
    /// node version
    pub static ref VERSION: Version = {
        if cfg!(feature = "sandbox") {
            "SAND.2.4"
        } else {
            "MAIN.2.4"
        }
        .parse()
        .unwrap()
    };
    /// node chain id (to avoid replay attacks)
    ///
    /// By signing an operation with an ID of the target chain
    /// (e.g. whether the operation targets LABNET, BUILDNET or MAINNET), the user is protected from
    /// a malicious actor that could steal operations created on BUILDNET / LABNET and try to replay
    /// them on the Massa MAINNET (and potentially stealing real coins)
    /// Chain id idea and implementation come from Ethereum EIPs:
    /// * https://github.com/ethereum/EIPs/blob/master/EIPS/eip-155.md
    /// * https://eips.ethereum.org/EIPS/eip-1344
    /// Chain id can be queried in Smart contracts (AssemblyScript: chain_id call) and in the
    /// jsonrpc get_status call
    pub static ref CHAINID: u64 = {
        // MASM (MainNet):           77658377
        // MASS (SecureNet):         77658383
        // MASB (BuildNet):          77658366
        // MASL (Labnet):            77658376
        // SANDBOX (Sandbox):        77
        match VERSION.to_string() {
            // Sandbox
            s if s.starts_with("SAND") => 77,
            // BuildNet
            s if s.starts_with("DEVN") => 77658366,
            // SecureNet
            s if s.starts_with("SECU") => 77658383,
            // MainNet
            s if s.starts_with("MAIN") => 77658377,
            // LabNet
            s if s.starts_with("LABN") => 77658376,
            _ => {
                panic!("Unhandled VERSION ({}), cannot compute chain id", *VERSION);
            }
        }
    };
}

/// Helper function to parse args for lazy_static evaluations
pub fn get_period_from_args() -> u64 {
    let mut last_start_period = 0;
    let mut parse_next = false;
    for args in std::env::args() {
        if parse_next {
            last_start_period = u64::from_str(&args).unwrap_or_default();
            break;
        }
        parse_next = args == *"--restart-from-snapshot-at-period";
    }
    last_start_period
}

/// Price of a roll in the network
pub const ROLL_PRICE: Amount = Amount::const_init(100, 0);
/// Block reward is given for each block creation
pub const BLOCK_REWARD: Amount = Amount::const_init(102, 2);
/// Cost to store one byte in the ledger
pub const LEDGER_COST_PER_BYTE: Amount = Amount::const_init(1, 4);
/// Cost for a base entry default 0.01 MASSA
pub const LEDGER_ENTRY_BASE_COST: Amount = Amount::const_init(1, 3);
/// Base size of a empty datastore entry (not counting the key nor the value)
pub const LEDGER_ENTRY_DATASTORE_BASE_SIZE: usize = 4;
/// Time between the periods in the same thread.
pub const T0: MassaTime = MassaTime::from_millis(16000);
/// Proof of stake seed for the initial draw
pub const INITIAL_DRAW_SEED: &str = "massa_genesis_seed";
/// Number of threads
pub const THREAD_COUNT: u8 = 32;
/// Number of endorsement
pub const ENDORSEMENT_COUNT: u32 = 16;
/// Threshold for fitness.
pub const DELTA_F0: u64 = 64 * (ENDORSEMENT_COUNT as u64 + 1);
/// Maximum number of operations per block
pub const MAX_OPERATIONS_PER_BLOCK: u32 = 5000;
/// Maximum block size in bytes
pub const MAX_BLOCK_SIZE: u32 = 300_000;
/// Maximum capacity of the asynchronous messages pool
pub const MAX_ASYNC_POOL_LENGTH: u64 = 1_000;
/// Maximum operation validity period count
pub const OPERATION_VALIDITY_PERIODS: u64 = 10;
/// Number of periods of executed operation and denunciation history to keep
pub const KEEP_EXECUTED_HISTORY_EXTRA_PERIODS: u64 = 10;
/// cycle duration in periods
pub const PERIODS_PER_CYCLE: u64 = 128;

/// Number of cycles saved in `PoSFinalState`
///
/// 6 for PoS itself so we can check denuncations on selections at C-2 after a bootstrap
/// See https://github.com/massalabs/massa/pull/3871
/// 1 for pruned cycle safety during bootstrap
pub const POS_SAVED_CYCLES: usize = 7;
/// Number of cycle draws saved in the selector cache
///
/// 5 to have a C-2 to C+2 range (6 cycles post-bootstrap give 5 cycle draws)
/// 1 for margin
pub const SELECTOR_DRAW_CACHE_SIZE: usize = 6;
/// Maximum number of consensus blocks in a bootstrap batch
pub const CONSENSUS_BOOTSTRAP_PART_SIZE: u64 = 50;
/// Maximum number of consensus block ids when sending a bootstrap cursor from the client
pub const MAX_CONSENSUS_BLOCKS_IDS: u64 = 300;
/// Maximum size of proof-of-stake rolls
pub const MAX_ROLLS_COUNT_LENGTH: u64 = 10_000;
/// Maximum size of proof-of-stake production stats
pub const MAX_PRODUCTION_STATS_LENGTH: u64 = 10_000;
/// Maximum size proof-of-stake deferred credits
pub const MAX_DEFERRED_CREDITS_LENGTH: u64 = 10_000;
/// Maximum size of executed ops
pub const MAX_EXECUTED_OPS_LENGTH: u64 = 1_000;
/// Maximum size of executed ops changes
pub const MAX_EXECUTED_OPS_CHANGES_LENGTH: u64 = 20_000;
/// Maximum length of a datastore key
pub const MAX_DATASTORE_KEY_LENGTH: u8 = 255;
/// Maximum length of an operation datastore key
pub const MAX_OPERATION_DATASTORE_KEY_LENGTH: u8 = MAX_DATASTORE_KEY_LENGTH;
/// Maximum length of a datastore value
pub const MAX_DATASTORE_VALUE_LENGTH: u64 = 10_000_000;
/// Maximum length of a datastore value
pub const MAX_BYTECODE_LENGTH: u64 = 10_000_000;
/// Maximum length of an operation datastore value
pub const MAX_OPERATION_DATASTORE_VALUE_LENGTH: u64 = 500_000;
/// Maximum ledger changes in a block
pub const MAX_LEDGER_CHANGES_PER_SLOT: u32 = u32::MAX;
/// Maximum production events in a block
pub const MAX_PRODUCTION_EVENTS_PER_BLOCK: u32 = u32::MAX;
/// Maximum ledger changes count
pub const MAX_LEDGER_CHANGES_COUNT: u64 =
    100_u32.saturating_mul(MAX_LEDGER_CHANGES_PER_SLOT) as u64;
/// Maximum number of key/values in the datastore of a ledger entry
pub const MAX_DATASTORE_ENTRY_COUNT: u64 = u64::MAX;
/// Maximum number of key/values in the datastore of a `ExecuteSC` operation
pub const MAX_OPERATION_DATASTORE_ENTRY_COUNT: u64 = 128;
/// Maximum length function name in call SC
pub const MAX_FUNCTION_NAME_LENGTH: u16 = u16::MAX;
/// Maximum size of parameters in call SC
pub const MAX_PARAMETERS_SIZE: u32 = 10_000_000;
/// Maximum length of `rng_seed` in thread cycle
pub const MAX_RNG_SEED_LENGTH: u32 = PERIODS_PER_CYCLE.saturating_mul(THREAD_COUNT as u64) as u32;
// ***********************
// Bootstrap constants
//

/// Max message size for bootstrap
/// Note: Update sizes are not limited, the 190Mb constant is to take them into account.
pub const MAX_BOOTSTRAP_MESSAGE_SIZE: u32 = MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE
    .saturating_add(MAX_BOOTSTRAP_VERSIONING_ELEMENTS_SIZE)
    .saturating_add(190_000_000_u32);
/// The number of bytes needed to encode [`MAX_BOOTSTRAP_MESSAGE_SIZE`]
pub const MAX_BOOTSTRAP_MESSAGE_SIZE_BYTES: usize =
    u32_be_bytes_min_length(MAX_BOOTSTRAP_MESSAGE_SIZE);
/// Max number of blocks we provide/ take into account while bootstrapping
pub const MAX_BOOTSTRAP_BLOCKS: u32 = 1000000;
/// max bootstrapped cliques
pub const MAX_BOOTSTRAP_CLIQUES: u32 = 1000;
/// max bootstrapped dependencies
pub const MAX_BOOTSTRAP_DEPS: u32 = 1000;
/// Max number of child nodes
pub const MAX_BOOTSTRAP_CHILDREN: u32 = 1000;
/// Max number of cycles in PoS bootstrap
pub const MAX_BOOTSTRAP_POS_CYCLES: u32 = 5;
/// Max async pool changes
pub const MAX_BOOTSTRAP_ASYNC_POOL_CHANGES: u64 = 100_000;
/// Max bytes in final states parts
pub const MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE: u32 = 100_000_000;
/// Max bytes in final states parts
pub const MAX_BOOTSTRAP_VERSIONING_ELEMENTS_SIZE: u32 = 10_000_000;
/// Max size of the IP list
pub const IP_LIST_MAX_SIZE: usize = 10000;
/// Size of the random bytes array used for the bootstrap, safe to import
pub const BOOTSTRAP_RANDOMNESS_SIZE_BYTES: usize = 32;
/// Max size of the printed error
pub const MAX_BOOTSTRAP_ERROR_LENGTH: u64 = 10000;

/// Protocol controller channel size
pub const PROTOCOL_CONTROLLER_CHANNEL_SIZE: usize = 1024;
/// Protocol event channel size
pub const PROTOCOL_EVENT_CHANNEL_SIZE: usize = 1024;
/// Pool controller operations channel size
pub const POOL_CONTROLLER_OPERATIONS_CHANNEL_SIZE: usize = 1024;
/// Pool controller endorsements channel size
pub const POOL_CONTROLLER_ENDORSEMENTS_CHANNEL_SIZE: usize = 1024;
/// Pool controller denunciations channel size
pub const POOL_CONTROLLER_DENUNCIATIONS_CHANNEL_SIZE: usize = 1024;

// ***********************
// Constants used for execution module (injected from ConsensusConfig)
//

/// Maximum of GAS allowed for a block
pub const MAX_GAS_PER_BLOCK: u64 = u32::MAX as u64;
/// Maximum of GAS allowed for asynchronous messages execution on one slot
pub const MAX_ASYNC_GAS: u64 = 1_000_000_000;
/// Constant cost applied to asynchronous messages (to take into account some costs related to snapshot)
pub const ASYNC_MSG_CST_GAS_COST: u64 = 750_000;
/// Gas used by a base operation (transaction, roll buy, roll sell)
pub const BASE_OPERATION_GAS_COST: u64 = 800_000; // approx MAX_GAS_PER_BLOCK / MAX_OPERATIONS_PER_BLOCK
/// Maximum event size in bytes
pub const MAX_EVENT_DATA_SIZE: usize = 50_000;

//
// Constants used in network
//

/// Max number of endorsements per message
pub const MAX_ENDORSEMENTS_PER_MESSAGE: u32 = 1024;
/// node send channel size
pub const NODE_SEND_CHANNEL_SIZE: usize = 10_000;
/// max duplex buffer size
pub const MAX_DUPLEX_BUFFER_SIZE: usize = 1024;
/// network controller communication channel size
pub const NETWORK_CONTROLLER_CHANNEL_SIZE: usize = 10_000;
/// network event channel size
pub const NETWORK_EVENT_CHANNEL_SIZE: usize = 10_000;
/// network node command channel size
pub const NETWORK_NODE_COMMAND_CHANNEL_SIZE: usize = 10_000;
/// network node event channel size
pub const NETWORK_NODE_EVENT_CHANNEL_SIZE: usize = 10_000;

//
// Constants used in protocol
//
/// Maximum of time we keep the operations in the storage of the propagation thread
pub const MAX_OPERATION_STORAGE_TIME: MassaTime = MassaTime::from_millis(60000);
/// Maximum size of channel used for commands in retrieval thread of operations
pub const MAX_SIZE_CHANNEL_COMMANDS_RETRIEVAL_OPERATIONS: usize = 10000;
/// Maximum size of channel used for commands in propagation thread of operations
pub const MAX_SIZE_CHANNEL_COMMANDS_PROPAGATION_OPERATIONS: usize = 10000;
/// Maximum size of channel used for commands in retrieval thread of block
pub const MAX_SIZE_CHANNEL_COMMANDS_RETRIEVAL_BLOCKS: usize = 10000;
/// Maximum size of channel used for commands in propagation thread of block
pub const MAX_SIZE_CHANNEL_COMMANDS_PROPAGATION_BLOCKS: usize = 10000;
/// Maximum size of channel used for commands in retrieval thread of endorsements
pub const MAX_SIZE_CHANNEL_COMMANDS_RETRIEVAL_ENDORSEMENTS: usize = 10000;
/// Maximum size of channel used for commands in propagation thread of endorsements
pub const MAX_SIZE_CHANNEL_COMMANDS_PROPAGATION_ENDORSEMENTS: usize = 10000;
/// Maximum size of channel used for commands in connectivity thread
pub const MAX_SIZE_CHANNEL_COMMANDS_CONNECTIVITY: usize = 10000;
/// Maximum size of channel used for commands in peers management thread
pub const MAX_SIZE_CHANNEL_COMMANDS_PEERS: usize = 10000;
/// Maximum size of channel used for commands in peer testers thread
pub const MAX_SIZE_CHANNEL_COMMANDS_PEER_TESTERS: usize = 10000;
/// Maximum size of channel used to send network events to the operation handler
pub const MAX_SIZE_CHANNEL_NETWORK_TO_OPERATION_HANDLER: usize = 10000;
/// Maximum size of channel used to send network events to the block handler
pub const MAX_SIZE_CHANNEL_NETWORK_TO_BLOCK_HANDLER: usize = 10000;
/// Maximum size of channel used to send network events to the endorsement handler
pub const MAX_SIZE_CHANNEL_NETWORK_TO_ENDORSEMENT_HANDLER: usize = 10000;
/// Maximum size of channel used to send network events to the peer handler
pub const MAX_SIZE_CHANNEL_NETWORK_TO_PEER_HANDLER: usize = 10000;
/// Maximum number of peer in a announcement list of peer
pub const MAX_PEERS_IN_ANNOUNCEMENT_LIST: u64 = 100;
/// Maximum number of listeners for a peer
pub const MAX_LISTENERS_PER_PEER: u64 = 100;
//
// Constants used in versioning
//
/// Threshold to accept a new versioning
pub const VERSIONING_THRESHOLD_TRANSITION_ACCEPTED: Ratio<u64> = Ratio::new_raw(75, 100);
/// Block count to process in MipStoreStats (for state change threshold)
pub const MIP_STORE_STATS_BLOCK_CONSIDERED: usize = 1000;
/// Minimum value allowed for activation delay (in MIP info)
pub const VERSIONING_ACTIVATION_DELAY_MIN: MassaTime = T0.saturating_mul(PERIODS_PER_CYCLE);

//
// Constants for denunciation factory
//

/// denunciation expiration delta
pub const DENUNCIATION_EXPIRE_PERIODS: u64 = PERIODS_PER_CYCLE;
/// Max number of denunciations that can be included in a block header
pub const MAX_DENUNCIATIONS_PER_BLOCK_HEADER: u32 = 128;
/// Number of roll to remove per denunciation
pub const ROLL_COUNT_TO_SLASH_ON_DENUNCIATION: u64 = 1;
/// Maximum size of executed denunciations
pub const MAX_DENUNCIATION_CHANGES_LENGTH: u64 = 1_000;

// Some checks at compile time that should not be ignored!
#[allow(clippy::assertions_on_constants)]
const _: () = {
    assert!(THREAD_COUNT > 1);
    assert!((T0).as_millis() >= 1);
    assert!((T0).as_millis() % (THREAD_COUNT as u64) == 0);
};
