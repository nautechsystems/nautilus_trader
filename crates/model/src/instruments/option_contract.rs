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
        CorrectnessResult, check_equal_u8, check_valid_string_ascii,
        check_valid_string_ascii_optional,
    },
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::{Instrument, any::InstrumentAny, tick_scheme::check_tick_scheme};
use crate::{
    enums::{
        AssetClass, ExerciseStyle, InstrumentClass, OptionKind, OptionPremiumStyle, SettlementType,
    },
    identifiers::{InstrumentId, Symbol},
    types::{
        currency::Currency,
        money::Money,
        price::{Price, check_positive_price},
        quantity::{Quantity, check_positive_quantity},
    },
};

/// Represents a generic option contract instrument.
#[repr(C)]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct OptionContract {
    /// The instrument ID.
    pub id: InstrumentId,
    /// The raw/local/native symbol for the instrument, assigned by the venue.
    pub raw_symbol: Symbol,
    /// The option contract asset class.
    pub asset_class: AssetClass,
    /// The exchange ISO 10383 Market Identifier Code (MIC) where the instrument trades.
    pub exchange: Option<Ustr>,
    /// The underlying asset.
    pub underlying: Ustr,
    /// The actual underlying instrument, which may trade on another venue; absent when unknown.
    #[serde(default)]
    pub underlying_instrument_id: Option<InstrumentId>,
    /// The kind of option (PUT | CALL).
    pub option_kind: OptionKind,
    /// The option strike price.
    pub strike_price: Price,
    /// The currency in which the strike price is denominated.
    pub strike_currency: Currency,
    /// The currency used to quote the option premium.
    pub quote_currency: Currency,
    /// The currency used to settle the contract.
    pub settlement_currency: Currency,
    /// The premium cash flow style; absent when unspecified.
    #[serde(default)]
    pub premium_style: Option<OptionPremiumStyle>,
    /// The exercise style; absent when unknown.
    #[serde(default)]
    pub exercise_style: Option<ExerciseStyle>,
    /// The contractual settlement type; absent when unknown or not represented by a single type.
    #[serde(default)]
    pub settlement_type: Option<SettlementType>,
    /// UNIX timestamp (nanoseconds) for contract activation.
    pub activation_ns: UnixNanos,
    /// UNIX timestamp (nanoseconds) for contract expiration.
    pub expiration_ns: UnixNanos,
    /// The price decimal precision.
    pub price_precision: u8,
    /// The trading size decimal precision.
    pub size_precision: u8,
    /// The minimum price increment (tick size).
    pub price_increment: Price,
    /// The minimum size increment.
    pub size_increment: Quantity,
    /// The option multiplier.
    pub multiplier: Quantity,
    /// The rounded lot unit size (standard/board).
    pub lot_size: Quantity,
    /// The maximum allowable order quantity.
    pub max_quantity: Option<Quantity>,
    /// The minimum allowable order quantity.
    pub min_quantity: Option<Quantity>,
    /// The maximum allowable quoted price.
    pub max_price: Option<Price>,
    /// The minimum allowable quoted price.
    pub min_price: Option<Price>,
    /// The initial (order) margin requirement in percentage of order value.
    pub margin_init: Decimal,
    /// The maintenance (position) margin in percentage of position value.
    pub margin_maint: Decimal,
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
impl OptionContract {
    #[expect(clippy::too_many_arguments)]
    fn new_checked(
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        exchange: Option<Ustr>,
        underlying: Ustr,
        underlying_instrument_id: Option<InstrumentId>,
        option_kind: OptionKind,
        strike_price: Price,
        strike_currency: Currency,
        quote_currency: Currency,
        settlement_currency: Currency,
        premium_style: Option<OptionPremiumStyle>,
        exercise_style: Option<ExerciseStyle>,
        settlement_type: Option<SettlementType>,
        activation_ns: UnixNanos,
        expiration_ns: UnixNanos,
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
        tick_scheme: Option<Ustr>,
        info: Option<Params>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> CorrectnessResult<Self> {
        check_valid_string_ascii_optional(exchange, stringify!(exchange))?;
        check_valid_string_ascii(underlying, stringify!(underlying))?;
        check_equal_u8(
            price_precision,
            price_increment.precision,
            stringify!(price_precision),
            stringify!(price_increment.precision),
        )?;
        check_positive_price(price_increment, stringify!(price_increment))?;
        check_positive_price(strike_price, stringify!(strike_price))?;
        check_tick_scheme(tick_scheme)?;
        check_positive_quantity(multiplier, stringify!(multiplier))?;
        check_positive_quantity(lot_size, stringify!(lot_size))?;

        Ok(Self {
            id: instrument_id,
            raw_symbol,
            asset_class,
            exchange,
            underlying,
            underlying_instrument_id,
            option_kind,
            strike_price,
            strike_currency,
            quote_currency,
            settlement_currency,
            premium_style,
            exercise_style,
            settlement_type,
            activation_ns,
            expiration_ns,
            price_precision,
            size_precision: 0,
            price_increment,
            size_increment: Quantity::from(1),
            multiplier,
            lot_size,
            max_quantity,
            min_quantity: Some(min_quantity.unwrap_or(1.into())),
            max_price,
            min_price,
            margin_init: margin_init.unwrap_or_default(),
            margin_maint: margin_maint.unwrap_or_default(),
            tick_scheme,
            info,
            ts_event,
            ts_init,
        })
    }

    /// Returns a fluent builder for a [`OptionContract`] instance.
    ///
    /// Required fields are enforced at compile time; optional fields can be omitted and use the
    /// same defaults as checked construction. The same correctness checks run on `build`.
    ///
    /// # Errors
    ///
    /// Returns an error if any input validation fails.
    #[builder(start_fn = builder, finish_fn = build)]
    pub fn build_checked(
        instrument_id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        exchange: Option<Ustr>,
        underlying: Ustr,
        underlying_instrument_id: Option<InstrumentId>,
        option_kind: OptionKind,
        strike_price: Price,
        strike_currency: Currency,
        quote_currency: Currency,
        settlement_currency: Currency,
        premium_style: Option<OptionPremiumStyle>,
        exercise_style: Option<ExerciseStyle>,
        settlement_type: Option<SettlementType>,
        activation_ns: UnixNanos,
        expiration_ns: UnixNanos,
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
            underlying_instrument_id,
            option_kind,
            strike_price,
            strike_currency,
            quote_currency,
            settlement_currency,
            premium_style,
            exercise_style,
            settlement_type,
            activation_ns,
            expiration_ns,
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
            tick_scheme,
            info,
            ts_event,
            ts_init,
        )
    }
}

impl PartialEq<Self> for OptionContract {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for OptionContract {}

impl Hash for OptionContract {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Instrument for OptionContract {
    fn into_any(self) -> InstrumentAny {
        InstrumentAny::OptionContract(self)
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
        InstrumentClass::Option
    }
    fn underlying(&self) -> Option<Ustr> {
        Some(self.underlying)
    }

    fn base_currency(&self) -> Option<Currency> {
        None
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
        Some(self.option_kind)
    }

    fn exchange(&self) -> Option<Ustr> {
        self.exchange
    }

    fn strike_currency(&self) -> Option<Currency> {
        Some(self.strike_currency)
    }

    fn strike_price(&self) -> Option<Price> {
        Some(self.strike_price)
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
        0
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

    fn tick_scheme(&self) -> Option<Ustr> {
        self.tick_scheme
    }

    fn info(&self) -> Option<&Params> {
        self.info.as_ref()
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
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use crate::{
        enums::{
            AssetClass, ExerciseStyle, InstrumentClass, OptionKind, OptionPremiumStyle,
            SettlementType,
        },
        identifiers::{InstrumentId, Symbol},
        instruments::{Instrument, OptionContract, stubs::*},
        types::{Currency, Price, Quantity},
    };

    #[rstest]
    fn test_trait_accessors(option_contract_appl: OptionContract) {
        assert_eq!(
            option_contract_appl.id(),
            InstrumentId::from("AAPL211217C00150000.OPRA"),
        );
        assert_eq!(option_contract_appl.asset_class(), AssetClass::Equity);
        assert_eq!(
            option_contract_appl.instrument_class(),
            InstrumentClass::Option
        );
        assert_eq!(option_contract_appl.quote_currency(), Currency::USD());
        assert!(!option_contract_appl.is_inverse());
        assert_eq!(option_contract_appl.option_kind(), Some(OptionKind::Call));
        assert_eq!(
            option_contract_appl.strike_price(),
            Some(Price::from("149.0"))
        );
        assert_eq!(option_contract_appl.underlying(), Some(Ustr::from("AAPL")));
        assert_eq!(option_contract_appl.exchange(), Some(Ustr::from("GMNI")));
        assert!(option_contract_appl.activation_ns().is_some());
        assert!(option_contract_appl.expiration_ns().is_some());
        assert_eq!(option_contract_appl.size_precision(), 0);
        assert_eq!(option_contract_appl.size_increment(), Quantity::from("1"));
        assert_eq!(
            option_contract_appl.min_quantity(),
            Some(Quantity::from("1"))
        );
    }

    #[rstest]
    fn test_new_checked_price_precision_mismatch() {
        let result = OptionContract::new_checked(
            InstrumentId::from("TEST.OPRA"),
            Symbol::from("TEST"),
            AssetClass::Equity,
            Some(Ustr::from("GMNI")),
            Ustr::from("AAPL"),
            None,
            OptionKind::Call,
            Price::from("150.0"),
            Currency::USD(),
            Currency::USD(),
            Currency::USD(),
            None,
            None,
            None,
            0.into(),
            0.into(),
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
            0.into(),
            0.into(),
        );
        assert!(result.is_err());
    }

    #[rstest]
    fn test_new_checked_zero_multiplier() {
        let result = OptionContract::new_checked(
            InstrumentId::from("TEST.OPRA"),
            Symbol::from("TEST"),
            AssetClass::Equity,
            Some(Ustr::from("GMNI")),
            Ustr::from("AAPL"),
            None,
            OptionKind::Call,
            Price::from("150.0"),
            Currency::USD(),
            Currency::USD(),
            Currency::USD(),
            None,
            None,
            None,
            0.into(),
            0.into(),
            2,
            Price::from("0.01"),
            Quantity::from("0"), // zero multiplier
            Quantity::from(1),
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
    #[case(Price::from("0"))]
    #[case(Price::from("-1"))]
    fn test_new_checked_rejects_non_positive_strike_price(#[case] strike_price: Price) {
        let result = OptionContract::new_checked(
            InstrumentId::from("TEST.OPRA"),
            Symbol::from("TEST"),
            AssetClass::Equity,
            Some(Ustr::from("GMNI")),
            Ustr::from("AAPL"),
            None,
            OptionKind::Call,
            strike_price,
            Currency::USD(),
            Currency::USD(),
            Currency::USD(),
            None,
            None,
            None,
            0.into(),
            0.into(),
            2,
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
            0.into(),
            0.into(),
        );

        // Assert on the parameter name, not merely `is_err`: this constructor validates a
        // dozen other fields, and a bare error check would pass if an unrelated one fired.
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("'strike_price' not positive")
        );
    }

    #[rstest]
    fn test_serialization_roundtrip(option_contract_appl: OptionContract) {
        let json = serde_json::to_string(&option_contract_appl).unwrap();
        let deserialized: OptionContract = serde_json::from_str(&json).unwrap();
        assert_eq!(json, serde_json::to_string(&deserialized).unwrap());
    }

    #[rstest]
    fn test_builder_matches_new_checked() {
        let positional = OptionContract::new_checked(
            InstrumentId::from("AAPL211217C00150000.OPRA"),
            Symbol::from("AAPL211217C00150000"),
            AssetClass::Equity,
            Some(Ustr::from("GMNI")),
            Ustr::from("AAPL"),
            Some(InstrumentId::from("AAPL.NASDAQ")),
            OptionKind::Call,
            Price::from("149.0"),
            Currency::JPY(),
            Currency::USD(),
            Currency::EUR(),
            Some(OptionPremiumStyle::FuturesStyle),
            Some(ExerciseStyle::American),
            Some(SettlementType::Physical),
            1.into(),
            2.into(),
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
            None,
            None,
            3.into(),
            4.into(),
        )
        .unwrap();

        let built = OptionContract::builder()
            .instrument_id(InstrumentId::from("AAPL211217C00150000.OPRA"))
            .raw_symbol(Symbol::from("AAPL211217C00150000"))
            .asset_class(AssetClass::Equity)
            .exchange(Ustr::from("GMNI"))
            .underlying(Ustr::from("AAPL"))
            .underlying_instrument_id(InstrumentId::from("AAPL.NASDAQ"))
            .option_kind(OptionKind::Call)
            .strike_price(Price::from("149.0"))
            .strike_currency(Currency::JPY())
            .quote_currency(Currency::USD())
            .settlement_currency(Currency::EUR())
            .premium_style(OptionPremiumStyle::FuturesStyle)
            .exercise_style(ExerciseStyle::American)
            .settlement_type(SettlementType::Physical)
            .activation_ns(1.into())
            .expiration_ns(2.into())
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
            .ts_event(3.into())
            .ts_init(4.into())
            .build()
            .unwrap();

        assert_eq!(
            built.underlying_instrument_id,
            Some(InstrumentId::from("AAPL.NASDAQ"))
        );
        assert_eq!(built.premium_style, Some(OptionPremiumStyle::FuturesStyle));
        assert_eq!(built.exercise_style, Some(ExerciseStyle::American));
        assert_eq!(built.settlement_type, Some(SettlementType::Physical));
        assert_eq!(
            serde_json::to_value(&positional).unwrap(),
            serde_json::to_value(&built).unwrap(),
        );
    }

    #[rstest]
    #[case(OptionKind::Call, ExerciseStyle::American, SettlementType::Cash)]
    #[case(OptionKind::Put, ExerciseStyle::European, SettlementType::Physical)]
    #[case(OptionKind::Call, ExerciseStyle::European, SettlementType::Physical)]
    #[case(OptionKind::Put, ExerciseStyle::Bermudan, SettlementType::Cash)]
    fn test_explicit_option_terms_round_trip(
        #[from(option_contract_appl)] mut option: OptionContract,
        #[case] kind: OptionKind,
        #[case] style: ExerciseStyle,
        #[case] settlement: SettlementType,
    ) {
        let legacy = option.clone();
        let underlying_id = InstrumentId::from("AAPL.NASDAQ");
        option.option_kind = kind;
        option.underlying_instrument_id = Some(underlying_id);
        option.exercise_style = Some(style);
        option.settlement_type = Some(settlement);
        let mut encoded = serde_json::to_value(&option).unwrap();
        let restored: OptionContract = serde_json::from_value(encoded.clone()).unwrap();
        let mut old = encoded.clone();
        old.as_object_mut()
            .unwrap()
            .remove("underlying_instrument_id");
        old.as_object_mut().unwrap().remove("exercise_style");
        old.as_object_mut().unwrap().remove("settlement_type");
        let restored_legacy: OptionContract = serde_json::from_value(old).unwrap();

        assert_eq!(serde_json::to_value(&restored).unwrap(), encoded);
        encoded["underlying_instrument_id"] = serde_json::Value::Null;
        encoded["exercise_style"] = serde_json::Value::Null;
        encoded["settlement_type"] = serde_json::Value::Null;
        assert_eq!(serde_json::to_value(&restored_legacy).unwrap(), encoded);
        assert_eq!(option, legacy);
    }
}
