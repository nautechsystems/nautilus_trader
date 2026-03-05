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

use rust_decimal::Decimal;

use crate::{
    instruments::Instrument,
    types::{Money, Price, Quantity},
};

/// Determines how margin requirements are calculated for leveraged positions.
pub trait MarginModel {
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

/// Enum dispatch for [`MarginModel`] implementations.
#[derive(Debug, Clone)]
pub enum MarginModelAny {
    Standard(StandardMarginModel),
    Leveraged(LeveragedMarginModel),
}

impl MarginModel for MarginModelAny {
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
pub struct StandardMarginModel;

impl MarginModel for StandardMarginModel {
    fn calculate_initial_margin(
        &self,
        instrument: &dyn Instrument,
        quantity: Quantity,
        price: Price,
        _leverage: Decimal,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        let use_quote = use_quote_for_inverse.unwrap_or(false);
        let notional = instrument.calculate_notional_value(quantity, price, Some(use_quote));
        let margin = notional.as_decimal() * instrument.margin_init();
        let currency = margin_currency(instrument, use_quote)?;
        Money::from_decimal(margin, currency)
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
        let notional = instrument.calculate_notional_value(quantity, price, Some(use_quote));
        let margin = notional.as_decimal() * instrument.margin_maint();
        let currency = margin_currency(instrument, use_quote)?;
        Money::from_decimal(margin, currency)
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
pub struct LeveragedMarginModel;

impl MarginModel for LeveragedMarginModel {
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
        let notional = instrument.calculate_notional_value(quantity, price, Some(use_quote));
        let adjusted = notional.as_decimal() / leverage;
        let margin = adjusted * instrument.margin_init();
        let currency = margin_currency(instrument, use_quote)?;
        Money::from_decimal(margin, currency)
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
        let notional = instrument.calculate_notional_value(quantity, price, Some(use_quote));
        let adjusted = notional.as_decimal() / leverage;
        let margin = adjusted * instrument.margin_maint();
        let currency = margin_currency(instrument, use_quote)?;
        Money::from_decimal(margin, currency)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        instruments::{CryptoPerpetual, Instrument, stubs::crypto_perpetual_ethusdt},
        types::{Currency, Price, Quantity},
    };

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
    fn test_margin_model_any_default_is_leveraged() {
        let model = MarginModelAny::default();
        assert!(matches!(model, MarginModelAny::Leveraged(_)));
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
