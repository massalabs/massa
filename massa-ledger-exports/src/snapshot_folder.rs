use anyhow::Result;
use std::{
    fs,
    io::{self, Read, Write},
    path,
};
use walkdir::WalkDir;
use zip::ZipWriter;

pub struct SnapshotCreator {

}

impl SnapshotCreator {
    pub fn init(snapshot_path: path::PathBuf, ledger: path::PathBuf) {

    }
}

pub fn extract_snapshot(
    snapshot_path: String,
    to_path: path::PathBuf,
    folder_to_extract: Option<String>,
) {
    let fname = path::Path::new(&snapshot_path);
    let file = fs::File::open(fname).unwrap();
    let output_dir_path = path::Path::new(&to_path);

    let mut archive = zip::ZipArchive::new(file).unwrap();

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).unwrap();
        let outpath = match file.enclosed_name() {
            Some(path) => path.to_owned(),
            None => continue,
        };

        if let Some(folder) = &folder_to_extract && !outpath.starts_with(folder) {
            continue;
        }

        if (*file.name()).ends_with('/') {
            println!("File {} extracted to \"{}\"", i, outpath.display());
            fs::create_dir_all(output_dir_path.join(&outpath)).unwrap();
        } else {
            println!(
                "File {} extracted to \"{}\" ({} bytes)",
                i,
                outpath.display(),
                file.size()
            );
            if let Some(p) = output_dir_path.join(&outpath).parent() {
                if !p.exists() {
                    fs::create_dir_all(p).unwrap();
                }
            }
            let mut outfile = fs::File::create(output_dir_path.join(&outpath)).unwrap();
            io::copy(&mut file, &mut outfile).unwrap();
        }

        // Get and Set permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            if let Some(mode) = file.unix_mode() {
                fs::set_permissions(&outpath, fs::Permissions::from_mode(mode)).unwrap();
            }
        }
    }
}

fn add_dir_to_archive<W: io::Write + io::Seek>(
    zip: &mut ZipWriter<W>,
    src_dir: String,
    prefix: String,
    options: zip::write::FileOptions,
) -> Result<()> {
    let walkdir = WalkDir::new(src_dir);
    let it = walkdir.into_iter().filter_map(|e| e.ok());

    let mut buffer = Vec::new();
    for entry in it {
        let path = entry.path();
        let name = path.strip_prefix(path::Path::new(&prefix)).unwrap();

        // Write file or directory explicitly
        // Some unzip tools unzip files with directory paths correctly, some do not!
        if path.is_file() {
            println!("adding file {path:?} as {name:?} ...");
            #[allow(deprecated)]
            zip.start_file_from_path(name, options)?;
            let mut f = fs::File::open(path)?;

            f.read_to_end(&mut buffer)?;
            zip.write_all(&buffer)?;
            buffer.clear();
        } else if !name.as_os_str().is_empty() {
            // Only if not root! Avoids path spec / warning
            // and mapname conversion failed error on unzip
            println!("adding dir {path:?} as {name:?} ...");
            #[allow(deprecated)]
            zip.add_directory_from_path(name, options)?;
        }
    }
    Ok(())
}

pub fn create_snapshot(
    ledger_path: String,
    final_state_path: String,
    snapshot_path: String,
) -> Result<()> {
    // /!\ This assumes the ledger path contains the ledger and the final state path contains the final state.
    // These folders should already contain everything!

    // First, create the archive
    let fname = path::Path::new(&snapshot_path);
    let file = fs::File::create(fname).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Bzip2)
        .unix_permissions(0o755);

    // Add all the files in the ledger_path
    add_dir_to_archive(&mut zip, ledger_path, String::from("ledger"), options)?;

    // Add all the files in the final_state_path
    add_dir_to_archive(
        &mut zip,
        final_state_path,
        String::from("final_state"),
        options,
    )?;

    zip.finish()?;

    Ok(())
}
