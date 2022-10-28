//! ```cargo
//! [dependencies]
//! tar="0.4"
//! flate2="1.0"
//! glob="0.3"
//! fs_extra="1"
//! ureq="2"
//! thiserror="1"
//! ```
use std::fs::File;
use std::path::Path;
use std::io::{Cursor, Read};
use std::num::ParseIntError;

extern crate flate2;
extern crate tar;
extern crate glob;
extern crate fs_extra;
extern crate ureq;
use flate2::read::GzDecoder;
use tar::Archive;
use glob::glob;
// use fs_extra::dir::{remove, copy, CopyOptions};

const TAG: &str = "TEST.16.3"; // git tag
const ARCHIVE_MAX_SIZE: u64 = 1048576; // Maximum archive file size to download in bytes (here: 1Mb)
// const ARCHIVE_MAX_SIZE: u64 = 2; // Maximum archive file size to download in bytes (DEBUG)
// Destination path for wasm file & src files (relative to repo root)
const PATH_DST_BASE_1: &str = "massa-execution-worker/src/tests/wasm/";
// const PATH_DST_BASE_2: &str = "massa-execution-worker/src/tests/wasm/sources/";

#[derive(Debug, thiserror::Error)]
enum DlFileError {
    #[error("i/o error: {0}")]
    IO(#[from] std::io::Error),
    #[error("ureq error: {0}")]
    Ureq(#[from] ureq::Error),
    #[error("parse error: {0}")]
    Parse(#[from] ParseIntError)
}

/// Archive download using given url
fn dl_file(url: &str, to_file: &str, max_size: u64) -> Result<(), DlFileError> {

    let resp = ureq::get(url)
       .call()?;

    let mut bytes: Vec<u8> = match resp.header("Content-Length") {
        Some(l) => Vec::with_capacity(l.parse()?),
        None => Vec::with_capacity(max_size as usize),
    };

    resp.into_reader()
        .take(max_size)
        .read_to_end(&mut bytes)?;

    let mut buf = Cursor::new(bytes);
    let mut file = std::fs::File::create(to_file)?;
    std::io::copy(&mut buf, &mut file)?;

    Ok(())
}

fn main() -> Result<(), std::io::Error> {

    println!("Using tag: {} for release of massa-unit-tests-src repo...", TAG);

    let path = format!("massa_unit_tests_{}.tar.gz", TAG);
    let url = format!("https://github.com/massalabs/massa-unit-tests-src/releases/download/{}/{}", TAG, path);

    let extract_folder = format!("extract_massa_unit_tests_src_{}", TAG);

    if Path::new(&extract_folder).exists() {
        println!("Please remove the folder: {} before runnning this script", extract_folder);
        std::process::exit(1);
    }

    println!("Downloading: {}...", url);
    let res = dl_file(&url, &path, ARCHIVE_MAX_SIZE);

    if res.is_err() {
        println!("Unable to download release archive from github: {:?}", res);
        std::process::exit(2);
    } else {
        println!("Download done.")
    }

    let tar_gz = File::open(path)?;
    let tar = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(tar);
    archive.unpack(extract_folder.clone())?;

    // Copy wasm files
    let pattern_src_1 = format!("{}/massa_unit_tests/*.wasm", extract_folder.clone());
    let path_dst_base = Path::new(PATH_DST_BASE_1);

    for entry in glob(&pattern_src_1).expect("Failed to read glob pattern (wasm)") {
        match entry {
            Ok(path) => {
                let path_file_name = match path.file_name() {
                    Some(fname) => fname,
                    None => { println!("Unable to extract file name from: {:?}", path); continue },
                };

                let path_dst = path_dst_base.join(path_file_name);
                println!("{:?} -> {:?}", path.display(), path_dst.display());
                let copy_res = std::fs::copy(path, path_dst);

                if copy_res.is_err() {
                    println!("Copy error: {:?}", copy_res);
                }
            },
            Err(e) => {
                println!("{:?}", e);
            }
        }
    }

    Ok(())
}
