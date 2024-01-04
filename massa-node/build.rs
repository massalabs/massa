// Copyright (c) 2023 MASSA LABS <info@massa.net>

use std::{env, fs, path::Path, process::Command};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("Failed to get Cargo dir");
    let cargo_dir = &Path::new(&manifest_dir);
    let target_initial_setup_path = cargo_dir.join("genesis-ledger").join("node_initial_setup");
    let node_config_path = cargo_dir.join("base_config");
    let cargo_dir_abs = cargo_dir.canonicalize().expect("Failed to get Cargo dir");
    let workspace_dir = cargo_dir_abs.parent().expect("Failed to get workspace dir");

    if !target_initial_setup_path.exists() {
        env::set_current_dir(&workspace_dir).expect("cd failed");
        Command::new("git")
            .args(["submodule", "update", "--init"])
            .output()
            .expect("git submodule update failed");
    }

    if !target_initial_setup_path.exists() {
        panic!("git submodule update failed");
    }

    let initial_files = vec![
        "deferred_credits.json",
        "initial_ledger.json",
        "initial_rolls.json",
    ];
    for file in initial_files {
        let source = target_initial_setup_path.join(file);
        let destination = node_config_path.join(file);

        if !source.exists() {
            panic!("Initial setup file {} does not exist", file);
        }
        if destination.exists() {
            fs::remove_file(&destination).expect("Failed to remove initial setup files");
        }
        fs::hard_link(source, destination).expect("Failed to hard link initial setup files");
    }
}
