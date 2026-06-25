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

use std::hash::{Hash, Hasher};

use nautilus_core::{
    Params, UnixNanos,
    correctness::{CorrectnessResult, CorrectnessResultExt, FAILED, check_equal_u8},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::{
    Instrument,
    any::InstrumentAny,
    tick_scheme::{BETFAIR_TICK_SCHEME, BETFAIR_TICK_SCHEME_NAME, check_tick_scheme},
};
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
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
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
    /// The registered variable tick scheme name.
    pub tick_scheme: Option<Ustr>,
    /// Additional instrument metadata as a JSON-serializable dictionary.
    pub info: Option<Params>,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

#[bon::bon]
impl BettingInstrument {
    /// Creates a new [`BettingInstrument`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if any input validation fails (precision mismatches or non-positive increments).
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    #[expect(clippy::too_many_arguments)]
    pub fn new_checked(
        instrument_id: InstrumentId,
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
        tick_scheme: Option<Ustr>,
        info: Option<Params>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> CorrectnessResult<Self> {
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
        check_tick_scheme(tick_scheme)?;

        Ok(Self {
            id: instrument_id,
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
            tick_scheme,
            info,
            ts_event,
            ts_init,
        })
    }

    /// Creates a new [`BettingInstrument`] instance by parsing and validating input parameters.
    ///
    /// # Panics
    ///
    /// Panics if any required parameter is invalid or parsing fails during `new_checked`.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
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
        tick_scheme: Option<Ustr>,
        info: Option<Params>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new_checked(
            instrument_id,
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
            tick_scheme,
            info,
            ts_event,
            ts_init,
        )
        .expect_display(FAILED)
    }

    /// Returns a fluent builder for a [`BettingInstrument`] instance.
    ///
    /// Required fields are enforced at compile time; optional fields can be omitted and default
    /// the same way they do in [`BettingInstrument::new_checked`], which the builder calls so the
    /// same correctness checks run on `build`.
    ///
    /// # Errors
    ///
    /// Returns an error if any input validation fails (see [`BettingInstrument::new_checked`]).
    #[builder(start_fn = builder, finish_fn = build)]
    pub fn build_checked(
        instrument_id: InstrumentId,
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
        tick_scheme: Option<Ustr>,
        info: Option<Params>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> CorrectnessResult<Self> {
        Self::new_checked(
            instrument_id,
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
            tick_scheme,
            info,
            ts_event,
            ts_init,
        )
    }

    fn uses_betfair_tick_scheme(&self) -> bool {
        self.id.venue.as_str() == BETFAIR_TICK_SCHEME_NAME
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
    fn tick_scheme(&self) -> Option<Ustr> {
        self.tick_scheme.or_else(|| {
            self.uses_betfair_tick_scheme()
                .then(|| Ustr::from(BETFAIR_TICK_SCHEME_NAME))
        })
    }

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
        self.max_price.or_else(|| {
            self.uses_betfair_tick_scheme()
                .then(|| BETFAIR_TICK_SCHEME.max_price())
        })
    }

    fn min_price(&self) -> Option<Price> {
        self.min_price.or_else(|| {
            self.uses_betfair_tick_scheme()
                .then(|| BETFAIR_TICK_SCHEME.min_price())
        })
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }

    fn margin_init(&self) -> Decimal {
        self.margin_init
    }

    fn margin_maint(&self) -> Decimal {
        self.margin_maint
    }

    fn maker_fee(&self) -> Decimal {
        self.maker_fee
    }

    fn taker_fee(&self) -> Decimal {
        self.taker_fee
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

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use crate::{
        enums::{AssetClass, InstrumentClass},
        identifiers::{InstrumentId, Symbol},
        instruments::{BettingInstrument, Instrument, stubs::*},
        types::{Currency, Money, Price, Quantity},
    };

    #[rstest]
    fn test_trait_accessors(betting: BettingInstrument) {
        assert_eq!(betting.asset_class(), AssetClass::Alternative);
        assert_eq!(betting.instrument_class(), InstrumentClass::SportsBetting);
        assert_eq!(betting.quote_currency(), Currency::GBP());
        assert!(!betting.is_inverse());
        assert_eq!(betting.price_precision(), 2);
        assert_eq!(betting.size_precision(), 2);
        assert_eq!(betting.price_increment(), Price::from("0.01"));
        assert_eq!(betting.size_increment(), Quantity::from("0.01"));
        assert_eq!(betting.margin_init(), dec!(1));
        assert_eq!(betting.margin_maint(), dec!(1));
    }

    #[rstest]
    fn test_new_checked_price_precision_mismatch() {
        let result = BettingInstrument::new_checked(
            InstrumentId::from("1-123.BETFAIR"),
            "1-123".into(),
            6423,
            "Football".into(),
            1,
            "NFL".into(),
            1,
            "NFL".into(),
            "GB".into(),
            0.into(),
            "ODDS".into(),
            "1-123".into(),
            "Winner".into(),
            "SPECIAL".into(),
            0.into(),
            50214,
            "Team".into(),
            0.0,
            Currency::GBP(),
            4, // mismatch
            2,
            Price::from("0.01"),
            Quantity::from("0.01"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            0.into(),
            0.into(),
        );
        assert!(result.is_err());
    }

    #[rstest]
    fn test_serialization_roundtrip(betting: BettingInstrument) {
        let json = serde_json::to_string(&betting).unwrap();
        let deserialized: BettingInstrument = serde_json::from_str(&json).unwrap();
        assert_eq!(betting, deserialized);
    }

    #[rstest]
    fn test_betfair_tick_scheme_navigation(mut betting: BettingInstrument) {
        betting.max_price = None;
        betting.min_price = None;

        assert_eq!(betting.min_price(), Some(Price::from("1.01")));
        assert_eq!(betting.max_price(), Some(Price::from("1000.00")));
        assert_eq!(betting.next_ask_price(4.0, 1), Some(Price::from("4.10")));
        assert_eq!(betting.next_bid_price(2.027, 2), Some(Price::from("1.99")));
        assert_eq!(betting.next_bid_prices(1.102, 20).len(), 10);
        assert_eq!(betting.next_ask_prices(1.102, 20).len(), 20);
    }

    #[rstest]
    fn test_non_betfair_venue_no_tick_scheme(mut betting: BettingInstrument) {
        betting.id = InstrumentId::from("1-123456789.SMARKETS");
        betting.max_price = None;
        betting.min_price = None;

        assert!(betting.tick_scheme().is_none());
        assert!(betting.min_price().is_none());
        assert!(betting.max_price().is_none());
    }

    #[rstest]
    fn test_builder_matches_new_checked() {
        let positional = BettingInstrument::new_checked(
            InstrumentId::from("1-123456789.BETFAIR"),
            Symbol::from("1-123456789"),
            6423,
            "American Football".into(),
            12_282_733,
            "NFL".into(),
            29_678_534,
            "NFL".into(),
            "GB".into(),
            1.into(),
            "ODDS".into(),
            "1-123456789".into(),
            "AFC Conference Winner".into(),
            "SPECIAL".into(),
            2.into(),
            50214,
            "Kansas City Chiefs".into(),
            0.0,
            Currency::GBP(),
            2,
            2,
            Price::from("0.01"),
            Quantity::from("0.01"),
            Some(Quantity::from("1000")),
            Some(Quantity::from("1")),
            Some(Money::from("10000 GBP")),
            Some(Money::from("10 GBP")),
            Some(Price::from("100.00")),
            Some(Price::from("1.00")),
            Some(dec!(0.01)),
            Some(dec!(0.02)),
            Some(dec!(0.0002)),
            Some(dec!(0.0004)),
            None,
            None,
            3.into(),
            4.into(),
        )
        .unwrap();

        let built = BettingInstrument::builder()
            .instrument_id(InstrumentId::from("1-123456789.BETFAIR"))
            .raw_symbol(Symbol::from("1-123456789"))
            .event_type_id(6423)
            .event_type_name("American Football".into())
            .competition_id(12_282_733)
            .competition_name("NFL".into())
            .event_id(29_678_534)
            .event_name("NFL".into())
            .event_country_code("GB".into())
            .event_open_date(1.into())
            .betting_type("ODDS".into())
            .market_id("1-123456789".into())
            .market_name("AFC Conference Winner".into())
            .market_type("SPECIAL".into())
            .market_start_time(2.into())
            .selection_id(50214)
            .selection_name("Kansas City Chiefs".into())
            .selection_handicap(0.0)
            .currency(Currency::GBP())
            .price_precision(2)
            .size_precision(2)
            .price_increment(Price::from("0.01"))
            .size_increment(Quantity::from("0.01"))
            .max_quantity(Quantity::from("1000"))
            .min_quantity(Quantity::from("1"))
            .max_notional(Money::from("10000 GBP"))
            .min_notional(Money::from("10 GBP"))
            .max_price(Price::from("100.00"))
            .min_price(Price::from("1.00"))
            .margin_init(dec!(0.01))
            .margin_maint(dec!(0.02))
            .maker_fee(dec!(0.0002))
            .taker_fee(dec!(0.0004))
            .ts_event(3.into())
            .ts_init(4.into())
            .build()
            .unwrap();

        assert_eq!(
            serde_json::to_value(&positional).unwrap(),
            serde_json::to_value(&built).unwrap(),
        );
    }
}
