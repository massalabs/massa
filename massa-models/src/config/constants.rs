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

use crate::{address::ADDRESS_SIZE_BYTES, amount::Amount, version::Version};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use num::rational::Ratio;

/// Limit on the number of peers we advertise to others.
pub const MAX_ADVERTISE_LENGTH: u32 = 10000;
/// Maximum message length in bytes
pub const MAX_MESSAGE_SIZE: u32 = 1048576000;
/// Max number of hash in the message `AskForBlocks`
pub const MAX_ASK_BLOCKS_PER_MESSAGE: u32 = 128;
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
    pub static ref GENESIS_TIMESTAMP: MassaTime = if cfg!(feature = "sandbox") {
        std::env::var("GENESIS_TIMESTAMP").map(|timestamp| timestamp.parse::<u64>().unwrap().into()).unwrap_or_else(|_|
            MassaTime::now(0)
                .unwrap()
                .saturating_add(MassaTime::from_millis(1000 * 10))
        )
    } else {
        1669852801000.into()  // Thursday, December 01, 2022 00:00:01 AM UTC
    };

    /// TESTNET: time when the blockclique is ended.
    pub static ref END_TIMESTAMP: Option<MassaTime> = if cfg!(feature = "sandbox") {
        None
    } else {
        Some(1672466400000.into())  // Saturday, December 31, 2022 6:00:00 PM UTC
    };
    /// `KeyPair` to sign genesis blocks.
    pub static ref GENESIS_KEY: KeyPair = KeyPair::from_str("S1UxdCJv5ckDK8z87E5Jq5fEfSVLi2cTHgtpfZy7iURs3KpPns8")
        .unwrap();
    /// number of cycle misses (strictly) above which stakers are deactivated
    pub static ref POS_MISS_RATE_DEACTIVATION_THRESHOLD: Ratio<u64> = Ratio::new(7, 10);
    /// node version
    pub static ref VERSION: Version = {
        if cfg!(feature = "sandbox") {
            "SAND.0.0"
        } else {
            "TEST.17.2"
        }
        .parse()
        .unwrap()
    };
}

/// Price of a roll in the network
pub const ROLL_PRICE: Amount = Amount::from_mantissa_scale(100, 0);
/// Block reward is given for each block creation
pub const BLOCK_REWARD: Amount = Amount::from_mantissa_scale(3, 1);
/// Cost to store one byte in the ledger
pub const LEDGER_COST_PER_BYTE: Amount = Amount::from_mantissa_scale(25, 5);
/// Cost for a base entry (address + balance (5 bytes constant))
pub const LEDGER_ENTRY_BASE_SIZE: usize = ADDRESS_SIZE_BYTES + 8;
/// Cost for a base entry datastore 10 bytes constant to avoid paying more for longer keys
pub const LEDGER_ENTRY_DATASTORE_BASE_SIZE: usize = 10;
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
pub const MAX_BLOCK_SIZE: u32 = 500_000;
/// Maximum capacity of the asynchronous messages pool
pub const MAX_ASYNC_POOL_LENGTH: u64 = 10_000;
/// Maximum data size in async message
pub const MAX_ASYNC_MESSAGE_DATA: u64 = 1_000_000;
/// Maximum operation validity period count
pub const OPERATION_VALIDITY_PERIODS: u64 = 10;
/// cycle duration in periods
pub const PERIODS_PER_CYCLE: u64 = 128;
/// PoS saved cycles: number of cycles saved in `PoSFinalState`
///
/// 4 for PoS itself and 1 for bootstrap safety
pub const POS_SAVED_CYCLES: usize = 5;
/// Maximum size batch of data in a part of the ledger
pub const LEDGER_PART_SIZE_MESSAGE_BYTES: u64 = 1_000_000;
/// Maximum async messages in a batch of the bootstrap of the async pool
pub const ASYNC_POOL_BOOTSTRAP_PART_SIZE: u64 = 100;
/// Maximum proof-of-stake deferred credits in a bootstrap batch
pub const DEFERRED_CREDITS_BOOTSTRAP_PART_SIZE: u64 = 100;
/// Maximum executed ops per slot in a bootstrap batch
pub const EXECUTED_OPS_BOOTSTRAP_PART_SIZE: u64 = 10;
/// Maximum number of consensus blocks in a bootstrap batch
pub const CONSENSUS_BOOTSTRAP_PART_SIZE: u64 = 50;
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
pub const MAX_BOOTSTRAP_MESSAGE_SIZE: u32 = 1048576000;
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
/// Max number of address and random entries for PoS bootstrap
pub const MAX_BOOTSTRAP_POS_ENTRIES: u32 = 1000000000;
/// Max async pool changes
pub const MAX_BOOTSTRAP_ASYNC_POOL_CHANGES: u64 = 100_000;
/// Max bytes in final states parts
pub const MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE: u64 = 1_000_000_000;
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
/// Pool controller channel size
pub const POOL_CONTROLLER_CHANNEL_SIZE: usize = 1024;

// ***********************
// Constants used for execution module (injected from ConsensusConfig)
//

/// Maximum of GAS allowed for a block
pub const MAX_GAS_PER_BLOCK: u64 = 1_000_000_000;
/// Maximum of GAS allowed for asynchronous messages execution on one slot
pub const MAX_ASYNC_GAS: u64 = 1_000_000_000;

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

// Some checks at compile time that should not be ignored!
#[allow(clippy::assertions_on_constants)]
const _: () = {
    assert!(THREAD_COUNT > 1);
    assert!((T0).to_millis() >= 1);
    assert!((T0).to_millis() % (THREAD_COUNT as u64) == 0);
};
