// Copyright (c) 2023 MASSA LABS <info@massa.net>

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "build-tonic")]
    tonic::build()?;

    #[cfg(not(feature = "build-tonic"))]
    println!("cargo:warning=build-tonic feature is disabled, you can update the generated code from protobuf files by running: cargo check --features build-tonic");

    Ok(())
}

#[cfg(feature = "build-tonic")]
mod tonic {
    use glob::glob;
    use std::path::PathBuf;

    pub fn build() -> Result<(), Box<dyn std::error::Error>> {
        //TODO add download external protos files instead of doing it manually
        let protos = find_protos("proto/massa/");

        tonic_build::configure()
            .build_server(true)
            .build_transport(true)
            .build_client(true)
            .type_attribute(
                ".google.api.HttpRule",
                "#[cfg(not(doctest))]\n\
                 #[allow(dead_code)]\n\
                 pub struct HttpRuleComment{}\n\
                 /// HACK: see docs in [`HttpRuleComment`] ignored in doctest pass",
            )
            .file_descriptor_set_path("src/api.bin")
            .include_file("_includes.rs")
            .out_dir("src/")
            .compile(
                &protos,
                &["proto/massa/api/v1/", "proto/third-party"], // specify the root location to search proto dependencies
            )
            .map_err(|e| e.into())
    }

    fn find_protos(dir_path: &str) -> Vec<PathBuf> {
        glob(&format!("{dir_path}/**/*.proto"))
            .unwrap()
            .flatten()
            .collect()
    }
}
