use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Serialize, Deserialize)]
pub struct Slot {
    pub period: u64,
    pub thread: u8,
}

impl Slot {
    pub fn new(period: u64, thread: u8) -> Slot {
        Slot { period, thread }
    }
    pub fn from_tuple((period, thread): (u64, u8)) -> Slot {
        Slot { period, thread }
    }
    pub fn into_bytes(&self) -> Vec<u8> {
        let mut vec = Vec::with_capacity(9);
        vec.extend_from_slice(&self.period.to_be_bytes());
        vec.extend_from_slice(&self.thread.to_be_bytes());
        vec
    }
}
