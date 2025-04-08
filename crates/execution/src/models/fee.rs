// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_model::{
    enums::LiquiditySide,
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    types::{Money, Price, Quantity},
};
use rust_decimal::prelude::ToPrimitive;

pub trait FeeModel {
    fn get_commission(
        &self,
        order: &OrderAny,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<Money>;
}

#[derive(Clone, Debug)]
pub enum FeeModelAny {
    Fixed(FixedFeeModel),
    MakerTaker(MakerTakerFeeModel),
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
        }
    }
}

impl Default for FeeModelAny {
    fn default() -> Self {
        Self::MakerTaker(MakerTakerFeeModel)
    }
}

#[derive(Debug, Clone)]
pub struct FixedFeeModel {
    commission: Money,
    zero_commission: Money,
    change_commission_once: bool,
}

impl FixedFeeModel {
    /// Creates a new [`FixedFeeModel`] instance.
    pub fn new(commission: Money, change_commission_once: Option<bool>) -> anyhow::Result<Self> {
        if commission.as_f64() < 0.0 {
            anyhow::bail!("Commission must be greater than or equal to zero.")
        }
        let zero_commission = Money::new(0.0, commission.currency);
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
            Some(LiquiditySide::NoLiquiditySide) | None => anyhow::bail!("Liquidity side not set."),
        };
        if instrument.is_inverse() {
            Ok(Money::new(commission, instrument.base_currency().unwrap()))
        } else {
            Ok(Money::new(commission, instrument.quote_currency()))
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderType},
        instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
        orders::{
            Order,
            builder::OrderTestBuilder,
            stubs::{TestOrderEventStubs, TestOrderStubs},
        },
        types::{Currency, Money, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::prelude::ToPrimitive;

    use super::{FeeModel, FixedFeeModel, MakerTakerFeeModel};

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
        let maker_fee = aud_usd.maker_fee().to_f64().unwrap();
        let price = Price::from("1.0");
        let limit_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(aud_usd.id())
            .side(OrderSide::Sell)
            .price(price)
            .quantity(Quantity::from(100_000))
            .build();
        let order_filled =
            TestOrderStubs::make_filled_order(&limit_order, &aud_usd, LiquiditySide::Maker);
        let expected_commission_amount =
            order_filled.quantity().as_f64() * price.as_f64() * maker_fee;
        let commission = fee_model
            .get_commission(
                &order_filled,
                Quantity::from(100_000),
                Price::from("1.0"),
                &aud_usd,
            )
            .unwrap();
        assert_eq!(commission.as_f64(), expected_commission_amount);
    }

    #[rstest]
    fn test_maker_taker_fee_model_taker_commission() {
        let fee_model = MakerTakerFeeModel;
        let aud_usd = InstrumentAny::CurrencyPair(audusd_sim());
        let maker_fee = aud_usd.taker_fee().to_f64().unwrap();
        let price = Price::from("1.0");
        let limit_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(aud_usd.id())
            .side(OrderSide::Sell)
            .price(price)
            .quantity(Quantity::from(100_000))
            .build();

        let order_filled =
            TestOrderStubs::make_filled_order(&limit_order, &aud_usd, LiquiditySide::Taker);
        let expected_commission_amount =
            order_filled.quantity().as_f64() * price.as_f64() * maker_fee;
        let commission = fee_model
            .get_commission(
                &order_filled,
                Quantity::from(100_000),
                Price::from("1.0"),
                &aud_usd,
            )
            .unwrap();
        assert_eq!(commission.as_f64(), expected_commission_amount);
    }
}
