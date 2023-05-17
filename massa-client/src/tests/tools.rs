extern crate toml_edit;

use massa_time::MassaTime;
use std::fs;
use toml_edit::{value, Document};

pub(crate) fn _update_genesis_timestamp(config_path: &str) {
    let toml = fs::read_to_string(config_path).expect("Unable to read file");
    let mut doc = toml.parse::<Document>().unwrap();
    doc["consensus"]["genesis_timestamp"] =
        value(format!("{}", MassaTime::now().unwrap().to_millis()));
    fs::write(config_path, doc.to_string()).expect("Unable to write file");
}
