use massa_models::slot::Slot;
use rocksdb::{checkpoint::Checkpoint, DB};

pub fn backup_db(db: &DB, slot: Slot) {
    let mut subpath = String::from("backup_");
    subpath.push_str(slot.period.to_string().as_str());
    subpath.push('_');
    subpath.push_str(slot.thread.to_string().as_str());

    Checkpoint::new(db)
        .expect("Cannot init checkpoint")
        .create_checkpoint(db.path().join(subpath))
        .expect("Failed to create checkpoint");
}
