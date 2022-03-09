//! DEFAULT VALUES USED TO INITIALISE DIVERS CONFIGURATIONS STRUCTURES
//!
//!
//! # Default hardcoded
//!
//! Each crates may contains a `settings.rs` or a `config.rs` the `Default`
//! implementation of each object take the default Values from the following
//! file.
//!
//! These values are the hardcoded values that make sens to never be modifyed
//! by a user. Generally, this values are passed with dependency injection in a `cfg`
//! parameter for each worker, that is conveniant for unit tests.
//!
//! A parralell file with the same const definitions exist for the testing case.
//! (`default_testing.rs`) But as for the current file you shouldn't modify it.
use crate::{Amount, Version};
use massa_signature::PrivateKey;
use massa_time::MassaTime;
use num::rational::Ratio;

/// Limit on the number of peers we advertise to others.
pub const MAX_ADVERTISE_LENGTH: u32 = 10000;
/// Maximum message length in bytes
pub const MAX_MESSAGE_SIZE: u32 = 1048576000;
/// Max number of hash in the message AskForBlocks
pub const MAX_ASK_BLOCKS_PER_MESSAGE: u32 = 128;
/// Max number of operations per message
pub const MAX_OPERATIONS_PER_MESSAGE: u32 = 1024;
/// Length of the handshake random signature
pub const HANDSHAKE_RANDOMNESS_SIZE_BYTES: usize = 32;

// Consensus static parameters (defined by protocol used)
// Changing one of the following values is considered as a breaking change
// Values differ in `test` flavor building for faster CI and simpler scenarios
pub const CHANNEL_SIZE: usize = 256;

lazy_static::lazy_static! {
    /// Time in millis when the blockclique started.
    pub static ref GENESIS_TIMESTAMP: MassaTime = if cfg!(feature = "sandbox") {
        MassaTime::now()
            .unwrap()
            .saturating_add(MassaTime::from(1000 * 60 * 3))
    } else {
        1646334000000.into()
    };

    /// TESTNET: time when the blockclique is ended.
    pub static ref END_TIMESTAMP: Option<MassaTime> = if cfg!(feature = "sandbox") {
        None
    } else {
        Some(1648764000000.into())
    };
    /// Private_key to sign genesis blocks.
    pub static ref GENESIS_KEY: PrivateKey = "SGoTK5TJ9ZcCgQVmdfma88UdhS6GK94aFEYAsU3F1inFayQ6S"
        .parse()
        .unwrap();
    /// number of cycle misses (strictly) above which stakers are deactivated
    pub static ref POS_MISS_RATE_DEACTIVATION_THRESHOLD: Ratio<u64> = Ratio::new(7, 10);
    pub static ref VERSION: Version = {
        if cfg!(feature = "sandbox") {
            "SAND.0.0"
        } else {
            "TEST.8.0"
        }
        .parse()
        .unwrap()
    };
}

/// Price of a roll in the network
pub const ROLL_PRICE: Amount = Amount::from_raw(100 * AMOUNT_DECIMAL_FACTOR);
/// Block reward is given for each block creation
pub const BLOCK_REWARD: Amount = Amount::from_raw((0.3 * AMOUNT_DECIMAL_FACTOR as f64) as u64);
/// Time between the periods in the same thread.
pub const T0: MassaTime = MassaTime::from(16000);
/// Proof of stake seed for the initial draw
pub const INITIAL_DRAW_SEED: &str = "massa_genesis_seed";
/// Number of threads
pub const THREAD_COUNT: u8 = 32;
/// Number of endorsement
pub const ENDORSEMENT_COUNT: u32 = 9;
/// Threshold for fitness.
pub const DELTA_F0: u64 = 640;
/// Maximum number of operations per block
pub const MAX_OPERATIONS_PER_BLOCK: u32 = 204800;
/// Maximum block size in bytes
pub const MAX_BLOCK_SIZE: u32 = 204800;
/// Maximum operation validity period count
pub const OPERATION_VALIDITY_PERIODS: u64 = 10;
/// cycle duration in periods
pub const PERIODS_PER_CYCLE: u64 = 128;
/// PoS lookback cycles: when drawing for cycle N, we use the rolls from cycle N - pos_lookback_cycles - 1
pub const POS_LOOKBACK_CYCLES: u64 = 2;
/// PoS lock cycles: when some rolls are released, we only credit the coins back to their owner after waiting  pos_lock_cycles
pub const POS_LOCK_CYCLES: u64 = 1;

// ***********************
// Bootstrap constants
//

/// Max message size for bootstrap
pub const MAX_BOOTSTRAP_MESSAGE_SIZE: u32 = 1048576000;
/// Max number of blocks we provide/ take into account while bootstrapping
pub const MAX_BOOTSTRAP_BLOCKS: u32 = 1000000;
pub const MAX_BOOTSTRAP_CLIQUES: u32 = 1000;
pub const MAX_BOOTSTRAP_DEPS: u32 = 1000;
/// Max number of child nodes
pub const MAX_BOOTSTRAP_CHILDREN: u32 = 1000;
/// Max number of cycles in PoS bootstrap
pub const MAX_BOOTSTRAP_POS_CYCLES: u32 = 5;
/// Max number of address and rng entries for PoS bootstrap
pub const MAX_BOOTSTRAP_POS_ENTRIES: u32 = 1000000000;
/// Max size of the IP list
pub const IP_LIST_MAX_SIZE: usize = 10000;
/// Size of the random bytes array used for the bootstrap, safe to import
pub const BOOTSTRAP_RANDOMNESS_SIZE_BYTES: usize = 32;

// ***********************
// Constants used for execution module (injected from ConsensusConfig)
//

/// Maximum of GAS allowed for a block
pub const MAX_GAS_PER_BLOCK: u64 = 100_000_000;

//
// Constants used in network
//

/// Max number of endorsements per message
pub const MAX_ENDORSEMENTS_PER_MESSAGE: u32 = 1024;
pub const NODE_SEND_CHANNEL_SIZE: usize = 1024;
pub const MAX_DUPLEX_BUFFER_SIZE: usize = 1024;

//
// Divers constants
//

pub const ADDRESS_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;
/// Safe to import
pub const AMOUNT_DECIMAL_FACTOR: u64 = 1_000_000_000;
pub const BLOCK_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;
pub const ENDORSEMENT_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;
pub const OPERATION_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;
pub const SLOT_KEY_SIZE: usize = 9;

/// Size of the event id hash used in execution module, safe to import
pub const EVENT_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;

// Some checks at compile time that should not be ignored!
#[allow(clippy::assertions_on_constants)]
const _: () = {
    assert!(THREAD_COUNT > 1);
    assert!((T0).to_millis() >= 1);
    assert!((T0).to_millis() % (THREAD_COUNT as u64) == 0);
};
