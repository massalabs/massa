use tempfile::NamedTempFile;

pub fn get_temp_private_key_file() -> NamedTempFile {
    NamedTempFile::new().expect("cannot create temp file")
}
