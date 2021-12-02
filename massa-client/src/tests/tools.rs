extern crate toml_edit;

use std::fs;
use time::UTime;
use toml_edit::{value, Document};

// TODO: move me in massa-node tests to fix ... see #1269
fn _update_genesis_timestamp() {
    let toml = fs::read_to_string("base_config/config.toml").expect("Unable to read file");
    let mut doc = toml.parse::<Document>().unwrap();
    doc["consensus"]["genesis_timestamp"] =
        value(format!("{}", UTime::now(1000 * 60).unwrap().to_millis()));
    doc["consensus"]["end_timestamp"] = value(format!(
        "{}",
        UTime::now(1000 * 60 * 60).unwrap().to_millis()
    ));
    fs::write("config/config.toml", doc.to_string()).expect("Unable to write file");
}
