// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::hash::{Hash, Hasher};

use nautilus_core::{
    UnixNanos,
    correctness::{FAILED, check_equal_u8},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::{Instrument, any::InstrumentAny};
use crate::{
    enums::{AssetClass, InstrumentClass, OptionKind},
    identifiers::{InstrumentId, Symbol},
    types::{
        currency::Currency,
        money::Money,
        price::{Price, check_positive_price},
        quantity::{Quantity, check_positive_quantity},
    },
};

/// Represents a betting instrument with complete market and selection details.
#[repr(C)]
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct BettingInstrument {
    /// The instrument ID.
    pub id: InstrumentId,
    /// The raw/local/native symbol for the instrument, assigned by the venue.
    pub raw_symbol: Symbol,
    /// The event type identifier (e.g. 1=Soccer, 2=Tennis).
    pub event_type_id: u64,
    /// The name of the event type (e.g. "Soccer", "Tennis").
    pub event_type_name: Ustr,
    /// The competition/league identifier.
    pub competition_id: u64,
    /// The name of the competition (e.g. "English Premier League").
    pub competition_name: Ustr,
    /// The unique identifier for the event.
    pub event_id: u64,
    /// The name of the event (e.g. "Arsenal vs Chelsea").
    pub event_name: Ustr,
    /// The ISO country code where the event takes place.
    pub event_country_code: Ustr,
    /// UNIX timestamp (nanoseconds) when the event becomes available for betting.
    pub event_open_date: UnixNanos,
    /// The type of betting (e.g. "ODDS", "LINE").
    pub betting_type: Ustr,
    /// The unique identifier for the betting market.
    pub market_id: Ustr,
    /// The name of the market (e.g. "Match Odds", "Total Goals").
    pub market_name: Ustr,
    /// The type of market (e.g. "WIN", "PLACE").
    pub market_type: Ustr,
    /// UNIX timestamp (nanoseconds) when betting starts for this market.
    pub market_start_time: UnixNanos,
    /// The unique identifier for the selection within the market.
    pub selection_id: u64,
    /// The name of the selection (e.g. "Arsenal", "Over 2.5").
    pub selection_name: Ustr,
    /// The handicap value for the selection, if applicable.
    pub selection_handicap: f64,
    /// The contract currency.
    pub currency: Currency,
    /// The price decimal precision.
    pub price_precision: u8,
    /// The trading size decimal precision.
    pub size_precision: u8,
    /// The minimum price increment (tick size).
    pub price_increment: Price,
    /// The minimum size increment.
    pub size_increment: Quantity,
    /// The initial (order) margin requirement in percentage of order value.
    pub margin_init: Decimal,
    /// The maintenance (position) margin in percentage of position value.
    pub margin_maint: Decimal,
    /// The fee rate for liquidity makers as a percentage of order value.
    pub maker_fee: Decimal,
    /// The fee rate for liquidity takers as a percentage of order value.
    pub taker_fee: Decimal,
    /// The maximum allowable order quantity.
    pub max_quantity: Option<Quantity>,
    /// The minimum allowable order quantity.
    pub min_quantity: Option<Quantity>,
    /// The maximum allowable order notional value.
    pub max_notional: Option<Money>,
    /// The minimum allowable order notional value.
    pub min_notional: Option<Money>,
    /// The maximum allowable quoted price.
    pub max_price: Option<Price>,
    /// The minimum allowable quoted price.
    pub min_price: Option<Price>,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

impl BettingInstrument {
    /// Creates a new [`BettingInstrument`] instance with correctness checking.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    #[allow(clippy::too_many_arguments)]
    pub fn new_checked(
        id: InstrumentId,
        raw_symbol: Symbol,
        event_type_id: u64,
        event_type_name: Ustr,
        competition_id: u64,
        competition_name: Ustr,
        event_id: u64,
        event_name: Ustr,
        event_country_code: Ustr,
        event_open_date: UnixNanos,
        betting_type: Ustr,
        market_id: Ustr,
        market_name: Ustr,
        market_type: Ustr,
        market_start_time: UnixNanos,
        selection_id: u64,
        selection_name: Ustr,
        selection_handicap: f64,
        currency: Currency,
        price_precision: u8,
        size_precision: u8,
        price_increment: Price,
        size_increment: Quantity,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_notional: Option<Money>,
        min_notional: Option<Money>,
        max_price: Option<Price>,
        min_price: Option<Price>,
        margin_init: Option<Decimal>,
        margin_maint: Option<Decimal>,
        maker_fee: Option<Decimal>,
        taker_fee: Option<Decimal>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Self> {
        check_equal_u8(
            price_precision,
            price_increment.precision,
            stringify!(price_precision),
            stringify!(price_increment.precision),
        )?;
        check_equal_u8(
            size_precision,
            size_increment.precision,
            stringify!(size_precision),
            stringify!(size_increment.precision),
        )?;
        check_positive_price(price_increment, stringify!(price_increment))?;
        check_positive_quantity(size_increment, stringify!(size_increment))?;

        Ok(Self {
            id,
            raw_symbol,
            event_type_id,
            event_type_name,
            competition_id,
            competition_name,
            event_id,
            event_name,
            event_country_code,
            event_open_date,
            betting_type,
            market_id,
            market_name,
            market_type,
            market_start_time,
            selection_id,
            selection_name,
            selection_handicap,
            currency,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            margin_init: margin_init.unwrap_or(dec!(1)),
            margin_maint: margin_maint.unwrap_or(dec!(1)),
            maker_fee: maker_fee.unwrap_or_default(),
            taker_fee: taker_fee.unwrap_or_default(),
            ts_event,
            ts_init,
        })
    }

    /// Creates a new [`BettingInstrument`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: InstrumentId,
        raw_symbol: Symbol,
        event_type_id: u64,
        event_type_name: Ustr,
        competition_id: u64,
        competition_name: Ustr,
        event_id: u64,
        event_name: Ustr,
        event_country_code: Ustr,
        event_open_date: UnixNanos,
        betting_type: Ustr,
        market_id: Ustr,
        market_name: Ustr,
        market_type: Ustr,
        market_start_time: UnixNanos,
        selection_id: u64,
        selection_name: Ustr,
        selection_handicap: f64,
        currency: Currency,
        price_precision: u8,
        size_precision: u8,
        price_increment: Price,
        size_increment: Quantity,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_notional: Option<Money>,
        min_notional: Option<Money>,
        max_price: Option<Price>,
        min_price: Option<Price>,
        margin_init: Option<Decimal>,
        margin_maint: Option<Decimal>,
        maker_fee: Option<Decimal>,
        taker_fee: Option<Decimal>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new_checked(
            id,
            raw_symbol,
            event_type_id,
            event_type_name,
            competition_id,
            competition_name,
            event_id,
            event_name,
            event_country_code,
            event_open_date,
            betting_type,
            market_id,
            market_name,
            market_type,
            market_start_time,
            selection_id,
            selection_name,
            selection_handicap,
            currency,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            ts_event,
            ts_init,
        )
        .expect(FAILED)
    }
}

impl PartialEq<Self> for BettingInstrument {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for BettingInstrument {}

impl Hash for BettingInstrument {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Instrument for BettingInstrument {
    fn into_any(self) -> InstrumentAny {
        InstrumentAny::Betting(self)
    }

    fn id(&self) -> InstrumentId {
        self.id
    }

    fn raw_symbol(&self) -> Symbol {
        self.raw_symbol
    }

    fn asset_class(&self) -> AssetClass {
        AssetClass::Alternative
    }

    fn instrument_class(&self) -> InstrumentClass {
        InstrumentClass::SportsBetting
    }

    fn underlying(&self) -> Option<Ustr> {
        None
    }

    fn quote_currency(&self) -> Currency {
        self.currency
    }

    fn base_currency(&self) -> Option<Currency> {
        None
    }

    fn settlement_currency(&self) -> Currency {
        self.currency
    }

    fn isin(&self) -> Option<Ustr> {
        None
    }

    fn exchange(&self) -> Option<Ustr> {
        None
    }

    fn option_kind(&self) -> Option<OptionKind> {
        None
    }

    fn is_inverse(&self) -> bool {
        false
    }

    fn price_precision(&self) -> u8 {
        self.price_precision
    }

    fn size_precision(&self) -> u8 {
        self.size_precision
    }

    fn price_increment(&self) -> Price {
        self.price_increment
    }

    fn size_increment(&self) -> Quantity {
        self.size_increment
    }

    fn multiplier(&self) -> Quantity {
        Quantity::from(1)
    }

    fn lot_size(&self) -> Option<Quantity> {
        Some(Quantity::from(1))
    }

    fn max_quantity(&self) -> Option<Quantity> {
        self.max_quantity
    }

    fn min_quantity(&self) -> Option<Quantity> {
        self.min_quantity
    }

    fn max_price(&self) -> Option<Price> {
        self.max_price
    }

    fn min_price(&self) -> Option<Price> {
        self.min_price
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }

    fn strike_price(&self) -> Option<Price> {
        None
    }

    fn activation_ns(&self) -> Option<UnixNanos> {
        Some(self.market_start_time)
    }

    fn expiration_ns(&self) -> Option<UnixNanos> {
        None
    }

    fn max_notional(&self) -> Option<Money> {
        self.max_notional
    }

    fn min_notional(&self) -> Option<Money> {
        self.min_notional
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::instruments::{BettingInstrument, stubs::*};

    #[rstest]
    fn test_equality(betting: BettingInstrument) {
        let cloned = betting;
        assert_eq!(betting, cloned);
    }
}
