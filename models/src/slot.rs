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
<<<<<<< HEAD
    pub fn into_bytes(&self) -> [u8; 9] {
        let mut bytes = [0; 9];
        bytes[..8].copy_from_slice(&self.period.to_be_bytes());
        bytes[8] = self.thread.to_be_bytes()[0];
        bytes
        /*        let mut vec = Vec::with_capacity(9);
        vec.extend_from_slice(&self.period.to_be_bytes());
        //vec.push(255);
        vec.extend_from_slice(&self.thread.to_be_bytes());
        //vec.insert(8, self.thread);
        vec */
=======
    pub fn to_bytes(&self) -> [u8; 9] {
        let mut res = [0u8; 9];
        res[..8].clone_from_slice(&self.period.to_be_bytes());
        res[8] = self.thread;
        res
>>>>>>> 4183eda9654961e4f2f69797364b7b9de51dad01
    }
}
