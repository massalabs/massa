// Copyright (c) 2021 MASSA LABS <info@massa.net>

extern crate directories;
use bootstrap::settings::BootstrapSettings;
use consensus::ConsensusConfig;
use directories::ProjectDirs;
use models::api::APISettings;
use models::Version;
use network::NetworkConfig;
use pool::PoolSettings;
use protocol_exports::ProtocolSettings;
use serde::Deserialize;

lazy_static::lazy_static! {
    pub static ref SETTINGS: Settings = {
        println!("dfsqf");
        let mut settings = config::Config::default();
        let config_path = std::env::var("MASSA_CONFIG_PATH").unwrap_or_else(|_| "base_config/config.toml".to_string());
        settings.merge(config::File::with_name(&config_path)).unwrap_or_else(|_| panic!("failed to read {} config.... {}", config_path, std::env::current_dir().unwrap().as_path().to_str().unwrap()));
        if let Some(proj_dirs) = ProjectDirs::from("com", "MassaLabs", "massa.toml") { // Portable user config loading
            let user_config_path = proj_dirs.config_dir();
            if user_config_path.exists() {
                let path_str = user_config_path.to_str().unwrap();
                settings.merge(config::File::with_name(path_str)).unwrap_or_else(|_| panic!("failed to read {} user config", path_str));
            }
        }
        settings.merge(config::Environment::with_prefix("MASSA_NODE")).unwrap();
        settings.try_into().unwrap()
    };
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub struct LoggingSettings {
    pub level: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub logging: LoggingSettings,
    pub protocol: ProtocolSettings,
    pub network: NetworkConfig,
    pub consensus: ConsensusConfig,
    pub api: APISettings,
    pub bootstrap: BootstrapSettings,
    pub pool: PoolSettings,
    pub version: Version,
}
