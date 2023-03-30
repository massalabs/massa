// Copyright (c) 2023 MASSA LABS <info@massa.net>
// 
//! ## **Overview**
//!
//! This module contains Protobuf message definitions for the Massa blockchain API. 
//! It uses utilizes the `prost-build` tool to generate Rust code from the Protobuf definitions.
//!
//! ## **Structure**
//! 
//! * `build.rs`: This file contains build instructions for generating Rust code from the Protobuf definitions using the `prost-build` tool.
//! * `proto/`: This directory contains the Protobuf message definitions for the Massa blockchain API
//! * `src/`: This directory contains the generated Rust code for the Protobuf message definitions. 
//! It also includes a `_includes.rs` file for importing the generated Rust modules and an `api.bin` file for server reflection protocol.
//! 
//! ## **Usage**
//! To use this module, simply include it as a dependency in your Rust project's `Cargo.toml` file.
//! You can then import the necessary Rust modules for the Massa API and use the Protobuf messages as needed.
//!
#![warn(unused_crate_dependencies)]

/// Google protos Module
pub mod google {
    /// Google API Module
    pub mod api {
        include!("google.api.rs");
    }
    /// Google RPC Module
    pub mod rpc {
        include!("google.rpc.rs");
    }
}

/// Massa protos Module
pub mod massa {
    /// Massa API Module
    pub mod api {
        /// Version 1 of the Massa protos
        pub mod v1 {
            include!("massa.api.v1.rs");
            /// Compiled file descriptor set for the Massa protos
            pub const FILE_DESCRIPTOR_SET: &[u8] = include_bytes!("api.bin");
        }
    }
}
