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

/// Represents a generic index instrument.
///
/// An index is typically not directly tradable.
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
pub struct IndexInstrument {
    /// The instrument ID.
    pub id: InstrumentId,
    /// The raw/local/native symbol for the instrument, assigned by the venue.
    pub raw_symbol: Symbol,
    /// The index currency.
    pub currency: Currency,
    /// The price decimal precision.
    pub price_precision: u8,
    /// The trading size decimal precision.
    pub size_precision: u8,
    /// The minimum price increment (tick size).
    pub price_increment: Price,
    /// The minimum size increment.
    pub size_increment: Quantity,
    /// Additional instrument metadata as a JSON-serializable dictionary.
    pub info: Option<Params>,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

impl IndexInstrument {
    /// Creates a new [`IndexInstrument`] instance with correctness checking.
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
        currency: Currency,
        price_precision: u8,
        size_precision: u8,
        price_increment: Price,
        size_increment: Quantity,
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

        Ok(Self {
            id: instrument_id,
            raw_symbol,
            currency,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            info,
            ts_event,
            ts_init,
        })
    }

    /// Creates a new [`IndexInstrument`] instance.
    ///
    /// # Panics
    ///
    /// Panics if any parameter is invalid (see `new_checked`).
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        currency: Currency,
        price_precision: u8,
        size_precision: u8,
        price_increment: Price,
        size_increment: Quantity,
        info: Option<Params>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new_checked(
            instrument_id,
            raw_symbol,
            currency,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            info,
            ts_event,
            ts_init,
        )
        .expect_display(FAILED)
    }
}

impl PartialEq<Self> for IndexInstrument {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for IndexInstrument {}

impl Hash for IndexInstrument {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Instrument for IndexInstrument {
    fn into_any(self) -> InstrumentAny {
        InstrumentAny::IndexInstrument(self)
    }

    fn id(&self) -> InstrumentId {
        self.id
    }

    fn raw_symbol(&self) -> Symbol {
        self.raw_symbol
    }

    fn asset_class(&self) -> AssetClass {
        AssetClass::Index
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
        None
    }

    fn max_quantity(&self) -> Option<Quantity> {
        None
    }

    fn min_quantity(&self) -> Option<Quantity> {
        None
    }

    fn max_notional(&self) -> Option<Money> {
        None
    }

    fn min_notional(&self) -> Option<Money> {
        None
    }

    fn max_price(&self) -> Option<Price> {
        None
    }

    fn min_price(&self) -> Option<Price> {
        None
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

    use crate::{
        enums::{AssetClass, InstrumentClass},
        identifiers::{InstrumentId, Symbol},
        instruments::{IndexInstrument, Instrument, stubs::*},
        types::{Currency, Price, Quantity},
    };

    #[rstest]
    fn test_trait_accessors(index_instrument_spx: IndexInstrument) {
        assert_eq!(index_instrument_spx.id(), InstrumentId::from("SPX.INDEX"));
        assert_eq!(index_instrument_spx.asset_class(), AssetClass::Index);
        assert_eq!(
            index_instrument_spx.instrument_class(),
            InstrumentClass::Spot
        );
        assert_eq!(index_instrument_spx.quote_currency(), Currency::USD());
        assert!(!index_instrument_spx.is_inverse());
        assert_eq!(index_instrument_spx.price_precision(), 2);
        assert_eq!(index_instrument_spx.size_precision(), 0);
    }

    #[rstest]
    fn test_new_checked_price_precision_mismatch() {
        let result = IndexInstrument::new_checked(
            InstrumentId::from("SPX.INDEX"),
            Symbol::from("SPX"),
            Currency::USD(),
            4, // mismatch
            0,
            Price::from("0.01"),
            Quantity::from("1"),
            None,
            0.into(),
            0.into(),
        );
        assert!(result.is_err());
    }

    #[rstest]
    fn test_serialization_roundtrip(index_instrument_spx: IndexInstrument) {
        let json = serde_json::to_string(&index_instrument_spx).unwrap();
        let deserialized: IndexInstrument = serde_json::from_str(&json).unwrap();
        assert_eq!(index_instrument_spx, deserialized);
    }
}
