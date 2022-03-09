// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Build here the default client settings from the config file toml
use massa_models::constants::build_massa_settings;
use massa_time::MassaTime;
use serde::Deserialize;
use std::{net::IpAddr, path::PathBuf};

lazy_static::lazy_static! {
    pub static ref SETTINGS: Settings = build_massa_settings("massa-client", "MASSA_CLIENT");
}

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub default_node: DefaultNode,
    pub history: usize,
    pub history_file_path: PathBuf,
    pub timeout: MassaTime,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DefaultNode {
    pub ip: IpAddr,
    pub private_port: u16,
    pub public_port: u16,
}

#[cfg(test)]
#[test]
fn test_load_client_config() {
    let _ = *SETTINGS;
}
