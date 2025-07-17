// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Performance reporting and metrics tracking for blockchain operations.

use std::{fmt::Display, time::Instant};

#[derive(Debug, Clone)]
pub enum BlockchainItem {
    Blocks,
    PoolCreatedEvents,
}

impl Display for BlockchainItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Tracks performance metrics during block synchronization
#[derive(Debug)]
pub struct BlockchainSyncReporter {
    item: BlockchainItem,
    start_time: Instant,
    last_progress_time: Instant,
    blocks_processed: u64,
    total_blocks: u64,
    progress_update_interval: u64,
    next_progress_threshold: u64,
}

impl BlockchainSyncReporter {
    /// Creates a new metrics tracker for block synchronization
    #[must_use]
    pub fn new(
        item: BlockchainItem,
        from_block: u64,
        total_blocks: u64,
        update_interval: u64,
    ) -> Self {
        let now = Instant::now();
        Self {
            item,
            start_time: now,
            last_progress_time: now,
            blocks_processed: 0,
            total_blocks,
            progress_update_interval: update_interval,
            next_progress_threshold: from_block + update_interval,
        }
    }

    /// Updates metrics after a database operation
    pub const fn update(&mut self, batch_size: usize) {
        self.blocks_processed += batch_size as u64;
    }

    /// Checks if progress should be logged based on the current block number
    #[must_use]
    pub const fn should_log_progress(&self, block_number: u64, current_block: u64) -> bool {
        block_number >= self.next_progress_threshold || block_number >= current_block
    }

    /// Logs current progress with detailed metrics
    pub fn log_progress(&mut self, block_number: u64) {
        let elapsed = self.start_time.elapsed();
        let interval_elapsed = self.last_progress_time.elapsed();
        let interval_blocks =
            if block_number > self.next_progress_threshold - self.progress_update_interval {
                block_number - (self.next_progress_threshold - self.progress_update_interval)
            } else {
                self.progress_update_interval
            };

        // Calculate rates
        let avg_rate = self.blocks_processed as f64 / elapsed.as_secs_f64();
        let current_rate = interval_blocks as f64 / interval_elapsed.as_secs_f64();
        let progress_pct =
            (self.blocks_processed as f64 / self.total_blocks as f64 * 100.0).min(100.0);

        tracing::info!(
            "Syncing {} progress: {:.1}% | Block: {} | Rate: {:.0} blocks/s | Avg: {:.0} blocks/s",
            self.item,
            progress_pct,
            block_number,
            current_rate,
            avg_rate,
        );

        self.next_progress_threshold = block_number + self.progress_update_interval;
        self.last_progress_time = Instant::now();
    }

    /// Logs final statistics summary
    pub fn log_final_stats(&self) {
        let total_elapsed = self.start_time.elapsed();
        let avg_rate = self.blocks_processed as f64 / total_elapsed.as_secs_f64();
        tracing::info!(
            "Finished syncing {} | Total: {} blocks in {:.1}s | Avg rate: {:.0} blocks/s",
            self.item,
            self.blocks_processed,
            total_elapsed.as_secs_f64(),
            avg_rate,
        );
    }
}
