use tempfile::NamedTempFile;

use crate::Wallet;

/// Creates a temporary file and a temporary wallet.
pub fn create_test_wallet() -> Wallet {
    let wallet_file = NamedTempFile::new().expect("cannot create temp file");
    Wallet::new(wallet_file.path().to_path_buf(), "test".to_string()).unwrap()
}
