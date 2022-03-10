use tokio::time::Instant;

use massa_models::prehash::{Map, PreHashed};
use massa_protocol_exports::KnowledgeViewConfig;
use massa_time::MassaTime;

#[derive(Debug, Clone)]
pub struct KnowledgeView<U: PreHashed> {
    known: Map<U, (bool, Instant)>,
    wanted: Map<U, Instant>,
    asked: Map<U, Instant>,
}

impl<U: PreHashed> KnowledgeView<U> {
    /// if there are too many items, prune them
    pub fn prune(&mut self, cfg: KnowledgeViewConfig) {
        todo!()
    }

    /// these three function would update the timestamps if needed and all the other updates too
    pub fn insert_known(&mut self, items: &impl Iterator<Item = U>, knows: bool, instant: Instant) {
        todo!()
    }
    pub fn insert_wanted(&mut self, items: &impl Iterator<Item = U>) {
        todo!()
    }
    pub fn insert_asked(&mut self, items: &impl Iterator<Item = U>) {
        todo!()
    }

    pub fn get_known(&self, item: &U) -> Option<(bool, Instant)> {
        todo!()
    }

    pub fn get_asked(&self, item: &U) -> Option<Instant> {
        todo!()
    }

    // the remove functions too
    pub fn remove_known(&mut self, items: &impl Iterator<Item = U>) {
        todo!()
    }

    pub fn contains_and_update_wanted(&mut self, item: &U) -> bool {
        todo!()
    }
    /// remove from wanted items
    /// return true if it was present
    pub fn remove_wanted(&mut self, items: &impl Iterator<Item = U>) -> bool {
        todo!()
    }
    pub fn remove_asked(&mut self, items: &impl Iterator<Item = U>) {
        todo!()
    }

    pub fn knows(&self, item: &U) -> bool {
        todo!()
    }

    pub fn active_request_count(&self, timeout: MassaTime) -> usize {
        let now = Instant::now();
        self.asked
            .iter()
            .filter(|(_h, ask_t)| {
                ask_t
                    .checked_add(timeout.into())
                    .map_or(false, |timeout_t| timeout_t > now)
            })
            .count()
    }
}
