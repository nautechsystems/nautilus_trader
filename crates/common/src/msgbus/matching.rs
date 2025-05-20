// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 2Nautech Systems Pty Ltd. All rights reserved.
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

use ustr::Ustr;

use super::core::{Pattern, Topic};

/// Match a topic and a string pattern
/// pattern can contains -
/// '*' - match 0 or more characters after this
/// '?' - match any character once
/// 'a-z' - match the specific character
#[must_use]
pub fn is_matching(topic: &Ustr, pattern: &Ustr) -> bool {
    let mut table = [[false; 256]; 256];
    table[0][0] = true;

    let m = pattern.len();
    let n = topic.len();

    pattern.chars().enumerate().for_each(|(j, c)| {
        if c == '*' {
            table[0][j + 1] = table[0][j];
        }
    });

    topic.chars().enumerate().for_each(|(i, tc)| {
        pattern.chars().enumerate().for_each(|(j, pc)| {
            if pc == '*' {
                table[i + 1][j + 1] = table[i][j + 1] || table[i + 1][j];
            } else if pc == '?' || tc == pc {
                table[i + 1][j + 1] = table[i][j];
            }
        });
    });

    table[n][m]
}

/// Match a topic and a string pattern using iterative backtracking algorithm
/// pattern can contains -
/// '*' - match 0 or more characters after this
/// '?' - match any character once
/// 'a-z' - match the specific character
pub fn is_matching_backtracking(topic: Topic, pattern: Pattern) -> bool {
    let topic_bytes = topic.as_bytes();
    let pattern_bytes = pattern.as_bytes();

    is_matching_fast(topic_bytes, pattern_bytes)
}

#[must_use]
pub fn is_matching_fast(topic: &[u8], pattern: &[u8]) -> bool {
    // Stack to store states for backtracking (topic_idx, pattern_idx)
    let mut stack = vec![(0, 0)];

    while let Some((mut i, mut j)) = stack.pop() {
        loop {
            // Found a match if we've consumed both strings
            if i == topic.len() && j == pattern.len() {
                return true;
            }

            // If we've reached the end of the pattern, break to try other paths
            if j == pattern.len() {
                break;
            }

            // Handle '*' wildcard
            if pattern[j] == b'*' {
                // Try skipping '*' entirely first
                stack.push((i, j + 1));

                // Continue with matching current character and keeping '*'
                if i < topic.len() {
                    i += 1;
                    continue;
                }
                break;
            }
            // Handle '?' or exact character match
            else if i < topic.len() && (pattern[j] == b'?' || topic[i] == pattern[j]) {
                // Continue matching linearly without stack operations
                i += 1;
                j += 1;
                continue;
            }

            // No match found in current path
            break;
        }
    }

    false
}
