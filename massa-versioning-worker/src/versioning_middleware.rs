use std::collections::HashMap;

use massa_models::amount::Amount;
use massa_time::{MassaTime, TimeError};
use tracing::debug;

use crate::versioning::{Advance, MipStore};
use massa_versioning_exports::VersioningMiddlewareError;
use queues::{CircularBuffer, IsQueue};

/// Struct used to keep track of announced versions in previous blocks
pub struct VersioningMiddleware {
    latest_announcements: CircularBuffer<u32>,
    counts: HashMap<u32, usize>,
    count_blocks_considered: usize,
    mip_store: MipStore,
}

impl VersioningMiddleware {
    /// Creates a new empty versioning middleware.
    pub fn new(count_blocks_considered: usize, mip_store: MipStore) -> Self {
        VersioningMiddleware {
            latest_announcements: CircularBuffer::new(count_blocks_considered),
            counts: HashMap::new(),
            count_blocks_considered,
            mip_store,
        }
    }

    /// Add the announced version of a new block
    /// If needed, remove the info related to a ancient block
    pub fn new_block(&mut self, version: u32) {
        // This will never return Err for a queue::CircularBuffer
        if let Ok(removed_version) = self.latest_announcements.add(version) {
            // We update the count of the received version
            *self.counts.entry(version).or_default() += 1;

            if let Some(prev_version) = removed_version {
                // We update the count of the oldest version in the buffer, if at capacity
                *self.counts.entry(prev_version).or_insert(1) -= 1;
            }
        }

        let _ = self.create_and_send_advance_message_for_all();
    }

    fn create_and_send_advance_message_for_all(&mut self) -> Result<(), VersioningMiddlewareError> {
        let mut store = self.mip_store.0.write();

        let now = MassaTime::now().map_err(|_| {
            VersioningMiddlewareError::ModelsError(massa_models::error::ModelsError::TimeError(
                TimeError::TimeOverflowError,
            ))
        })?;

        // Possible optimisation: filter the store to avoid advancing on failed and active versions
        for (vi, state) in store.0.iter_mut() {
            let ratio_counts = 100.0 * *self.counts.get(&vi.version).unwrap_or(&0) as f32
                / self.count_blocks_considered as f32;

            let ratio_counts = Amount::from_mantissa_scale(ratio_counts.round() as u64, 0);

            let advance_msg = Advance {
                start_timestamp: vi.start,
                timeout: vi.timeout,
                threshold: ratio_counts,
                now,
            };

            state.on_advance(&advance_msg.clone());

            debug!("New state: vi: {:?}, state: {:?}", vi, state);
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

    use super::*;

    fn get_a_version_info(start: MassaTime, timeout: MassaTime) -> MipInfo {
        // A helper function to provide a  default MipInfo
        // Models a Massa Improvements Proposal (MIP-0002), transitioning component address to v2

        MipInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            components: HashMap::from([(MipComponent::Address, 2)]),
            start,
            timeout,
        }
    }

    #[tokio::test]
    async fn test_versioning_middleware() {
        let now = MassaTime::now().unwrap();

        let start = now.checked_add(MassaTime::from(2000)).unwrap();
        let timeout = now.checked_add(MassaTime::from(4000)).unwrap();

        let vi = get_a_version_info(start, timeout);

        let state = MipState::new(now);

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
