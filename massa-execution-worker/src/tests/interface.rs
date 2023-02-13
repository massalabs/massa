// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::address::Address;
use serial_test::serial;

use crate::interface_impl::InterfaceImpl;

#[test]
#[serial]
fn test_hash_sha256() {
    let interface = InterfaceImpl::new_default(
        Address::from_str("AU12cMW9zRKFDS43Z2W88VCmdQFxmHjAo54XvuVV34UzJeXRLXW9M").unwrap(),
        None,
    );
    let actual_hash = interface.hash_sha256("something".as_bytes()).unwrap();
    let expected_hash =
        "3fc9b689459d738f8c88a3a48aa9e33542016b7a4052e001aaa536fca74813cb".as_bytes();
    assert_eq!(actual_hash, expected_hash);
}
