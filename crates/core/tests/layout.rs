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

use std::mem::{align_of, size_of};

use nautilus_core::UUID4;
use rstest::rstest;

#[rstest]
fn uuid4_layout_stays_c_string_compatible() {
    assert_eq!(size_of::<UUID4>(), 37);
    assert_eq!(align_of::<UUID4>(), 1);

    let uuid = UUID4::from("2d89666b-1a1e-4a75-b193-4eb3b454c757");
    let text = uuid.as_str();

    assert_eq!(text.len(), 36);
    assert_eq!(&text[8..9], "-");
    assert_eq!(&text[13..14], "-");
    assert_eq!(&text[18..19], "-");
    assert_eq!(&text[23..24], "-");
    assert_eq!(&text[14..15], "4");
    assert!(matches!(text.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
}
