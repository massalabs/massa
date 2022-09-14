use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::Seek,
};

use massa_ledger_exports::LedgerEntry;
use massa_models::{address::Address, amount::Amount};
use massa_signature::KeyPair;
use std::str::FromStr;
use tempfile::NamedTempFile;

pub fn get_initials() -> (NamedTempFile, HashMap<Address, LedgerEntry>) {
    let file = NamedTempFile::new().unwrap();
    let mut rolls: BTreeMap<Address, u64> = BTreeMap::new();
    let mut ledger: HashMap<Address, LedgerEntry> = HashMap::new();

    // thread 0 / 31
    let keypair_0 =
        KeyPair::from_str("S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ").unwrap();
    let addr_0 = Address::from_public_key(&keypair_0.get_public_key());
    rolls.insert(addr_0, 100);
    ledger.insert(
        addr_0,
        LedgerEntry {
            balance: Amount::from_str("300_000").unwrap(),
            ..Default::default()
        },
    );
    // thread 1 / 31
    let keypair_1 =
        KeyPair::from_str("S1JJeHiZv1C1zZN5GLFcbz6EXYiccmUPLkYuDFA3kayjxP39kFQ").unwrap();
    let addr_1 = Address::from_public_key(&keypair_1.get_public_key());
    rolls.insert(addr_1, 100);
    ledger.insert(
        addr_1,
        LedgerEntry {
            balance: Amount::from_str("300_000").unwrap(),
            ..Default::default()
        },
    );

    // write file
    serde_json::to_writer_pretty::<&File, BTreeMap<Address, u64>>(file.as_file(), &rolls)
        .expect("unable to write ledger file");
    file.as_file()
        .seek(std::io::SeekFrom::Start(0))
        .expect("could not seek file");

    (file, ledger)
}
