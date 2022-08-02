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

use crate::{Amount, Version};
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
            MassaTime::now()
                .unwrap()
                .saturating_add(MassaTime::from(1000 * 10))
        )
    } else {
        1659434400000.into()  // Tuesday, August 2, 2022 10:00:00 UTC
    };

    /// TESTNET: time when the blockclique is ended.
    pub static ref END_TIMESTAMP: Option<MassaTime> = if cfg!(feature = "sandbox") {
        None
    } else {
        Some(1661853600000.into())  // Tuesday, August 30, 2022 10:00:00 UTC
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
            "TEST.13.0"
        }
        .parse()
        .unwrap()
    };
}

/// Price of a roll in the network
pub const ROLL_PRICE: Amount = Amount::from_mantissa_scale(100, 0);
/// Block reward is given for each block creation
pub const BLOCK_REWARD: Amount = Amount::from_mantissa_scale(3, 1);
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
pub const MAX_OPERATIONS_PER_BLOCK: u32 = 409600;
/// Maximum block size in bytes
pub const MAX_BLOCK_SIZE: u32 = 409600;
/// Maximum capacity of the asynchronous messages pool
pub const MAX_ASYNC_POOL_LENGTH: u64 = 10_000;
/// Maximum operation validity period count
pub const OPERATION_VALIDITY_PERIODS: u64 = 10;
/// cycle duration in periods
pub const PERIODS_PER_CYCLE: u64 = 128;
/// PoS look back cycles: when drawing for cycle N, we use the rolls from cycle N - `pos_look` `back_cycles` - 1
pub const POS_LOOKBACK_CYCLES: u64 = 2;
/// PoS lock cycles: when some rolls are released, we only credit the coins back to their owner after waiting `pos_lock_cycles`
pub const POS_LOCK_CYCLES: u64 = 1;
/// Maximum size batch of data in a part of the ledger
pub const LEDGER_PART_SIZE_MESSAGE_BYTES: u64 = 1000000;
/// Maximum async messages in a batch of the bootstrap of the async pool
pub const ASYNC_POOL_PART_SIZE_MESSAGE_BYTES: u64 = 1000000;
/// Maximum length of a datastore key
pub const MAX_DATASTORE_KEY_LENGTH: u8 = 255;

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
/// Max size of the IP list
pub const IP_LIST_MAX_SIZE: usize = 10000;
/// Size of the random bytes array used for the bootstrap, safe to import
pub const BOOTSTRAP_RANDOMNESS_SIZE_BYTES: usize = 32;

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
pub const NODE_SEND_CHANNEL_SIZE: usize = 1024;
/// max duplex buffer size
pub const MAX_DUPLEX_BUFFER_SIZE: usize = 1024;

//
// Divers constants
//

/// address size
pub const ADDRESS_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;
/// Safe to import, amount decimal factor
pub const AMOUNT_DECIMAL_FACTOR: u64 = 1_000_000_000;
/// block id size
pub const BLOCK_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;
/// endorsement id size
pub const ENDORSEMENT_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;
/// operation id size
pub const OPERATION_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;
/// operation id prefix size
pub const OPERATION_ID_PREFIX_SIZE_BYTES: usize = 17;
/// slot as a key size
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
