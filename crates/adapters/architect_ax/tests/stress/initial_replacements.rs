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

use std::time::Duration;

// Replacement attempts in the shared retry budget, including the first
pub(super) const BUDGET: usize = 8;

pub(super) fn within_limit(unsubscribes: usize, timeout_secs: u64, elapsed: Duration) -> bool {
    if timeout_secs == 0 {
        return unsubscribes == 0;
    }

    let ceiling_attempts =
        usize::try_from(elapsed.as_secs() / 60).expect("elapsed recovery minutes fit usize");

    (1..=BUDGET + ceiling_attempts).contains(&unsubscribes)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::disabled_no_replacements(0, 0, 0, true)]
    #[case::disabled_replacement(1, 0, 0, false)]
    #[case::enabled_no_recovery(0, 1, 0, false)]
    #[case::first_replacement(1, 1, 0, true)]
    #[case::second_replacement(2, 1, 0, true)]
    #[case::longer_timeout(2, 10, 0, true)]
    #[case::budget_used(BUDGET, 1, 59, true)]
    #[case::budget_exceeded(BUDGET + 1, 1, 59, false)]
    #[case::ceiling_allowance(BUDGET + 1, 1, 60, true)]
    #[case::ceiling_overrun(BUDGET + 2, 1, 60, false)]
    #[case::later_ceiling_allowance(BUDGET + 2, 1, 180, true)]
    fn replacement_limit(
        #[case] unsubscribes: usize,
        #[case] timeout_secs: u64,
        #[case] elapsed_secs: u64,
        #[case] expected: bool,
    ) {
        let actual = within_limit(
            unsubscribes,
            timeout_secs,
            Duration::from_secs(elapsed_secs),
        );

        assert_eq!(actual, expected);
    }
}
