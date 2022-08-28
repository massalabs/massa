use std::{fs::File, io::Seek, str::FromStr};

use massa_models::{address::Address, config::THREAD_COUNT, rolls::RollCounts};
use tempfile::NamedTempFile;

pub fn get_initial_rolls() -> NamedTempFile {
    let file = NamedTempFile::new().unwrap();
    let mut rolls = Vec::new();
    for _ in 0..THREAD_COUNT {
        let mut rolls_per_thread = RollCounts::new();
        rolls_per_thread.0.insert(
            Address::from_str("A12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
            2,
        );
        rolls.push(rolls_per_thread);
    }
    serde_json::to_writer_pretty::<&File, Vec<RollCounts>>(file.as_file(), &rolls)
        .expect("unable to write ledger file");
    file.as_file()
        .seek(std::io::SeekFrom::Start(0))
        .expect("could not seek file");
    file
}
