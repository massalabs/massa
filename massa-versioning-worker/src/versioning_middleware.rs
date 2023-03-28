use std::collections::HashMap;

use massa_models::amount::Amount;
use massa_time::{MassaTime, TimeError};
use tracing::debug;

use crate::versioning::{Advance, MipStore};
use massa_versioning_exports::{VersioningConfig, VersioningMiddlewareError};
use queues::{CircularBuffer, IsQueue};

/// Struct used to keep track of announced versions in previous blocks
pub struct VersioningMiddleware {
    config: VersioningConfig,
    latest_announcements: CircularBuffer<u32>,
    counts: HashMap<u32, usize>,
    mip_store: MipStore,
}

impl VersioningMiddleware {
    /// Creates a new empty versioning middleware.
    pub fn new(config: VersioningConfig, mip_store: MipStore) -> Self {
        VersioningMiddleware {
            config: config.clone(),
            latest_announcements: CircularBuffer::new(config.count_blocks_considered),
            counts: HashMap::new(),
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

        // TODO / OPTIM: filter the store to avoid advancing on failed and active versions
        for (vi, state) in store.0.iter_mut() {
            let ratio_counts = 100.0 * *self.counts.get(&vi.version).unwrap_or(&0) as f32
                / self.config.count_blocks_considered as f32;

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
    use super::*;

    use core::time::Duration;
    use std::str::FromStr;

    use crate::versioning::{ComponentState, MipComponent, MipInfo, MipState, MipStoreRaw};

    // test only
    impl MipStoreRaw {
        /// A helper function to query MipStoreRaw
        fn get_state_for(&self, mi: &MipInfo) -> Option<&MipState> {
            self.0.get(mi)
        }
    }

    impl MipStore {
        /// A helper function to query MipStore
        fn get_state_for(&self, mi: &MipInfo) -> MipState {
            let store = self.0.read();
            store.get_state_for(mi).unwrap().clone()
        }
    }

    fn get_a_version_info(start: MassaTime, timeout: MassaTime) -> MipInfo {
        // A helper function to provide a  default MipInfo
        // Models a Massa Improvements Proposal (MIP-0001), transitioning component address to v1

        MipInfo {
            name: "MIP-0001".to_string(),
            version: 1,
            components: HashMap::from([(MipComponent::Address, 1)]),
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

        let vs = MipStore::try_from([(vi.clone(), state)]).expect("Cannot create the MipStore");

        let versioning_config = VersioningConfig {
            count_blocks_considered: 5,
        };

        let mut vm = VersioningMiddleware::new(versioning_config, vs.clone());

        // The MIP-0001 hasn't Started yet, so we do not announce it.
        assert_eq!(vs.get_state_for(&vi).inner, ComponentState::defined());
        assert_eq!(vs.get_network_version_current(), 0);
        assert_eq!(vs.get_network_version_to_announce(), 0);

        vm.new_block(0);
        // Need to wait a least 1ms between each block otherwise MipState filtered the advance msg
        tokio::time::sleep(Duration::from_millis(1)).await;
        vm.new_block(0);
        tokio::time::sleep(Duration::from_millis(1)).await;
        vm.new_block(0);
        tokio::time::sleep(Duration::from_millis(1)).await;
        vm.new_block(0);
        tokio::time::sleep(Duration::from_millis(1)).await;
        vm.new_block(0);
        tokio::time::sleep(Duration::from_millis(1)).await;
        vm.new_block(0);

        // The MIP-0001 hasn't Started yet, so we do not announce it.
        assert_eq!(vs.get_state_for(&vi).inner, ComponentState::defined());
        assert_eq!(vs.get_network_version_current(), 0);
        assert_eq!(vs.get_network_version_to_announce(), 0);

        tokio::time::sleep(Duration::from_secs(2)).await;

        vm.new_block(0);

        // The MIP-0001 has Started, we start announcing it.
        assert_eq!(
            vs.get_state_for(&vi).inner,
            ComponentState::started(Amount::zero())
        );
        assert_eq!(vs.get_network_version_current(), 0);
        assert_eq!(vs.get_network_version_to_announce(), 1);

        tokio::time::sleep(Duration::from_millis(1)).await;
        vm.new_block(1);
        assert_eq!(
            vs.get_state_for(&vi).inner,
            ComponentState::started(Amount::from_str("20.0").unwrap())
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
        vm.new_block(1);
        tokio::time::sleep(Duration::from_millis(1)).await;
        vm.new_block(1);
        tokio::time::sleep(Duration::from_millis(1)).await;
        vm.new_block(1);
        tokio::time::sleep(Duration::from_millis(1)).await;

        // Enough people voted for MIP-0001, but Timeout has not occurred, so we still announce it and don't consider it active
        assert_eq!(vs.get_state_for(&vi).inner, ComponentState::locked_in());
        assert_eq!(vs.get_network_version_current(), 0);
        assert_eq!(vs.get_network_version_to_announce(), 1);

        tokio::time::sleep(Duration::from_millis(2200)).await;
        vm.new_block(1);

        // Timeout occurred, so we don't announce MIP-0001 anymore, and consider it active
        assert_eq!(vs.get_state_for(&vi).inner, ComponentState::active());
        assert_eq!(vs.get_network_version_current(), 1);
        assert_eq!(vs.get_network_version_to_announce(), 0);
    }
}
