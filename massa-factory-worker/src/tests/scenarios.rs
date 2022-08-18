use std::sync::{Arc, RwLock};

use massa_factory_exports::{FactoryConfig, FactoryChannels};
use serial_test::serial;

use crate::start_factory;
use massa_wallet::test_exports::create_test_wallet;

#[test]
#[serial]
fn basic_creation() {

    start_factory(FactoryConfig::default(), Arc::new(RwLock::new(create_test_wallet())), FactoryChannels {

    });
}