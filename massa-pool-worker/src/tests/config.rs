//! Default values for testing configuration of pool module
use massa_pool_exports::PoolConfig;
lazy_static::lazy_static! {
    pub static ref POOL_CONFIG: PoolConfig = Default::default();
}
