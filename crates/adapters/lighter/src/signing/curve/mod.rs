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

//! ECgFp5 elliptic curve and its scalar field, as used by the Lighter signer.
//!
//! - [`Point`] is a curve point on `ECgFp5`, with addition, doubling, scalar
//!   multiplication, and the canonical `Fp5` encode/decode pair.
//! - [`Scalar`] is the prime-order scalar field modulo the group order `n`,
//!   with Montgomery-form multiplication and a signed-window recoding helper
//!   used by the variable-time scalar multiplication.
//!
//! Both modules sit on top of the field layer and contain no `unsafe`. Vector
//! tests under `test_data/` cross-check against the upstream Go reference.

mod ecgfp5;
mod scalar;

pub use ecgfp5::{AffinePoint, Point, batch_to_affine, lookup, lookup_ct, lookup_var_time};
pub use scalar::{LIMBS, ORDER, SCALAR_BYTES, Scalar, recode_signed_from_limbs};
