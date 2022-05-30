// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Build here the default client settings from the configuration file toml
#[cfg(not(feature = "testing"))]
use massa_models::constants::build_massa_settings;
use massa_time::MassaTime;
use serde::Deserialize;
use std::{net::SocketAddr, path::PathBuf};

#[cfg(feature = "testing")]
lazy_static::lazy_static! {
    pub static ref SETTINGS: Settings = Default::default();
}

#[cfg(not(feature = "testing"))]
lazy_static::lazy_static! {
    pub static ref SETTINGS: Settings = build_massa_settings("massa-client", "MASSA_CLIENT");
}

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub default_node: NodeAddr,
    pub history: usize,
    pub history_file_path: PathBuf,
    pub timeout: MassaTime,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_node: Default::default(),
            history: 10,
            history_file_path: "config/.massa_history".into(),
            timeout: 1000.into(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct NodeAddr {
    pub public_ip: SocketAddr,
    pub private_ip: SocketAddr,
}

impl Default for NodeAddr {
    fn default() -> Self {
        Self {
            public_ip: "0.0.0.0:33035".parse().unwrap(),
            private_ip: "127.0.0.1:33034".parse().unwrap(),
        }
    }
}

#[cfg(test)]
#[test]
fn test_load_client_config() {
    let _ = *SETTINGS;
}
