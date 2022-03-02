//! DEFAULT VALUES USED TO INITIALISE DIVERS CONFIGURATIONS STRUCTURES
//! Same as default constants but in testing mode. You can access to them with
//! the `testing` feature activated.
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
//! See `./default.rs` for more documentation about each constants
//!
//! /!\ The following values are good as it. If you want a specific configuration,
//! create a mutable configuration and inject it as a dependency in the worker.
//!
//! Ex:
//! ```ignore
//! let cfg = UsedConfig {
//!    value_to_modify: 5,
//!    ..UsedConfig::default()
//! };
//! ```
use crate::{Amount, Version};
use massa_signature::{generate_random_private_key, PrivateKey};
use massa_time::MassaTime;
use num::rational::Ratio;
use std::net::{IpAddr, Ipv4Addr};

lazy_static::lazy_static! {
    pub static ref GENESIS_KEY: PrivateKey = generate_random_private_key();
    /// Time in millis when the blockclique started.
    pub static ref GENESIS_TIMESTAMP: MassaTime = if cfg!(feature = "sandbox") {
        MassaTime::now()
            .unwrap()
            .saturating_add(MassaTime::from(1000 * 60 * 3))
    } else {
        1643918400000.into()
    };

    /// TESTNET: time when the blockclique is ended.
    pub static ref END_TIMESTAMP: Option<MassaTime> = None;
    // Be carefull:
    // The `GENESIS_TIMESTAMP` shouldn't be used as it in test because if you start the full test
    // process, the first use is effectivelly `MassaTime::now().unwrap()` but will be outdated for
    // the latest test. That's the reason why we choose to reset it each time we get a ConsensusConfig.
    pub static ref POS_MISS_RATE_DEACTIVATION_THRESHOLD: Ratio<u64> = Ratio::new(1, 1);
    pub static ref VERSION: Version = "DEVE.0.0".parse().unwrap();
}

/// Size of the random bytes array used for the bootstrap, safe to import
pub const ADDRESS_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;
/// Safe to import
pub const AMOUNT_DECIMAL_FACTOR: u64 = 1_000_000_000;
pub const BASE_NETWORK_CONTROLLER_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(169, 202, 0, 10));
pub const BLOCK_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;
pub const BLOCK_REWARD: Amount = Amount::from_raw(AMOUNT_DECIMAL_FACTOR);
pub const BOOTSTRAP_RANDOMNESS_SIZE_BYTES: usize = 32;
pub const CHANNEL_SIZE: usize = 256;
pub const DELTA_F0: u64 = 32;
pub const ENDORSEMENT_COUNT: u32 = 0;
pub const ENDORSEMENT_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;
pub const EVENT_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;
pub const HANDSHAKE_RANDOMNESS_SIZE_BYTES: usize = 32;
pub const INITIAL_DRAW_SEED: &str = "massa_genesis_seed";
pub const IP_LIST_MAX_SIZE: usize = 10000;
pub const MAX_ADVERTISE_LENGTH: u32 = 10;
pub const MAX_ASK_BLOCKS_PER_MESSAGE: u32 = 3;
pub const MAX_BLOCK_SIZE: u32 = 3145728; // 3 * 1024 * 1024
pub const MAX_BOOTSTRAP_BLOCKS: u32 = 100;
pub const MAX_BOOTSTRAP_CHILDREN: u32 = 10;
pub const MAX_BOOTSTRAP_CLIQUES: u32 = 100;
pub const MAX_BOOTSTRAP_DEPS: u32 = 100;
pub const MAX_DUPLEX_BUFFER_SIZE: usize = 1024;
pub const MAX_ENDORSEMENTS_PER_MESSAGE: u32 = 1024;
pub const MAX_BOOTSTRAP_MESSAGE_SIZE: u32 = 100_000_000;
pub const MAX_BOOTSTRAP_POS_ENTRIES: u32 = 1000;
pub const MAX_BOOTSTRAP_POS_CYCLES: u32 = 5;
pub const MAX_GAS_PER_BLOCK: u64 = 100_000_000;
pub const MAX_MESSAGE_SIZE: u32 = 3145728; // 3 * 1024 * 1024
pub const MAX_OPERATIONS_PER_BLOCK: u32 = 1024;
pub const MAX_OPERATIONS_PER_MESSAGE: u32 = 1024;
pub const NODE_SEND_CHANNEL_SIZE: usize = 1024;
pub const OPERATION_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;
pub const OPERATION_VALIDITY_PERIODS: u64 = 1;
pub const PERIODS_PER_CYCLE: u64 = 100;
pub const POS_LOCK_CYCLES: u64 = 1;
pub const POS_LOOKBACK_CYCLES: u64 = 2;
pub const ROLL_PRICE: Amount = Amount::from_raw(100 * AMOUNT_DECIMAL_FACTOR);
pub const SLOT_KEY_SIZE: usize = 9;
pub const THREAD_COUNT: u8 = 2;
pub const T0: MassaTime = MassaTime::from(32000);

/*************************
* Mocked constant section
* -----------------------
*
* The following section contains constants that are use for default
* configuration when, for unit-testing, there is no config file.
*
* We describe here the default configuration but the configuration continue
* to be flexible for each test.
*
* Ex:
* let cfg = ConsensusSettings {
*    future_block_processing_max_periods: 5,
*    ..Default::default()
* };
*/

pub const DISABLE_BLOCK_CREATION: bool = true;
pub const FORCE_KEEP_FINAL_PERIOD: u64 = 0;
pub const FUTURE_BLOCK_PROCESSING_MAX_PERIODS: u64 = 10;
pub const LEDGER_CACHE_CAPACITY: u64 = 1_000_000;
pub const LEDGER_RESET_AT_STARTUP: bool = true;
pub const MAX_DEPENDENCY_BLOCK: usize = 10;
pub const MAX_DISCARED_BLOCKS: usize = 10;
pub const MAX_FUTURE_PROCESSING_BLOCK: usize = 10;
pub const MAX_ITEM_RETURN_COUNT: usize = 1000;
pub const MAX_OPERATION_FILL_ATTEMPTS: u32 = 6;
pub const OPERATION_BATCH_SIZE: usize = 3;
pub const POS_DRAW_CACHED_CYCLE: usize = 10;

// Note: In the `massa-network`, the default values are defined in the `settings.rs` of the
// `massa-network` crate.

lazy_static::lazy_static! {
    pub static ref BLOCK_DB_PRUNE_INTERVAL: MassaTime = 1000.into();
    pub static ref LEDGER_FLUSH_INTERVAL: Option<MassaTime> = Some(200.into());
    pub static ref MAX_SEND_WAIT: MassaTime = 500.into();
    pub static ref STATS_TIMESPAN: MassaTime = 60000.into();
}
