// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::massa::api::v1::{IndexedSlot, Slot};

impl From<massa_models::slot::IndexedSlot> for IndexedSlot {
    fn from(s: massa_models::slot::IndexedSlot) -> Self {
        IndexedSlot {
            index: s.index as u64,
            slot: Some(s.slot.into()),
        }
    }
}

impl From<massa_models::slot::Slot> for Slot {
    fn from(s: massa_models::slot::Slot) -> Self {
        Slot {
            period: s.period,
            thread: s.thread as u32,
        }
    }
}
