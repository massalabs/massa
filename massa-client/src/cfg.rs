// Copyright (c) 2021 MASSA LABS <info@massa.net>

use serde::Deserialize;
use std::net::IpAddr;
use std::sync::RwLock;

const BASE_CONFIG_PATH: &str = "base_config/config.toml";
const OVERRIDE_CONFIG_PATH: &str = "config/config.toml";

lazy_static::lazy_static! {
    static ref SETTINGS: RwLock<config::Config> = RwLock::new({
        let mut settings = config::Config::default();

        settings
            .merge(config::File::with_name(BASE_CONFIG_PATH))
            .unwrap();

        if std::path::Path::new(OVERRIDE_CONFIG_PATH).is_file() {
            settings
                .merge(config::File::with_name(OVERRIDE_CONFIG_PATH))
                .unwrap();
        }
        settings
            .merge(config::Environment::with_prefix("MASSA_CLIENT"))
            .unwrap();
        settings
    });
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

impl Settings {
    pub(crate) fn load() -> Settings {
        SETTINGS.read().unwrap().clone().try_into().unwrap()
    }
}
