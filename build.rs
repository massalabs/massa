// Example custom build script.
fn main() {
    let _ = std::process::Command::new("git")
        .args(&["submodule", "update", "--init"])
        .status();
}
