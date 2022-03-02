// Copyright (c) 2022 MASSA LABS <info@massa.net>

/// This file defines testing tools related to the config
use crate::LedgerConfig;
use massa_models::{
    constants::default_testing::{FINAL_HISTORY_LENGTH, THREAD_COUNT},
    Address, Amount,
};
use std::collections::BTreeMap;
use std::io::prelude::*;
use tempfile::NamedTempFile;

/// Default value of LedgerConfig used for tests
impl Default for LedgerConfig {
    fn default() -> Self {
        LedgerConfig {
            // unused by the mock (you can use `LedgerConfig::sample()` to get
            // a NamedTempFile in addition)
            initial_sce_ledger_path: "".into(),
            final_history_length: FINAL_HISTORY_LENGTH,
            thread_count: THREAD_COUNT,
        }
    }
}

impl LedgerConfig {
    pub fn sample(ledger: &BTreeMap<Address, Amount>) -> (Self, NamedTempFile) {
        let ledger_file_named = NamedTempFile::new().expect("cannot create temp file");
        serde_json::to_writer_pretty(ledger_file_named.as_file(), &ledger)
            .expect("unable to write ledger file");
        ledger_file_named
            .as_file()
            .seek(std::io::SeekFrom::Start(0))
            .expect("could not seek file");
        (
            Self {
                initial_sce_ledger_path: ledger_file_named.path().to_path_buf(),
                final_history_length: 0,
                thread_count: THREAD_COUNT,
            },
            ledger_file_named,
        )
    }
}
