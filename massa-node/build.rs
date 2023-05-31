use massa_models::config::constants::VERSION;
use std::fs;
use std::path::Path;
use toml_edit::{Document, Formatted, Item, Value};
use walkdir::WalkDir;

fn update_workspace_packages_version(
    new_version: String,
    workspace_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    for entry in WalkDir::new(workspace_path)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_name() == "Cargo.toml" {
            check_package_version(new_version.clone(), entry.path())?;
        }
    }

    Ok(())
}

fn check_package_version(
    new_version: String,
    cargo_toml_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let cargo_toml_content = fs::read_to_string(cargo_toml_path)?;
    let mut doc = cargo_toml_content.parse::<Document>()?;

    if let Some(package) = doc["package"].as_table_mut() {
        if let Some(version) = package.get_mut("version") {
            let to_string = version.to_string().replace('\"', "");
            let actual_version = to_string.trim();
            if new_version.ne(actual_version) {
                *version = Item::Value(Value::String(Formatted::new(new_version)));
            }
        }
    }

    let updated_cargo_toml_content = doc.to_string();
    fs::write(cargo_toml_path, updated_cargo_toml_content)?;

    Ok(())
}

fn main() {
    let mut to_string = VERSION.to_string();

    if to_string.contains("TEST") || to_string.contains("SAND") {
        // TestNet and Sandbox versions < 1.0.0
        to_string.replace_range(..4, "0");
    } else {
        // Main net version >= 1.0.0
        // to_string.replace_range(..4, "1");
        panic!("todo for mainnet");
    };

    let workspace_path = Path::new("../");

    if let Err(e) = update_workspace_packages_version(to_string, workspace_path) {
        panic!("Error updating workspace packages version: {}", e);
    }
}
