use massa_models::config::constants::VERSION;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("hello world");

    let mut to_string = VERSION.to_string();

    if to_string.contains("TEST") || to_string.contains("SAND") {
        to_string.replace_range(0..3, "0");
    } else {
        to_string.replace_range(0..3, "1");
    };

    println!("version: {}", to_string);

    // let dest_path = Path::new("/Users/urvoy/dev/massa").join("hello.rs");
    // fs::write(
    //     &dest_path,
    //     "pub fn message() -> &'static str {
    //         \"Hello, World!\"
    //     }
    //     ",
    // )
    // .unwrap();

    let workspace_path = Path::new("../"); // Mettez ici le chemin vers votre workspace

    let output = Command::new("cargo")
        .arg("run")
        .arg("--manifest-path")
        .arg(workspace_path.join("Cargo.toml"))
        .env("CARGO_MANIFEST_DIR", workspace_path)
        .env("NEW_VERSION", to_string)
        .status()
        .expect("Erreur lors de l'exécution du script de pré-construction.");

    if output.success() {
        println!("Script de pré-construction exécuté avec succès !");
    } else {
        eprintln!("Erreur lors de l'exécution du script de pré-construction.");
    }
}
