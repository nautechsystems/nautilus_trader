// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

//! Position sizing calculation functions.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use nautilus_model::{
    instruments::any::InstrumentAny,
    types::{money::Money, price::Price, quantity::Quantity},
};
use rust_decimal::Decimal;

#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn calculate_fixed_risk_position_size(
    instrument: InstrumentAny,
    entry: Price,
    stop_loss: Price,
    equity: Money,
    risk: Decimal,
    commission_rate: Decimal,
    exchange_range: Decimal,
    hard_limit: Option<Decimal>,
    unit_batch_size: Decimal,
    units: usize,
) -> Quantity {
    todo!()
}
