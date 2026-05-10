// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Block time monitoring for dYdX short-term order expiration estimation.
//!
//! This module provides [`BlockTimeMonitor`], a component that tracks rolling average
//! block times from WebSocket data to enable accurate estimation of short-term order
//! expiration in wall-clock time.
//!
//! # Overview
//!
//! dYdX short-term orders expire by block height (typically 20 blocks). Without knowing
//! the actual block time, it's impossible to estimate when an order will expire in
//! wall-clock time. This monitor captures block timestamps from WebSocket updates and

use std::{
    collections::VecDeque,
    sync::{
        RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

use chrono::{DateTime, Utc};

/// Default rolling window size for block time averaging.
///
/// 100 blocks at ~500ms/block = ~50 seconds of data.
pub const DEFAULT_BLOCK_TIME_WINDOW_SIZE: usize = 100;

/// Default block time in milliseconds (dYdX mainnet ~500ms).
///
/// Used as fallback when insufficient samples are available.
pub const DEFAULT_BLOCK_TIME_MS: u64 = 500;

/// Minimum number of samples required before trusting the rolling average.
///
/// Below this threshold, [`BlockTimeMonitor::estimated_seconds_per_block`] returns `None`
/// and [`BlockTimeMonitor::seconds_per_block_or_default`] uses the default value.
pub const MIN_SAMPLES_FOR_ESTIMATE: usize = 5;

/// Minimum valid block time in milliseconds.
///
/// Any calculated block time below this threshold is considered invalid
/// (likely due to clock skew, data corruption, or integer division truncation).
/// When detected, the monitor falls back to the default block time.
pub const MIN_VALID_BLOCK_TIME_MS: f64 = 50.0;

/// Internal rolling window buffer for block samples.
///
/// Uses a `VecDeque` for O(1) push/pop operations with bounded memory.
/// Includes deduplication to skip repeated block heights during rapid replays.
#[derive(Debug)]
struct BlockTimeWindow {
    /// Circular buffer of (height, timestamp) samples.
    samples: VecDeque<(u64, DateTime<Utc>)>,
    /// Maximum capacity of the window.
    capacity: usize,
    /// Last recorded block height for deduplication.
    last_height: Option<u64>,
}

impl BlockTimeWindow {
    /// Creates a new window with specified capacity.
    fn new(capacity: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(capacity),
            capacity,
            last_height: None,
        }
    }

    /// Records a new block sample.
    ///
    /// Skips duplicate block heights to prevent redundant entries during rapid
    /// block replays where the same height may be reported multiple times.
    fn record(&mut self, height: u64, time: DateTime<Utc>) {
        // Skip duplicate heights (rapid replays often repeat same block)
        if self.last_height == Some(height) {
            return;
        }
        self.last_height = Some(height);

        // Maintain bounded size: remove oldest when at capacity
        if self.samples.len() >= self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back((height, time));
    }

    /// Returns the number of samples in the window.
    fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// Computes the average seconds per block from the rolling window.
    ///
    /// Returns `None` if fewer than [`MIN_SAMPLES_FOR_ESTIMATE`] samples are available.
    fn average_seconds_per_block(&self) -> Option<f64> {
        let sample_count = self.sample_count();
        if sample_count < MIN_SAMPLES_FOR_ESTIMATE {
            return None;
        }

        // Sort samples by height to compute deltas between consecutive blocks
        let mut sorted: Vec<_> = self.samples.iter().copied().collect();
        sorted.sort_by_key(|(height, _)| *height);

        let mut total_delta_ms: i64 = 0;
        let mut delta_count: usize = 0;

        for window in sorted.windows(2) {
            let (h1, t1) = &window[0];
            let (h2, t2) = &window[1];

            // Skip duplicate heights (shouldn't happen with deduplication, but be safe)
            let height_diff = h2.saturating_sub(*h1);
            if height_diff == 0 {
                continue;
            }

            let time_diff_ms = (*t2 - *t1).num_milliseconds();
            if time_diff_ms <= 0 {
                continue; // Invalid time difference (clock skew or reorg)
            }

            // Normalize time difference by height difference for multi-block gaps
            let ms_per_block = time_diff_ms / height_diff as i64;
            total_delta_ms += ms_per_block;
            delta_count += 1;
        }

        if delta_count == 0 {
            return None;
        }

        let avg_ms = total_delta_ms as f64 / delta_count as f64;

        // Validate: block time must be at least MIN_VALID_BLOCK_TIME_MS
        // to avoid division issues with unrealistically small values
        if avg_ms < MIN_VALID_BLOCK_TIME_MS {
            return None;
        }

        Some(avg_ms / 1000.0)
    }
}

/// Monitors block times and provides estimation utilities for order expiration.
///
/// Thread-safe component that tracks rolling average block times from WebSocket data.
/// Uses atomic operations for the hot path (height reads) and a read-write lock for
/// less frequent operations (window updates, time estimation).
#[derive(Debug)]
pub struct BlockTimeMonitor {
    /// Current block height (atomic for fast reads on hot path).
    current_height: AtomicU64,
    /// Current block timestamp.
    current_time: RwLock<Option<DateTime<Utc>>>,
    /// Rolling window for block time averaging.
    window: RwLock<BlockTimeWindow>,
}

impl Default for BlockTimeMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockTimeMonitor {
    /// Creates a new [`BlockTimeMonitor`] with default window size.
    #[must_use]
    pub fn new() -> Self {
        Self::with_window_size(DEFAULT_BLOCK_TIME_WINDOW_SIZE)
    }

    /// Creates a new [`BlockTimeMonitor`] with custom window size.
    #[must_use]
    pub fn with_window_size(window_size: usize) -> Self {
        Self {
            current_height: AtomicU64::new(0),
            current_time: RwLock::new(None),
            window: RwLock::new(BlockTimeWindow::new(window_size)),
        }
    }

    /// Records a new block from WebSocket data.
    ///
    /// Should be called whenever a block height update is received.
    /// Updates the current height atomically and adds the sample to the rolling window.
    ///
    /// # Panics
    ///
    /// Panics if the RwLock is poisoned (should never happen in practice).
    pub fn record_block(&self, height: u64, time: DateTime<Utc>) {
        // Update current height atomically (hot path)
        self.current_height.store(height, Ordering::Release);

        // Update current time
        *self.current_time.write().expect("RwLock poisoned") = Some(time);

        // Add to rolling window
        self.window
            .write()
            .expect("RwLock poisoned")
            .record(height, time);
    }

    /// Returns the current block height.
    ///
    /// This is a fast, lock-free read suitable for hot paths.
    #[must_use]
    pub fn current_block_height(&self) -> u64 {
        self.current_height.load(Ordering::Acquire)
    }

    /// Returns the timestamp of the most recent block.
    ///
    /// # Panics
    ///
    /// Panics if the RwLock is poisoned (should never happen in practice).
    #[must_use]
    pub fn current_block_time(&self) -> Option<DateTime<Utc>> {
        *self.current_time.read().expect("RwLock poisoned")
    }

    /// Returns the estimated seconds per block based on rolling average.
    ///
    /// Returns `None` if fewer than [`MIN_SAMPLES_FOR_ESTIMATE`] samples are available.
    ///
    /// # Panics
    ///
    /// Panics if the RwLock is poisoned (should never happen in practice).
    #[must_use]
    pub fn estimated_seconds_per_block(&self) -> Option<f64> {
        self.window
            .read()
            .expect("RwLock poisoned")
            .average_seconds_per_block()
    }

    /// Returns estimated seconds per block, falling back to default if unavailable.
    ///
    /// Uses [`DEFAULT_BLOCK_TIME_MS`] (500ms) when insufficient samples.
    #[must_use]
    pub fn seconds_per_block_or_default(&self) -> f64 {
        self.estimated_seconds_per_block()
            .unwrap_or(DEFAULT_BLOCK_TIME_MS as f64 / 1000.0)
    }

    /// Estimates how many blocks will occur in the given duration.
    ///
    /// Uses the rolling average if available, otherwise falls back to default block time.
    /// Result is capped at `u32::MAX` to prevent overflow from edge cases.
    #[must_use]
    pub fn estimate_blocks_for_duration(&self, duration_secs: f64) -> u32 {
        let secs_per_block = self.seconds_per_block_or_default();
        let blocks = (duration_secs / secs_per_block).ceil();
        // Cap at u32::MAX to prevent overflow from infinity or very large values
        blocks.min(f64::from(u32::MAX)) as u32
    }

    /// Estimates the wall-clock time when a specific block height will be reached.
    ///
    /// Returns `None` if:
    /// - Insufficient samples for reliable estimation
    /// - No current block time available
    /// - Target block is in the past
    #[must_use]
    pub fn estimate_expiry_time(&self, expiry_block: u64) -> Option<DateTime<Utc>> {
        let current_height = self.current_block_height();
        let current_time = self.current_block_time()?;
        let secs_per_block = self.estimated_seconds_per_block()?;

        if expiry_block <= current_height {
            // Block already passed
            return None;
        }

        let blocks_remaining = expiry_block - current_height;
        let seconds_remaining = blocks_remaining as f64 * secs_per_block;

        Some(
            current_time
                + chrono::Duration::milliseconds((seconds_remaining * 1000.0).round() as i64),
        )
    }

    /// Estimates remaining lifetime in seconds for an order expiring at the given block.
    ///
    /// Returns `None` if insufficient data or block already passed.
    #[must_use]
    pub fn estimate_remaining_lifetime_secs(&self, expiry_block: u64) -> Option<f64> {
        let current_height = self.current_block_height();

        if expiry_block <= current_height {
            return Some(0.0);
        }

        let blocks_remaining = expiry_block - current_height;
        let secs_per_block = self.estimated_seconds_per_block()?;

        Some(blocks_remaining as f64 * secs_per_block)
    }

    /// Returns `true` if the monitor has enough samples for reliable estimation.
    ///
    /// # Panics
    ///
    /// Panics if the RwLock is poisoned (should never happen in practice).
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.window.read().expect("RwLock poisoned").sample_count() >= MIN_SAMPLES_FOR_ESTIMATE
    }

    /// Returns the number of samples collected in the rolling window.
    ///
    /// # Panics
    ///
    /// Panics if the RwLock is poisoned (should never happen in practice).
    #[must_use]
    pub fn sample_count(&self) -> usize {
        self.window.read().expect("RwLock poisoned").sample_count()
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_new_monitor_not_ready() {
        let monitor = BlockTimeMonitor::new();
        assert!(!monitor.is_ready());
        assert_eq!(monitor.current_block_height(), 0);
        assert!(monitor.estimated_seconds_per_block().is_none());
    }

    #[rstest]
    fn test_record_updates_height() {
        let monitor = BlockTimeMonitor::new();
        let now = Utc::now();

        monitor.record_block(100, now);
        assert_eq!(monitor.current_block_height(), 100);

        monitor.record_block(101, now + Duration::milliseconds(500));
        assert_eq!(monitor.current_block_height(), 101);
    }

    #[rstest]
    fn test_seconds_per_block_or_default_before_ready() {
        let monitor = BlockTimeMonitor::new();
        let default = DEFAULT_BLOCK_TIME_MS as f64 / 1000.0;
        assert!((monitor.seconds_per_block_or_default() - default).abs() < 0.001);
    }

    #[rstest]
    fn test_becomes_ready_after_min_samples() {
        let monitor = BlockTimeMonitor::new();
        let mut time = Utc::now();

        for i in 0..MIN_SAMPLES_FOR_ESTIMATE {
            monitor.record_block(100 + i as u64, time);
            time += Duration::milliseconds(500);
        }

        assert!(monitor.is_ready());
    }

    #[rstest]
    fn test_average_block_time_calculation() {
        let monitor = BlockTimeMonitor::new();
        let mut time = Utc::now();
        let block_time_ms = 500;

        // Record enough samples with consistent 500ms block time
        for i in 0..10 {
            monitor.record_block(100 + i as u64, time);
            time += Duration::milliseconds(block_time_ms);
        }

        let estimated = monitor.estimated_seconds_per_block().unwrap();
        assert!(
            (estimated - 0.5).abs() < 0.1,
            "Expected ~0.5s, was {estimated}"
        );
    }

    #[rstest]
    fn test_estimate_blocks_for_duration() {
        let monitor = BlockTimeMonitor::new();
        let mut time = Utc::now();

        // Set up with 500ms block time
        for i in 0..10 {
            monitor.record_block(100 + i as u64, time);
            time += Duration::milliseconds(500);
        }

        // 10 seconds should be ~20 blocks at 500ms/block
        let blocks = monitor.estimate_blocks_for_duration(10.0);
        assert!((18..=22).contains(&blocks), "Expected ~20, was {blocks}");
    }

    #[rstest]
    fn test_estimate_expiry_time() {
        let monitor = BlockTimeMonitor::new();
        let start_time = Utc::now();
        let mut time = start_time;

        // Set up with 500ms block time, ending at block 109
        for i in 0..10 {
            monitor.record_block(100 + i as u64, time);
            time += Duration::milliseconds(500);
        }

        // After loop: current block is 109, current_block_time = time - 500ms
        // Expiry at block 129 = 20 blocks from 109
        let expiry_time = monitor.estimate_expiry_time(129).unwrap();
        // Expected: current_block_time + (20 blocks * 500ms)
        let current_block_time = time - Duration::milliseconds(500);
        let expected = current_block_time + Duration::milliseconds(20 * 500);

        let diff_ms = (expiry_time - expected).num_milliseconds().abs();
        assert!(diff_ms < 1000, "Expected ~{expected}, was {expiry_time}");
    }

    #[rstest]
    fn test_estimate_expiry_time_past_block() {
        let monitor = BlockTimeMonitor::new();
        let time = Utc::now();

        monitor.record_block(100, time);

        // Block 50 is in the past
        assert!(monitor.estimate_expiry_time(50).is_none());
    }

    #[rstest]
    fn test_estimate_remaining_lifetime() {
        let monitor = BlockTimeMonitor::new();
        let mut time = Utc::now();

        // Set up with 500ms block time
        for i in 0..10 {
            monitor.record_block(100 + i as u64, time);
            time += Duration::milliseconds(500);
        }

        // Current height is 109, expiry at 129 (20 blocks)
        let remaining = monitor.estimate_remaining_lifetime_secs(129).unwrap();
        assert!(
            (remaining - 10.0).abs() < 1.0,
            "Expected ~10s, was {remaining}"
        );
    }

    #[rstest]
    fn test_circular_buffer_wraps() {
        let monitor = BlockTimeMonitor::with_window_size(5);
        let mut time = Utc::now();

        // Record more samples than window size
        for i in 0..10 {
            monitor.record_block(100 + i as u64, time);
            time += Duration::milliseconds(500);
        }

        // Should still have only 5 samples
        assert_eq!(monitor.sample_count(), 5);
        assert!(monitor.is_ready());
    }

    #[rstest]
    fn test_handles_non_consecutive_blocks() {
        let monitor = BlockTimeMonitor::new();
        let mut time = Utc::now();

        // Record blocks with a gap (100, 101, 102, 105, 106)
        monitor.record_block(100, time);
        time += Duration::milliseconds(500);
        monitor.record_block(101, time);
        time += Duration::milliseconds(500);
        monitor.record_block(102, time);
        time += Duration::milliseconds(1500); // Skip 3 blocks
        monitor.record_block(105, time);
        time += Duration::milliseconds(500);
        monitor.record_block(106, time);

        // Should still calculate a reasonable estimate
        assert!(monitor.is_ready());
        let estimated = monitor.estimated_seconds_per_block().unwrap();
        // Expect ~500ms per block even with the gap
        assert!(
            (estimated - 0.5).abs() < 0.2,
            "Expected ~0.5s, was {estimated}"
        );
    }

    #[rstest]
    fn test_deduplicates_same_block_height() {
        let monitor = BlockTimeMonitor::with_window_size(10);
        let time = Utc::now();

        // Record same block height multiple times (rapid replay scenario)
        monitor.record_block(100, time);
        monitor.record_block(100, time + Duration::milliseconds(10));
        monitor.record_block(100, time + Duration::milliseconds(20));
        monitor.record_block(100, time + Duration::milliseconds(30));

        // Should only have 1 sample due to deduplication
        assert_eq!(monitor.sample_count(), 1);
    }

    #[rstest]
    fn test_rapid_replay_bounded_memory() {
        let monitor = BlockTimeMonitor::with_window_size(5);
        let mut time = Utc::now();

        // Simulate rapid replay: 1000 block updates
        for i in 0..1000 {
            monitor.record_block(100 + i as u64, time);
            time += Duration::milliseconds(500);
        }

        // Buffer should never exceed capacity
        assert_eq!(monitor.sample_count(), 5);
        assert!(monitor.is_ready());

        // Estimate should still be valid
        let estimated = monitor.estimated_seconds_per_block().unwrap();
        assert!(
            (estimated - 0.5).abs() < 0.1,
            "Expected ~0.5s, was {estimated}"
        );
    }

    #[rstest]
    fn test_rapid_replay_with_duplicate_heights() {
        let monitor = BlockTimeMonitor::with_window_size(10);
        let mut time = Utc::now();

        // Simulate rapid replay with duplicates: each block reported 3 times
        for block in 100..110 {
            for _ in 0..3 {
                monitor.record_block(block, time);
                time += Duration::milliseconds(100);
            }
            time += Duration::milliseconds(200); // Actual block time ~500ms
        }

        // Should have exactly 10 samples (one per unique block)
        assert_eq!(monitor.sample_count(), 10);
    }

    #[rstest]
    fn test_rejects_unrealistically_small_block_times() {
        let monitor = BlockTimeMonitor::with_window_size(10);
        let time = Utc::now();

        // Record blocks with extremely small time differences (1ms per block)
        // This is unrealistic and should be rejected
        for i in 0..10 {
            monitor.record_block(100 + i as u64, time + Duration::milliseconds(i));
        }

        // Should have samples but estimated time should be None (below threshold)
        assert!(monitor.is_ready());
        assert!(
            monitor.estimated_seconds_per_block().is_none(),
            "Expected None for unrealistically small block times"
        );

        // Should fall back to default
        let default = super::DEFAULT_BLOCK_TIME_MS as f64 / 1000.0;
        assert!((monitor.seconds_per_block_or_default() - default).abs() < 0.001);
    }

    #[rstest]
    fn test_estimate_blocks_handles_large_duration() {
        let monitor = BlockTimeMonitor::new();
        // Without samples, uses default (0.5s per block)

        // Very large duration should not overflow
        let blocks = monitor.estimate_blocks_for_duration(f64::MAX);
        assert_eq!(blocks, u32::MAX);
    }

    #[rstest]
    fn test_estimate_blocks_handles_zero_duration() {
        let monitor = BlockTimeMonitor::new();

        let blocks = monitor.estimate_blocks_for_duration(0.0);
        assert_eq!(blocks, 0);
    }
}
