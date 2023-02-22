use massa_models::versioning::{Advance, VersioningInfo, VersioningStore};
use massa_versioning_exports::VersioningError;
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

/// Struct used to keep track of announced versions in previous blocks
pub struct VersioningMiddleware {
    latest_announcements: VecDeque<u32>,
    counts: HashMap<u32, usize>,
    nb_blocks_considered: usize,
    versioning_store: VersioningStore,
}

impl VersioningMiddleware {
    /// Creates a new empty versioning middleware.
    pub fn new(nb_blocks_considered: usize, versioning_store: VersioningStore) -> Self {
        VersioningMiddleware {
            latest_announcements: VecDeque::with_capacity(nb_blocks_considered + 1),
            counts: HashMap::new(),
            nb_blocks_considered,
            versioning_store,
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

    fn get_version_info(&self, version: u32) -> Option<VersioningInfo> {
        let store = self.versioning_store.0.read().versioning_info.clone();

        let res: Vec<_> = store
            .iter()
            .filter(|&(k, _v)| k.version == version)
            .collect();

        match res.len() {
            0 => None,
            _ => Some(res[0].0.clone()),
        }
    }

    fn create_and_send_advance_message_for_all(&mut self) -> Result<(), VersioningError> {

        let mut store = self.versioning_store.0.write();
        
        let now = Instant::now();

        // Possible optimisation: filter the store to avoid advancing on failed and active versions
        for (vi,state) in store.versioning_info.iter_mut() {
            
            let ratio_counts = 100.0 * *self.counts.get(&vi.version).unwrap_or(&0) as f32 / self.nb_blocks_considered as f32;

            let advance_msg = Advance {
                start_timestamp: vi.start,
                timeout: vi.timeout,
                threshold: ratio_counts,
                now,
            };

            *state = state.on_advance(advance_msg);

            println!("New state: vi: {:?}, state: {:?}", vi, state);
        }

        Ok(())
    }

    fn create_and_send_advance_message(
        &self,
        count: usize,
        version: u32,
    ) -> Result<(), VersioningError> {
        match self.get_version_info(version) {
            None => {
                return Err(VersioningError::ModelsError(
                    massa_models::error::ModelsError::InvalidVersionError(String::from(
                        "Unknown version",
                    )),
                ))
            }
            Some(vi) => {
                let now = std::time::Instant::now();

                let ratio_counts = 100.0 * count as f32 / self.nb_blocks_considered as f32;

                let advance_msg = Advance {
                    start_timestamp: vi.start,
                    timeout: vi.timeout,
                    threshold: ratio_counts,
                    now,
                };

                let store_clone = self.versioning_store.clone();

                let mut raw_store = store_clone.0.write();

                let state = raw_store.versioning_info.get_mut(&vi).unwrap();

                *state = state.on_advance(advance_msg);
            }
        }
        Ok(())
    }

    /*
        /// Get version that was announced the most in the previous blocks,
        /// as well as the count
        pub fn most_announced_version(&mut self) -> (u32, usize) {
            match self.counts.iter().max_by_key(|(_, &v)| v) {
                Some((max_count_version, count)) => (*max_count_version, *count),
                None => (0, 0),
            }
        }

        /// Checks whether a given version should become active
        pub fn is_count_above_threshold(&mut self, version: u32) -> bool {
            match self.counts.get(&version) {
                Some(count) => *count >= (self.threshold * self.nb_blocks_considered as f32) as usize,
                None => false,
            }
        }
    */

    /*
        /// Get the current active version
        pub fn get_current_active_version(&self) -> u32 {
            self.current_active_version
        }

        /// Set the current active version
        pub fn set_current_active_version(&mut self, version: u32) {
            self.current_active_version = version;
        }

        /// Get the current supported versions
        pub fn get_current_supported_versions(&self) -> Vec<u32> {
            (*self.current_supported_versions.read().unwrap()).clone()
        }

        /// Add a new version to the currently supported versions list
        pub fn add_supported_version(&mut self, version: u32) {
            let mut write_supported_versions = (*self.current_supported_versions.write().unwrap()).clone();
            if !write_supported_versions.contains(&version) {
                write_supported_versions.push(version)
            }
        }

        /// Remove a version to the currently supported versions list
        pub fn remove_supported_version(&mut self, version: u32) {
            let mut write_supported_versions = (*self.current_supported_versions.write().unwrap()).clone();
            write_supported_versions.retain(|&x| x != version)
        }

        /// Get the current version to announce
        pub fn get_current_announced_version(&self) -> u32 {
            *self.current_announced_version.read().unwrap()
        }

        /// Set the current version to announce
        pub fn set_current_announced_version(&mut self, version: u32) {
            let mut write_announced_version = *self.current_announced_version.write().unwrap();
            write_announced_version = version;
        }
    */
}

#[cfg(test)]
mod test {
    use massa_models::versioning::VersioningComponent;
    use parking_lot::RwLock;
    use std::sync::Arc;
    use std::time::Instant;

    use core::time::Duration;
    use massa_models::versioning::VersioningState;
    use massa_models::versioning::VersioningStoreRaw;

    use super::*;

    fn get_default_version_info() -> VersioningInfo {
        // A default VersioningInfo used in many tests
        // Models a Massa Improvments Proposal (MIP-0002), transitioning component address to v2
        return VersioningInfo {
            name: "MIP-0002".to_string(),
            version: 2,
            component: VersioningComponent::Address,
            start: Instant::now() + Duration::from_secs(2),
            timeout: Instant::now() + Duration::from_secs(5),
        };
    }

    #[tokio::test]
    async fn test_versioning_middleware() {
        let vi = get_default_version_info();
        let state: VersioningState = Default::default();

        let mut raw_store = VersioningStoreRaw {
            versioning_info: HashMap::new(),
        };
        raw_store.versioning_info.insert(vi, state);

        let store = VersioningStore(Arc::new(RwLock::new(raw_store)));
        let mut vm = VersioningMiddleware::new(5, store.clone());

        assert_eq!(store.get_current_active_version(), 0);
        assert_eq!(store.get_current_version_to_announce(), 0);

        vm.new_block(0);
        vm.new_block(0);
        vm.new_block(0);
        vm.new_block(0);
        vm.new_block(0);
        vm.new_block(0);

        assert_eq!(store.get_current_active_version(), 0);
        assert_eq!(store.get_current_version_to_announce(), 0);

        tokio::time::sleep(Duration::from_secs(3)).await;
        
        vm.new_block(0);

        assert_eq!(store.get_current_active_version(), 0);
        assert_eq!(store.get_current_version_to_announce(), 2);

        vm.new_block(2);
        vm.new_block(2);
        vm.new_block(2);
        vm.new_block(2);
        vm.new_block(2);

        assert_eq!(store.get_current_active_version(), 0);
        assert_eq!(store.get_current_version_to_announce(), 2);

        tokio::time::sleep(Duration::from_secs(5)).await;

        vm.new_block(2);
        vm.new_block(2);
        vm.new_block(2);
        
        assert_eq!(store.get_current_active_version(), 2);
        assert_eq!(store.get_current_version_to_announce(), 2);
    }
}
