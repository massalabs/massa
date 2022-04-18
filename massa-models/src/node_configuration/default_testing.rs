//! DEFAULT VALUES USED TO INITIALIZE DIVERS CONFIGURATIONS STRUCTURES
//! Same as default constants but in testing mode. You can access to them with
//! the `testing` feature activated.
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
//! See `./default.rs` for more documentation about each constants
//!
//! The following values are good as it. If you want a specific configuration,
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
    /// genesis private keys
    pub static ref GENESIS_KEY: PrivateKey = generate_random_private_key();
    /// Time in milliseconds when the blockclique started.
    pub static ref GENESIS_TIMESTAMP: MassaTime = if cfg!(feature = "sandbox") {
        MassaTime::now()
            .unwrap()
            .saturating_add(MassaTime::from(1000 * 60 * 3))
    } else {
        1643918400000.into()
    };

    /// TESTNET: time when the blockclique is ended.
    pub static ref END_TIMESTAMP: Option<MassaTime> = None;
    /// Be careful:
    /// The `GENESIS_TIMESTAMP` shouldn't be used as it in test because if you start the full test
    /// process, the first use is effectively `MassaTime::now().unwrap()` but will be outdated for
    /// the latest test. That's the reason why we choose to reset it each time we get a `ConsensusConfig`.
    pub static ref POS_MISS_RATE_DEACTIVATION_THRESHOLD: Ratio<u64> = Ratio::new(1, 1);
    /// node version
    pub static ref VERSION: Version = "DEVE.0.0".parse().unwrap();
}

/// Size of the random bytes array used for the bootstrap, safe to import
pub const ADDRESS_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;
/// Safe to import
pub const AMOUNT_DECIMAL_FACTOR: u64 = 1_000_000_000;
/// our default ip
pub const BASE_NETWORK_CONTROLLER_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(169, 202, 0, 10));
/// block id size
pub const BLOCK_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;
/// reward for a block
pub const BLOCK_REWARD: Amount = Amount::from_raw(AMOUNT_DECIMAL_FACTOR);
/// random bootstrap message size
pub const BOOTSTRAP_RANDOMNESS_SIZE_BYTES: usize = 32;
/// channel size
pub const CHANNEL_SIZE: usize = 256;
/// fitness threshold
pub const DELTA_F0: u64 = 32;
/// target endorsement count
pub const ENDORSEMENT_COUNT: u32 = 0;
/// endorsement id size
pub const ENDORSEMENT_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;
/// event id size
pub const EVENT_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;
/// random handshake message size
pub const HANDSHAKE_RANDOMNESS_SIZE_BYTES: usize = 32;
/// initial seed
pub const INITIAL_DRAW_SEED: &str = "massa_genesis_seed";
/// max bootstrap ips kept size
pub const IP_LIST_MAX_SIZE: usize = 10000;
/// max advertised id length
pub const MAX_ADVERTISE_LENGTH: u32 = 10;
/// max ask for block per message
pub const MAX_ASK_BLOCKS_PER_MESSAGE: u32 = 3;
/// max block size 3 * 1024 * 1024
pub const MAX_BLOCK_SIZE: u32 = 3145728;
/// max asynchronous pool length
pub const MAX_ASYNC_POOL_LENGTH: u64 = 10_000;
/// max bootstrapped blocks
pub const MAX_BOOTSTRAP_BLOCKS: u32 = 100;
/// max bootstrapped children per block
pub const MAX_BOOTSTRAP_CHILDREN: u32 = 10;
/// max bootstrapped cliques
pub const MAX_BOOTSTRAP_CLIQUES: u32 = 100;
/// max bootstrapped dependencies
pub const MAX_BOOTSTRAP_DEPS: u32 = 100;
/// max duplex buffer size
pub const MAX_DUPLEX_BUFFER_SIZE: usize = 1024;
/// max endorsements per message
pub const MAX_ENDORSEMENTS_PER_MESSAGE: u32 = 1024;
/// max bootstrap message size
pub const MAX_BOOTSTRAP_MESSAGE_SIZE: u32 = 100_000_000;
/// max bootstrapped proof of stake entries
pub const MAX_BOOTSTRAP_POS_ENTRIES: u32 = 1000;
/// max bootstrapped proof of take cycles
pub const MAX_BOOTSTRAP_POS_CYCLES: u32 = 5;
/// max gas per block
pub const MAX_GAS_PER_BLOCK: u64 = 100_000_000;
/// max asynchronous gas
pub const MAX_ASYNC_GAS: u64 = 10_000_000;
/// max message size 3 * 1024 * 1024
pub const MAX_MESSAGE_SIZE: u32 = 3145728;
/// max number of operation per block
pub const MAX_OPERATIONS_PER_BLOCK: u32 = 1024;
/// max number of operation per message
pub const MAX_OPERATIONS_PER_MESSAGE: u32 = 1024;
/// node send channel size
pub const NODE_SEND_CHANNEL_SIZE: usize = 1024;
/// operation id size
pub const OPERATION_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;
/// operation validity periods
pub const OPERATION_VALIDITY_PERIODS: u64 = 1;
/// periods per cycle
pub const PERIODS_PER_CYCLE: u64 = 100;
/// proof of stake lock cycles
pub const POS_LOCK_CYCLES: u64 = 1;
/// proof of stake look back cycle
pub const POS_LOOKBACK_CYCLES: u64 = 2;
/// roll price
pub const ROLL_PRICE: Amount = Amount::from_raw(100 * AMOUNT_DECIMAL_FACTOR);
/// serialized slot size
pub const SLOT_KEY_SIZE: usize = 9;
/// thread count
pub const THREAD_COUNT: u8 = 2;
/// period length in milliseconds, sometimes overridden in `config.rs` or `setting.rs`
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

/// normally in `config.toml`, allow execution worker to lag smoothly
pub const CURSOR_DELAY: MassaTime = MassaTime::from(0);
/// normally in `config.toml`, if the node will create blocks
pub const DISABLE_BLOCK_CREATION: bool = true;
/// normally in `config.toml`, final history length
pub const FINAL_HISTORY_LENGTH: usize = 10;
/// normally in `config.toml`, forcefully kept periods
pub const FORCE_KEEP_FINAL_PERIOD: u64 = 0;
/// normally in `config.toml`, if slot is after `FUTURE_BLOCK_PROCESSING_MAX_PERIODS`, the block is not processed
pub const FUTURE_BLOCK_PROCESSING_MAX_PERIODS: u64 = 10;
/// normally in `config.toml`, ledger cache capacity
pub const LEDGER_CACHE_CAPACITY: u64 = 1_000_000;
/// normally in `config.toml`, if the ledger need a reset at start up
pub const LEDGER_RESET_AT_STARTUP: bool = true;
/// normally in `config.toml`, max unknown dependencies kept
pub const MAX_DEPENDENCY_BLOCK: usize = 10;
/// normally in `config.toml`, max discarded blocks kept
pub const MAX_DISCARDED_BLOCKS: usize = 10;
/// normally in `config.toml`, max final events kept
pub const MAX_FINAL_EVENTS: usize = 10;
/// normally in `config.toml`, max in the future kept blocks
pub const MAX_FUTURE_PROCESSING_BLOCK: usize = 10;
/// normally in `config.toml`, max item count returned
pub const MAX_ITEM_RETURN_COUNT: usize = 1000;
/// normally in `config.toml`, max operation fill attempts
pub const MAX_OPERATION_FILL_ATTEMPTS: u32 = 6;
/// normally in `config.toml`, operation batch size
pub const OPERATION_BATCH_SIZE: usize = 3;
/// normally in `config.toml`, proof of stake cached cycle
pub const POS_DRAW_CACHED_CYCLE: usize = 10;
/// normally in `config.toml`, read only queue length
pub const READONLY_QUEUE_LENGTH: usize = 10;

// Note: In the `massa-network`, the default values are defined in the `settings.rs` of the
// `massa-network` crate.

lazy_static::lazy_static! {
    /// blocks are pruned every `BLOCK_DB_PRUNE_INTERVAL` milliseconds
    pub static ref BLOCK_DB_PRUNE_INTERVAL: MassaTime = 1000.into();
    /// ledger is saved on disk every `LEDGER_FLUSH_INTERVAL` milliseconds
    pub static ref LEDGER_FLUSH_INTERVAL: Option<MassaTime> = Some(200.into());
    /// we wait `MAX_SEND_WAIT` milliseconds to send a message
    pub static ref MAX_SEND_WAIT: MassaTime = 500.into();
    /// stats are considered for `STATS_TIMESPAN` milliseconds
    pub static ref STATS_TIMESPAN: MassaTime = 60000.into();
}
