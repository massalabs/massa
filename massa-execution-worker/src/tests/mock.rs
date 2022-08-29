use std::{collections::BTreeMap, fs::File, io::Seek};

use massa_models::{address::Address, amount::Amount};
use massa_signature::KeyPair;
use std::str::FromStr;
use tempfile::NamedTempFile;

pub fn get_initials() -> (NamedTempFile, BTreeMap<Address, Amount>) {
    let file = NamedTempFile::new().unwrap();
    let mut rolls: BTreeMap<Address, u64> = BTreeMap::new();
    let mut ledger: BTreeMap<Address, Amount> = BTreeMap::default();

    // thread 0 / 31
    let keypair_0 =
        KeyPair::from_str("S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ").unwrap();
    let addr_0 = Address::from_public_key(&keypair_0.get_public_key());
    rolls.insert(addr_0, 100_000);
    ledger.insert(addr_0, Amount::from_raw(100_000));
    // thread 1 / 31
    let keypair_1 =
        KeyPair::from_str("S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ").unwrap();
    let addr_1 = Address::from_public_key(&keypair_1.get_public_key());
    rolls.insert(addr_1, 100_000);
    ledger.insert(addr_1, Amount::from_raw(100_000));

    // write file
    serde_json::to_writer_pretty::<&File, BTreeMap<Address, u64>>(file.as_file(), &rolls)
        .expect("unable to write ledger file");
    file.as_file()
        .seek(std::io::SeekFrom::Start(0))
        .expect("could not seek file");

    (file, ledger)
}
