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
        CorrectnessResult, CorrectnessResultExt, FAILED, check_equal_u8,
        check_valid_string_ascii_optional,
    },
};
use rust_decimal::Decimal;
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
        quantity::Quantity,
    },
};

/// Represents a generic equity instrument.
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
pub struct Equity {
    /// The instrument ID.
    pub id: InstrumentId,
    /// The raw/local/native symbol for the instrument, assigned by the venue.
    pub raw_symbol: Symbol,
    /// The instruments International Securities Identification Number (ISIN).
    pub isin: Option<Ustr>,
    /// The futures contract currency.
    pub currency: Currency,
    /// The price decimal precision.
    pub price_precision: u8,
    /// The minimum price increment (tick size).
    pub price_increment: Price,
    /// The initial (order) margin requirement in percentage of order value.
    pub margin_init: Decimal,
    /// The maintenance (position) margin in percentage of position value.
    pub margin_maint: Decimal,
    /// The fee rate for liquidity makers as a percentage of order value.
    pub maker_fee: Decimal,
    /// The fee rate for liquidity takers as a percentage of order value.
    pub taker_fee: Decimal,
    /// The rounded lot unit size (standard/board).
    pub lot_size: Option<Quantity>,
    /// The maximum allowable order quantity.
    pub max_quantity: Option<Quantity>,
    /// The minimum allowable order quantity.
    pub min_quantity: Option<Quantity>,
    /// The maximum allowable quoted price.
    pub max_price: Option<Price>,
    /// The minimum allowable quoted price.
    pub min_price: Option<Price>,
    /// Additional instrument metadata as a JSON-serializable dictionary.
    pub info: Option<Params>,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

impl Equity {
    /// Creates a new [`Equity`] instance with correctness checking.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    /// # Errors
    ///
    /// Returns an error if any input validation fails.
    #[expect(clippy::too_many_arguments)]
    pub fn new_checked(
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        isin: Option<Ustr>,
        currency: Currency,
        price_precision: u8,
        price_increment: Price,
        lot_size: Option<Quantity>,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_price: Option<Price>,
        min_price: Option<Price>,
        margin_init: Option<Decimal>,
        margin_maint: Option<Decimal>,
        maker_fee: Option<Decimal>,
        taker_fee: Option<Decimal>,
        info: Option<Params>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> CorrectnessResult<Self> {
        check_valid_string_ascii_optional(isin.map(|u| u.as_str()), stringify!(isin))?;
        check_equal_u8(
            price_precision,
            price_increment.precision,
            stringify!(price_precision),
            stringify!(price_increment.precision),
        )?;
        check_positive_price(price_increment, stringify!(price_increment))?;

        Ok(Self {
            id: instrument_id,
            raw_symbol,
            isin,
            currency,
            price_precision,
            price_increment,
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
            margin_init: margin_init.unwrap_or_default(),
            margin_maint: margin_maint.unwrap_or_default(),
            maker_fee: maker_fee.unwrap_or_default(),
            taker_fee: taker_fee.unwrap_or_default(),
            info,
            ts_event,
            ts_init,
        })
    }

    /// Creates a new [`Equity`] instance.
    ///
    /// # Panics
    ///
    /// Panics if any parameter is invalid (see `new_checked`).
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        isin: Option<Ustr>,
        currency: Currency,
        price_precision: u8,
        price_increment: Price,
        lot_size: Option<Quantity>,
        max_quantity: Option<Quantity>,
        min_quantity: Option<Quantity>,
        max_price: Option<Price>,
        min_price: Option<Price>,
        margin_init: Option<Decimal>,
        margin_maint: Option<Decimal>,
        maker_fee: Option<Decimal>,
        taker_fee: Option<Decimal>,
        info: Option<Params>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new_checked(
            instrument_id,
            raw_symbol,
            isin,
            currency,
            price_precision,
            price_increment,
            lot_size,
            max_quantity,
            min_quantity,
            max_price,
            min_price,
            margin_init,
            margin_maint,
            maker_fee,
            taker_fee,
            info,
            ts_event,
            ts_init,
        )
        .expect_display(FAILED)
    }
}

impl PartialEq<Self> for Equity {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Equity {}

impl Hash for Equity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Instrument for Equity {
    fn into_any(self) -> InstrumentAny {
        InstrumentAny::Equity(self)
    }

    fn id(&self) -> InstrumentId {
        self.id
    }

    fn raw_symbol(&self) -> Symbol {
        self.raw_symbol
    }

    fn asset_class(&self) -> AssetClass {
        AssetClass::Equity
    }

    fn instrument_class(&self) -> InstrumentClass {
        InstrumentClass::Spot
    }
    fn underlying(&self) -> Option<Ustr> {
        None
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
        self.isin
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

    fn is_inverse(&self) -> bool {
        false
    }

    fn price_precision(&self) -> u8 {
        self.price_precision
    }

    fn size_precision(&self) -> u8 {
        0
    }

    fn price_increment(&self) -> Price {
        self.price_increment
    }

    fn size_increment(&self) -> Quantity {
        Quantity::from(1)
    }

    fn multiplier(&self) -> Quantity {
        Quantity::from(1)
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

    use crate::{
        enums::{AssetClass, InstrumentClass},
        identifiers::{InstrumentId, Symbol},
        instruments::{Equity, Instrument, stubs::*},
        types::{Currency, Price, Quantity},
    };

    #[rstest]
    fn test_trait_accessors(equity_aapl: Equity) {
        assert_eq!(equity_aapl.id(), InstrumentId::from("AAPL.XNAS"));
        assert_eq!(equity_aapl.raw_symbol(), Symbol::from("AAPL"));
        assert_eq!(equity_aapl.asset_class(), AssetClass::Equity);
        assert_eq!(equity_aapl.instrument_class(), InstrumentClass::Spot);
        assert_eq!(equity_aapl.quote_currency(), Currency::USD());
        assert_eq!(equity_aapl.settlement_currency(), Currency::USD());
        assert!(!equity_aapl.is_inverse());
        assert_eq!(equity_aapl.price_precision(), 2);
        assert_eq!(equity_aapl.size_precision(), 0);
        assert_eq!(equity_aapl.price_increment(), Price::from("0.01"));
        assert_eq!(equity_aapl.size_increment(), Quantity::from("1"));
        assert_eq!(equity_aapl.multiplier(), Quantity::from("1"));
        assert_eq!(equity_aapl.base_currency(), None);
        assert_eq!(equity_aapl.underlying(), None);
        assert_eq!(equity_aapl.option_kind(), None);
        assert_eq!(equity_aapl.strike_price(), None);
        assert_eq!(equity_aapl.activation_ns(), None);
        assert_eq!(equity_aapl.expiration_ns(), None);
    }

    #[rstest]
    fn test_isin(equity_aapl: Equity) {
        assert_eq!(
            equity_aapl.isin().map(|u| u.to_string()),
            Some("US0378331005".to_string()),
        );
    }

    #[rstest]
    fn test_new_checked_price_precision_mismatch() {
        let result = Equity::new_checked(
            InstrumentId::from("AAPL.XNAS"),
            Symbol::from("AAPL"),
            None,
            Currency::USD(),
            3, // mismatch
            Price::from("0.01"),
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
    fn test_new_checked_zero_price_increment() {
        let result = Equity::new_checked(
            InstrumentId::from("AAPL.XNAS"),
            Symbol::from("AAPL"),
            None,
            Currency::USD(),
            0,
            Price::from("0"),
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
    fn test_new_checked_non_ascii_isin() {
        let result = Equity::new_checked(
            InstrumentId::from("AAPL.XNAS"),
            Symbol::from("AAPL"),
            Some(ustr::Ustr::from("US\u{00E9}378331005")),
            Currency::USD(),
            2,
            Price::from("0.01"),
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
        assert!(result.unwrap_err().to_string().contains("non-ASCII"));
    }

    #[rstest]
    fn test_serialization_roundtrip(equity_aapl: Equity) {
        let json = serde_json::to_string(&equity_aapl).unwrap();
        let deserialized: Equity = serde_json::from_str(&json).unwrap();
        assert_eq!(equity_aapl, deserialized);
    }
}
