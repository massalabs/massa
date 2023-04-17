//! ```cargo
//! [dependencies]
//! tar="0.4"
//! flate2="1.0"
//! glob="0.3"
//! fs_extra="1"
//! ureq="2"
//! thiserror="1"
//! clap={ version = "4", features= ["derive"] }
//! ```
use std::fs::File;
use std::io::Error;
use std::io::{Cursor, Read};
use std::num::ParseIntError;
use std::path::Path;

extern crate clap;
extern crate flate2;
extern crate fs_extra;
extern crate glob;
extern crate tar;
extern crate ureq;
use clap::Parser;
use flate2::read::GzDecoder;
use glob::glob;
use tar::Archive;

// git tag
const TAG: &str = "TEST.20.3";

// Maximum archive file size to download in bytes (here: 1Mb)
// const ARCHIVE_MAX_SIZE: u64 = 2; // Maximum archive file size to download in bytes (DEBUG)
// Destination path for wasm file & src files (relative to repo root)
const ARCHIVE_MAX_SIZE: u64 = 1048576;

// destination path
const PATH_DST_BASE_1: &str = "massa-execution-worker/src/tests/wasm/";

#[derive(Debug, thiserror::Error)]
enum DlFileError {
    #[error("i/o error: {0}")]
    IO(#[from] std::io::Error),
    #[error("ureq error: {0}")]
    Ureq(#[from] ureq::Error),
    #[error("parse error: {0}")]
    Parse(#[from] ParseIntError),
}

/// Archive download using given url
fn dl_file(url: &str, to_file: &str, max_size: u64) -> Result<(), DlFileError> {
    let resp = ureq::get(url).call()?;

    let mut bytes: Vec<u8> = match resp.header("Content-Length") {
        Some(l) => Vec::with_capacity(l.parse()?),
        None => Vec::with_capacity(max_size as usize),
    };

    resp.into_reader().take(max_size).read_to_end(&mut bytes)?;

    let mut buf = Cursor::new(bytes);
    let mut file = std::fs::File::create(to_file)?;
    std::io::copy(&mut buf, &mut file)?;

    Ok(())
}

fn download_src() -> Result<String, Error> {
    println!(
        "Using tag: {} for release of massa-unit-tests-src repo...",
        TAG
    );

    let path = format!("massa_unit_tests_{}.tar.gz", TAG);
    let url = format!(
        "https://github.com/massalabs/massa-unit-tests-src/releases/download/{}/{}",
        TAG, path
    );

    let extract_folder = format!("extract_massa_unit_tests_src_{}", TAG);

    if Path::new(&extract_folder).exists() {
        println!(
            "Please remove the folder: {} before runnning this script",
            extract_folder
        );
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

    Ok(format!(
        "{}/massa_unit_tests/*.wasm",
        extract_folder.clone()
    ))
}

/// Script input arguments
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Optional local pattern of the WASM files to copy
    #[arg(short, long)]
    local: Option<String>,
}

fn main() -> Result<(), Error> {
    let args = Args::parse();

    let pattern_src = if let Some(local_src) = args.local {
        println!("Using local sources");
        local_src
    } else {
        download_src()?
    };
    let path_dst_base = Path::new(PATH_DST_BASE_1);

    for entry in glob(&pattern_src).expect("Failed to read glob pattern (wasm)") {
        match entry {
            Ok(path) => {
                let path_file_name = match path.file_name() {
                    Some(fname) => fname,
                    None => {
                        println!("Unable to extract file name from: {:?}", path);
                        continue;
                    }
                };

                let path_dst = path_dst_base.join(path_file_name);
                println!("{:?} -> {:?}", path.display(), path_dst.display());
                let copy_res = std::fs::copy(path, path_dst);

                if copy_res.is_err() {
                    println!("Copy error: {:?}", copy_res);
                }
            }
            Err(e) => {
                println!("{:?}", e);
            }
        }
    }

    Ok(())
}
