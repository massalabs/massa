// Copyright (c) 2021 MASSA LABS <info@massa.net>

use directories::ProjectDirs;
use serde::Deserialize;
use std::net::IpAddr;

lazy_static::lazy_static! {
    // TODO: this code is duplicated from /massa-node/settings.rs and should be part of a custom crate
    pub static ref SETTINGS: Settings = {
        let mut settings = config::Config::default();
        let config_path = std::env::var("MASSA_CONFIG_PATH").unwrap_or_else(|_| "base_config/config.toml".to_string());
        settings.merge(config::File::with_name(&config_path)).unwrap_or_else(|_| panic!("failed to read {} config {}", config_path, std::env::current_dir().unwrap().as_path().to_str().unwrap()));
        if let Some(proj_dirs) = ProjectDirs::from("com", "MassaLabs", "massa-client") { // Portable user config loading
            let user_config_path = proj_dirs.config_dir();
            if user_config_path.exists() {
                let path_str = user_config_path.to_str().unwrap();
                settings.merge(config::File::with_name(path_str)).unwrap_or_else(|_| panic!("failed to read {} user config", path_str));
            }
        }
        settings.merge(config::Environment::with_prefix("MASSA_CLIENT")).unwrap();
        settings.try_into().unwrap()
    };
}

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub default_node: DefaultNode,
    pub history: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DefaultNode {
    pub ip: IpAddr,
    pub private_port: u16,
    pub public_port: u16,
}
