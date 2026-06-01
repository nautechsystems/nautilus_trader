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

use std::fmt::Debug;

use nautilus_model::{
    enums::LiquiditySide,
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::{Decimal, prelude::ToPrimitive};
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

#[derive(Clone, Debug)]
pub enum FeeModelAny {
    Fixed(FixedFeeModel),
    MakerTaker(MakerTakerFeeModel),
    PerContract(PerContractFeeModel),
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
    change_commission_once: bool,
}

impl FixedFeeModel {
    /// Creates a new [`FixedFeeModel`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if `commission` is negative.
    pub fn new(commission: Money, change_commission_once: Option<bool>) -> anyhow::Result<Self> {
        if commission.raw < 0 {
            anyhow::bail!("Commission must be greater than or equal to zero")
        }
        let zero_commission = Money::zero(commission.currency);
        Ok(Self {
            commission,
            zero_commission,
            change_commission_once: change_commission_once.unwrap_or(true),
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
        if !self.change_commission_once || order.filled_qty().is_zero() {
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
        _instrument: &InstrumentAny,
    ) -> anyhow::Result<Money> {
        let total = self.commission.as_f64() * fill_quantity.as_f64();
        Ok(Money::new(total, self.commission.currency))
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
            Some(LiquiditySide::Maker) => notional * instrument.maker_fee().to_f64().unwrap(),
            Some(LiquiditySide::Taker) => notional * instrument.taker_fee().to_f64().unwrap(),
            Some(LiquiditySide::NoLiquiditySide) | None => anyhow::bail!("Liquidity side not set"),
        };

        if instrument.is_inverse() {
            Ok(Money::new(commission, instrument.base_currency().unwrap()))
        } else {
            Ok(Money::new(commission, instrument.quote_currency()))
        }
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

    fn rate(&self, order: &OrderAny, instrument: &InstrumentAny) -> anyhow::Result<Decimal> {
        let rate = match order.liquidity_side() {
            Some(LiquiditySide::Maker) => self.maker_rate.unwrap_or_else(|| instrument.maker_fee()),
            Some(LiquiditySide::Taker) => self.taker_rate.unwrap_or_else(|| instrument.taker_fee()),
            Some(LiquiditySide::NoLiquiditySide) | None => anyhow::bail!("Liquidity side not set"),
        };
        check_fee_rate(Some(rate), "fee_rate")?;
        Ok(rate)
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
        let rate = self.rate(order, instrument)?;
        let rate_fee = if instrument.is_inverse() {
            rate
        } else {
            let underlying_px =
                underlying_px.ok_or_else(|| anyhow::anyhow!("Underlying price is required"))?;
            rate * underlying_px.as_decimal()
        };
        let cap_fee = self.cap * fill_px.as_decimal();
        let fee_per_contract = if rate_fee < cap_fee {
            rate_fee
        } else {
            cap_fee
        };
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

    fn rate(&self, order: &OrderAny, instrument: &InstrumentAny) -> anyhow::Result<Decimal> {
        let rate = match order.liquidity_side() {
            Some(LiquiditySide::Maker) => self.maker_rate.unwrap_or_else(|| instrument.maker_fee()),
            Some(LiquiditySide::Taker) => self.taker_rate.unwrap_or_else(|| instrument.taker_fee()),
            Some(LiquiditySide::NoLiquiditySide) | None => anyhow::bail!("Liquidity side not set"),
        };
        check_fee_rate(Some(rate), "fee_rate")?;
        Ok(rate)
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
        let rate = self.rate(order, instrument)?;
        let notional = instrument.calculate_notional_value(fill_quantity, fill_px, Some(false));
        let total = notional.as_decimal() * rate;
        Money::from_decimal(total, notional.currency).map_err(Into::into)
    }
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
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderType},
        instruments::{
            CryptoOption, Instrument, InstrumentAny,
            stubs::{audusd_sim, crypto_option_btc_deribit},
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
        CappedOptionFeeModel, FeeModel, FeeModelAny, FixedFeeModel, MakerTakerFeeModel,
        PerContractFeeModel, TieredNotionalOptionFeeModel,
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
    fn test_per_contract_fee_model_negative_commission_fails() {
        let result = PerContractFeeModel::new(Money::new(-1.0, Currency::USD()));
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
}
