use massa_ledger::{LedgerDB, LedgerEntry, LedgerSubEntry};
use massa_models::Address;
use std::collections::BTreeMap;
use std::{io::Write, path::PathBuf, str::FromStr};

fn main() {
    let db = LedgerDB::new(PathBuf::from_str("../massa-node/storage/ledger/rocks_db").unwrap());
    let res: BTreeMap<Address, LedgerEntry> = db
        .get_every_address()
        .iter()
        .map(|(addr, balance)| {
            (
                *addr,
                LedgerEntry {
                    parallel_balance: *balance,
                    bytecode: db
                        .get_sub_entry(addr, LedgerSubEntry::Bytecode)
                        .unwrap_or_default(),
                    datastore: db.get_entire_datastore(addr),
                },
            )
        })
        .collect();
    let mut file = std::fs::File::create("../DISK_LEDGER_DUMP.json").unwrap();
    let data = serde_json::to_string_pretty(&res).unwrap();
    file.write_all(data.as_bytes()).unwrap();
}
