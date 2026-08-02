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

//! Numeric conversions between wire integers and model raw types.
//!
//! These generic boundaries follow the model's resolved raw aliases independently of this
//! crate's `high-precision` forwarding feature. Keeping them generic is deliberate: the target
//! width comes from `PriceRaw`, `QuantityRaw`, and `MoneyRaw`, so codec code never needs to
//! inspect a precision feature of its own to decide how wide a value may be. Inlining these
//! conversions back into the codecs would reintroduce that coupling.

/// Converts a wire integer into the resolved model raw type, returning `None` on overflow.
#[inline]
pub(crate) fn wire_to_raw<R, W>(wire: W) -> Option<R>
where
    R: TryFrom<W>,
{
    R::try_from(wire).ok()
}

/// Converts a model raw integer into its lossless wire representation.
#[inline]
pub(crate) fn raw_to_wire<W, R>(raw: R) -> W
where
    W: From<R>,
{
    W::from(raw)
}
