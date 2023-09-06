use massa_models::{address::Address, prehash::PreHashMap};
use massa_signature::KeyPair;
use tempfile::TempDir;

use crate::Wallet;

/// Creates a temporary file and a temporary wallet.
pub fn create_test_wallet(default_accounts: Option<PreHashMap<Address, KeyPair>>) -> Wallet {
    let accounts = default_accounts.unwrap_or_default();
    let folder = TempDir::new().expect("cannot create temp dir");
    let mut wallet = Wallet::new(folder.path().to_path_buf(), "test".to_string()).unwrap();
    wallet
        .add_keypairs(accounts.values().cloned().collect())
        .unwrap();
    wallet
}
