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
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::{Instrument, any::InstrumentAny, tick_scheme::check_tick_scheme};
use crate::{
    enums::{AssetClass, CurrencyType, InstrumentClass, OptionKind},
    identifiers::{InstrumentId, Symbol},
    types::{
        currency::Currency,
        money::Money,
        price::{Price, check_positive_price},
        quantity::{Quantity, check_positive_quantity},
    },
};

/// Represents a generic currency pair instrument in a spot/cash market.
///
/// Can represent both Fiat FX and Cryptocurrency pairs.
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
pub struct CurrencyPair {
    /// The instrument ID for the instrument.
    pub id: InstrumentId,
    /// The raw/local/native symbol for the instrument, assigned by the venue.
    pub raw_symbol: Symbol,
    /// The base currency.
    pub base_currency: Currency,
    /// The quote currency.
    pub quote_currency: Currency,
    /// The price decimal precision.
    pub price_precision: u8,
    /// The trading size decimal precision.
    pub size_precision: u8,
    /// The minimum price increment (tick size).
    pub price_increment: Price,
    /// The minimum size increment.
    pub size_increment: Quantity,
    /// The contract multiplier.
    pub multiplier: Quantity,
    /// The rounded lot unit size.
    pub lot_size: Option<Quantity>,
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
impl CurrencyPair {
    /// Creates a new [`CurrencyPair`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if any input validation fails.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    #[expect(clippy::too_many_arguments)]
    pub fn new_checked(
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        base_currency: Currency,
        quote_currency: Currency,
        price_precision: u8,
        size_precision: u8,
        price_increment: Price,
        size_increment: Quantity,
        multiplier: Option<Quantity>,
        lot_size: Option<Quantity>,
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

        if let Some(multiplier) = multiplier {
            check_positive_quantity(multiplier, stringify!(multiplier))?;
        }

        if let Some(lot_size) = lot_size {
            check_positive_quantity(lot_size, stringify!(lot_size))?;
        }

        Ok(Self {
            id: instrument_id,
            raw_symbol,
            base_currency,
            quote_currency,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            multiplier: multiplier.unwrap_or(Quantity::from(1)),
            lot_size,
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            margin_init: margin_init.unwrap_or_default(),
            margin_maint: margin_maint.unwrap_or_default(),
            maker_fee: maker_fee.unwrap_or_default(),
            taker_fee: taker_fee.unwrap_or_default(),
            tick_scheme,
            info,
            ts_event,
            ts_init,
        })
    }

    /// Creates a new [`CurrencyPair`] instance.
    ///
    /// # Panics
    ///
    /// Panics if any input parameter is invalid (see `new_checked`).
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        base_currency: Currency,
        quote_currency: Currency,
        price_precision: u8,
        size_precision: u8,
        price_increment: Price,
        size_increment: Quantity,
        multiplier: Option<Quantity>,
        lot_size: Option<Quantity>,
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
            base_currency,
            quote_currency,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            multiplier,
            lot_size,
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

    /// Returns a fluent builder for a [`CurrencyPair`] instance.
    ///
    /// Required fields are enforced at compile time; optional fields can be omitted and default
    /// the same way they do in [`CurrencyPair::new_checked`], which the builder calls so the same
    /// correctness checks run on `build`.
    ///
    /// # Errors
    ///
    /// Returns an error if any input validation fails (see [`CurrencyPair::new_checked`]).
    #[builder(start_fn = builder, finish_fn = build)]
    pub fn build_checked(
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        base_currency: Currency,
        quote_currency: Currency,
        price_precision: u8,
        size_precision: u8,
        price_increment: Price,
        size_increment: Quantity,
        multiplier: Option<Quantity>,
        lot_size: Option<Quantity>,
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
            base_currency,
            quote_currency,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            multiplier,
            lot_size,
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
}

impl PartialEq<Self> for CurrencyPair {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for CurrencyPair {}

impl Hash for CurrencyPair {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Instrument for CurrencyPair {
    fn tick_scheme(&self) -> Option<Ustr> {
        self.tick_scheme
    }
    fn into_any(self) -> InstrumentAny {
        InstrumentAny::CurrencyPair(self)
    }

    fn id(&self) -> InstrumentId {
        self.id
    }

    fn raw_symbol(&self) -> Symbol {
        self.raw_symbol
    }

    fn asset_class(&self) -> AssetClass {
        if self.base_currency.currency_type == CurrencyType::Crypto
            || self.quote_currency.currency_type == CurrencyType::Crypto
        {
            AssetClass::Cryptocurrency
        } else {
            AssetClass::FX
        }
    }

    fn instrument_class(&self) -> InstrumentClass {
        InstrumentClass::Spot
    }

    fn underlying(&self) -> Option<Ustr> {
        None
    }

    fn base_currency(&self) -> Option<Currency> {
        Some(self.base_currency)
    }

    fn quote_currency(&self) -> Currency {
        self.quote_currency
    }

    fn settlement_currency(&self) -> Currency {
        self.quote_currency
    }
    fn isin(&self) -> Option<Ustr> {
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
        self.multiplier
    }

    fn lot_size(&self) -> Option<Quantity> {
        self.lot_size
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

    fn margin_init(&self) -> Decimal {
        self.margin_init
    }

    fn margin_maint(&self) -> Decimal {
        self.margin_maint
    }

    fn taker_fee(&self) -> Decimal {
        self.taker_fee
    }

    fn maker_fee(&self) -> Decimal {
        self.maker_fee
    }

    fn option_kind(&self) -> Option<OptionKind> {
        None
    }

    fn exchange(&self) -> Option<Ustr> {
        None
    }

    fn strike_price(&self) -> Option<Price> {
        None
    }

    fn activation_ns(&self) -> Option<UnixNanos> {
        None
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
        instruments::{CurrencyPair, Instrument, stubs::*},
        types::{Currency, Money, Price, Quantity},
    };

    #[rstest]
    fn test_trait_accessors(currency_pair_btcusdt: CurrencyPair) {
        assert_eq!(
            currency_pair_btcusdt.id(),
            InstrumentId::from("BTCUSDT.BINANCE")
        );
        assert_eq!(
            currency_pair_btcusdt.asset_class(),
            AssetClass::Cryptocurrency
        );
        assert_eq!(
            currency_pair_btcusdt.instrument_class(),
            InstrumentClass::Spot
        );
        assert_eq!(currency_pair_btcusdt.base_currency(), Some(Currency::BTC()));
        assert_eq!(currency_pair_btcusdt.quote_currency(), Currency::USDT());
        assert!(!currency_pair_btcusdt.is_inverse());
        assert_eq!(currency_pair_btcusdt.price_precision(), 2);
        assert_eq!(currency_pair_btcusdt.size_precision(), 6);
        assert_eq!(currency_pair_btcusdt.price_increment(), Price::from("0.01"));
        assert_eq!(
            currency_pair_btcusdt.size_increment(),
            Quantity::from("0.000001")
        );
    }

    #[rstest]
    fn test_new_checked_price_precision_mismatch() {
        let result = CurrencyPair::new_checked(
            InstrumentId::from("TEST.BINANCE"),
            Symbol::from("TEST"),
            Currency::BTC(),
            Currency::USDT(),
            4, // mismatch
            6,
            Price::from("0.01"),
            Quantity::from("0.000001"),
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
            None,
            None,
            0.into(),
            0.into(),
        );
        assert!(result.is_err());
    }

    #[rstest]
    #[case::zero_multiplier(Some(Quantity::from("0")), None)]
    #[case::zero_lot_size(None, Some(Quantity::from("0")))]
    fn test_new_checked_rejects_non_positive_sizing(
        #[case] multiplier: Option<Quantity>,
        #[case] lot_size: Option<Quantity>,
    ) {
        let result = CurrencyPair::new_checked(
            InstrumentId::from("TEST.BINANCE"),
            Symbol::from("TEST"),
            Currency::BTC(),
            Currency::USDT(),
            2,
            6,
            Price::from("0.01"),
            Quantity::from("0.000001"),
            multiplier,
            lot_size,
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
        let error = result.unwrap_err();
        assert!(error.to_string().contains("not positive"), "{error}");
    }

    #[rstest]
    fn test_serialization_roundtrip(currency_pair_btcusdt: CurrencyPair) {
        let json = serde_json::to_string(&currency_pair_btcusdt).unwrap();
        let deserialized: CurrencyPair = serde_json::from_str(&json).unwrap();
        assert_eq!(currency_pair_btcusdt, deserialized);
    }

    #[rstest]
    fn test_builder_matches_new_checked() {
        let positional = CurrencyPair::new_checked(
            InstrumentId::from("BTCUSDT.BINANCE"),
            Symbol::from("BTCUSDT"),
            Currency::BTC(),
            Currency::USDT(),
            2,
            6,
            Price::from("0.01"),
            Quantity::from("0.000001"),
            Some(Quantity::from("10")),
            Some(Quantity::from("5")),
            Some(Quantity::from("9000.0")),
            Some(Quantity::from("0.000001")),
            Some(Money::new(1_000_000.0, Currency::USDT())),
            Some(Money::new(10.0, Currency::USDT())),
            Some(Price::from("1000000.00")),
            Some(Price::from("0.01")),
            Some(dec!(0.01)),
            Some(dec!(0.02)),
            Some(dec!(0.0002)),
            Some(dec!(0.0004)),
            None,
            None,
            1.into(),
            2.into(),
        )
        .unwrap();

        let built = CurrencyPair::builder()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .raw_symbol(Symbol::from("BTCUSDT"))
            .base_currency(Currency::BTC())
            .quote_currency(Currency::USDT())
            .price_precision(2)
            .size_precision(6)
            .price_increment(Price::from("0.01"))
            .size_increment(Quantity::from("0.000001"))
            .multiplier(Quantity::from("10"))
            .lot_size(Quantity::from("5"))
            .max_quantity(Quantity::from("9000.0"))
            .min_quantity(Quantity::from("0.000001"))
            .max_notional(Money::new(1_000_000.0, Currency::USDT()))
            .min_notional(Money::new(10.0, Currency::USDT()))
            .max_price(Price::from("1000000.00"))
            .min_price(Price::from("0.01"))
            .margin_init(dec!(0.01))
            .margin_maint(dec!(0.02))
            .maker_fee(dec!(0.0002))
            .taker_fee(dec!(0.0004))
            .ts_event(1.into())
            .ts_init(2.into())
            .build()
            .unwrap();

        assert_eq!(
            serde_json::to_value(&positional).unwrap(),
            serde_json::to_value(&built).unwrap(),
        );
    }
}
