/// Represents the consistency status of cached blockchain blocks, tracking gaps in the block sequence.
///
/// This struct helps determine the optimal sync strategy by comparing the highest cached block
/// with the last continuous block number. When these values differ, it indicates gaps exist
/// in the cached data that require special handling during synchronization.
#[derive(Debug, Clone)]
pub struct CachedBlocksConsistencyStatus {
    /// The highest block number present in the cache
    pub max_block: u64,
    /// The highest block number in a continuous sequence from the beginning
    pub last_continuous_block: u64,
}

impl CachedBlocksConsistencyStatus {
    #[must_use]
    pub const fn new(max_block: u64, last_continuous_block: u64) -> Self {
        Self {
            max_block,
            last_continuous_block,
        }
    }

    /// Returns true if the cached blocks form a continuous sequence without gaps.
    #[must_use]
    pub const fn is_consistent(&self) -> bool {
        self.max_block == self.last_continuous_block
    }
}
