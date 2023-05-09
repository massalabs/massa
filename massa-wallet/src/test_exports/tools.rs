use std::io::Write;

use massa_cipher::encrypt;
use massa_models::{address::Address, prehash::PreHashMap};
use massa_signature::KeyPair;
use tempfile::NamedTempFile;

use crate::Wallet;

/// Creates a temporary file and a temporary wallet.
pub(crate)  fn create_test_wallet(default_accounts: Option<PreHashMap<Address, KeyPair>>) -> Wallet {
    let wallet_file = NamedTempFile::new().expect("cannot create temp file");
    let accounts = default_accounts.unwrap_or_default();
    wallet_file
        .as_file()
        .write_all(
            &encrypt(
                "test",
                serde_json::to_string::<PreHashMap<Address, KeyPair>>(&accounts)
                    .unwrap()
                    .as_bytes(),
            )
            .unwrap(),
        )
        .unwrap();
    Wallet::new(wallet_file.path().to_path_buf(), "test".to_string()).unwrap()
}
