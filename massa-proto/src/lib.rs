//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Massa protos module
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

/// models mapping
pub mod mapping;
