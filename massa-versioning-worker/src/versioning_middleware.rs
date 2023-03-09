use std::collections::{HashMap, VecDeque};

use massa_models::amount::Amount;
use massa_time::{MassaTime, TimeError};

use crate::versioning::{Advance, MipStore};
use massa_versioning_exports::VersioningError;

/// Struct used to keep track of announced versions in previous blocks
pub struct VersioningMiddleware {
    latest_announcements: VecDeque<u32>,
    counts: HashMap<u32, usize>,
    nb_blocks_considered: usize,
    mip_store: MipStore,
}

impl VersioningMiddleware {
    /// Creates a new empty versioning middleware.
    pub fn new(nb_blocks_considered: usize, mip_store: MipStore) -> Self {
        VersioningMiddleware {
            latest_announcements: VecDeque::with_capacity(nb_blocks_considered + 1),
            counts: HashMap::new(),
            nb_blocks_considered,
            mip_store,
        }
    }

    /// Add the announced version of a new block
    /// If needed, remove the info related to a ancient block
    pub fn new_block(&mut self, version: u32) {
        self.latest_announcements.push_back(version);

        *self.counts.entry(version).or_default() += 1;

        // If the queue is too large, remove the most ancient block and its associated count
        if self.latest_announcements.len() > self.nb_blocks_considered {
            let prev_version = self.latest_announcements.pop_front();
            if let Some(prev_version) = prev_version {
                *self.counts.entry(prev_version).or_insert(1) -= 1;
            }
        }

        //let _ = self.create_and_send_advance_message(*self.counts.get(&version).unwrap(), version);
        let _ = self.create_and_send_advance_message_for_all();
    }

    /*fn get_version_info(&self, version: u32) -> Option<MipInfo> {
        let store = self.mip_store.0.read().0.clone();

        let res: Vec<_> = store
            .iter()
            .filter(|&(k, _v)| k.version == version)
            .collect();

        match res.len() {
            0 => None,
            _ => Some(res[0].0.clone()),
        }
    }*/

    fn create_and_send_advance_message_for_all(&mut self) -> Result<(), VersioningError> {
        let mut store = self.mip_store.0.write();

        let now = MassaTime::now().map_err(|_| {
            VersioningError::ModelsError(massa_models::error::ModelsError::TimeError(
                TimeError::TimeOverflowError,
            ))
        })?;

        // Possible optimisation: filter the store to avoid advancing on failed and active versions
        for (vi, state) in store.0.iter_mut() {
            let ratio_counts = 100.0 * *self.counts.get(&vi.version).unwrap_or(&0) as f32
                / self.nb_blocks_considered as f32;

            let ratio_counts = Amount::from_mantissa_scale(ratio_counts.round() as u64, 0);

            let advance_msg = Advance {
                start_timestamp: vi.start,
                timeout: vi.timeout,
                threshold: ratio_counts,
                now,
            };

            state.on_advance(&advance_msg.clone());

            println!("New state: vi: {:?}, state: {:?}", vi, state);
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::versioning::{MipComponent, MipInfo, MipState, MipStoreRaw};
    use core::time::Duration;
    use parking_lot::RwLock;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use chrono::{NaiveDate, NaiveDateTime};

    use super::*;

    fn get_a_version_info() -> (NaiveDateTime, NaiveDateTime, MipInfo) {
        // A helper function to provide a  default VersioningInfo
        // Models a Massa Improvements Proposal (MIP-0002), transitioning component address to v2

        let start: NaiveDateTime = NaiveDate::from_ymd_opt(2017, 11, 01)
            .unwrap()
            .and_hms_opt(7, 33, 44)
            .unwrap();

        let timeout: NaiveDateTime = NaiveDate::from_ymd_opt(2017, 11, 11)
            .unwrap()
            .and_hms_opt(7, 33, 44)
            .unwrap();

        return (
            start,
            timeout,
            MipInfo {
                name: "MIP-0002".to_string(),
                version: 2,
                component: MipComponent::Address,
                component_version: 1,
                start: MassaTime::from(start.timestamp() as u64),
                timeout: MassaTime::from(timeout.timestamp() as u64),
            },
        );
    }

    #[tokio::test]
    async fn test_versioning_middleware() {
        let (start, _, vi) = get_a_version_info();
        let now_0 = MassaTime::from(start.timestamp() as u64);
        let state = MipState::new(now_0);

        let vs_raw = MipStoreRaw(BTreeMap::from([(vi.clone(), state)]));
        let vs = MipStore(Arc::new(RwLock::new(vs_raw)));

        let mut vm = VersioningMiddleware::new(5, vs.clone());

        assert_eq!(vs.get_network_version_current(), 0);
        assert_eq!(vs.get_network_version_to_announce(), 0);

        vm.new_block(0);
        vm.new_block(0);
        vm.new_block(0);
        vm.new_block(0);
        vm.new_block(0);
        vm.new_block(0);

        assert_eq!(vs.get_network_version_current(), 0);
        assert_eq!(vs.get_network_version_to_announce(), 0);

        tokio::time::sleep(Duration::from_secs(3)).await;

        vm.new_block(0);

        assert_eq!(vs.get_network_version_current(), 0);
        assert_eq!(vs.get_network_version_to_announce(), 2);

        vm.new_block(2);
        vm.new_block(2);
        vm.new_block(2);
        vm.new_block(2);
        vm.new_block(2);

        assert_eq!(vs.get_network_version_current(), 0);
        assert_eq!(vs.get_network_version_to_announce(), 2);

        tokio::time::sleep(Duration::from_secs(2)).await;

        vm.new_block(2);
        vm.new_block(2);
        vm.new_block(2);

        assert_eq!(vs.get_network_version_current(), 2);
        assert_eq!(vs.get_network_version_to_announce(), 0);
    }
}
