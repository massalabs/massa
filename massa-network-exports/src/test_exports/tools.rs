use tempfile::NamedTempFile;

/// named temp file for keypairs
pub fn get_temp_keypair_file() -> NamedTempFile {
    NamedTempFile::new().expect("cannot create temp file")
}
