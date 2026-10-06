use massa_models::config::constants::VERSION;
use std::fs;
use std::path::Path;
use toml_edit::{Document, Formatted, Item, Value};

/// update the version shared by the workspace packages
///
/// Massa packages inherit it with `version.workspace = true`, so only the
/// `[workspace.package]` table of the root Cargo.toml needs to be updated
///
/// return Ok(true) if the version has been updated
fn update_workspace_package_version(
    new_version: String,
    cargo_toml_path: &Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    let cargo_toml_content = fs::read_to_string(cargo_toml_path)?;
    let mut doc = cargo_toml_content.parse::<Document>()?;

    let version = doc["workspace"]["package"]
        .as_table_mut()
        .and_then(|package| package.get_mut("version"))
        .ok_or("missing [workspace.package] version in root Cargo.toml")?;

    let to_string = version.to_string().replace('\"', "");
    let actual_version = to_string.trim();
    if new_version.eq(actual_version) {
        return Ok(false);
    }

    println!(
        "Updating workspace package version from {} to {}",
        actual_version, new_version
    );
    *version = Item::Value(Value::String(Formatted::new(new_version)));
    fs::write(cargo_toml_path, doc.to_string())?;

    Ok(true)
}

pub(crate) fn update_package_versions() {
    println!("Updating package versions");
    let mut to_string = VERSION.to_string();

    if to_string.contains("SECU") || to_string.contains("SAND") {
        // TestNet and Sandbox versions < 1.0.0
        to_string.replace_range(..4, "0");
    } else {
        // Main net version >= 1.0.0
        to_string.replace_range(..5, "");
        to_string.push_str(".0");
    };

    let cargo_toml_path = Path::new("./Cargo.toml");

    match update_workspace_package_version(to_string, cargo_toml_path) {
        Err(e) => panic!("Error updating workspace package version: {}", e),
        Ok(updated) => println!("workspace package version updated: {}", updated),
    }
}
