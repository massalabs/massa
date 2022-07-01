use std::collections::HashMap;

use massa_cipher::encrypt;
use massa_models::{
    ledger_models::LedgerData,
    rolls::{RollCounts, RollUpdate, RollUpdates},
    Address,
};
use massa_signature::KeyPair;
use tempfile::NamedTempFile;

/// Password used for encryption in tests
pub const TEST_PASSWORD: &str = "PASSWORD";

/// generate a named temporary JSON ledger file
pub fn generate_ledger_file(ledger_vec: &HashMap<Address, LedgerData>) -> NamedTempFile {
    use std::io::prelude::*;
    let ledger_file_named = NamedTempFile::new().expect("cannot create temp file");
    serde_json::to_writer_pretty(ledger_file_named.as_file(), &ledger_vec)
        .expect("unable to write ledger file");
    ledger_file_named
        .as_file()
        .seek(std::io::SeekFrom::Start(0))
        .expect("could not seek file");
    ledger_file_named
}

/// generate staking key temp file from keypairs
pub fn generate_staking_keys_file(staking_keys: &[KeyPair]) -> NamedTempFile {
    use std::io::prelude::*;
    let file_named = NamedTempFile::new().expect("cannot create temp file");
    let json = serde_json::to_string_pretty(&staking_keys).expect("json serialization failed");
    let encrypted_data = encrypt(TEST_PASSWORD, json.as_bytes()).expect("encryption failed");
    std::fs::write(file_named.as_ref(), encrypted_data).expect("data writing failed");
    file_named
        .as_file()
        .seek(std::io::SeekFrom::Start(0))
        .expect("could not seek file");
    file_named
}

/// generate a named temporary JSON initial rolls file
pub fn generate_roll_counts_file(roll_counts: &RollCounts) -> NamedTempFile {
    use std::io::prelude::*;
    let roll_counts_file_named = NamedTempFile::new().expect("cannot create temp file");
    serde_json::to_writer_pretty(roll_counts_file_named.as_file(), &roll_counts.0)
        .expect("unable to write ledger file");
    roll_counts_file_named
        .as_file()
        .seek(std::io::SeekFrom::Start(0))
        .expect("could not seek file");
    roll_counts_file_named
}

/// generate a default named temporary JSON initial rolls file,
/// assuming two threads.
pub fn generate_default_roll_counts_file(stakers: Vec<KeyPair>) -> NamedTempFile {
    let mut roll_counts = RollCounts::default();
    for key in stakers.iter() {
        let address = Address::from_public_key(&key.get_public_key());
        let update = RollUpdate {
            roll_purchases: 1,
            roll_sales: 0,
        };
        let mut updates = RollUpdates::default();
        updates.apply(&address, &update).unwrap();
        roll_counts.apply_updates(&updates).unwrap();
    }
    generate_roll_counts_file(&roll_counts)
}
