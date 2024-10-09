// Copyright (c) 2022 MASSA LABS <info@massa.net>

use hex_literal::hex;
use massa_models::address::Address;
use massa_sc_runtime::Interface;
use std::str::FromStr;

use crate::interface_impl::InterfaceImpl;
#[test]
fn test_hash_sha256() {
    let interface = InterfaceImpl::new_default(
        Address::from_str("AU12cMW9zRKFDS43Z2W88VCmdQFxmHjAo54XvuVV34UzJeXRLXW9M").unwrap(),
        None,
    );
    let actual_hash = interface.hash_sha256(b"something").unwrap();
    let expected_hash =
        &hex!("3fc9b689459d738f8c88a3a48aa9e33542016b7a4052e001aaa536fca74813cb")[..];
    assert_eq!(actual_hash, expected_hash);
}

#[test]
fn test_evm_signature_verify() {
    let interface = InterfaceImpl::new_default(
        Address::from_str("AU12cMW9zRKFDS43Z2W88VCmdQFxmHjAo54XvuVV34UzJeXRLXW9M").unwrap(),
        None,
    );

    let _address = hex!("807a7bb5193edf9898b9092c1597bb966fe52514");
    let message_ = b"test";
    let mut signature_ = hex!("d0d05c35080635b5e865006c6c4f5b5d457ec342564d8fc67ce40edc264ccdab3f2f366b5bd1e38582538fed7fa6282148e86af97970a10cb3302896f5d68ef51b");
    let private_key_ = hex!("ed6602758bdd68dc9df67a6936ed69807a74b8cc89bdc18f3939149d02db17f3");

    // build original public key
    let private_key = libsecp256k1::SecretKey::parse_slice(&private_key_).unwrap();
    let public_key = libsecp256k1::PublicKey::from_secret_key(&private_key);

    let result = interface.evm_signature_verify(message_, &signature_, &public_key.serialize());
    assert!(result.is_ok());

    // Invalid v
    signature_[64] ^= 1;
    let result = interface.evm_signature_verify(message_, &signature_, &public_key.serialize());
    assert!(result.is_err());
}
