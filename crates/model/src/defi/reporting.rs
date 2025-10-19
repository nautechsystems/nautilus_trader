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

use thousands::Separable;

#[derive(Debug, Clone)]
pub enum BlockchainSyncReportItems {
    Blocks,
    PoolCreatedEvents,
    PoolEvents,
    PoolProfiling,
}

impl Display for BlockchainSyncReportItems {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Tracks performance metrics during block synchronization
#[derive(Debug, Clone)]
pub struct BlockchainSyncReporter {
    item: BlockchainSyncReportItems,
    start_time: Instant,
    last_progress_time: Instant,
    from_block: u64,
    blocks_processed: u64,
    blocks_since_last_report: u64,
    total_blocks: u64,
    progress_update_interval: u64,
    next_progress_threshold: u64,
}

impl BlockchainSyncReporter {
    /// Creates a new metrics tracker for block synchronization
    #[must_use]
    pub fn new(
        item: BlockchainSyncReportItems,
        from_block: u64,
        total_blocks: u64,
        update_interval: u64,
    ) -> Self {
        let now = Instant::now();
        Self {
            item,
            start_time: now,
            last_progress_time: now,
            from_block,
            blocks_processed: 0,
            blocks_since_last_report: 0,
            total_blocks,
            progress_update_interval: update_interval,
            next_progress_threshold: from_block + update_interval,
        }
    }

    /// Updates metrics after a database operation
    pub fn update(&mut self, batch_size: usize) {
        self.blocks_processed += batch_size as u64;
        self.blocks_since_last_report += batch_size as u64;
    }

    /// Checks if progress should be logged based on the current block number
    #[must_use]
    pub fn should_log_progress(&self, block_number: u64, current_block: u64) -> bool {
        let block_threshold_reached =
            block_number >= self.next_progress_threshold || block_number >= current_block;
        // Minimum 1 second between logs
        let time_threshold_reached = self.last_progress_time.elapsed().as_secs_f64() >= 1.0;

        block_threshold_reached && time_threshold_reached
    }

    /// Logs current progress with detailed metrics
    pub fn log_progress(&mut self, block_number: u64) {
        let elapsed = self.start_time.elapsed();
        let interval_elapsed = self.last_progress_time.elapsed();

        // Calculate rates - avoid division by zero
        let avg_rate = if elapsed.as_secs_f64() > 0.0 {
            self.blocks_processed as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };

        let current_rate = if interval_elapsed.as_secs_f64() > 0.001 {
            // Minimum 1ms
            self.blocks_since_last_report as f64 / interval_elapsed.as_secs_f64()
        } else {
            0.0
        };

        // Calculate progress based on actual block position relative to the sync range
        let blocks_completed = block_number.saturating_sub(self.from_block);
        let progress_pct = (blocks_completed as f64 / self.total_blocks as f64 * 100.0).min(100.0);

        tracing::info!(
            "Syncing {} progress: {:.1}% | Block: {} | Rate: {} blocks/s | Avg: {} blocks/s",
            self.item,
            progress_pct,
            block_number.separate_with_commas(),
            (current_rate.round() as u64).separate_with_commas(),
            (avg_rate.round() as u64).separate_with_commas(),
        );

        self.next_progress_threshold = block_number + self.progress_update_interval;
        self.last_progress_time = Instant::now();
        self.blocks_since_last_report = 0;
    }

    /// Logs final statistics summary
    pub fn log_final_stats(&self) {
        let total_elapsed = self.start_time.elapsed();
        let avg_rate = self.blocks_processed as f64 / total_elapsed.as_secs_f64();
        tracing::info!(
            "Finished syncing {} | Total: {} blocks in {:.1}s | Avg rate: {} blocks/s",
            self.item,
            self.blocks_processed.separate_with_commas(),
            total_elapsed.as_secs_f64(),
            (avg_rate.round() as u64).separate_with_commas(),
        );
    }
}
