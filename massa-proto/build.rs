// Copyright (c) 2023 MASSA LABS <info@massa.net>

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "build-tonic")]
    tonic::build()?;

    Ok(())
}

#[cfg(feature = "build-tonic")]
mod tonic {
    use glob::glob;
    use std::{path::PathBuf, process::Command};

    /// This function is responsible for building the Massa protobuf API and generating documentation
    pub(crate) fn build() -> Result<(), Box<dyn std::error::Error>> {
        // Find all protobuf files in the 'proto/massa/' directory
        let protos = find_protos("proto/massa/")?;

        // Configure and compile the protobuf API
        tonic_build::configure()
            .build_server(true)
            .build_transport(true)
            .build_client(true)
            .type_attribute(
                ".google.api.HttpRule",
                "#[cfg(not(doctest))]\n\
             #[allow(dead_code)]\n\
             pub(crate)  struct HttpRuleComment{}\n\
             /// HACK: see docs in [`HttpRuleComment`] ignored in doctest pass",
            )
            .file_descriptor_set_path("src/api.bin")
            .include_file("_includes.rs")
            .out_dir("src/")
            .compile(
                &protos,
                &["proto/massa/api/v1/", "proto/third-party"], // specify the root location to search proto dependencies
            )
            .map_err(|e| format!("protobuf compilation error: {:?}", e))?;

        // Generate documentation for the protobuf API
        generate_doc(&protos).map_err(|e| format!("protobuf documentation error: {:?}", e))?;

        // Return Ok if the build and documentation generation were successful
        Ok(())
    }

    /// Find all .proto files in the specified directory and its subdirectories
    fn find_protos(dir_path: &str) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
        let glob_pattern = format!("{dir_path}/**/*.proto", dir_path = dir_path);
        let paths = glob(&glob_pattern)?.flatten().collect();

        Ok(paths)
    }

    /// Generate markdown and HTML documentation for the given protocol buffer files
    fn generate_doc(protos: &Vec<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
        // Generate markdown documentation using protoc.
        let protoc_md_cmd_output = Command::new("protoc")
            .args(protos)
            .arg("--proto_path=./proto/massa/api/v1")
            .arg("--proto_path=./proto/third-party")
            .arg("--doc_out=./doc")
            .arg("--doc_opt=markdown,api.md")
            .output()?;

        // If protoc failed to generate markdown documentation, return an error.
        if !protoc_md_cmd_output.status.success() {
            return Err(format!(
                "protoc generate MARKDOWN documentation failed: {}",
                String::from_utf8_lossy(&protoc_md_cmd_output.stderr)
            )
            .into());
        }

        // Generate HTML documentation using protoc.
        let protoc_html_cmd_output = Command::new("protoc")
            .args(protos)
            .arg("--proto_path=./proto/massa/api/v1")
            .arg("--proto_path=./proto/third-party")
            .arg("--doc_out=./doc")
            .arg("--doc_opt=html,index.html")
            .output()?;

        // If protoc failed to generate HTML documentation, return an error.
        if !protoc_html_cmd_output.status.success() {
            return Err(format!(
                "protoc generate HTML documentation failed: {}",
                String::from_utf8_lossy(&protoc_md_cmd_output.stderr)
            )
            .into());
        }

        Ok(())
    }
}
