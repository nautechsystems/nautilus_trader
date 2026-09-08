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

use arrow::array::Decimal128Array;

use super::{FIXED_DECIMAL_PRECISION, FIXED_DECIMAL_SCALE, STANDARD_TO_DECIMAL_SCALE};

/// Converts legacy little-endian fixed-point test values to the open decimal representation.
pub(crate) fn decimal_array_from_bytes<const N: usize>(values: Vec<&[u8; N]>) -> Decimal128Array {
    let values = values
        .into_iter()
        .map(|bytes| {
            if bytes.iter().all(|byte| *byte == u8::MAX)
                || bytes
                    .iter()
                    .enumerate()
                    .all(|(index, byte)| *byte == if index == N - 1 { 0x7f } else { u8::MAX })
            {
                return None;
            }

            match N {
                8 => Some(
                    i128::from(i64::from_le_bytes(bytes.as_slice().try_into().unwrap()))
                        * STANDARD_TO_DECIMAL_SCALE,
                ),
                16 => Some(i128::from_le_bytes(bytes.as_slice().try_into().unwrap())),
                _ => panic!("unsupported legacy fixed-point width {N}"),
            }
        })
        .collect::<Vec<_>>();

    Decimal128Array::from(values)
        .with_precision_and_scale(FIXED_DECIMAL_PRECISION, FIXED_DECIMAL_SCALE)
        .unwrap()
}
