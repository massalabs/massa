extern crate toml_edit;

use std::fs;
use time::UTime;
use toml_edit::{value, Document};

pub fn update_genesis_timestamp(config_path: &str) {
    let toml = fs::read_to_string(config_path).expect("Unable to read file");
    let mut doc = toml.parse::<Document>().unwrap();
    doc["consensus"]["genesis_timestamp"] = value(format!(
        "{}",
        UTime::now(10000 * 60 * 60).unwrap().to_millis()
    ));
    fs::write(config_path, doc.to_string()).expect("Unable to write file");
}
