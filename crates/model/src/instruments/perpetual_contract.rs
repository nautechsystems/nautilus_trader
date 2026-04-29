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
        CorrectnessError, CorrectnessResult, CorrectnessResultExt, FAILED, check_equal_u8,
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
        quantity::{Quantity, check_positive_quantity},
    },
};

/// Represents a perpetual contract instrument (perpetual swap).
///
/// Supports perpetuals on any asset class including FX, equities,
/// commodities, indexes, and cryptocurrencies.
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
pub struct PerpetualContract {
    /// The instrument ID for the instrument.
    pub id: InstrumentId,
    /// The raw/local/native symbol for the instrument, assigned by the venue.
    pub raw_symbol: Symbol,
    /// The underlying asset identifier (e.g., "EURUSD", "NVDA", "GC").
    pub underlying: Ustr,
    /// The asset class of the perpetual contract.
    pub asset_class: AssetClass,
    /// The base currency (optional, set for FX/crypto underlyings).
    pub base_currency: Option<Currency>,
    /// The quote currency.
    pub quote_currency: Currency,
    /// The settlement currency.
    pub settlement_currency: Currency,
    /// If the instrument costing is inverse (quantity expressed in quote currency units).
    pub is_inverse: bool,
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
    /// The maximum allowable order notional value.
    pub max_notional: Option<Money>,
    /// The minimum allowable order notional value.
    pub min_notional: Option<Money>,
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

impl PerpetualContract {
    /// Creates a new [`PerpetualContract`] instance with correctness checking.
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
        underlying: Ustr,
        asset_class: AssetClass,
        base_currency: Option<Currency>,
        quote_currency: Currency,
        settlement_currency: Currency,
        is_inverse: bool,
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

        if is_inverse && base_currency.is_none() {
            return Err(CorrectnessError::PredicateViolation {
                message: "Inverse perpetual contract requires a `base_currency`".to_string(),
            });
        }

        Ok(Self {
            id: instrument_id,
            raw_symbol,
            underlying,
            asset_class,
            base_currency,
            quote_currency,
            settlement_currency,
            is_inverse,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            multiplier: multiplier.unwrap_or(Quantity::from(1)),
            lot_size: lot_size.unwrap_or(Quantity::from(1)),
            margin_init: margin_init.unwrap_or_default(),
            margin_maint: margin_maint.unwrap_or_default(),
            maker_fee: maker_fee.unwrap_or_default(),
            taker_fee: taker_fee.unwrap_or_default(),
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            info,
            ts_event,
            ts_init,
        })
    }

    /// Creates a new [`PerpetualContract`] instance.
    ///
    /// # Panics
    ///
    /// Panics if any input parameter is invalid (see `new_checked`).
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        underlying: Ustr,
        asset_class: AssetClass,
        base_currency: Option<Currency>,
        quote_currency: Currency,
        settlement_currency: Currency,
        is_inverse: bool,
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
        info: Option<Params>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new_checked(
            instrument_id,
            raw_symbol,
            underlying,
            asset_class,
            base_currency,
            quote_currency,
            settlement_currency,
            is_inverse,
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
            info,
            ts_event,
            ts_init,
        )
        .expect_display(FAILED)
    }
}

impl PartialEq<Self> for PerpetualContract {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for PerpetualContract {}

impl Hash for PerpetualContract {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Instrument for PerpetualContract {
    fn into_any(self) -> InstrumentAny {
        InstrumentAny::PerpetualContract(self)
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
        InstrumentClass::Swap
    }

    fn underlying(&self) -> Option<Ustr> {
        Some(self.underlying)
    }

    fn base_currency(&self) -> Option<Currency> {
        self.base_currency
    }

    fn quote_currency(&self) -> Currency {
        self.quote_currency
    }

    fn settlement_currency(&self) -> Currency {
        self.settlement_currency
    }

    fn isin(&self) -> Option<Ustr> {
        None
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
        self.is_inverse
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
        Some(self.lot_size)
    }

    fn max_quantity(&self) -> Option<Quantity> {
        self.max_quantity
    }

    fn min_quantity(&self) -> Option<Quantity> {
        self.min_quantity
    }

    fn max_notional(&self) -> Option<Money> {
        self.max_notional
    }

    fn min_notional(&self) -> Option<Money> {
        self.min_notional
    }

    fn max_price(&self) -> Option<Price> {
        self.max_price
    }

    fn min_price(&self) -> Option<Price> {
        self.min_price
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

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use ustr::Ustr;

    use crate::{
        enums::{AssetClass, InstrumentClass},
        identifiers::{InstrumentId, Symbol},
        instruments::{Instrument, PerpetualContract, stubs::*},
        types::{Currency, Price, Quantity},
    };

    #[rstest]
    fn test_trait_accessors(perpetual_contract_eurusd: PerpetualContract) {
        assert_eq!(
            perpetual_contract_eurusd.id(),
            InstrumentId::from("EURUSD-PERP.AX"),
        );
        assert_eq!(perpetual_contract_eurusd.asset_class(), AssetClass::FX);
        assert_eq!(
            perpetual_contract_eurusd.instrument_class(),
            InstrumentClass::Swap
        );
        assert_eq!(
            perpetual_contract_eurusd.base_currency(),
            Some(Currency::EUR())
        );
        assert_eq!(perpetual_contract_eurusd.quote_currency(), Currency::USD());
        assert_eq!(
            perpetual_contract_eurusd.settlement_currency(),
            Currency::USD()
        );
        assert!(!perpetual_contract_eurusd.is_inverse());
        assert_eq!(perpetual_contract_eurusd.price_precision(), 5);
        assert_eq!(perpetual_contract_eurusd.size_precision(), 0);
        assert_eq!(
            perpetual_contract_eurusd.price_increment(),
            Price::from("0.00001")
        );
        assert_eq!(
            perpetual_contract_eurusd.size_increment(),
            Quantity::from("1")
        );
        assert_eq!(
            perpetual_contract_eurusd.underlying(),
            Some(Ustr::from("EURUSD")),
        );
    }

    #[rstest]
    fn test_new_checked_inverse_without_base_currency() {
        let result = PerpetualContract::new_checked(
            InstrumentId::from("TEST.EXCHANGE"),
            Symbol::from("TEST"),
            Ustr::from("TEST"),
            AssetClass::FX,
            None, // no base_currency
            Currency::USD(),
            Currency::USD(),
            true, // is_inverse
            5,
            0,
            Price::from("0.00001"),
            Quantity::from("1"),
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
        assert!(result.unwrap_err().to_string().contains("base_currency"),);
    }

    #[rstest]
    fn test_new_checked_price_precision_mismatch() {
        let result = PerpetualContract::new_checked(
            InstrumentId::from("TEST.EXCHANGE"),
            Symbol::from("TEST"),
            Ustr::from("TEST"),
            AssetClass::FX,
            Some(Currency::EUR()),
            Currency::USD(),
            Currency::USD(),
            false,
            3, // mismatch
            0,
            Price::from("0.00001"),
            Quantity::from("1"),
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
    fn test_serialization_roundtrip(perpetual_contract_eurusd: PerpetualContract) {
        let json = serde_json::to_string(&perpetual_contract_eurusd).unwrap();
        let deserialized: PerpetualContract = serde_json::from_str(&json).unwrap();
        assert_eq!(perpetual_contract_eurusd, deserialized);
    }
}
