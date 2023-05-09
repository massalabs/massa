pub(crate)  mod google {
    pub(crate)  mod api {
        include!("google.api.rs");
    }
    pub(crate)  mod rpc {
        include!("google.rpc.rs");
    }
}
pub(crate)  mod massa {
    pub(crate)  mod api {
        pub(crate)  mod v1 {
            include!("massa.api.v1.rs");
        }
    }
}
