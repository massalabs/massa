use std::time::Instant;

use massa_models::prehash::{Map, PreHashed};
use massa_protocol_exports::KnowledgeViewConfig;

#[derive(Debug, Clone)]
pub struct KnowledgeView<U: PreHashed> {
    known: Map<U, (bool, Instant)>,
    wanted: Map<U, Instant>,
    asked: Map<U, Instant>,
}

impl<U: PreHashed> KnowledgeView<U> {
    /// if there are too many items, prune them
    fn prune(&mut self, cfg: KnowledgeViewConfig) {
        todo!()
    }

    /// these three function would update the timestamps if needed and all the other updates too
    fn insert_known(&mut self, item: U, knows: bool) {
        todo!()
    }
    fn insert_wanted(&mut self, item: U) {
        todo!()
    }
    fn insert_asked(&mut self, item: U) {
        todo!()
    }

    // the remove functions too
    fn remove_known(&mut self, item: U) {
        todo!()
    }
    fn remove_wanted(&mut self, item: U) {
        todo!()
    }
    fn remove_asked(&mut self, item: U) {
        todo!()
    }

    fn knows(&self, item: U) -> bool {
        todo!()
    }
}
