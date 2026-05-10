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

//! Integration tests for `OKXDataClient`.

use nautilus_okx::common::consts::resolve_book_depth;
use rstest::rstest;

#[rstest]
#[case::depth_0_passes_through(0, 0)]
#[case::depth_400_passes_through(400, 400)]
#[case::depth_50_passes_through(50, 50)]
#[case::depth_1_clamps_to_50(1, 50)]
#[case::depth_5_clamps_to_50(5, 50)]
#[case::depth_10_clamps_to_50(10, 50)]
#[case::depth_25_clamps_to_50(25, 50)]
#[case::depth_49_clamps_to_50(49, 50)]
#[case::depth_51_clamps_to_400(51, 400)]
#[case::depth_100_clamps_to_400(100, 400)]
#[case::depth_200_clamps_to_400(200, 400)]
#[case::depth_500_clamps_to_400(500, 400)]
#[case::depth_1000_clamps_to_400(1000, 400)]
fn test_resolve_book_depth(#[case] raw_depth: usize, #[case] expected: usize) {
    assert_eq!(resolve_book_depth(raw_depth), expected);
}
