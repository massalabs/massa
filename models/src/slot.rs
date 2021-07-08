use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Serialize, Deserialize)]
pub struct Slot {
    period: u64,
    thread: u8,
}

impl Slot {
    pub fn new(period: u64, thread: u8) -> Slot {
        Slot { period, thread }
    }
    pub fn new_tuple((period, thread): (u64, u8)) -> Slot {
        Slot { period, thread }
    }
    pub fn into_bytes(&self) -> Vec<u8> {
        let mut vec = Vec::with_capacity(9);
        vec.extend_from_slice(&self.period.to_be_bytes());
        vec[8] = self.thread;
        vec
    }
}
