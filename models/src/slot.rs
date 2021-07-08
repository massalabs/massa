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
    pub fn to_bytes(&self) -> [u8; 9] {
        let mut res = [0u8; 9];
        res[..8].clone_from_slice(&self.period.to_be_bytes());
        res[8] = self.thread;
        res
    }
}
