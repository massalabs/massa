use std::{collections::BTreeMap, fs::File, io::Seek};

use massa_models::address::Address;
use tempfile::NamedTempFile;

use super::scenarios_mandatories::get_random_address;

pub fn get_initial_rolls() -> NamedTempFile {
    let file = NamedTempFile::new().unwrap();
    let mut rolls: BTreeMap<Address, u64> = BTreeMap::new();
    for _ in 0..8 {
        rolls.insert(get_random_address(), 2);
    }
    serde_json::to_writer_pretty::<&File, BTreeMap<Address, u64>>(file.as_file(), &rolls)
        .expect("unable to write ledger file");
    file.as_file()
        .seek(std::io::SeekFrom::Start(0))
        .expect("could not seek file");
    file
}
