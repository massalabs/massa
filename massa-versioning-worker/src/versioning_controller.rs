use std::collections::{HashMap, VecDeque};

const NB_BLOCKS_CONSIDERED: usize = 5;
const THRESHOLD: usize = 3;

/// Struct used to keep track of announced versions in previous blocks
/// Contains additional utilities
pub struct VersioningMiddleware {
    latest_announcements: VecDeque<u32>,
    counts: HashMap<u32, usize>,
    current_active_version: u32,
    current_supported_versions: Vec<u32>,
    current_announced_version: u32,
}

impl Default for VersioningMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl VersioningMiddleware {
    /// Creates a new empty versioning middleware.
    pub fn new() -> Self {
        VersioningMiddleware {
            latest_announcements: VecDeque::with_capacity(NB_BLOCKS_CONSIDERED + 1),
            counts: HashMap::new(),
            current_active_version: 0,
            current_supported_versions: Vec::new(),
            current_announced_version: 0,
        }
    }

    /// Add the announced version of a new block
    /// If needed, remove the info related to a ancient block
    pub fn new_block(&mut self, version: u32) {
        self.latest_announcements.push_back(version);

        *self.counts.entry(version).or_default() += 1;

        // If the queue is too large, remove the most ancient block and its associated count
        if self.latest_announcements.len() > NB_BLOCKS_CONSIDERED {
            let prev_version = self.latest_announcements.pop_front();
            if let Some(prev_version) = prev_version {
                *self.counts.entry(prev_version).or_insert(1) -= 1;
            }
        }
    }

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
            Some(count) => *count >= THRESHOLD,
            None => false,
        }
    }

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
        self.current_supported_versions.clone()
    }

    /// Add a new version to the currently supported versions list
    pub fn add_supported_version(&mut self, version: u32) {
        if !self.current_supported_versions.contains(&version) {
            self.current_supported_versions.push(version)
        }
    }

    /// Remove a version to the currently supported versions list
    pub fn remove_supported_version(&mut self, version: u32) {
        self.current_supported_versions.retain(|&x| x != version)
    }

    /// Get the current version to announce
    pub fn get_current_announced_version(&self) -> u32 {
        self.current_announced_version
    }

    /// Set the current version to announce
    pub fn set_current_announced_version(&mut self, version: u32) {
        self.current_announced_version = version;
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_versioning_middleware() {
        let mut vm = VersioningMiddleware::new();

        assert!(THRESHOLD <= NB_BLOCKS_CONSIDERED);

        //In the following, we assume:
        //const NB_BLOCKS_CONSIDERED: usize = 5;
        //const THRESHOLD: usize = 3;
        vm.new_block(1);
        vm.new_block(1);

        assert!(!vm.is_count_above_threshold(1));

        vm.new_block(2);
        vm.new_block(1);

        assert!(vm.is_count_above_threshold(1));

        vm.new_block(2);
        vm.new_block(2);

        assert!(vm.is_count_above_threshold(2));
        assert!(vm.most_announced_version() == (2, 3));

        vm.new_block(1);
        vm.new_block(1);

        assert!(!vm.is_count_above_threshold(2));
    }
}
