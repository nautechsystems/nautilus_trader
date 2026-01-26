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

//! Pattern matching for message bus topic subscriptions.

use super::mstr::{MStr, Pattern, Topic};

/// Match a topic against a pattern with wildcard support.
///
/// Wildcards:
/// - `*` matches zero or more characters
/// - `?` matches exactly one character
///
/// Uses a greedy two-pointer algorithm. Chosen over DP (O(n*m) space),
/// recursive backtracking (stack overflow risk), and regex (compilation overhead).
pub fn is_matching_backtracking(topic: MStr<Topic>, pattern: MStr<Pattern>) -> bool {
    is_matching(topic.as_bytes(), pattern.as_bytes())
}

/// Match topic bytes against pattern bytes.
#[must_use]
#[inline]
pub fn is_matching(topic: &[u8], pattern: &[u8]) -> bool {
    // Fast path for exact matches (no wildcards)
    if topic.len() == pattern.len() && !pattern.contains(&b'*') && !pattern.contains(&b'?') {
        return topic == pattern;
    }

    is_matching_greedy(topic, pattern)
}

/// Greedy wildcard matching. Tracks the last `*` position and backtracks
/// when needed. O(n+m) for typical patterns, O(n*m) worst case.
#[inline]
fn is_matching_greedy(topic: &[u8], pattern: &[u8]) -> bool {
    let mut i = 0;
    let mut j = 0;
    let mut star_idx: Option<usize> = None;
    let mut match_idx = 0;

    while i < topic.len() {
        if j < pattern.len() && (pattern[j] == b'?' || pattern[j] == topic[i]) {
            i += 1;
            j += 1;
        } else if j < pattern.len() && pattern[j] == b'*' {
            star_idx = Some(j);
            match_idx = i;
            j += 1;
        } else if let Some(si) = star_idx {
            // Backtrack: try matching one more char with the last '*'
            j = si + 1;
            match_idx += 1;
            i = match_idx;
        } else {
            return false;
        }
    }

    // Skip trailing '*' in pattern
    while j < pattern.len() && pattern[j] == b'*' {
        j += 1;
    }

    j == pattern.len()
}

#[cfg(test)]
mod tests {
    use rand::{Rng, SeedableRng, rngs::StdRng};
    use regex::Regex;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("a", "*", true)]
    #[case("a", "a", true)]
    #[case("a", "b", false)]
    #[case("data.quotes.BINANCE", "data.*", true)]
    #[case("data.quotes.BINANCE", "data.quotes*", true)]
    #[case("data.quotes.BINANCE", "data.*.BINANCE", true)]
    #[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.*", true)]
    #[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ETH*", true)]
    #[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ETH???", false)]
    #[case("data.trades.BINANCE.ETHUSD", "data.*.BINANCE.ETH???", true)]
    // We don't support [seq] style pattern
    #[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ET[HC]USDT", false)]
    // We don't support [!seq] style pattern
    #[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ET[!ABC]USDT", false)]
    // We don't support [^seq] style pattern
    #[case("data.trades.BINANCE.ETHUSDT", "data.*.BINANCE.ET[^ABC]USDT", false)]
    fn test_is_matching(#[case] topic: &str, #[case] pattern: &str, #[case] expected: bool) {
        assert_eq!(
            is_matching_backtracking(topic.into(), pattern.into()),
            expected
        );
    }

    #[rstest]
    // Empty and edge cases
    #[case(b"", b"", true)]
    #[case(b"", b"*", true)]
    #[case(b"", b"?", false)]
    #[case(b"", b"a", false)]
    #[case(b"a", b"", false)]
    // Wildcard-only patterns
    #[case(b"abc", b"*", true)]
    #[case(b"abc", b"***", true)]
    #[case(b"abc", b"???", true)]
    #[case(b"abc", b"????", false)]
    #[case(b"abc", b"??", false)]
    // Consecutive stars
    #[case(b"abc", b"a**c", true)]
    #[case(b"abc", b"**c", true)]
    #[case(b"abc", b"a**", true)]
    // Mixed consecutive
    #[case(b"abc", b"*?*", true)]
    #[case(b"ab", b"*?*", true)]
    #[case(b"a", b"*?*", true)]
    #[case(b"", b"*?*", false)]
    // Pattern longer than topic
    #[case(b"ab", b"abc", false)]
    #[case(b"ab", b"ab?", false)]
    fn test_is_matching_bytes(
        #[case] topic: &[u8],
        #[case] pattern: &[u8],
        #[case] expected: bool,
    ) {
        assert_eq!(is_matching(topic, pattern), expected);
    }

    fn convert_pattern_to_regex(pattern: &str) -> String {
        let mut regex = String::new();
        regex.push('^');

        for c in pattern.chars() {
            match c {
                '.' => regex.push_str("\\."),
                '*' => regex.push_str(".*"),
                '?' => regex.push('.'),
                _ => regex.push(c),
            }
        }

        regex.push('$');
        regex
    }

    #[rstest]
    #[case("a??.quo*es.?I?AN*ET?US*T", "^a..\\.quo.*es\\..I.AN.*ET.US.*T$")]
    #[case("da?*.?u*?s??*NC**ETH?", "^da..*\\..u.*.s...*NC.*.*ETH.$")]
    fn test_convert_pattern_to_regex(#[case] pat: &str, #[case] regex: &str) {
        assert_eq!(convert_pattern_to_regex(pat), regex);
    }

    fn generate_pattern_from_topic(topic: &str, rng: &mut StdRng) -> String {
        let mut pattern = String::new();

        for c in topic.chars() {
            let val: f64 = rng.random();
            // 10% chance of wildcard
            if val < 0.1 {
                pattern.push('*');
            }
            // 20% chance of question mark
            else if val < 0.3 {
                pattern.push('?');
            }
            // 20% chance of skipping
            else if val < 0.5 {
                continue;
            }
            // 50% chance of keeping the character
            else {
                pattern.push(c);
            };
        }

        pattern
    }

    #[rstest]
    fn test_matching_backtracking() {
        let topic = "data.quotes.BINANCE.ETHUSDT";
        let mut rng = StdRng::seed_from_u64(42);

        for i in 0..1000 {
            let pattern = generate_pattern_from_topic(topic, &mut rng);
            let regex_pattern = convert_pattern_to_regex(&pattern);
            let regex = Regex::new(&regex_pattern).unwrap();
            assert_eq!(
                is_matching_backtracking(topic.into(), pattern.as_str().into()),
                regex.is_match(topic),
                "Failed to match on iteration: {i}, pattern: \"{pattern}\", topic: {topic}, regex: \"{regex_pattern}\""
            );
        }
    }
}
