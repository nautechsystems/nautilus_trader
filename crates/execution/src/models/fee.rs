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

use std::{fmt::Debug, rc::Rc};

use nautilus_model::{
    enums::LiquiditySide,
    identifiers::GENERIC_SPREAD_ID_SEPARATOR,
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

pub trait FeeModel {
    /// Calculates commission for a fill.
    ///
    /// # Errors
    ///
    /// Returns an error if commission calculation fails.
    fn get_commission(
        &self,
        order: &OrderAny,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<Money>;

    /// Calculates commission for a fill with additional pricing context.
    ///
    /// # Errors
    ///
    /// Returns an error if commission calculation fails.
    fn get_commission_with_context(
        &self,
        order: &OrderAny,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &InstrumentAny,
        _underlying_px: Option<Price>,
    ) -> anyhow::Result<Money> {
        self.get_commission(order, fill_quantity, fill_px, instrument)
    }
}

/// Shared runtime handle for a fee model.
#[derive(Clone)]
pub struct FeeModelHandle(Rc<dyn FeeModel>);

impl FeeModelHandle {
    /// Creates a new [`FeeModelHandle`] from a fee model.
    #[must_use]
    pub fn new<T>(model: T) -> Self
    where
        T: FeeModel + 'static,
    {
        Self(Rc::new(model))
    }

    /// Creates a new [`FeeModelHandle`] from an existing reference-counted model.
    #[must_use]
    pub fn from_rc(model: Rc<dyn FeeModel>) -> Self {
        Self(model)
    }
}

impl Debug for FeeModelHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple(stringify!(FeeModelHandle))
            .field(&"<dyn FeeModel>")
            .finish()
    }
}

impl FeeModel for FeeModelHandle {
    fn get_commission(
        &self,
        order: &OrderAny,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<Money> {
        self.0
            .get_commission(order, fill_quantity, fill_px, instrument)
    }

    fn get_commission_with_context(
        &self,
        order: &OrderAny,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &InstrumentAny,
        underlying_px: Option<Price>,
    ) -> anyhow::Result<Money> {
        self.0
            .get_commission_with_context(order, fill_quantity, fill_px, instrument, underlying_px)
    }
}

impl Default for FeeModelHandle {
    fn default() -> Self {
        FeeModelAny::default().into()
    }
}

impl From<FeeModelAny> for FeeModelHandle {
    fn from(model: FeeModelAny) -> Self {
        Self::new(model)
    }
}

#[derive(Clone, Debug)]
pub enum FeeModelAny {
    Fixed(FixedFeeModel),
    MakerTaker(MakerTakerFeeModel),
    PerContract(PerContractFeeModel),
    ProbabilityPrice(ProbabilityPriceFeeModel),
    CappedOption(CappedOptionFeeModel),
    TieredNotionalOption(TieredNotionalOptionFeeModel),
}

impl FeeModel for FeeModelAny {
    fn get_commission(
        &self,
        order: &OrderAny,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<Money> {
        match self {
            Self::Fixed(model) => model.get_commission(order, fill_quantity, fill_px, instrument),
            Self::MakerTaker(model) => {
                model.get_commission(order, fill_quantity, fill_px, instrument)
            }
            Self::PerContract(model) => {
                model.get_commission(order, fill_quantity, fill_px, instrument)
            }
            Self::ProbabilityPrice(model) => {
                model.get_commission(order, fill_quantity, fill_px, instrument)
            }
            Self::CappedOption(model) => {
                model.get_commission(order, fill_quantity, fill_px, instrument)
            }
            Self::TieredNotionalOption(model) => {
                model.get_commission(order, fill_quantity, fill_px, instrument)
            }
        }
    }

    fn get_commission_with_context(
        &self,
        order: &OrderAny,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &InstrumentAny,
        underlying_px: Option<Price>,
    ) -> anyhow::Result<Money> {
        match self {
            Self::Fixed(model) => model.get_commission_with_context(
                order,
                fill_quantity,
                fill_px,
                instrument,
                underlying_px,
            ),
            Self::MakerTaker(model) => model.get_commission_with_context(
                order,
                fill_quantity,
                fill_px,
                instrument,
                underlying_px,
            ),
            Self::PerContract(model) => model.get_commission_with_context(
                order,
                fill_quantity,
                fill_px,
                instrument,
                underlying_px,
            ),
            Self::ProbabilityPrice(model) => model.get_commission_with_context(
                order,
                fill_quantity,
                fill_px,
                instrument,
                underlying_px,
            ),
            Self::CappedOption(model) => model.get_commission_with_context(
                order,
                fill_quantity,
                fill_px,
                instrument,
                underlying_px,
            ),
            Self::TieredNotionalOption(model) => model.get_commission_with_context(
                order,
                fill_quantity,
                fill_px,
                instrument,
                underlying_px,
            ),
        }
    }
}

impl Default for FeeModelAny {
    fn default() -> Self {
        Self::MakerTaker(MakerTakerFeeModel)
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.execution",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.execution")
)]
pub struct FixedFeeModel {
    commission: Money,
    zero_commission: Money,
    charge_commission_once: bool,
}

impl FixedFeeModel {
    /// Creates a new [`FixedFeeModel`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if `commission` is negative.
    pub fn new(commission: Money, charge_commission_once: Option<bool>) -> anyhow::Result<Self> {
        if commission.raw < 0 {
            anyhow::bail!("Commission must be greater than or equal to zero")
        }
        let zero_commission = Money::zero(commission.currency);
        Ok(Self {
            commission,
            zero_commission,
            charge_commission_once: charge_commission_once.unwrap_or(true),
        })
    }
}

impl FeeModel for FixedFeeModel {
    fn get_commission(
        &self,
        order: &OrderAny,
        _fill_quantity: Quantity,
        _fill_px: Price,
        _instrument: &InstrumentAny,
    ) -> anyhow::Result<Money> {
        if !self.charge_commission_once || order.filled_qty().is_zero() {
            Ok(self.commission)
        } else {
            Ok(self.zero_commission)
        }
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.execution",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.execution")
)]
pub struct PerContractFeeModel {
    commission: Money,
}

impl PerContractFeeModel {
    /// Creates a new [`PerContractFeeModel`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if `commission` is negative.
    pub fn new(commission: Money) -> anyhow::Result<Self> {
        if commission.raw < 0 {
            anyhow::bail!("Commission must be greater than or equal to zero")
        }
        Ok(Self { commission })
    }
}

impl FeeModel for PerContractFeeModel {
    fn get_commission(
        &self,
        _order: &OrderAny,
        fill_quantity: Quantity,
        _fill_px: Price,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<Money> {
        let total = self.commission.as_decimal()
            * fill_quantity.as_decimal()
            * spread_contract_count(instrument)?;
        Money::from_decimal(total, self.commission.currency).map_err(Into::into)
    }
}

fn spread_contract_count(instrument: &InstrumentAny) -> anyhow::Result<Decimal> {
    let instrument_id = instrument.id();
    let symbol = instrument_id.symbol.as_str();
    if !instrument.is_spread() || !symbol.contains(GENERIC_SPREAD_ID_SEPARATOR) {
        return Ok(Decimal::ONE);
    }

    let mut total = 0_i64;

    for component in symbol.split(GENERIC_SPREAD_ID_SEPARATOR) {
        let ratio = spread_leg_ratio(component)
            .ok_or_else(|| anyhow::anyhow!("Invalid generic spread leg component: {component}"))?;
        total = total.checked_add(ratio).ok_or_else(|| {
            anyhow::anyhow!("Generic spread contract count overflowed for {symbol}")
        })?;
    }

    Ok(total.into())
}

fn spread_leg_ratio(component: &str) -> Option<i64> {
    if let Some(rest) = component.strip_prefix("((") {
        let (ratio, symbol) = rest.split_once("))")?;
        return spread_leg_ratio_parts(ratio, symbol);
    }

    let rest = component.strip_prefix('(')?;
    let (ratio, symbol) = rest.split_once(')')?;
    spread_leg_ratio_parts(ratio, symbol)
}

fn spread_leg_ratio_parts(ratio: &str, symbol: &str) -> Option<i64> {
    if symbol.is_empty() {
        return None;
    }

    ratio.parse::<i64>().ok().filter(|ratio| *ratio > 0)
}

#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.execution",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.execution")
)]
pub struct MakerTakerFeeModel;

impl FeeModel for MakerTakerFeeModel {
    fn get_commission(
        &self,
        order: &OrderAny,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<Money> {
        let notional = instrument.calculate_notional_value(fill_quantity, fill_px, Some(false));
        let commission = match order.liquidity_side() {
            Some(LiquiditySide::Maker) => notional * instrument.maker_fee(),
            Some(LiquiditySide::Taker) => notional * instrument.taker_fee(),
            Some(LiquiditySide::NoLiquiditySide) | None => anyhow::bail!("Liquidity side not set"),
        };

        if instrument.is_inverse() {
            Money::from_decimal(commission, instrument.base_currency().unwrap()).map_err(Into::into)
        } else {
            Money::from_decimal(commission, instrument.quote_currency()).map_err(Into::into)
        }
    }
}

/// Fee model for probability-priced outcome shares.
///
/// Applies `qty * fee_rate * p * (1 - p)` using the instrument's maker or
/// taker fee rate. This matches venues that represent outcome shares as
/// [`InstrumentAny::BinaryOption`] instruments quoted on a `[0, 1]`
/// probability scale.
///
/// This model covers quote-currency match-time exchange fees only.
/// Venue-specific rebate programs or non-quote fee assets remain outside the
/// core execution layer.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.execution",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.execution")
)]
pub struct ProbabilityPriceFeeModel;

impl FeeModel for ProbabilityPriceFeeModel {
    fn get_commission(
        &self,
        order: &OrderAny,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<Money> {
        if !matches!(instrument, InstrumentAny::BinaryOption(_)) {
            anyhow::bail!("ProbabilityPriceFeeModel requires a binary option instrument");
        }

        let fill_price = fill_px.as_decimal();
        if !(Decimal::ZERO..=Decimal::ONE).contains(&fill_price) {
            anyhow::bail!("ProbabilityPriceFeeModel requires a fill price in [0, 1]");
        }

        let fee_rate = match order.liquidity_side() {
            Some(LiquiditySide::Maker) => instrument.maker_fee(),
            Some(LiquiditySide::Taker) => instrument.taker_fee(),
            Some(LiquiditySide::NoLiquiditySide) | None => anyhow::bail!("Liquidity side not set"),
        };

        let commission =
            (fill_quantity.as_decimal() * fee_rate * fill_price * (Decimal::ONE - fill_price))
                .round_dp(5);

        Money::from_decimal(commission, instrument.quote_currency()).map_err(Into::into)
    }
}

#[derive(Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.execution",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.execution")
)]
pub struct CappedOptionFeeModel {
    maker_rate: Option<Decimal>,
    taker_rate: Option<Decimal>,
    cap: Decimal,
}

impl Debug for CappedOptionFeeModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(CappedOptionFeeModel))
            .field("maker_rate", &self.maker_rate)
            .field("taker_rate", &self.taker_rate)
            .field("cap_rate", &self.cap)
            .finish()
    }
}

impl CappedOptionFeeModel {
    /// Creates a new [`CappedOptionFeeModel`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if any supplied rate is negative.
    pub fn new(
        maker_rate: Option<Decimal>,
        taker_rate: Option<Decimal>,
        cap_rate: Option<Decimal>,
    ) -> anyhow::Result<Self> {
        check_fee_rate(maker_rate, "maker_rate")?;
        check_fee_rate(taker_rate, "taker_rate")?;

        let cap_rate = cap_rate.unwrap_or(dec!(0.125));
        check_fee_rate(Some(cap_rate), "cap_rate")?;

        Ok(Self {
            maker_rate,
            taker_rate,
            cap: cap_rate,
        })
    }
}

impl Default for CappedOptionFeeModel {
    fn default() -> Self {
        Self::new(None, None, None).unwrap()
    }
}

impl FeeModel for CappedOptionFeeModel {
    fn get_commission(
        &self,
        order: &OrderAny,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<Money> {
        self.get_commission_with_context(order, fill_quantity, fill_px, instrument, None)
    }

    fn get_commission_with_context(
        &self,
        order: &OrderAny,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &InstrumentAny,
        underlying_px: Option<Price>,
    ) -> anyhow::Result<Money> {
        check_option_instrument(instrument, "CappedOptionFeeModel")?;
        let rate = option_fee_rate(order, instrument, self.maker_rate, self.taker_rate)?;
        let multiplier = instrument.multiplier().as_decimal();
        let rate_fee = if instrument.is_inverse() {
            rate
        } else {
            let underlying_px =
                underlying_px.ok_or_else(|| anyhow::anyhow!("Underlying price is required"))?;
            rate * underlying_px.as_decimal()
        };
        let cap_fee = self.cap * fill_px.as_decimal();
        let fee_per_contract = rate_fee.min(cap_fee) * multiplier;
        let total = fee_per_contract * fill_quantity.as_decimal();
        Money::from_decimal(total, commission_currency(instrument)).map_err(Into::into)
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.execution",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.execution")
)]
pub struct TieredNotionalOptionFeeModel {
    maker_rate: Option<Decimal>,
    taker_rate: Option<Decimal>,
}

impl TieredNotionalOptionFeeModel {
    /// Creates a new [`TieredNotionalOptionFeeModel`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if any supplied rate is negative.
    pub fn new(maker_rate: Option<Decimal>, taker_rate: Option<Decimal>) -> anyhow::Result<Self> {
        check_fee_rate(maker_rate, "maker_rate")?;
        check_fee_rate(taker_rate, "taker_rate")?;

        Ok(Self {
            maker_rate,
            taker_rate,
        })
    }
}

impl Default for TieredNotionalOptionFeeModel {
    fn default() -> Self {
        Self::new(None, None).unwrap()
    }
}

impl FeeModel for TieredNotionalOptionFeeModel {
    fn get_commission(
        &self,
        order: &OrderAny,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<Money> {
        check_option_instrument(instrument, "TieredNotionalOptionFeeModel")?;
        let rate = option_fee_rate(order, instrument, self.maker_rate, self.taker_rate)?;
        let notional = instrument.calculate_notional_value(fill_quantity, fill_px, Some(false));
        let total = notional.as_decimal() * rate;
        Money::from_decimal(total, notional.currency).map_err(Into::into)
    }
}

fn option_fee_rate(
    order: &OrderAny,
    instrument: &InstrumentAny,
    maker_rate: Option<Decimal>,
    taker_rate: Option<Decimal>,
) -> anyhow::Result<Decimal> {
    let rate = match order.liquidity_side() {
        Some(LiquiditySide::Maker) => maker_rate.unwrap_or_else(|| instrument.maker_fee()),
        Some(LiquiditySide::Taker) => taker_rate.unwrap_or_else(|| instrument.taker_fee()),
        Some(LiquiditySide::NoLiquiditySide) | None => anyhow::bail!("Liquidity side not set"),
    };
    check_fee_rate(Some(rate), "fee_rate")?;
    Ok(rate)
}

fn check_fee_rate(rate: Option<Decimal>, name: &str) -> anyhow::Result<()> {
    if rate.is_some_and(|rate| rate < Decimal::ZERO) {
        anyhow::bail!("`{name}` must be greater than or equal to zero");
    }
    Ok(())
}

fn check_option_instrument(instrument: &InstrumentAny, model_name: &str) -> anyhow::Result<()> {
    if !matches!(
        instrument,
        InstrumentAny::CryptoOption(_) | InstrumentAny::OptionContract(_)
    ) {
        anyhow::bail!("{model_name} requires an option instrument");
    }
    Ok(())
}

fn commission_currency(instrument: &InstrumentAny) -> Currency {
    if instrument.is_inverse() {
        instrument.settlement_currency()
    } else {
        instrument.quote_currency()
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderType},
        identifiers::InstrumentId,
        instruments::{
            BinaryOption, CryptoOption, Instrument, InstrumentAny, OptionContract,
            stubs::{
                audusd_sim, binary_option, crypto_option_btc_deribit, option_contract_appl,
                option_spread,
            },
        },
        orders::{
            Order, OrderAny,
            builder::OrderTestBuilder,
            stubs::{TestOrderEventStubs, TestOrderStubs},
        },
        types::{Currency, Money, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    use super::{
        CappedOptionFeeModel, FeeModel, FeeModelAny, FeeModelHandle, FixedFeeModel,
        MakerTakerFeeModel, PerContractFeeModel, ProbabilityPriceFeeModel,
        TieredNotionalOptionFeeModel,
    };

    #[rstest]
    fn test_fixed_model_single_fill() {
        let expected_commission = Money::new(1.0, Currency::USD());
        let aud_usd = InstrumentAny::CurrencyPair(audusd_sim());
        let fee_model = FixedFeeModel::new(expected_commission, None).unwrap();
        let market_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(aud_usd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();
        let accepted_order = TestOrderStubs::make_accepted_order(&market_order);
        let commission = fee_model
            .get_commission(
                &accepted_order,
                Quantity::from(100_000),
                Price::from("1.0"),
                &aud_usd,
            )
            .unwrap();
        assert_eq!(commission, expected_commission);
    }

    #[rstest]
    #[case(OrderSide::Buy, true, Money::from("1 USD"), Money::from("0 USD"))]
    #[case(OrderSide::Sell, true, Money::from("1 USD"), Money::from("0 USD"))]
    #[case(OrderSide::Buy, false, Money::from("1 USD"), Money::from("1 USD"))]
    #[case(OrderSide::Sell, false, Money::from("1 USD"), Money::from("1 USD"))]
    fn test_fixed_model_multiple_fills(
        #[case] order_side: OrderSide,
        #[case] charge_commission_once: bool,
        #[case] expected_first_fill: Money,
        #[case] expected_next_fill: Money,
    ) {
        let aud_usd = InstrumentAny::CurrencyPair(audusd_sim());
        let fee_model =
            FixedFeeModel::new(expected_first_fill, Some(charge_commission_once)).unwrap();
        let market_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(aud_usd.id())
            .side(order_side)
            .quantity(Quantity::from(100_000))
            .build();
        let mut accepted_order = TestOrderStubs::make_accepted_order(&market_order);
        let commission_first_fill = fee_model
            .get_commission(
                &accepted_order,
                Quantity::from(50_000),
                Price::from("1.0"),
                &aud_usd,
            )
            .unwrap();
        let fill = TestOrderEventStubs::filled(
            &accepted_order,
            &aud_usd,
            None,
            None,
            None,
            Some(Quantity::from(50_000)),
            None,
            None,
            None,
            None,
        );
        accepted_order.apply(fill).unwrap();
        let commission_next_fill = fee_model
            .get_commission(
                &accepted_order,
                Quantity::from(50_000),
                Price::from("1.0"),
                &aud_usd,
            )
            .unwrap();
        assert_eq!(commission_first_fill, expected_first_fill);
        assert_eq!(commission_next_fill, expected_next_fill);
    }

    #[rstest]
    fn test_maker_taker_fee_model_maker_commission() {
        let fee_model = MakerTakerFeeModel;
        let aud_usd = InstrumentAny::CurrencyPair(audusd_sim());
        let maker_fee = aud_usd.maker_fee();
        let price = Price::from("1.0");
        let limit_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(aud_usd.id())
            .side(OrderSide::Sell)
            .price(price)
            .quantity(Quantity::from(100_000))
            .build();
        let fill = TestOrderStubs::make_filled_order(&limit_order, &aud_usd, LiquiditySide::Maker);
        let expected_commission = fill.quantity().as_decimal() * price.as_decimal() * maker_fee;
        let commission = fee_model
            .get_commission(&fill, Quantity::from(100_000), Price::from("1.0"), &aud_usd)
            .unwrap();
        assert_eq!(commission.as_decimal(), expected_commission);
    }

    #[rstest]
    fn test_maker_taker_fee_model_uses_decimal_rounding() {
        let fee_model = MakerTakerFeeModel;
        let aud_usd = InstrumentAny::CurrencyPair(audusd_sim());
        let price = Price::from("1.0");
        let quantity = Quantity::from("117250");
        let limit_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(aud_usd.id())
            .side(OrderSide::Sell)
            .price(price)
            .quantity(quantity)
            .build();
        let fill = TestOrderStubs::make_filled_order(&limit_order, &aud_usd, LiquiditySide::Maker);

        let commission = fee_model
            .get_commission(&fill, quantity, price, &aud_usd)
            .unwrap();

        assert_eq!(commission, Money::from("2.34 USD"));
    }

    #[rstest]
    fn test_maker_taker_fee_model_taker_commission() {
        let fee_model = MakerTakerFeeModel;
        let aud_usd = InstrumentAny::CurrencyPair(audusd_sim());
        let taker_fee = aud_usd.taker_fee();
        let price = Price::from("1.0");
        let limit_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(aud_usd.id())
            .side(OrderSide::Sell)
            .price(price)
            .quantity(Quantity::from(100_000))
            .build();

        let fill = TestOrderStubs::make_filled_order(&limit_order, &aud_usd, LiquiditySide::Taker);
        let expected_commission = fill.quantity().as_decimal() * price.as_decimal() * taker_fee;
        let commission = fee_model
            .get_commission(&fill, Quantity::from(100_000), Price::from("1.0"), &aud_usd)
            .unwrap();
        assert_eq!(commission.as_decimal(), expected_commission);
    }

    #[rstest]
    fn test_per_contract_fee_model() {
        let commission_per_contract = Money::new(0.50, Currency::USD());
        let aud_usd = InstrumentAny::CurrencyPair(audusd_sim());
        let fee_model = PerContractFeeModel::new(commission_per_contract).unwrap();
        let market_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(aud_usd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100))
            .build();
        let accepted_order = TestOrderStubs::make_accepted_order(&market_order);
        let commission = fee_model
            .get_commission(
                &accepted_order,
                Quantity::from(100),
                Price::from("1.0"),
                &aud_usd,
            )
            .unwrap();
        assert_eq!(commission, Money::new(50.0, Currency::USD()));
    }

    #[rstest]
    fn test_per_contract_fee_model_non_spread_symbol_with_separator_charges_one_contract() {
        let commission_per_contract = Money::from("1.25 USD");
        let fee_model = PerContractFeeModel::new(commission_per_contract).unwrap();
        let mut aud_usd = audusd_sim();
        aud_usd.id = InstrumentId::from("AUD___USD.SIM");
        let instrument = InstrumentAny::CurrencyPair(aud_usd);
        let market_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(2))
            .build();
        let accepted_order = TestOrderStubs::make_accepted_order(&market_order);

        let commission = fee_model
            .get_commission(
                &accepted_order,
                Quantity::from(2),
                Price::from("1.0"),
                &instrument,
            )
            .unwrap();

        assert_eq!(commission, Money::from("2.50 USD"));
    }

    #[rstest]
    fn test_per_contract_fee_model_option_spread_charges_each_contract() {
        let commission_per_contract = Money::from("1.25 USD");
        let fee_model = PerContractFeeModel::new(commission_per_contract).unwrap();
        let spread_id = InstrumentId::from("((2))SPY C410___(1)SPY C400.SMART");
        let mut option_spread = option_spread();
        option_spread.id = spread_id;
        let instrument = InstrumentAny::OptionSpread(option_spread);
        let market_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(2))
            .build();
        let accepted_order = TestOrderStubs::make_accepted_order(&market_order);

        let commission = fee_model
            .get_commission(
                &accepted_order,
                Quantity::from(2),
                Price::from("1.0"),
                &instrument,
            )
            .unwrap();

        assert_eq!(commission, Money::from("7.50 USD"));
    }

    #[rstest]
    fn test_per_contract_fee_model_non_generic_option_spread_charges_one_contract() {
        let commission_per_contract = Money::from("1.25 USD");
        let fee_model = PerContractFeeModel::new(commission_per_contract).unwrap();
        let instrument = InstrumentAny::OptionSpread(option_spread());
        let market_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(2))
            .build();
        let accepted_order = TestOrderStubs::make_accepted_order(&market_order);

        let commission = fee_model
            .get_commission(
                &accepted_order,
                Quantity::from(2),
                Price::from("1.0"),
                &instrument,
            )
            .unwrap();

        assert_eq!(commission, Money::from("2.50 USD"));
    }

    #[rstest]
    fn test_per_contract_fee_model_malformed_generic_spread_fails() {
        let commission_per_contract = Money::from("1.25 USD");
        let fee_model = PerContractFeeModel::new(commission_per_contract).unwrap();
        let spread_id = InstrumentId::from("(1)SPY C400___SPY C410.SMART");
        let mut option_spread = option_spread();
        option_spread.id = spread_id;
        let instrument = InstrumentAny::OptionSpread(option_spread);
        let market_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(2))
            .build();
        let accepted_order = TestOrderStubs::make_accepted_order(&market_order);

        let result = fee_model.get_commission(
            &accepted_order,
            Quantity::from(2),
            Price::from("1.0"),
            &instrument,
        );

        assert_eq!(
            result.unwrap_err().to_string(),
            "Invalid generic spread leg component: SPY C410"
        );
    }

    #[rstest]
    fn test_per_contract_fee_model_generic_spread_contract_count_overflow_fails() {
        let commission_per_contract = Money::from("1.25 USD");
        let fee_model = PerContractFeeModel::new(commission_per_contract).unwrap();
        let max_ratio = i64::MAX;
        let spread_symbol = format!("({max_ratio})SPY C400___({max_ratio})SPY C410");
        let spread_id = InstrumentId::from(format!("{spread_symbol}.SMART"));
        let mut option_spread = option_spread();
        option_spread.id = spread_id;
        let instrument = InstrumentAny::OptionSpread(option_spread);
        let market_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(2))
            .build();
        let accepted_order = TestOrderStubs::make_accepted_order(&market_order);

        let result = fee_model.get_commission(
            &accepted_order,
            Quantity::from(2),
            Price::from("1.0"),
            &instrument,
        );

        assert_eq!(
            result.unwrap_err().to_string(),
            format!("Generic spread contract count overflowed for {spread_symbol}")
        );
    }

    #[rstest]
    fn test_per_contract_fee_model_partial_fill() {
        let commission_per_contract = Money::new(1.25, Currency::USD());
        let aud_usd = InstrumentAny::CurrencyPair(audusd_sim());
        let fee_model = PerContractFeeModel::new(commission_per_contract).unwrap();
        let market_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(aud_usd.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from(1000))
            .build();
        let accepted_order = TestOrderStubs::make_accepted_order(&market_order);
        let commission = fee_model
            .get_commission(
                &accepted_order,
                Quantity::from(400),
                Price::from("1.0"),
                &aud_usd,
            )
            .unwrap();
        assert_eq!(commission, Money::new(500.0, Currency::USD()));
    }

    #[rstest]
    fn test_per_contract_fee_model_uses_decimal_rounding() {
        let commission_per_contract = Money::from("0.50 USD");
        let aud_usd = InstrumentAny::CurrencyPair(audusd_sim());
        let fee_model = PerContractFeeModel::new(commission_per_contract).unwrap();
        let market_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(aud_usd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("5"))
            .build();
        let accepted_order = TestOrderStubs::make_accepted_order(&market_order);

        let commission = fee_model
            .get_commission(
                &accepted_order,
                Quantity::from("4.69"),
                Price::from("1.0"),
                &aud_usd,
            )
            .unwrap();

        assert_eq!(commission, Money::from("2.34 USD"));
    }

    #[rstest]
    fn test_per_contract_fee_model_negative_commission_fails() {
        let result = PerContractFeeModel::new(Money::new(-1.0, Currency::USD()));
        assert!(result.is_err());
    }

    #[rstest]
    #[case::crypto_p97("0.072", "0.970", "0.00210")]
    #[case::sports_p50("0.03", "0.500", "0.00750")]
    #[case::sports_p30("0.03", "0.300", "0.00630")]
    fn test_probability_price_fee_model_taker_commission(
        mut binary_option: BinaryOption,
        #[case] taker_fee: &str,
        #[case] price: &str,
        #[case] expected: &str,
    ) {
        binary_option.taker_fee = Decimal::from_str_exact(taker_fee).unwrap();
        let instrument = InstrumentAny::BinaryOption(binary_option);
        let fill = binary_option_fill_order(&instrument, LiquiditySide::Taker, price);
        let fee_model = ProbabilityPriceFeeModel;

        let commission = fee_model
            .get_commission(
                &fill,
                Quantity::from("1.00"),
                Price::from(price),
                &instrument,
            )
            .unwrap();

        assert_eq!(commission.currency, Currency::USDC());
        assert_eq!(
            commission.as_decimal(),
            Decimal::from_str_exact(expected).unwrap()
        );
    }

    #[rstest]
    fn test_probability_price_fee_model_maker_commission_uses_instrument_rate(
        mut binary_option: BinaryOption,
    ) {
        binary_option.maker_fee = dec!(0.01);
        let instrument = InstrumentAny::BinaryOption(binary_option);
        let fill = binary_option_fill_order(&instrument, LiquiditySide::Maker, "0.500");
        let fee_model = FeeModelAny::ProbabilityPrice(ProbabilityPriceFeeModel);

        let commission = fee_model
            .get_commission(
                &fill,
                Quantity::from("1.00"),
                Price::from("0.500"),
                &instrument,
            )
            .unwrap();

        assert_eq!(commission, Money::from("0.00250 USDC"));
    }

    #[rstest]
    fn test_fee_model_handle_calls_custom_model_without_model_clone() {
        let calls = Rc::new(Cell::new(0));
        let expected_commission = Money::from("1.23 USD");
        let aud_usd = InstrumentAny::CurrencyPair(audusd_sim());
        let market_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(aud_usd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();
        let accepted_order = TestOrderStubs::make_accepted_order(&market_order);
        let fee_model = FeeModelHandle::new(CountingFeeModel {
            calls: Rc::clone(&calls),
            commission: expected_commission,
        });
        let cloned_fee_model = fee_model.clone();
        drop(fee_model);

        let commission = cloned_fee_model
            .get_commission(
                &accepted_order,
                Quantity::from(100_000),
                Price::from("1.0"),
                &aud_usd,
            )
            .unwrap();

        assert_eq!(calls.get(), 1);
        assert_eq!(commission, expected_commission);
    }

    #[rstest]
    fn test_fee_model_handle_from_rc_calls_custom_model() {
        let calls = Rc::new(Cell::new(0));
        let expected_commission = Money::from("1.23 USD");
        let aud_usd = InstrumentAny::CurrencyPair(audusd_sim());
        let market_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(aud_usd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from(100_000))
            .build();
        let accepted_order = TestOrderStubs::make_accepted_order(&market_order);
        let model = Rc::new(CountingFeeModel {
            calls: Rc::clone(&calls),
            commission: expected_commission,
        });
        let fee_model = FeeModelHandle::from_rc(model);

        let commission = fee_model
            .get_commission(
                &accepted_order,
                Quantity::from(100_000),
                Price::from("1.0"),
                &aud_usd,
            )
            .unwrap();

        assert_eq!(calls.get(), 1);
        assert_eq!(commission, expected_commission);
    }

    struct CountingFeeModel {
        calls: Rc<Cell<u32>>,
        commission: Money,
    }

    impl FeeModel for CountingFeeModel {
        fn get_commission(
            &self,
            _order: &OrderAny,
            _fill_quantity: Quantity,
            _fill_px: Price,
            _instrument: &InstrumentAny,
        ) -> anyhow::Result<Money> {
            self.calls.set(self.calls.get() + 1);
            Ok(self.commission)
        }
    }

    #[rstest]
    fn test_probability_price_fee_model_rejects_non_binary_instrument() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let fill = binary_option_fill_order(&instrument, LiquiditySide::Taker, "0.500");
        let fee_model = ProbabilityPriceFeeModel;

        let result = fee_model.get_commission(
            &fill,
            Quantity::from("1.00"),
            Price::from("0.500"),
            &instrument,
        );

        assert!(result.is_err());
    }

    #[rstest]
    #[case::maker(Some(dec!(-0.0001)), Some(dec!(0.0003)), None, "maker_rate")]
    #[case::taker(Some(dec!(0.0001)), Some(dec!(-0.0003)), None, "taker_rate")]
    #[case::cap(Some(dec!(0.0001)), Some(dec!(0.0003)), Some(dec!(-0.125)), "cap_rate")]
    fn test_capped_option_fee_model_negative_rate_fails(
        #[case] maker_rate: Option<Decimal>,
        #[case] taker_rate: Option<Decimal>,
        #[case] cap_rate: Option<Decimal>,
        #[case] expected_field: &str,
    ) {
        let result = CappedOptionFeeModel::new(maker_rate, taker_rate, cap_rate);

        assert_eq!(
            result.unwrap_err().to_string(),
            format!("`{expected_field}` must be greater than or equal to zero")
        );
    }

    #[rstest]
    fn test_capped_option_fee_model_maker_commission_rate_bound(
        crypto_option_btc_deribit: CryptoOption,
    ) {
        let instrument = InstrumentAny::CryptoOption(crypto_option_btc_deribit);
        let fill = option_fill_order(&instrument, LiquiditySide::Maker);
        let fee_model = FeeModelAny::CappedOption(
            CappedOptionFeeModel::new(Some(dec!(0.0001)), Some(dec!(0.0003)), None).unwrap(),
        );

        let commission = fee_model
            .get_commission_with_context(
                &fill,
                Quantity::from("2.0"),
                Price::from("100.00"),
                &instrument,
                Some(Price::from("50000.00")),
            )
            .unwrap();

        assert_eq!(commission.currency, Currency::USD());
        assert_eq!(commission.as_decimal(), dec!(10.00));
    }

    #[rstest]
    fn test_capped_option_fee_model_taker_commission_cap_bound(
        crypto_option_btc_deribit: CryptoOption,
    ) {
        let instrument = InstrumentAny::CryptoOption(crypto_option_btc_deribit);
        let fill = option_fill_order(&instrument, LiquiditySide::Taker);
        let fee_model =
            CappedOptionFeeModel::new(Some(dec!(0.0001)), Some(dec!(0.0003)), None).unwrap();

        let commission = fee_model
            .get_commission_with_context(
                &fill,
                Quantity::from("2.0"),
                Price::from("10.00"),
                &instrument,
                Some(Price::from("50000.00")),
            )
            .unwrap();

        assert_eq!(commission.currency, Currency::USD());
        assert_eq!(commission.as_decimal(), dec!(2.50));
    }

    #[rstest]
    fn test_capped_option_fee_model_applies_contract_multiplier(
        mut option_contract_appl: OptionContract,
    ) {
        option_contract_appl.multiplier = Quantity::from(100);
        let instrument = InstrumentAny::OptionContract(option_contract_appl);
        let fill = option_fill_order(&instrument, LiquiditySide::Maker);
        let fee_model =
            CappedOptionFeeModel::new(Some(dec!(0.0001)), Some(dec!(0.0003)), None).unwrap();

        let commission = fee_model
            .get_commission_with_context(
                &fill,
                Quantity::from("2"),
                Price::from("2.00"),
                &instrument,
                Some(Price::from("150.00")),
            )
            .unwrap();

        assert_eq!(commission.currency, Currency::USD());
        assert_eq!(commission.as_decimal(), dec!(3.00));
    }

    #[rstest]
    fn test_capped_option_fee_model_inverse_commission_uses_settlement_currency(
        mut crypto_option_btc_deribit: CryptoOption,
    ) {
        crypto_option_btc_deribit.is_inverse = true;
        let instrument = InstrumentAny::CryptoOption(crypto_option_btc_deribit);
        let fill = option_fill_order(&instrument, LiquiditySide::Taker);
        let fee_model =
            CappedOptionFeeModel::new(Some(dec!(0.0001)), Some(dec!(0.0003)), None).unwrap();

        let commission = fee_model
            .get_commission(
                &fill,
                Quantity::from("2.0"),
                Price::from("0.010"),
                &instrument,
            )
            .unwrap();

        assert_eq!(commission.currency, Currency::BTC());
        assert_eq!(commission.as_decimal(), dec!(0.0006));
    }

    #[rstest]
    fn test_capped_option_fee_model_requires_underlying_price(
        crypto_option_btc_deribit: CryptoOption,
    ) {
        let instrument = InstrumentAny::CryptoOption(crypto_option_btc_deribit);
        let fill = option_fill_order(&instrument, LiquiditySide::Taker);
        let fee_model = CappedOptionFeeModel::default();

        let result = fee_model.get_commission(
            &fill,
            Quantity::from("1.0"),
            Price::from("10.00"),
            &instrument,
        );

        assert!(result.is_err());
    }

    #[rstest]
    fn test_capped_option_fee_model_rejects_non_option_instrument() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let fill = option_fill_order(&instrument, LiquiditySide::Taker);
        let fee_model = CappedOptionFeeModel::default();

        let result = fee_model.get_commission_with_context(
            &fill,
            Quantity::from("1.0"),
            Price::from("10.00"),
            &instrument,
            Some(Price::from("50000.00")),
        );

        assert!(result.is_err());
    }

    #[rstest]
    #[case::maker(LiquiditySide::Maker, dec!(0.04))]
    #[case::taker(LiquiditySide::Taker, dec!(0.10))]
    fn test_tiered_notional_option_fee_model_commission(
        crypto_option_btc_deribit: CryptoOption,
        #[case] liquidity_side: LiquiditySide,
        #[case] expected_commission: Decimal,
    ) {
        let instrument = InstrumentAny::CryptoOption(crypto_option_btc_deribit);
        let fill = option_fill_order(&instrument, liquidity_side);
        let fee_model = FeeModelAny::TieredNotionalOption(
            TieredNotionalOptionFeeModel::new(Some(dec!(0.0002)), Some(dec!(0.0005))).unwrap(),
        );

        let commission = fee_model
            .get_commission(
                &fill,
                Quantity::from("2.0"),
                Price::from("100.00"),
                &instrument,
            )
            .unwrap();

        assert_eq!(commission.currency, Currency::USD());
        assert_eq!(commission.as_decimal(), expected_commission);
    }

    #[rstest]
    fn test_tiered_notional_option_fee_model_inverse_commission_uses_base_currency(
        mut crypto_option_btc_deribit: CryptoOption,
    ) {
        crypto_option_btc_deribit.is_inverse = true;
        let instrument = InstrumentAny::CryptoOption(crypto_option_btc_deribit);
        let fill = option_fill_order(&instrument, LiquiditySide::Taker);
        let fee_model =
            TieredNotionalOptionFeeModel::new(Some(dec!(0.0002)), Some(dec!(0.0005))).unwrap();

        let commission = fee_model
            .get_commission(
                &fill,
                Quantity::from("2.0"),
                Price::from("0.010"),
                &instrument,
            )
            .unwrap();

        assert_eq!(commission.currency, Currency::BTC());
        assert_eq!(commission.as_decimal(), dec!(0.10));
    }

    #[rstest]
    fn test_tiered_notional_option_fee_model_rejects_non_option_instrument() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let fill = option_fill_order(&instrument, LiquiditySide::Taker);
        let fee_model = TieredNotionalOptionFeeModel::default();

        let result = fee_model.get_commission(
            &fill,
            Quantity::from("1.0"),
            Price::from("10.00"),
            &instrument,
        );

        assert!(result.is_err());
    }

    #[rstest]
    #[case::maker(Some(dec!(-0.0002)), Some(dec!(0.0005)), "maker_rate")]
    #[case::taker(Some(dec!(0.0002)), Some(dec!(-0.0005)), "taker_rate")]
    fn test_tiered_notional_option_fee_model_negative_rate_fails(
        #[case] maker_rate: Option<Decimal>,
        #[case] taker_rate: Option<Decimal>,
        #[case] expected_field: &str,
    ) {
        let result = TieredNotionalOptionFeeModel::new(maker_rate, taker_rate);

        assert_eq!(
            result.unwrap_err().to_string(),
            format!("`{expected_field}` must be greater than or equal to zero")
        );
    }

    #[rstest]
    fn test_tiered_notional_option_fee_model_requires_liquidity_side(
        crypto_option_btc_deribit: CryptoOption,
    ) {
        let instrument = InstrumentAny::CryptoOption(crypto_option_btc_deribit);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .price(Price::from("100.00"))
            .quantity(Quantity::from("2.0"))
            .build();
        let fee_model = TieredNotionalOptionFeeModel::default();

        let result = fee_model.get_commission(
            &order,
            Quantity::from("1.0"),
            Price::from("10.00"),
            &instrument,
        );

        assert!(result.is_err());
    }

    fn option_fill_order(instrument: &InstrumentAny, liquidity_side: LiquiditySide) -> OrderAny {
        let limit_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .price(Price::from("100.00"))
            .quantity(Quantity::from("2.0"))
            .build();

        TestOrderStubs::make_filled_order(&limit_order, instrument, liquidity_side)
    }

    fn binary_option_fill_order(
        instrument: &InstrumentAny,
        liquidity_side: LiquiditySide,
        price: &str,
    ) -> OrderAny {
        let limit_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .price(Price::from(price))
            .quantity(Quantity::from("1.00"))
            .build();

        TestOrderStubs::make_filled_order(&limit_order, instrument, liquidity_side)
    }
}
