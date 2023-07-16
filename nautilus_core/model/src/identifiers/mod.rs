// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{
    ffi::{c_char, CStr},
    fmt::{Debug, Display, Formatter},
    marker::PhantomData,
    str::FromStr,
};

use anyhow::Ok;
use nautilus_core::correctness;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ustr::Ustr;

#[macro_use]
mod macros;

pub mod client_id;
pub mod client_order_id;
pub mod component_id;
pub mod exec_algorithm_id;
pub mod instrument_id;
pub mod order_list_id;
pub mod position_id;
pub mod strategy_id;
pub mod symbol;
pub mod trade_id;
pub mod trader_id;
pub mod venue;
pub mod venue_order_id;

pub struct AccountIdTag;
pub struct ClientIdTag;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Identifier<T> {
    pub value: Ustr,
    kind: PhantomData<T>,
}

impl<T> Debug for Identifier<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.value)
    }
}

impl<T> Display for Identifier<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.value)
    }
}

impl Identifier<AccountIdTag> {
    #[must_use]
    pub fn new(s: &str) -> Self {
        correctness::valid_string(s, "`Identifier<AccountId>` value");
        correctness::string_contains(s, "-", "`TraderId` value");

        Self {
            value: Ustr::from(s),
            kind: PhantomData,
        }
    }
}

impl Identifier<ClientIdTag> {
    #[must_use]
    pub fn new(s: &str) -> Self {
        correctness::valid_string(s, "`ClientId` value");

        Self {
            value: Ustr::from(s),
            kind: PhantomData,
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////

/// Intern a C string pointer
///
/// # Safety
///
/// - Assumes `ptr` is a valid C string pointer.
#[no_mangle]
pub unsafe extern "C" fn intern_string(ptr: *const c_char) -> *const c_char {
    Ustr::from(CStr::from_ptr(ptr).to_str().expect("CStr::from_ptr failed")).as_char_ptr()
}

/// Return the hash of an interned string
///
/// # Safety
/// - Assumes `ptr` is an interned string
pub unsafe extern "C" fn interned_string_hash(ptr: *const c_char) -> u64 {
    Ustr::from_existing(CStr::from_ptr(ptr).to_str().expect("CStr::from_ptr failed"))
        .map(|entry| entry.precomputed_hash())
        .expect("Did not find entry for given string")
}

impl_from_str_for_identifier!(client_id::ClientId);
impl_from_str_for_identifier!(client_order_id::ClientOrderId);
impl_from_str_for_identifier!(component_id::ComponentId);
impl_from_str_for_identifier!(exec_algorithm_id::ExecAlgorithmId);
impl_from_str_for_identifier!(order_list_id::OrderListId);
impl_from_str_for_identifier!(position_id::PositionId);
impl_from_str_for_identifier!(strategy_id::StrategyId);
impl_from_str_for_identifier!(symbol::Symbol);
impl_from_str_for_identifier!(trade_id::TradeId);
impl_from_str_for_identifier!(trader_id::TraderId);
impl_from_str_for_identifier!(venue::Venue);
impl_from_str_for_identifier!(venue_order_id::VenueOrderId);

impl_serialization_for_identifier!(client_id::ClientId);
impl_serialization_for_identifier!(client_order_id::ClientOrderId);
impl_serialization_for_identifier!(component_id::ComponentId);
impl_serialization_for_identifier!(exec_algorithm_id::ExecAlgorithmId);
impl_serialization_for_identifier!(order_list_id::OrderListId);
impl_serialization_for_identifier!(position_id::PositionId);
impl_serialization_for_identifier!(strategy_id::StrategyId);
impl_serialization_for_identifier!(symbol::Symbol);
impl_serialization_for_identifier!(trade_id::TradeId);
impl_serialization_for_identifier!(trader_id::TraderId);
impl_serialization_for_identifier!(venue::Venue);
impl_serialization_for_identifier!(venue_order_id::VenueOrderId);
