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

use std::time::Instant;

/// Tracks performance metrics during block synchronization
#[derive(Debug)]
pub struct BlockSyncMetrics {
    start_time: Instant,
    last_progress_time: Instant,
    blocks_processed: u64,
    from_block: u64,
    total_blocks: u64,
    progress_update_interval: u64,
    next_progress_threshold: u64,
}

impl BlockSyncMetrics {
    /// Creates a new metrics tracker for block synchronization
    #[must_use]
    pub fn new(from_block: u64, total_blocks: u64, update_interval: u64) -> Self {
        let now = Instant::now();
        Self {
            start_time: now,
            last_progress_time: now,
            blocks_processed: 0,
            from_block,
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

        // Estimate remaining time
        let blocks_remaining = self.total_blocks.saturating_sub(self.blocks_processed);
        let eta_display = calculate_eta(blocks_remaining, avg_rate);

        tracing::info!(
            "Block sync progress: {:.1}% | Block: {} | Rate: {:.0} blocks/s | Avg: {:.0} blocks/s | ETA: {}",
            progress_pct,
            block_number,
            current_rate,
            avg_rate,
            eta_display
        );

        self.next_progress_threshold = block_number + self.progress_update_interval;
        self.last_progress_time = Instant::now();
    }

    /// Logs final statistics summary
    pub fn log_final_stats(&self) {
        let total_elapsed = self.start_time.elapsed();
        let avg_rate = self.blocks_processed as f64 / total_elapsed.as_secs_f64();
        tracing::info!(
            "Finished syncing blocks | Total: {} blocks in {:.1}s | Avg rate: {:.0} blocks/s",
            self.blocks_processed,
            total_elapsed.as_secs_f64(),
            avg_rate
        );
    }
}

/// Formats duration into human-readable time (e.g., "3.2h", "45m", "2.5d")
fn format_duration(seconds: f64) -> String {
    if seconds < 60.0 {
        format!("{seconds:.0}s")
    } else if seconds < 3600.0 {
        format!("{:.0} minutes", seconds / 60.0)
    } else if seconds < 86400.0 {
        format!("{:.1} hours", seconds / 3600.0)
    } else {
        format!("{:.1} days", seconds / 86400.0)
    }
}

/// Calculates and formats ETA based on rate and remaining work
fn calculate_eta(blocks_remaining: u64, avg_rate: f64) -> String {
    let eta_seconds = blocks_remaining as f64 / avg_rate;
    if eta_seconds < 60.0 {
        format!("{eta_seconds:.0}s")
    } else if eta_seconds < 3600.0 {
        format!("{:.0}m", eta_seconds / 60.0)
    } else if eta_seconds < 86400.0 {
        format!("{:.1}h", eta_seconds / 3600.0)
    } else {
        format!("{:.1}d", eta_seconds / 86400.0)
    }
}
