// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
    instruments::any::InstrumentAny,
    orders::any::OrderAny,
    types::{money::Money, price::Price, quantity::Quantity},
};

pub trait FeeModel {
    fn get_commission(
        &self,
        order: &OrderAny,
        fill_quantity: Quantity,
        fill_px: Price,
        instrument: &InstrumentAny,
    ) -> Money;
}

#[derive(Debug, Clone)]
pub struct FixedFeeModel {
    commission: Money,
    zero_commission: Money,
    change_commission_once: bool,
}

impl FixedFeeModel {
    pub fn new(commission: Money, change_commission_once: Option<bool>) -> anyhow::Result<Self> {
        if commission.as_f64() < 0.0 {
            anyhow::bail!("Commission must be greater than or equal to zero.")
        }
        let zero_commission = Money::new(0.0, commission.currency)?;
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
    ) -> Money {
        if !self.change_commission_once || order.filled_qty().is_zero() {
            self.commission
        } else {
            self.zero_commission
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::OrderSide,
        instruments::{any::InstrumentAny, stubs::audusd_sim},
        orders::stubs::{TestOrderEventStubs, TestOrderStubs},
        types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
    };
    use rstest::rstest;

    use crate::models::fee::{FeeModel, FixedFeeModel};

    #[rstest]
    fn test_fixed_model_single_fill() {
        let expected_commission = Money::new(1.0, Currency::USD()).unwrap();
        let aud_usd = InstrumentAny::CurrencyPair(audusd_sim());
        let fee_model = FixedFeeModel::new(expected_commission, None).unwrap();
        let market_order = TestOrderStubs::market_order(
            aud_usd.id(),
            OrderSide::Buy,
            Quantity::from(100_000),
            None,
            None,
        );
        let accepted_order = TestOrderStubs::make_accepted_order(&market_order);
        let commission = fee_model.get_commission(
            &accepted_order,
            Quantity::from(100_000),
            Price::from("1.0"),
            &aud_usd,
        );
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
        let market_order = TestOrderStubs::market_order(
            aud_usd.id(),
            order_side,
            Quantity::from(100_000),
            None,
            None,
        );
        let mut accepted_order = TestOrderStubs::make_accepted_order(&market_order);
        let commission_first_fill = fee_model.get_commission(
            &accepted_order,
            Quantity::from(50_000),
            Price::from("1.0"),
            &aud_usd,
        );
        let fill = TestOrderEventStubs::order_filled(
            &accepted_order,
            &aud_usd,
            None,
            None,
            None,
            Some(Quantity::from(50_000)),
            None,
            None,
            None,
        );
        accepted_order.apply(fill).unwrap();
        let commission_next_fill = fee_model.get_commission(
            &accepted_order,
            Quantity::from(50_000),
            Price::from("1.0"),
            &aud_usd,
        );
        assert_eq!(commission_first_fill, expected_first_fill);
        assert_eq!(commission_next_fill, expected_next_fill);
    }
}
