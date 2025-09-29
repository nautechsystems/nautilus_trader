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

use std::collections::HashMap;

use alloy_primitives::U256;

use crate::defi::tick_map::bit_math::{least_significant_bit, most_significant_bit};

/// Calculate word position and bit position for the target tick.
fn tick_position(tick: i32) -> (i16, u8) {
    let word_pos = (tick >> 8) as i16;
    let bit_pos = (tick & 0xFF) as u8;
    (word_pos, bit_pos)
}

/// Represents a tick bitmap similar to Uniswap V3 with tick spacing
#[derive(Debug, Clone, Default)]
pub struct TickBitmap {
    /// Mapping of word positions to bitmap words (256 bits each)
    words: HashMap<i16, U256>,
    /// Minimum spacing between valid ticks for the pool
    tick_spacing: i32,
}

impl TickBitmap {
    /// Create a new empty bitmap
    pub fn new(tick_spacing: u32) -> Self {
        Self {
            words: HashMap::new(),
            tick_spacing: tick_spacing as i32,
        }
    }

    fn compress_tick(&self, tick: i32) -> i32 {
        tick / self.tick_spacing
    }

    /// Flip a bit in the bitmap for the given tick (toggle on/off).
    ///
    /// # Panics
    ///
    /// Panics if `tick` is not a multiple of the configured tick spacing.
    pub fn flip_tick(&mut self, tick: i32) {
        let remainder = tick % self.tick_spacing;
        if remainder != 0 {
            panic!(
                "Tick must be multiple of tick spacing: tick={}, tick_spacing={}, remainder={}",
                tick, self.tick_spacing, remainder
            );
        }

        let compressed_tick = self.compress_tick(tick);
        let (word_position, bit_position) = tick_position(compressed_tick);

        let word = self.words.entry(word_position).or_insert(U256::ZERO);

        // Toggle the bit using XOR
        *word ^= U256::from(1u128) << bit_position;

        // Remove empty words to save storage
        if *word == U256::ZERO {
            self.words.remove(&word_position);
        }
    }

    /// Check if a tick is initialized (bit is set) in the bitmap
    pub fn is_initialized(&self, tick: i32) -> bool {
        let compressed_tick = self.compress_tick(tick);
        let (word_position, bit_position) = tick_position(compressed_tick);

        if let Some(&word) = self.words.get(&word_position) {
            (word & (U256::from(1u128) << bit_position)) != U256::ZERO
        } else {
            false
        }
    }

    /// Returns the next initialized tick contained in the same word (or adjacent word) as the tick that is either
    /// to the left (less than or equal to) or right (greater than) of the given tick
    pub fn next_initialized_tick_within_one_word(
        &self,
        tick: i32,
        less_than_or_equal: bool,
    ) -> (i32, bool) {
        let mut compressed_tick = self.compress_tick(tick);
        // Subtract 1 for negative non-multiples
        if tick < 0 && tick % self.tick_spacing != 0 {
            compressed_tick -= 1;
        }

        if less_than_or_equal {
            let (word_pos, bit_pos) = tick_position(compressed_tick);
            // all the 1s at or to the right of the current bitPos
            let mask =
                (U256::from(1u128) << bit_pos) - U256::from(1u128) + (U256::from(1u128) << bit_pos);
            let word = self.words.get(&word_pos).copied().unwrap_or(U256::ZERO);
            let masked = word & mask;

            // if there are no initialized ticks to the right of or at the current tick, return rightmost in the word
            let initialized = !masked.is_zero();
            // overflow/underflow is possible, but prevented externally by limiting both tickSpacing and tick
            let next = if initialized {
                (compressed_tick - (bit_pos as i32) + most_significant_bit(masked))
                    * self.tick_spacing
            } else {
                (compressed_tick - (bit_pos as i32)) * self.tick_spacing
            };
            (next, initialized)
        } else {
            // start from the word of the next tick, since the current tick state doesn't matter
            let (word_pos, bit_pos) = tick_position(compressed_tick + 1);
            // all the 1s at or to the left of the bitPos
            let mask = !((U256::from(1u128) << bit_pos) - U256::from(1u128));
            let word = self.words.get(&word_pos).copied().unwrap_or(U256::ZERO);
            let masked = word & mask;

            // if there are no initialized ticks to the left of the current tick, return leftmost in the word
            let initialized = !masked.is_zero();
            // overflow/underflow is possible, but prevented externally by limiting both tickSpacing and tick
            let next = if initialized {
                (compressed_tick + 1 + least_significant_bit(masked) - (bit_pos as i32))
                    * self.tick_spacing
            } else {
                (compressed_tick + 1 + (255i32) - (bit_pos as i32)) * self.tick_spacing // type(uint8).max = 255
            };
            (next, initialized)
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::{fixture, rstest};

    use super::*;

    #[fixture]
    fn tick_bitmap() -> TickBitmap {
        TickBitmap::new(1)
    }

    #[rstest]
    fn test_tick_to_positions() {
        // Test positive tick
        assert_eq!(tick_position(256), (1, 0));

        // Test negative tick
        assert_eq!(tick_position(-256), (-1, 0));

        // Test tick within a word
        assert_eq!(tick_position(100), (0, 100));
    }

    #[rstest]
    fn test_flip_tick_toggle(mut tick_bitmap: TickBitmap) {
        // Initially tick should not be initialized
        assert!(!tick_bitmap.is_initialized(100));

        // Toggle tick (should initialize it)
        tick_bitmap.flip_tick(100);
        assert!(tick_bitmap.is_initialized(100));

        // Toggle again (should clear it)
        tick_bitmap.flip_tick(100);
        assert!(!tick_bitmap.is_initialized(100));

        // Check that other ticks are not affected
        assert!(!tick_bitmap.is_initialized(99));
        assert!(!tick_bitmap.is_initialized(101));
    }

    #[rstest]
    fn test_multiple_ticks_same_word(mut tick_bitmap: TickBitmap) {
        // Initialize multiple ticks in the same word (0-255)
        tick_bitmap.flip_tick(50);
        tick_bitmap.flip_tick(100);
        tick_bitmap.flip_tick(200);

        assert!(tick_bitmap.is_initialized(50));
        assert!(tick_bitmap.is_initialized(100));
        assert!(tick_bitmap.is_initialized(200));
        assert!(!tick_bitmap.is_initialized(51));
    }

    #[rstest]
    fn test_multiple_ticks_different_words(mut tick_bitmap: TickBitmap) {
        // Initialize ticks in different words
        tick_bitmap.flip_tick(100); // Word 0
        tick_bitmap.flip_tick(300); // Word 1
        tick_bitmap.flip_tick(-100); // Word -1

        assert!(tick_bitmap.is_initialized(100));
        assert!(tick_bitmap.is_initialized(300));
        assert!(tick_bitmap.is_initialized(-100));
    }

    #[rstest]
    fn test_next_initialized_tick_within_one_word_basic(mut tick_bitmap: TickBitmap) {
        // Initialize compressed ticks (these represent tick indices, not raw ticks)
        tick_bitmap.flip_tick(2); // Compressed tick 2
        tick_bitmap.flip_tick(3); // Compressed tick 3

        // Search forward from tick 60 (compressed: 60/60 = 1)
        let (tick, initialized) = tick_bitmap.next_initialized_tick_within_one_word(1, false);
        assert!(initialized);
        assert_eq!(tick, 2); // Should find compressed tick 2
    }

    #[rstest]
    fn test_next_initialized_tick_within_one_word_backward(mut tick_bitmap: TickBitmap) {
        // Initialize compressed ticks
        tick_bitmap.flip_tick(1); // Compressed tick 1
        tick_bitmap.flip_tick(2); // Compressed tick 2

        // Search backward from tick 3
        let (tick, initialized) = tick_bitmap.next_initialized_tick_within_one_word(3, true);
        assert!(initialized);
        assert_eq!(tick, 2); // Should find tick 2
    }

    #[rstest]
    fn test_next_initialized_tick_within_one_word_no_match(tick_bitmap: TickBitmap) {
        // Search in empty bitmap
        let (_, initialized) = tick_bitmap.next_initialized_tick_within_one_word(60, false);
        assert!(!initialized);

        let (_, initialized) = tick_bitmap.next_initialized_tick_within_one_word(60, true);
        assert!(!initialized);
    }

    #[rstest]
    fn test_next_initialized_tick_with_negative_ticks(mut tick_bitmap: TickBitmap) {
        // Initialize compressed negative ticks
        tick_bitmap.flip_tick(-2); // Compressed tick -2
        tick_bitmap.flip_tick(-1); // Compressed tick -1

        // Search forward from -3
        let (tick, initialized) = tick_bitmap.next_initialized_tick_within_one_word(-3, false);
        assert!(initialized);
        assert_eq!(tick, -2); // Should find tick -2
    }

    #[fixture]
    fn tick_bitmap_uniswapv3_testing() -> TickBitmap {
        // Based on values in https://github.com/Uniswap/v3-core/blob/main/test/TickBitmap.spec.ts#L89
        let mut tick_bitmap = TickBitmap::new(1);
        tick_bitmap.flip_tick(-200);
        tick_bitmap.flip_tick(-55);
        tick_bitmap.flip_tick(-4);
        tick_bitmap.flip_tick(70);
        tick_bitmap.flip_tick(78);
        tick_bitmap.flip_tick(84);
        tick_bitmap.flip_tick(139);
        tick_bitmap.flip_tick(240);
        tick_bitmap.flip_tick(535);

        tick_bitmap
    }

    #[rstest]
    fn test_uniswapv3_test_cases_lte_false(tick_bitmap_uniswapv3_testing: TickBitmap) {
        let mut bitmap = tick_bitmap_uniswapv3_testing;

        // Returns the tick to the right if at initialized tick.
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(78, false);
        assert_eq!(next, 84);
        assert!(initialized);
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(-55, false);
        assert_eq!(next, -4);
        assert!(initialized);
        // Returns the tick directly to the right.
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(77, false);
        assert_eq!(next, 78);
        assert!(initialized);
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(-56, false);
        assert_eq!(next, -55);
        assert!(initialized);
        // Returns the next words initialized tick if on the right boundary.
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(255, false);
        assert_eq!(next, 511); // (255 + 255 = 510, and next is 511)
        assert!(!initialized); // This is not an initialized tick
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(-257, false);
        assert_eq!(next, -200);
        assert!(initialized);
        // Returns the next initialized tick from the next word.
        bitmap.flip_tick(340);
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(328, false);
        assert_eq!(next, 340);
        assert!(initialized);
        // It does not exceed the boundary.
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(508, false);
        assert_eq!(next, 511);
        assert!(!initialized);
        // Skips the half-word.
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(383, false);
        assert_eq!(next, 511);
        assert!(!initialized);
    }

    #[rstest]
    fn test_uniswapv3_test_cases_lte_true(tick_bitmap_uniswapv3_testing: TickBitmap) {
        let mut bitmap = tick_bitmap_uniswapv3_testing;

        // Returns them same tick if initialized
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(78, true);
        assert_eq!(next, 78);
        assert!(initialized);
        // Returns tick directly to the left of input tick if not initialized.
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(79, true);
        assert_eq!(next, 78);
        assert!(initialized);
        // It should not exceed the word boundary.
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(258, true);
        assert_eq!(next, 256);
        assert!(!initialized);
        // At the word boundary should be correct.
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(256, true);
        assert_eq!(next, 256);
        assert!(!initialized);
        // Left or word boundary should be correct.
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(255, true);
        assert_eq!(next, 240);
        assert!(initialized);
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(72, true);
        assert_eq!(next, 70);
        assert!(initialized);
        // Word boundary negative.
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(-257, true);
        assert_eq!(next, -512);
        assert!(!initialized);
        // Entire empty word
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(1023, true);
        assert_eq!(next, 768);
        assert!(!initialized);
        // Halfway through empty word
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(900, true);
        assert_eq!(next, 768);
        assert!(!initialized);
        // If boundary is initialized
        bitmap.flip_tick(768);
        let (next, initialized) = bitmap.next_initialized_tick_within_one_word(900, true);
        assert_eq!(next, 768);
        assert!(initialized);
    }
}
