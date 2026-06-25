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
    correctness::{
        CorrectnessResult, CorrectnessResultExt, FAILED, check_equal_u8, check_valid_string_ascii,
        check_valid_string_ascii_optional,
    },
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::{Instrument, any::InstrumentAny, tick_scheme::check_tick_scheme};
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

/// Represents a generic option spread instrument.
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
pub struct OptionSpread {
    /// The instrument ID.
    pub id: InstrumentId,
    /// The raw/local/native symbol for the instrument, assigned by the venue.
    pub raw_symbol: Symbol,
    /// The option spread asset class.
    pub asset_class: AssetClass,
    /// The exchange ISO 10383 Market Identifier Code (MIC) where the instrument trades.
    pub exchange: Option<Ustr>,
    /// The underlying asset.
    pub underlying: Ustr,
    /// The strategy type of the spread.
    pub strategy_type: Ustr,
    /// UNIX timestamp (nanoseconds) for contract activation.
    pub activation_ns: UnixNanos,
    /// UNIX timestamp (nanoseconds) for contract expiration.
    pub expiration_ns: UnixNanos,
    /// The option spread currency.
    pub currency: Currency,
    /// The price decimal precision.
    pub price_precision: u8,
    /// The minimum price increment (tick size).
    pub price_increment: Price,
    /// The minimum size increment.
    pub size_increment: Quantity,
    /// The trading size decimal precision.
    pub size_precision: u8,
    /// The option multiplier.
    pub multiplier: Quantity,
    /// The rounded lot unit size (standard/board).
    pub lot_size: Quantity,
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
impl OptionSpread {
    /// Creates a new [`OptionSpread`] instance with correctness checking.
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
        asset_class: AssetClass,
        exchange: Option<Ustr>,
        underlying: Ustr,
        strategy_type: Ustr,
        activation_ns: UnixNanos,
        expiration_ns: UnixNanos,
        currency: Currency,
        price_precision: u8,
        price_increment: Price,
        multiplier: Quantity,
        lot_size: Quantity,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
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
        check_valid_string_ascii_optional(exchange.map(|u| u.as_str()), stringify!(exchange))?;
        check_valid_string_ascii(strategy_type.as_str(), stringify!(strategy_type))?;
        check_equal_u8(
            price_precision,
            price_increment.precision,
            stringify!(price_precision),
            stringify!(price_increment.precision),
        )?;
        check_positive_price(price_increment, stringify!(price_increment))?;
        check_tick_scheme(tick_scheme)?;
        check_positive_quantity(multiplier, stringify!(multiplier))?;
        check_positive_quantity(lot_size, stringify!(lot_size))?;

        Ok(Self {
            id: instrument_id,
            raw_symbol,
            asset_class,
            exchange,
            underlying,
            strategy_type,
            activation_ns,
            expiration_ns,
            currency,
            price_precision,
            price_increment,
            size_precision: 0,
            size_increment: Quantity::from("1"),
            multiplier,
            lot_size,
            margin_init: margin_init.unwrap_or_default(),
            margin_maint: margin_maint.unwrap_or_default(),
            maker_fee: maker_fee.unwrap_or_default(),
            taker_fee: taker_fee.unwrap_or_default(),
            max_quantity,
            min_quantity: Some(min_quantity.unwrap_or(1.into())),
            max_price,
            min_price,
            tick_scheme,
            info,
            ts_event,
            ts_init,
        })
    }

    /// Creates a new [`OptionSpread`] instance.
    ///
    /// # Panics
    ///
    /// Panics if any input parameter is invalid (see `new_checked`).
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        exchange: Option<Ustr>,
        underlying: Ustr,
        strategy_type: Ustr,
        activation_ns: UnixNanos,
        expiration_ns: UnixNanos,
        currency: Currency,
        price_precision: u8,
        price_increment: Price,
        multiplier: Quantity,
        lot_size: Quantity,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
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
            asset_class,
            exchange,
            underlying,
            strategy_type,
            activation_ns,
            expiration_ns,
            currency,
            price_precision,
            price_increment,
            multiplier,
            lot_size,
            max_quantity,
            min_quantity,
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

    /// Returns a fluent builder for a [`OptionSpread`] instance.
    ///
    /// Required fields are enforced at compile time; optional fields can be omitted and default
    /// the same way they do in [`OptionSpread::new_checked`], which the builder calls so the same
    /// correctness checks run on `build`.
    ///
    /// # Errors
    ///
    /// Returns an error if any input validation fails (see [`OptionSpread::new_checked`]).
    #[builder(start_fn = builder, finish_fn = build)]
    pub fn build_checked(
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        exchange: Option<Ustr>,
        underlying: Ustr,
        strategy_type: Ustr,
        activation_ns: UnixNanos,
        expiration_ns: UnixNanos,
        currency: Currency,
        price_precision: u8,
        price_increment: Price,
        multiplier: Quantity,
        lot_size: Quantity,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
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
            asset_class,
            exchange,
            underlying,
            strategy_type,
            activation_ns,
            expiration_ns,
            currency,
            price_precision,
            price_increment,
            multiplier,
            lot_size,
            max_quantity,
            min_quantity,
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

impl PartialEq<Self> for OptionSpread {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for OptionSpread {}

impl Hash for OptionSpread {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Instrument for OptionSpread {
    fn tick_scheme(&self) -> Option<Ustr> {
        self.tick_scheme
    }
    fn into_any(self) -> InstrumentAny {
        InstrumentAny::OptionSpread(self)
    }

    fn id(&self) -> InstrumentId {
        self.id
    }

    fn raw_symbol(&self) -> Symbol {
        self.raw_symbol
    }

    fn asset_class(&self) -> AssetClass {
        self.asset_class
    }

    fn instrument_class(&self) -> InstrumentClass {
        InstrumentClass::OptionSpread
    }
    fn underlying(&self) -> Option<Ustr> {
        Some(self.underlying)
    }

    fn base_currency(&self) -> Option<Currency> {
        None
    }

    fn quote_currency(&self) -> Currency {
        self.currency
    }

    fn settlement_currency(&self) -> Currency {
        self.currency
    }

    fn isin(&self) -> Option<Ustr> {
        None
    }

    fn option_kind(&self) -> Option<OptionKind> {
        None
    }

    fn exchange(&self) -> Option<Ustr> {
        self.exchange
    }

    fn strike_price(&self) -> Option<Price> {
        None
    }

    fn strategy_type(&self) -> Option<Ustr> {
        Some(self.strategy_type)
    }

    fn activation_ns(&self) -> Option<UnixNanos> {
        Some(self.activation_ns)
    }

    fn expiration_ns(&self) -> Option<UnixNanos> {
        Some(self.expiration_ns)
    }

    fn is_inverse(&self) -> bool {
        false
    }

    fn price_precision(&self) -> u8 {
        self.price_precision
    }

    fn size_precision(&self) -> u8 {
        0 // No fractional units
    }

    fn price_increment(&self) -> Price {
        self.price_increment
    }

    fn size_increment(&self) -> Quantity {
        Quantity::from(1)
    }

    fn multiplier(&self) -> Quantity {
        self.multiplier
    }

    fn lot_size(&self) -> Option<Quantity> {
        Some(self.lot_size)
    }

    fn max_quantity(&self) -> Option<Quantity> {
        self.max_quantity
    }

    fn min_quantity(&self) -> Option<Quantity> {
        self.min_quantity
    }

    fn max_notional(&self) -> Option<Money> {
        None
    }

    fn min_notional(&self) -> Option<Money> {
        None
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

    fn maker_fee(&self) -> Decimal {
        self.maker_fee
    }

    fn taker_fee(&self) -> Decimal {
        self.taker_fee
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use crate::{
        enums::{AssetClass, InstrumentClass},
        identifiers::{InstrumentId, Symbol},
        instruments::{Instrument, OptionSpread, stubs::*},
        types::{Currency, Price, Quantity},
    };

    #[rstest]
    fn test_trait_accessors(option_spread: OptionSpread) {
        assert_eq!(
            option_spread.id(),
            InstrumentId::from("UD:U$: GN 2534559.GLBX")
        );
        assert_eq!(option_spread.asset_class(), AssetClass::FX);
        assert_eq!(
            option_spread.instrument_class(),
            InstrumentClass::OptionSpread
        );
        assert_eq!(option_spread.quote_currency(), Currency::USD());
        assert!(!option_spread.is_inverse());
        assert_eq!(option_spread.exchange(), Some(Ustr::from("XCME")));
        assert_eq!(option_spread.size_precision(), 0);
        assert_eq!(option_spread.size_increment(), Quantity::from("1"));
        assert_eq!(option_spread.min_quantity(), Some(Quantity::from("1")));
    }

    #[rstest]
    fn test_new_checked_price_precision_mismatch() {
        let result = OptionSpread::new_checked(
            InstrumentId::from("TEST.GLBX"),
            Symbol::from("TEST"),
            AssetClass::FX,
            Some(Ustr::from("XCME")),
            Ustr::from("SR3"),
            Ustr::from("GN"),
            0.into(),
            0.into(),
            Currency::USD(),
            4, // mismatch
            Price::from("0.01"),
            Quantity::from(1),
            Quantity::from(1),
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
    fn test_serialization_roundtrip(option_spread: OptionSpread) {
        let json = serde_json::to_string(&option_spread).unwrap();
        let deserialized: OptionSpread = serde_json::from_str(&json).unwrap();
        assert_eq!(option_spread, deserialized);
    }

    #[rstest]
    fn test_builder_matches_new_checked() {
        let positional = OptionSpread::new_checked(
            InstrumentId::from("UD:U$: GN 2534559.GLBX"),
            Symbol::from("UD:U$: GN 2534559"),
            AssetClass::FX,
            Some(Ustr::from("XCME")),
            Ustr::from("SR3"),
            Ustr::from("GN"),
            1.into(),
            2.into(),
            Currency::USD(),
            2,
            Price::from("0.01"),
            Quantity::from(10),
            Quantity::from(5),
            Some(Quantity::from("100")),
            Some(Quantity::from("1")),
            Some(Price::from("999.0")),
            Some(Price::from("1.0")),
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

        let built = OptionSpread::builder()
            .instrument_id(InstrumentId::from("UD:U$: GN 2534559.GLBX"))
            .raw_symbol(Symbol::from("UD:U$: GN 2534559"))
            .asset_class(AssetClass::FX)
            .exchange(Ustr::from("XCME"))
            .underlying(Ustr::from("SR3"))
            .strategy_type(Ustr::from("GN"))
            .activation_ns(1.into())
            .expiration_ns(2.into())
            .currency(Currency::USD())
            .price_precision(2)
            .price_increment(Price::from("0.01"))
            .multiplier(Quantity::from(10))
            .lot_size(Quantity::from(5))
            .max_quantity(Quantity::from("100"))
            .min_quantity(Quantity::from("1"))
            .max_price(Price::from("999.0"))
            .min_price(Price::from("1.0"))
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
