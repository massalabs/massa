// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::str::FromStr;

use hex_literal::hex;
use sha2::Digest;

use crate::interface_impl::InterfaceImpl;
use massa_execution_exports::ExecutionConfig;
use massa_models::address::Address;
use massa_sc_runtime::Interface;

#[test]
fn test_hash_sha256() {
    // test hashing using sha256 algo

    let interface = InterfaceImpl::new_default(
        Address::from_str("AU12cMW9zRKFDS43Z2W88VCmdQFxmHjAo54XvuVV34UzJeXRLXW9M").unwrap(),
        None,
        None,
    );
    let actual_hash = interface.hash_sha256(b"something").unwrap();
    let expected_hash =
        &hex!("3fc9b689459d738f8c88a3a48aa9e33542016b7a4052e001aaa536fca74813cb")[..];
    assert_eq!(actual_hash, expected_hash);
}

#[test]
fn test_evm_signature_verify() {
    // Test that we can verify an Ethereum signature

    let interface = InterfaceImpl::new_default(
        Address::from_str("AU12cMW9zRKFDS43Z2W88VCmdQFxmHjAo54XvuVV34UzJeXRLXW9M").unwrap(),
        None,
        None,
    );

    let _address = hex!("807a7bb5193edf9898b9092c1597bb966fe52514");
    let message_ = b"test";
    let signature_ = hex!("d0d05c35080635b5e865006c6c4f5b5d457ec342564d8fc67ce40edc264ccdab3f2f366b5bd1e38582538fed7fa6282148e86af97970a10cb3302896f5d68ef51b");
    let private_key_ = hex!("ed6602758bdd68dc9df67a6936ed69807a74b8cc89bdc18f3939149d02db17f3");

    // build original public key
    let private_key = libsecp256k1::SecretKey::parse_slice(&private_key_).unwrap();
    let public_key = libsecp256k1::PublicKey::from_secret_key(&private_key);

    let result = interface.evm_signature_verify(message_, &signature_, &public_key.serialize());
    assert!(result.is_ok());

    // Invalid v
    {
        let mut signature_2_ = signature_;
        signature_2_[64] ^= 1;
        let result =
            interface.evm_signature_verify(message_, &signature_2_, &public_key.serialize());
        assert!(result.is_err());
    }
}

#[test]
fn test_evm_get_pubkey_from_signature() {
    // Test that we can retrieve the public key from an Ethereum signature

    let interface = InterfaceImpl::new_default(
        Address::from_str("AU12cMW9zRKFDS43Z2W88VCmdQFxmHjAo54XvuVV34UzJeXRLXW9M").unwrap(),
        None,
        None,
    );

    // let _address = hex!("807a7bb5193edf9898b9092c1597bb966fe52514");
    let message_ = b"test";
    let signature_ = hex!("d0d05c35080635b5e865006c6c4f5b5d457ec342564d8fc67ce40edc264ccdab3f2f366b5bd1e38582538fed7fa6282148e86af97970a10cb3302896f5d68ef51b");
    let private_key_ = hex!("ed6602758bdd68dc9df67a6936ed69807a74b8cc89bdc18f3939149d02db17f3");

    // build original public key
    let private_key = libsecp256k1::SecretKey::parse_slice(&private_key_).unwrap();
    let public_key = libsecp256k1::PublicKey::from_secret_key(&private_key);

    // build the hash
    let prefix = format!("\x19Ethereum Signed Message:\n{}", message_.len());
    let to_hash = [prefix.as_bytes(), message_].concat();
    let full_hash = sha3::Keccak256::digest(to_hash);

    let result = interface.evm_get_pubkey_from_signature(&full_hash, &signature_);
    assert!(result.is_ok());
    assert_eq!(public_key.serialize(), result.unwrap().as_ref());

    // Invalid s - expect failure
    {
        let mut signature_2 =
            libsecp256k1::Signature::parse_standard_slice(&signature_[..64]).unwrap();
        signature_2.s = -signature_2.s;
        assert!(signature_2.s.is_high());
        let result = interface.evm_get_pubkey_from_signature(&full_hash, &signature_2.serialize());
        assert!(result.is_err());
    }

    // Invalid v - expect failure
    {
        let mut signature_2_ = signature_;
        signature_2_[64] ^= 1;
        let result = interface.evm_get_pubkey_from_signature(&full_hash, &signature_2_);
        assert!(result.is_err());
    }
}

#[test]
fn test_evm_address() {
    // Test that we can retrieve an Ethereum address from an Ethereum public key (from a private key)

    let interface = InterfaceImpl::new_default(
        Address::from_str("AU12cMW9zRKFDS43Z2W88VCmdQFxmHjAo54XvuVV34UzJeXRLXW9M").unwrap(),
        None,
        None,
    );

    let _address = hex!("807a7bb5193edf9898b9092c1597bb966fe52514");
    // let message_ = b"test";
    // let signature_ = hex!("d0d05c35080635b5e865006c6c4f5b5d457ec342564d8fc67ce40edc264ccdab3f2f366b5bd1e38582538fed7fa6282148e86af97970a10cb3302896f5d68ef51b");
    let private_key_ = hex!("ed6602758bdd68dc9df67a6936ed69807a74b8cc89bdc18f3939149d02db17f3");

    // build original public key
    let private_key = libsecp256k1::SecretKey::parse_slice(&private_key_).unwrap();

    let public_key = libsecp256k1::PublicKey::from_secret_key(&private_key);
    let res = interface
        .evm_get_address_from_pubkey(&public_key.serialize())
        .unwrap();
    // println!("***result: {:?}", res.to_vec());
    // println!("***add: {:?}", _address);
    assert_eq!(res, _address);
}

#[test]
fn test_emit_event() {
    // emit 2 events and check that the 2nd event is rejected (because the limit is reached)

    let config = ExecutionConfig {
        max_event_per_operation: 1,
        ..Default::default()
    };

    let interface = InterfaceImpl::new_default(
        Address::from_str("AU12cMW9zRKFDS43Z2W88VCmdQFxmHjAo54XvuVV34UzJeXRLXW9M").unwrap(),
        None,
        Some(config),
    );

    let res = interface.generate_event("foo".to_string());
    assert!(res.is_ok());
    let res_2 = interface.generate_event("foo".to_string());
    assert!(res_2.is_err());
    println!("res_2: {:?}", res_2);
    if let Err(e) = res_2 {
        assert!(e.to_string().contains("Too many event for this operation"));
    }
}

#[test]
fn test_emit_event_too_large() {
    // emit 2 events and check that the 2nd event is rejected (because the msg is too large)

    let config = ExecutionConfig {
        max_event_size: 10,
        ..Default::default()
    };

    let interface = InterfaceImpl::new_default(
        Address::from_str("AU12cMW9zRKFDS43Z2W88VCmdQFxmHjAo54XvuVV34UzJeXRLXW9M").unwrap(),
        None,
        Some(config.clone()),
    );

    let res = interface.generate_event("a".repeat(config.max_event_size).to_string());
    assert!(res.is_ok());
    let res_2 = interface.generate_event("b".repeat(config.max_event_size + 1).to_string());
    assert!(res_2.is_err());
    println!("res_2: {:?}", res_2);
    if let Err(e) = res_2 {
        assert!(e.to_string().contains("Event data size is too large"));
    }
}
