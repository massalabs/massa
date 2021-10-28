// Copyright (c) 2021 MASSA LABS <info@massa.net>

use serde::Deserialize;
use std::net::IpAddr;
use std::sync::RwLock;

lazy_static::lazy_static! {
    static ref SETTINGS: RwLock<config::Config> = RwLock::new({
        let mut settings = config::Config::default();
        settings
            .merge(config::File::with_name("base_config/config.toml"))
            .unwrap();
        settings
            .merge(config::File::with_name("config/config.toml"))
            .unwrap();
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
