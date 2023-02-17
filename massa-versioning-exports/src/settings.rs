pub struct VersioningConfig {
    /// Nb blocks to consider for versioning stats
    pub nb_blocks_considered: usize,
    /// Proportion of blocks to switch to another version
    pub threshold: f32,
}
