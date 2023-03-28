#[derive(Clone)]
pub struct VersioningConfig {
    /// Nb blocks to consider for versioning stats
    pub count_blocks_considered: usize,
}
