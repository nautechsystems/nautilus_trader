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

//! Pure builder functions that convert Nautilus execution commands into Kraken WS param structs.

use chrono::{DateTime, Utc};
use nautilus_common::messages::execution::{CancelOrder, ModifyOrder, SubmitOrder};
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce, TriggerType},
    orders::{Order, any::OrderAny},
};

use crate::{
    common::{
        enums::{KrakenOrderSide, KrakenOrderType, KrakenSpotTrigger, KrakenTimeInForce},
        parse::truncate_cl_ord_id,
    },
    websocket::spot_v2::messages::{
        KrakenWsAddOrderParams, KrakenWsAmendOrderParams, KrakenWsCancelOrderParams,
        KrakenWsTriggerParams,
    },
};

/// Builds WebSocket `add_order` parameters from a Nautilus submit command and the cached order.
///
/// # Errors
///
/// Returns an error if:
/// - The order side cannot be mapped to a Kraken side.
/// - The order type is not supported on the WS path.
/// - The time in force is not supported for the given order type.
/// - GTD time in force is missing `expire_time`.
pub fn build_add_order_params(
    cmd: &SubmitOrder,
    order: &OrderAny,
    token: String,
    leverage: Option<u16>,
) -> anyhow::Result<KrakenWsAddOrderParams> {
    let order_type = order.order_type();
    let order_side = order.order_side();
    let time_in_force = order.time_in_force();

    let side = match order_side {
        OrderSide::Buy => KrakenOrderSide::Buy,
        OrderSide::Sell => KrakenOrderSide::Sell,
        _ => anyhow::bail!("Invalid order side: {order_side:?}"),
    };

    if matches!(
        order_type,
        OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
    ) {
        anyhow::bail!("Trailing stop orders are not yet supported on the Kraken WS path; use REST",);
    }

    if order.display_qty().is_some() {
        anyhow::bail!(
            "Iceberg (display_qty) orders are not supported on the Kraken WS path; use REST"
        );
    }

    let kraken_order_type = match order_type {
        OrderType::Market => KrakenOrderType::Market,
        OrderType::Limit => KrakenOrderType::Limit,
        OrderType::StopMarket => KrakenOrderType::StopLoss,
        OrderType::StopLimit => KrakenOrderType::StopLossLimit,
        OrderType::MarketIfTouched => KrakenOrderType::TakeProfit,
        OrderType::LimitIfTouched => KrakenOrderType::TakeProfitLimit,
        _ => anyhow::bail!("Unsupported order type for Kraken WS: {order_type:?}"),
    };

    let is_limit_order = matches!(
        order_type,
        OrderType::Limit | OrderType::StopLimit | OrderType::LimitIfTouched
    );

    if is_limit_order && order.price().is_none() {
        anyhow::bail!("limit_price is required for order type {order_type:?}");
    }

    let ws_tif = compute_ws_time_in_force(is_limit_order, time_in_force, order.expire_time())?;
    let expire_time = match (ws_tif, order.expire_time()) {
        (Some(KrakenTimeInForce::GoodTilDate), Some(ts)) => Some(format_expire_time(ts)),
        _ => None,
    };

    let is_conditional = matches!(
        order_type,
        OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched
    );

    let limit_price = order.price().map(|p| p.as_f64());

    let trigger = if is_conditional {
        let trigger_ref = match order.trigger_type() {
            Some(TriggerType::IndexPrice) => KrakenSpotTrigger::Index,
            Some(TriggerType::LastPrice | TriggerType::Default) | None => KrakenSpotTrigger::Last,
            Some(other) => anyhow::bail!(
                "Unsupported trigger type for Kraken Spot WS: {other:?} (only LastPrice and IndexPrice supported)",
            ),
        };
        order.trigger_price().map(|tp| KrakenWsTriggerParams {
            reference: trigger_ref,
            price: tp.as_f64(),
            price_type: None,
        })
    } else {
        None
    };

    if is_conditional && trigger.is_none() {
        anyhow::bail!("trigger_price is required for conditional order type {order_type:?}");
    }

    let symbol = cmd.instrument_id.symbol.inner().to_string();
    let cl_ord_id = Some(truncate_cl_ord_id(&order.client_order_id()));
    let post_only = order.is_post_only().then_some(true);
    let reduce_only = order.is_reduce_only().then_some(true);

    Ok(KrakenWsAddOrderParams {
        order_type: kraken_order_type,
        side,
        order_qty: order.quantity().as_f64(),
        symbol,
        token,
        limit_price,
        time_in_force: ws_tif,
        expire_time,
        cl_ord_id,
        post_only,
        reduce_only,
        leverage,
        trigger,
        conditional: None,
    })
}

/// Formats a [`UnixNanos`] timestamp as an RFC3339 string suitable for the Kraken
/// WebSocket v2 `expire_time` field.
///
/// `UnixNanos` is a `u64` whose maximum value (`~1.8e19` ns) corresponds to
/// year 2554, well within both `i64::MAX` seconds and `chrono`'s representable
/// range, so the conversion cannot fail for any in-range input.
pub(crate) fn format_expire_time(ts: UnixNanos) -> String {
    let raw = ts.as_u64();
    let secs = (raw / 1_000_000_000) as i64;
    let nanos = (raw % 1_000_000_000) as u32;
    DateTime::<Utc>::from_timestamp(secs, nanos)
        .expect("Invariant: UnixNanos always fits a valid DateTime<Utc>")
        .to_rfc3339()
}

/// Builds WebSocket `amend_order` parameters from a Nautilus modify command.
///
/// Prefers `venue_order_id` (as `order_id`) over `client_order_id` (as `cl_ord_id`);
/// `cmd.client_order_id` is always set so a fallback is always available.
pub fn build_amend_order_params(cmd: &ModifyOrder, token: String) -> KrakenWsAmendOrderParams {
    let order_id = cmd.venue_order_id.as_ref().map(|id| id.to_string());
    let cl_ord_id = if order_id.is_none() {
        Some(truncate_cl_ord_id(&cmd.client_order_id))
    } else {
        None
    };

    KrakenWsAmendOrderParams {
        token,
        order_id,
        cl_ord_id,
        order_qty: cmd.quantity.map(|q| q.as_f64()),
        limit_price: cmd.price.map(|p| p.as_f64()),
        trigger_price: cmd.trigger_price.map(|p| p.as_f64()),
    }
}

/// Builds WebSocket `cancel_order` parameters from a Nautilus cancel command.
///
/// Prefers `venue_order_id` (as `order_id`) over `client_order_id` (as `cl_ord_id`),
/// mirroring the REST cancel path which prefers the venue identifier since Kraken
/// always knows it.
pub fn build_cancel_order_params(cmd: &CancelOrder, token: String) -> KrakenWsCancelOrderParams {
    if let Some(ref venue_id) = cmd.venue_order_id {
        KrakenWsCancelOrderParams {
            token,
            order_id: Some(vec![venue_id.to_string()]),
            cl_ord_id: None,
        }
    } else {
        KrakenWsCancelOrderParams {
            token,
            order_id: None,
            cl_ord_id: Some(vec![truncate_cl_ord_id(&cmd.client_order_id)]),
        }
    }
}

pub(crate) fn compute_ws_time_in_force(
    is_limit_order: bool,
    time_in_force: TimeInForce,
    expire_time: Option<UnixNanos>,
) -> anyhow::Result<Option<KrakenTimeInForce>> {
    if !is_limit_order {
        return Ok(None);
    }

    match time_in_force {
        TimeInForce::Gtc => Ok(None),
        TimeInForce::Ioc => Ok(Some(KrakenTimeInForce::ImmediateOrCancel)),
        TimeInForce::Fok => {
            anyhow::bail!("FOK time in force is not supported on Kraken WS v2; use REST")
        }
        TimeInForce::Gtd => {
            expire_time.ok_or_else(|| {
                anyhow::anyhow!("GTD time in force requires expire_time parameter")
            })?;
            Ok(Some(KrakenTimeInForce::GoodTilDate))
        }
        _ => anyhow::bail!("Unsupported time in force: {time_in_force:?}"),
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::identifiers::{
        ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId,
    };
    use rstest::rstest;

    use super::*;

    fn make_cancel_order(client_order_id: &str, venue_order_id: Option<&str>) -> CancelOrder {
        CancelOrder {
            trader_id: TraderId::from("TESTER-001"),
            client_id: None,
            strategy_id: StrategyId::from("S-001"),
            instrument_id: InstrumentId::from("XBT/USD.KRAKEN"),
            client_order_id: ClientOrderId::from(client_order_id),
            venue_order_id: venue_order_id.map(VenueOrderId::new),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
        }
    }

    #[rstest]
    fn test_format_expire_time_rfc3339() {
        let ts = UnixNanos::from(1_767_225_599_000_000_000_u64);
        assert_eq!(format_expire_time(ts), "2025-12-31T23:59:59+00:00");
    }

    #[rstest]
    fn test_format_expire_time_max_unix_nanos_handled() {
        // u64::MAX nanos ≈ year 2554; chrono accepts it. Documents that the
        // function never panics for any in-range UnixNanos value.
        let ts = UnixNanos::from(u64::MAX);
        let formatted = format_expire_time(ts);
        assert!(formatted.starts_with("2554-"), "unexpected: {formatted}");
    }

    #[rstest]
    fn test_build_add_order_params_trailing_stop_market_bails() {
        use nautilus_model::{
            enums::TrailingOffsetType,
            orders::trailing_stop_market::TrailingStopMarketOrder,
            types::{Price, Quantity},
        };
        use rust_decimal::Decimal;

        let trader_id = TraderId::from("TESTER-001");
        let strategy_id = StrategyId::from("S-001");
        let instrument_id = InstrumentId::from("BTC/USD.KRAKEN");
        let cl_ord_id = ClientOrderId::from("O-1");

        let order = OrderAny::TrailingStopMarket(TrailingStopMarketOrder::new(
            trader_id,
            strategy_id,
            instrument_id,
            cl_ord_id,
            OrderSide::Buy,
            Quantity::from("0.01"),
            Price::from("50000.00"),
            TriggerType::LastPrice,
            Decimal::new(100, 0),
            TrailingOffsetType::Price,
            TimeInForce::Gtc,
            None,
            false,
            false,
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
            UUID4::new(),
            UnixNanos::default(),
        ));

        let cmd = SubmitOrder {
            trader_id,
            client_id: None,
            strategy_id,
            instrument_id,
            client_order_id: cl_ord_id,
            order_init: order.init_event().clone(),
            exec_algorithm_id: None,
            position_id: None,
            params: None,
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
        };

        let err = build_add_order_params(&cmd, &order, "TKN".to_string(), None)
            .expect_err("TrailingStopMarket must bail to REST");
        let msg = format!("{err}");
        assert!(
            msg.contains("Trailing stop") && msg.contains("REST"),
            "unexpected error: {msg}",
        );
    }

    #[rstest]
    fn test_build_add_order_params_iceberg_bails() {
        use nautilus_model::{
            orders::limit::LimitOrder,
            types::{Price, Quantity},
        };

        let trader_id = TraderId::from("TESTER-001");
        let strategy_id = StrategyId::from("S-001");
        let instrument_id = InstrumentId::from("BTC/USD.KRAKEN");
        let cl_ord_id = ClientOrderId::from("O-1");

        let order = OrderAny::Limit(LimitOrder::new(
            trader_id,
            strategy_id,
            instrument_id,
            cl_ord_id,
            OrderSide::Buy,
            Quantity::from("1.0"),
            Price::from("50000.00"),
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
            Some(Quantity::from("0.1")), // display_qty -> iceberg
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
            UUID4::new(),
            UnixNanos::default(),
        ));

        let cmd = SubmitOrder {
            trader_id,
            client_id: None,
            strategy_id,
            instrument_id,
            client_order_id: cl_ord_id,
            order_init: order.init_event().clone(),
            exec_algorithm_id: None,
            position_id: None,
            params: None,
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
        };

        let err = build_add_order_params(&cmd, &order, "TKN".to_string(), None)
            .expect_err("Iceberg orders must bail to REST");
        let msg = format!("{err}");
        assert!(
            msg.contains("Iceberg") && msg.contains("REST"),
            "unexpected error: {msg}",
        );
    }

    #[rstest]
    fn test_build_add_order_params_unsupported_trigger_type_bails() {
        use nautilus_model::{
            orders::stop_market::StopMarketOrder,
            types::{Price, Quantity},
        };

        let trader_id = TraderId::from("TESTER-001");
        let strategy_id = StrategyId::from("S-001");
        let instrument_id = InstrumentId::from("BTC/USD.KRAKEN");
        let cl_ord_id = ClientOrderId::from("O-1");

        let order = OrderAny::StopMarket(StopMarketOrder::new(
            trader_id,
            strategy_id,
            instrument_id,
            cl_ord_id,
            OrderSide::Buy,
            Quantity::from("0.01"),
            Price::from("50000.00"),
            TriggerType::MarkPrice,
            TimeInForce::Gtc,
            None,  // expire_time
            false, // reduce_only
            false, // quote_quantity
            None,  // display_qty
            None,  // emulation_trigger
            None,  // trigger_instrument_id
            None,  // contingency_type
            None,  // order_list_id
            None,  // linked_order_ids
            None,  // parent_order_id
            None,  // exec_algorithm_id
            None,  // exec_algorithm_params
            None,  // exec_spawn_id
            None,  // tags
            UUID4::new(),
            UnixNanos::default(),
        ));

        let cmd = SubmitOrder {
            trader_id,
            client_id: None,
            strategy_id,
            instrument_id,
            client_order_id: cl_ord_id,
            order_init: order.init_event().clone(),
            exec_algorithm_id: None,
            position_id: None,
            params: None,
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
        };

        let err = build_add_order_params(&cmd, &order, "TKN".to_string(), None)
            .expect_err("MarkPrice trigger must bail to REST");
        let msg = format!("{err}");
        assert!(
            msg.contains("trigger type") && msg.contains("LastPrice"),
            "unexpected error: {msg}",
        );
    }

    #[rstest]
    fn test_compute_ws_time_in_force_fok_bails() {
        let result = compute_ws_time_in_force(true, TimeInForce::Fok, None);
        let err = result.expect_err("FOK should bail on WS path");
        let msg = format!("{err}");
        assert!(
            msg.contains("FOK") && msg.contains("REST"),
            "unexpected error: {msg}",
        );
    }

    #[rstest]
    fn test_build_cancel_order_params_with_venue_id() {
        let cmd = make_cancel_order("O-20260505-001", Some("OABCDE-12345-FGHIJ"));
        let params = build_cancel_order_params(&cmd, "TOKEN".to_string());

        let ids = params.order_id.as_ref().unwrap();
        assert_eq!(ids, &["OABCDE-12345-FGHIJ"]);
        assert!(params.cl_ord_id.is_none());
    }

    #[rstest]
    fn test_build_cancel_order_params_falls_back_to_client_id() {
        let cmd = make_cancel_order("O-20260505-001", None);
        let params = build_cancel_order_params(&cmd, "TOKEN".to_string());

        assert!(params.order_id.is_none());
        let cl_ord_ids = params.cl_ord_id.as_ref().unwrap();
        assert_eq!(cl_ord_ids, &["O-20260505-001"]);
    }

    #[rstest]
    fn test_build_cancel_order_params_long_client_id_is_truncated() {
        let cmd = make_cancel_order("O202602270023210040011", None);
        let params = build_cancel_order_params(&cmd, "TOKEN".to_string());

        assert!(params.order_id.is_none());
        let cl_ord_ids = params.cl_ord_id.as_ref().unwrap();
        assert_eq!(cl_ord_ids.len(), 1);
        let cl_ord_id = &cl_ord_ids[0];
        assert!(
            cl_ord_id.len() <= 18,
            "cl_ord_id length was {}",
            cl_ord_id.len()
        );
    }

    #[rstest]
    fn test_build_amend_order_params_with_venue_id() {
        use nautilus_model::types::{Price, Quantity};

        let cmd = ModifyOrder {
            trader_id: TraderId::from("TESTER-001"),
            client_id: None,
            strategy_id: StrategyId::from("S-001"),
            instrument_id: InstrumentId::from("XBT/USD.KRAKEN"),
            client_order_id: ClientOrderId::from("O-001"),
            venue_order_id: Some(VenueOrderId::new("OABCDE-12345-FGHIJ")),
            quantity: Some(Quantity::new(0.1, 1)),
            price: Some(Price::new(50000.0, 1)),
            trigger_price: None,
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
        };

        let params = build_amend_order_params(&cmd, "TOKEN".to_string());

        assert_eq!(params.order_id.as_deref(), Some("OABCDE-12345-FGHIJ"));
        assert!(params.cl_ord_id.is_none());
        assert!((params.order_qty.unwrap() - 0.1).abs() < 1e-10);
        assert!((params.limit_price.unwrap() - 50000.0).abs() < 1e-10);
        assert!(params.trigger_price.is_none());
    }

    #[rstest]
    fn test_build_amend_order_params_falls_back_to_client_id() {
        use nautilus_model::types::Quantity;

        let cmd = ModifyOrder {
            trader_id: TraderId::from("TESTER-001"),
            client_id: None,
            strategy_id: StrategyId::from("S-001"),
            instrument_id: InstrumentId::from("XBT/USD.KRAKEN"),
            client_order_id: ClientOrderId::from("O-001"),
            venue_order_id: None,
            quantity: Some(Quantity::new(0.2, 1)),
            price: None,
            trigger_price: None,
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
        };

        let params = build_amend_order_params(&cmd, "TOKEN".to_string());

        assert!(params.order_id.is_none());
        assert_eq!(params.cl_ord_id.as_deref(), Some("O-001"));
    }
}
