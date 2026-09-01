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

//! Pluggable margin calculation models for [`MarginAccount`](super::MarginAccount).

use std::{fmt::Debug, sync::Arc};

use rust_decimal::Decimal;

use crate::{
    instruments::Instrument,
    types::{Money, Price, Quantity},
};

/// Determines how margin requirements are calculated for leveraged positions.
pub trait MarginModel: Send + Sync {
    /// Returns the stable model name used in canonical backtest results.
    #[must_use]
    fn name(&self) -> &'static str;

    /// Calculates the initial (order) margin requirement.
    ///
    /// # Errors
    ///
    /// Returns an error if margin cannot be computed (e.g. invalid instrument).
    fn calculate_initial_margin(
        &self,
        instrument: &dyn Instrument,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money>;

    /// Calculates the maintenance (position) margin requirement.
    ///
    /// # Errors
    ///
    /// Returns an error if margin cannot be computed (e.g. invalid instrument).
    fn calculate_maintenance_margin(
        &self,
        instrument: &dyn Instrument,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money>;
}

/// Shared runtime handle for a margin model.
#[derive(Clone)]
pub struct MarginModelHandle(Arc<dyn MarginModel>);

impl MarginModelHandle {
    /// Creates a new [`MarginModelHandle`] from a margin model.
    #[must_use]
    pub fn new<T>(model: T) -> Self
    where
        T: MarginModel + 'static,
    {
        Self(Arc::new(model))
    }

    /// Creates a new [`MarginModelHandle`] from an existing atomically reference-counted model.
    #[must_use]
    pub fn from_arc(model: Arc<dyn MarginModel>) -> Self {
        Self(model)
    }
}

impl Debug for MarginModelHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple(stringify!(MarginModelHandle))
            .field(&"<dyn MarginModel>")
            .finish()
    }
}

impl MarginModel for MarginModelHandle {
    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn calculate_initial_margin(
        &self,
        instrument: &dyn Instrument,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        self.0.calculate_initial_margin(
            instrument,
            quantity,
            price,
            leverage,
            use_quote_for_inverse,
        )
    }

    fn calculate_maintenance_margin(
        &self,
        instrument: &dyn Instrument,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        self.0.calculate_maintenance_margin(
            instrument,
            quantity,
            price,
            leverage,
            use_quote_for_inverse,
        )
    }
}

/// Enum dispatch for [`MarginModel`] implementations.
#[derive(Debug, Clone)]
pub enum MarginModelAny {
    Standard(StandardMarginModel),
    Leveraged(LeveragedMarginModel),
}

impl MarginModel for MarginModelAny {
    fn name(&self) -> &'static str {
        match self {
            Self::Standard(model) => model.name(),
            Self::Leveraged(model) => model.name(),
        }
    }

    fn calculate_initial_margin(
        &self,
        instrument: &dyn Instrument,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        match self {
            Self::Standard(m) => m.calculate_initial_margin(
                instrument,
                quantity,
                price,
                leverage,
                use_quote_for_inverse,
            ),
            Self::Leveraged(m) => m.calculate_initial_margin(
                instrument,
                quantity,
                price,
                leverage,
                use_quote_for_inverse,
            ),
        }
    }

    fn calculate_maintenance_margin(
        &self,
        instrument: &dyn Instrument,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        match self {
            Self::Standard(m) => m.calculate_maintenance_margin(
                instrument,
                quantity,
                price,
                leverage,
                use_quote_for_inverse,
            ),
            Self::Leveraged(m) => m.calculate_maintenance_margin(
                instrument,
                quantity,
                price,
                leverage,
                use_quote_for_inverse,
            ),
        }
    }
}

impl Default for MarginModelAny {
    fn default() -> Self {
        Self::Leveraged(LeveragedMarginModel)
    }
}

impl Default for MarginModelHandle {
    fn default() -> Self {
        MarginModelAny::default().into()
    }
}

impl From<MarginModelAny> for MarginModelHandle {
    fn from(model: MarginModelAny) -> Self {
        Self::new(model)
    }
}

/// Resolves the margin currency based on instrument properties.
fn margin_currency(
    instrument: &dyn Instrument,
    use_quote_for_inverse: bool,
) -> anyhow::Result<crate::types::Currency> {
    if instrument.is_inverse() && !use_quote_for_inverse {
        instrument.base_currency().ok_or_else(|| {
            anyhow::anyhow!(
                "Inverse instrument {} has no base currency",
                instrument.id()
            )
        })
    } else {
        Ok(instrument.quote_currency())
    }
}

/// Uses fixed margin percentages without leverage division.
///
/// Margin is calculated as `notional_value * margin_rate`, ignoring the
/// account leverage. Appropriate for traditional brokers where margin
/// requirements are fixed percentages of notional value.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct StandardMarginModel;

impl MarginModel for StandardMarginModel {
    fn name(&self) -> &'static str {
        "standard"
    }

    fn calculate_initial_margin(
        &self,
        instrument: &dyn Instrument,
        quantity: Quantity,
        price: Price,
        _leverage: Decimal,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        let use_quote = use_quote_for_inverse.unwrap_or(false);
        let notional = instrument.try_calculate_notional_value(quantity, price, Some(use_quote))?;
        // Spreads and options may quote negative, which carries the sign into the notional.
        // A requirement is a reserve against exposure magnitude, so take it on `abs`.
        let margin = notional
            .as_decimal()
            .abs()
            .checked_mul(instrument.margin_init())
            .ok_or_else(|| anyhow::anyhow!("initial margin calculation overflow"))?;
        let currency = margin_currency(instrument, use_quote)?;
        Money::from_decimal(margin, currency).map_err(Into::into)
    }

    fn calculate_maintenance_margin(
        &self,
        instrument: &dyn Instrument,
        quantity: Quantity,
        price: Price,
        _leverage: Decimal,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        let use_quote = use_quote_for_inverse.unwrap_or(false);
        let notional = instrument.try_calculate_notional_value(quantity, price, Some(use_quote))?;
        let margin = notional
            .as_decimal()
            .abs()
            .checked_mul(instrument.margin_maint())
            .ok_or_else(|| anyhow::anyhow!("maintenance margin calculation overflow"))?;
        let currency = margin_currency(instrument, use_quote)?;
        Money::from_decimal(margin, currency).map_err(Into::into)
    }
}

/// Divides notional value by leverage before applying margin rates.
///
/// Margin is calculated as `(notional_value / leverage) * margin_rate`.
/// This is the default model, appropriate for crypto exchanges and venues
/// where leverage directly reduces margin requirements.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct LeveragedMarginModel;

impl MarginModel for LeveragedMarginModel {
    fn name(&self) -> &'static str {
        "leveraged"
    }

    fn calculate_initial_margin(
        &self,
        instrument: &dyn Instrument,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        if leverage <= Decimal::ZERO {
            anyhow::bail!("Invalid leverage {leverage} for {}", instrument.id());
        }
        let use_quote = use_quote_for_inverse.unwrap_or(false);
        let notional = instrument.try_calculate_notional_value(quantity, price, Some(use_quote))?;
        let margin = notional
            .as_decimal()
            .abs()
            .checked_div(leverage)
            .and_then(|adjusted| adjusted.checked_mul(instrument.margin_init()))
            .ok_or_else(|| anyhow::anyhow!("initial margin calculation overflow"))?;
        let currency = margin_currency(instrument, use_quote)?;
        Money::from_decimal(margin, currency).map_err(Into::into)
    }

    fn calculate_maintenance_margin(
        &self,
        instrument: &dyn Instrument,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        if leverage <= Decimal::ZERO {
            anyhow::bail!("Invalid leverage {leverage} for {}", instrument.id());
        }
        let use_quote = use_quote_for_inverse.unwrap_or(false);
        let notional = instrument.try_calculate_notional_value(quantity, price, Some(use_quote))?;
        let margin = notional
            .as_decimal()
            .abs()
            .checked_div(leverage)
            .and_then(|adjusted| adjusted.checked_mul(instrument.margin_maint()))
            .ok_or_else(|| anyhow::anyhow!("maintenance margin calculation overflow"))?;
        let currency = margin_currency(instrument, use_quote)?;
        Money::from_decimal(margin, currency).map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;
    use crate::{
        enums::AssetClass,
        identifiers::{InstrumentId, Symbol},
        instruments::{
            CryptoPerpetual, FuturesSpread, Instrument, stubs::crypto_perpetual_ethusdt,
        },
        types::{Currency, Price, Quantity},
    };

    struct FixedMarginModel {
        initial: Money,
        maintenance: Money,
    }

    impl MarginModel for FixedMarginModel {
        fn name(&self) -> &'static str {
            "fixed"
        }

        fn calculate_initial_margin(
            &self,
            _instrument: &dyn Instrument,
            _quantity: Quantity,
            _price: Price,
            _leverage: Decimal,
            _use_quote_for_inverse: Option<bool>,
        ) -> anyhow::Result<Money> {
            Ok(self.initial)
        }

        fn calculate_maintenance_margin(
            &self,
            _instrument: &dyn Instrument,
            _quantity: Quantity,
            _price: Price,
            _leverage: Decimal,
            _use_quote_for_inverse: Option<bool>,
        ) -> anyhow::Result<Money> {
            Ok(self.maintenance)
        }
    }

    fn ethusdt() -> CryptoPerpetual {
        crypto_perpetual_ethusdt()
    }

    #[rstest]
    fn test_leveraged_initial_margin() {
        let model = LeveragedMarginModel;
        let instrument = ethusdt();
        let quantity = Quantity::from("10.000");
        let price = Price::from("5000.00");
        let leverage = dec!(10);

        let margin = model
            .calculate_initial_margin(&instrument, quantity, price, leverage, None)
            .unwrap();

        // notional = 10 * 5000 = 50000, adjusted = 50000/10 = 5000
        // margin = 5000 * margin_init
        let expected = Decimal::from(50000) / leverage * instrument.margin_init();
        assert_eq!(margin.as_decimal(), expected);
        assert_eq!(margin.currency, Currency::USDT());
    }

    #[rstest]
    fn test_standard_ignores_leverage() {
        let model = StandardMarginModel;
        let instrument = ethusdt();
        let quantity = Quantity::from("10.000");
        let price = Price::from("5000.00");

        let margin_low = model
            .calculate_initial_margin(&instrument, quantity, price, dec!(2), None)
            .unwrap();
        let margin_high = model
            .calculate_initial_margin(&instrument, quantity, price, dec!(100), None)
            .unwrap();

        // StandardMarginModel ignores leverage so both should be equal
        assert_eq!(margin_low, margin_high);
    }

    /// A spread carrying non-zero margin rates, so the assertions below cannot pass on a
    /// zero requirement. `FuturesSpread` is one of the three classes permitting a negative
    /// price (see `InstrumentClass::allows_negative_price`).
    fn negative_price_spread() -> FuturesSpread {
        FuturesSpread::builder()
            .instrument_id(InstrumentId::from("ESM4-ESU4.GLBX"))
            .raw_symbol(Symbol::from("ESM4-ESU4"))
            .asset_class(AssetClass::Index)
            .underlying(Ustr::from("ES"))
            .strategy_type(Ustr::from("EQ"))
            .activation_ns(1_000.into())
            .expiration_ns(2_000.into())
            .currency(Currency::USD())
            .price_precision(2)
            .price_increment(Price::from("0.01"))
            .multiplier(Quantity::from(50))
            .lot_size(Quantity::from(1))
            .margin_init(dec!(0.01))
            .margin_maint(dec!(0.02))
            .ts_event(1.into())
            .ts_init(2.into())
            .build()
            .unwrap()
    }

    #[rstest]
    fn test_standard_margin_is_positive_for_a_negative_price() {
        let model = StandardMarginModel;
        let instrument = negative_price_spread();
        let quantity = Quantity::from(2);
        let positive = Price::from("2.00");
        let negative = Price::from("-2.00");

        let initial = model
            .calculate_initial_margin(&instrument, quantity, negative, dec!(1), None)
            .unwrap();
        let maintenance = model
            .calculate_maintenance_margin(&instrument, quantity, negative, dec!(1), None)
            .unwrap();

        // notional magnitude = 2 * 50 * 2.00 = 200
        assert_eq!(initial.as_decimal(), dec!(2));
        assert_eq!(maintenance.as_decimal(), dec!(4));
        // A negative quote reserves the same as the equivalent positive one.
        assert_eq!(
            initial,
            model
                .calculate_initial_margin(&instrument, quantity, positive, dec!(1), None)
                .unwrap()
        );
    }

    #[rstest]
    fn test_leveraged_margin_is_positive_for_a_negative_price() {
        let model = LeveragedMarginModel;
        let instrument = negative_price_spread();
        let quantity = Quantity::from(2);
        let negative = Price::from("-2.00");
        let leverage = dec!(10);

        let initial = model
            .calculate_initial_margin(&instrument, quantity, negative, leverage, None)
            .unwrap();
        let maintenance = model
            .calculate_maintenance_margin(&instrument, quantity, negative, leverage, None)
            .unwrap();

        // notional magnitude = 200, adjusted = 200 / 10 = 20
        assert_eq!(initial.as_decimal(), dec!(0.2));
        assert_eq!(maintenance.as_decimal(), dec!(0.4));
    }

    #[rstest]
    fn test_leveraged_zero_leverage_errors() {
        let model = LeveragedMarginModel;
        let instrument = ethusdt();

        let result = model.calculate_initial_margin(
            &instrument,
            Quantity::from("1.000"),
            Price::from("5000.00"),
            Decimal::ZERO,
            None,
        );

        assert!(result.is_err());
    }

    #[rstest]
    fn test_leveraged_margin_decimal_overflow_returns_error() {
        let model = LeveragedMarginModel;
        let instrument = ethusdt();

        let result = model.calculate_initial_margin(
            &instrument,
            Quantity::from("1.000"),
            Price::from("5000.00"),
            Decimal::new(1, 28),
            None,
        );

        assert_eq!(
            result.unwrap_err().to_string(),
            "initial margin calculation overflow"
        );
    }

    #[rstest]
    fn test_margin_model_any_default_is_leveraged() {
        let model = MarginModelAny::default();
        assert!(matches!(model, MarginModelAny::Leveraged(_)));
        assert_eq!(model.name(), "leveraged");
    }

    #[rstest]
    fn test_margin_model_handle_calls_custom_model() {
        let initial = Money::from("12.34 USDT");
        let maintenance = Money::from("5.67 USDT");
        let model: Arc<dyn MarginModel> = Arc::new(FixedMarginModel {
            initial,
            maintenance,
        });
        let handle = MarginModelHandle::from_arc(model);
        let cloned_handle = handle.clone();
        drop(handle);
        let instrument = ethusdt();

        let initial_result = cloned_handle
            .calculate_initial_margin(
                &instrument,
                Quantity::from("1.000"),
                Price::from("5000.00"),
                dec!(10),
                None,
            )
            .unwrap();
        let maintenance_result = cloned_handle
            .calculate_maintenance_margin(
                &instrument,
                Quantity::from("1.000"),
                Price::from("5000.00"),
                dec!(10),
                None,
            )
            .unwrap();

        assert_eq!(cloned_handle.name(), "fixed");
        assert_eq!(initial_result, initial);
        assert_eq!(maintenance_result, maintenance);
    }

    #[rstest]
    fn test_maintenance_margin() {
        let model = LeveragedMarginModel;
        let instrument = ethusdt();
        let quantity = Quantity::from("10.000");
        let price = Price::from("5000.00");
        let leverage = dec!(10);

        let margin = model
            .calculate_maintenance_margin(&instrument, quantity, price, leverage, None)
            .unwrap();

        let expected = Decimal::from(50000) / leverage * instrument.margin_maint();
        assert_eq!(margin.as_decimal(), expected);
    }
}
