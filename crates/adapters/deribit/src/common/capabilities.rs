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

use nautilus_model::{
    enums::{OrderType, TimeInForce, TriggerType},
    instruments::{Instrument, InstrumentAny},
    orders::Order,
};

pub(crate) const IS_CBE_ROUTED: &str = "is_cbe_routed";
pub(crate) const IS_CSR: &str = "is_csr";

pub(crate) fn validate_order(instrument: &InstrumentAny, order: &dyn Order) -> anyhow::Result<()> {
    if matches!(instrument, InstrumentAny::CurrencyPair(_)) {
        anyhow::ensure!(
            order.time_in_force() != TimeInForce::Gtd,
            "Deribit spot instruments do not support good_til_day"
        );
    }

    if !is_routed_spot(instrument) {
        return Ok(());
    }

    anyhow::ensure!(
        matches!(
            order.order_type(),
            OrderType::Limit | OrderType::Market | OrderType::StopLimit
        ),
        "Deribit routed spot does not support order type {}",
        order.order_type()
    );
    anyhow::ensure!(
        matches!(
            order.time_in_force(),
            TimeInForce::Gtc | TimeInForce::Ioc | TimeInForce::Fok
        ),
        "Deribit routed spot does not support time in force {}",
        order.time_in_force()
    );
    anyhow::ensure!(
        !order.is_reduce_only(),
        "Deribit routed spot does not support reduce_only"
    );
    anyhow::ensure!(
        order.display_qty().is_none(),
        "Deribit routed spot does not support iceberg orders"
    );
    anyhow::ensure!(
        order.contingency_type().is_none()
            && order.linked_order_ids().is_none_or(|ids| ids.is_empty()),
        "Deribit routed spot does not support linked orders"
    );

    if order.order_type() == OrderType::StopLimit {
        anyhow::ensure!(
            matches!(
                order.trigger_type(),
                Some(TriggerType::Default | TriggerType::LastPrice)
            ),
            "Deribit routed spot stop-limit orders require last_price triggers"
        );
    }

    Ok(())
}

pub(crate) fn validate_public_trades(instrument: &InstrumentAny) -> anyhow::Result<()> {
    anyhow::ensure!(
        !is_routed_spot(instrument),
        "Deribit routed spot does not support public trades or charts for {}",
        instrument.id()
    );
    Ok(())
}

fn is_routed_spot(instrument: &InstrumentAny) -> bool {
    matches!(instrument, InstrumentAny::CurrencyPair(_))
        && instrument
            .info()
            .is_some_and(|info| info.contains_key(IS_CBE_ROUTED) || info.contains_key(IS_CSR))
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        enums::ContingencyType,
        identifiers::{ClientOrderId, InstrumentId},
        orders::OrderTestBuilder,
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use serde_json::Value;

    use super::*;
    use crate::{
        common::{parse::parse_deribit_instrument_any, testing::routed_spot_instrument},
        http::models::DeribitInstrument,
    };

    #[rstest]
    #[case(true, true)]
    #[case(true, false)]
    #[case(false, true)]
    #[case(false, false)]
    fn routed_spot_metadata_survives_parsing(#[case] cbe: bool, #[case] csr: bool) {
        let mut value: Value = serde_json::from_str(include_str!(
            "../../test_data/http_get_instrument_routed_spot.json"
        ))
        .unwrap();

        if !cbe {
            value.as_object_mut().unwrap().remove("is_cbe_routed");
        }

        if !csr {
            value.as_object_mut().unwrap().remove("is_csr");
        }

        let json = value.to_string();
        let definition: DeribitInstrument = serde_json::from_str(&json).unwrap();
        let instrument =
            parse_deribit_instrument_any(&definition, UnixNanos::from(31), UnixNanos::from(29))
                .unwrap()
                .unwrap();

        assert_eq!(definition.is_cbe_routed, cbe.then_some(true));
        assert_eq!(definition.is_csr, csr.then_some(true));
        assert_eq!(definition.block_trade_commission, None);
        assert_eq!(definition.block_trade_min_trade_amount, None);
        assert_eq!(definition.block_trade_tick_size, None);
        assert_eq!(instrument.id(), InstrumentId::from("BTC_USDC.DERIBIT"));
        assert_eq!(instrument.price_increment(), Price::from("0.01"));
        assert_eq!(instrument.size_increment(), Quantity::from("0.0001"));
        assert_eq!(is_routed_spot(&instrument), cbe || csr);
        assert_eq!(
            instrument.info().map(|info| info.len()),
            (cbe || csr).then_some(usize::from(cbe) + usize::from(csr))
        );

        if let Some(info) = instrument.info() {
            assert_eq!(info.get("is_cbe_routed"), cbe.then_some(&Value::Bool(true)));
            assert_eq!(info.get("is_csr"), csr.then_some(&Value::Bool(true)));
        }
    }

    #[rstest]
    #[case(OrderType::StopMarket)]
    #[case(OrderType::MarketIfTouched)]
    #[case(OrderType::LimitIfTouched)]
    fn routed_spot_rejects_unsupported_order_types(#[case] order_type: OrderType) {
        let instrument = routed_spot_instrument();
        let order = OrderTestBuilder::new(order_type)
            .instrument_id(instrument.id())
            .quantity(Quantity::from("0.0042"))
            .price(Price::from("45000.25"))
            .trigger_price(Price::from("44000.50"))
            .trigger_type(TriggerType::LastPrice)
            .build();

        assert_eq!(
            validate_order(&instrument, &order).unwrap_err().to_string(),
            format!("Deribit routed spot does not support order type {order_type}")
        );
    }

    #[rstest]
    #[case("reduce_only", "Deribit routed spot does not support reduce_only")]
    #[case("iceberg", "Deribit routed spot does not support iceberg orders")]
    #[case("oco", "Deribit routed spot does not support linked orders")]
    #[case("oto", "Deribit routed spot does not support linked orders")]
    #[case("linked_ids", "Deribit routed spot does not support linked orders")]
    fn routed_spot_rejects_order_features(#[case] feature: &str, #[case] expected: &str) {
        let instrument = routed_spot_instrument();
        let mut builder = OrderTestBuilder::new(OrderType::Limit);
        builder
            .instrument_id(instrument.id())
            .quantity(Quantity::from("0.0042"))
            .price(Price::from("45000.25"));

        match feature {
            "reduce_only" => {
                builder.reduce_only(true);
            }
            "iceberg" => {
                builder.display_qty(Quantity::from("0.0011"));
            }
            "oco" => {
                builder
                    .contingency_type(ContingencyType::Oco)
                    .linked_order_ids(vec![ClientOrderId::from("linked-001")]);
            }
            "oto" => {
                builder
                    .contingency_type(ContingencyType::Oto)
                    .linked_order_ids(vec![ClientOrderId::from("linked-001")]);
            }
            "linked_ids" => {
                builder.linked_order_ids(vec![ClientOrderId::from("linked-001")]);
            }
            _ => unreachable!(),
        }

        let order = builder.build();
        assert_eq!(
            validate_order(&instrument, &order).unwrap_err().to_string(),
            expected
        );
    }

    #[rstest]
    #[case(TriggerType::Default, true)]
    #[case(TriggerType::LastPrice, true)]
    #[case(TriggerType::MarkPrice, false)]
    #[case(TriggerType::IndexPrice, false)]
    fn routed_spot_stop_limit_triggers(#[case] trigger: TriggerType, #[case] supported: bool) {
        let instrument = routed_spot_instrument();
        let order = OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(instrument.id())
            .quantity(Quantity::from("0.0042"))
            .price(Price::from("45000.25"))
            .trigger_price(Price::from("46000.50"))
            .trigger_type(trigger)
            .build();

        assert_eq!(validate_order(&instrument, &order).is_ok(), supported);
    }

    #[rstest]
    #[case(OrderType::Limit, TimeInForce::Gtc, true)]
    #[case(OrderType::Market, TimeInForce::Gtc, true)]
    #[case(OrderType::Limit, TimeInForce::Ioc, true)]
    #[case(OrderType::Market, TimeInForce::Ioc, true)]
    #[case(OrderType::Limit, TimeInForce::Fok, true)]
    #[case(OrderType::Market, TimeInForce::Fok, true)]
    #[case(OrderType::Limit, TimeInForce::Gtd, false)]
    #[case(OrderType::Limit, TimeInForce::Day, false)]
    #[case(OrderType::Market, TimeInForce::Day, false)]
    fn routed_spot_time_in_force(
        #[case] order_type: OrderType,
        #[case] tif: TimeInForce,
        #[case] supported: bool,
    ) {
        let instrument = routed_spot_instrument();
        let order = OrderTestBuilder::new(order_type)
            .instrument_id(instrument.id())
            .quantity(Quantity::from("0.0042"))
            .price(Price::from("45000.25"))
            .time_in_force(tif)
            .expire_time(UnixNanos::from(1_800_000_000_000_000_000))
            .build();

        assert_eq!(validate_order(&instrument, &order).is_ok(), supported);
    }

    #[rstest]
    fn ordinary_spot_preserves_order_and_public_data_features() {
        let mut instrument = routed_spot_instrument();

        let InstrumentAny::CurrencyPair(pair) = &mut instrument else {
            unreachable!()
        };

        pair.info = None;
        let order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument.id())
            .quantity(Quantity::from("0.0042"))
            .trigger_price(Price::from("46000.50"))
            .trigger_type(TriggerType::MarkPrice)
            .reduce_only(true)
            .build();

        assert_eq!(
            validate_order(&instrument, &order).map_err(|e| e.to_string()),
            Ok(())
        );
        assert_eq!(
            validate_public_trades(&instrument).map_err(|e| e.to_string()),
            Ok(())
        );
    }
}
