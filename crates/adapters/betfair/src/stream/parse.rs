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

//! Parsing utilities that convert Betfair stream messages into Nautilus domain models.
//!
//! MCM (Market Change Messages) are parsed into order book deltas, trade ticks,
//! and instrument status updates. OCM (Order Change Messages) are parsed into
//! order status reports and fill reports.

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, InstrumentStatus, OrderBookDelta, OrderBookDeltas, TradeTick},
    enums::{
        AggressorSide, BookAction, LiquiditySide, MarketStatusAction, OrderSide, OrderType,
        RecordFlag, TimeInForce,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::{
        consts::{BETFAIR_PRICE_PRECISION, BETFAIR_QUANTITY_PRECISION},
        enums::{MarketStatus, resolve_streaming_order_status},
        parse::parse_millis_timestamp,
    },
    stream::messages::{RunnerChange, UnmatchedOrder},
};

/// Parses a single runner's book data into [`OrderBookDeltas`].
///
/// Handles both full image snapshots (`is_snapshot = true`) and delta updates.
///
/// Only processes price-keyed fields (`atb`/`atl`). Level-indexed fields
/// (`batb`/`batl`/`bdatb`/`bdatl`) are ignored because correct delta
/// processing requires stateful level-to-price tracking. The stream
/// subscription should use `EX_ALL_OFFERS` which populates `atb`/`atl`.
///
/// Book side mapping (Betfair exchange convention):
/// - `atb` (available to back) -> [`OrderSide::Buy`] (bid side)
/// - `atl` (available to lay) -> [`OrderSide::Sell`] (ask side)
///
/// For snapshots: emits a Clear delta followed by Add deltas for each level.
/// For updates: emits Update or Delete (when volume is zero) deltas.
///
/// Returns `Ok(None)` if the runner change contains no processable book data.
///
/// # Errors
///
/// Returns an error if price or quantity values cannot be converted.
pub fn parse_runner_book_deltas(
    instrument_id: InstrumentId,
    rc: &RunnerChange,
    is_snapshot: bool,
    sequence: u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<OrderBookDeltas>> {
    let atb_len = rc.atb.as_ref().map_or(0, Vec::len);
    let atl_len = rc.atl.as_ref().map_or(0, Vec::len);
    let total_levels = atb_len + atl_len;

    if total_levels == 0 && !is_snapshot {
        return Ok(None);
    }

    let snapshot_flags = if is_snapshot {
        RecordFlag::F_SNAPSHOT as u8
    } else {
        0
    };
    let mut deltas = Vec::with_capacity(total_levels + usize::from(is_snapshot));

    if is_snapshot {
        let mut clear = OrderBookDelta::clear(instrument_id, sequence, ts_event, ts_init);

        if total_levels == 0 {
            clear.flags |= RecordFlag::F_LAST as u8;
        }
        deltas.push(clear);
    }

    // Buy side (bid): atb (available to back) is price-keyed
    for pv in rc.atb.as_deref().unwrap_or(&[]) {
        let action = if is_snapshot {
            BookAction::Add
        } else if pv.volume == Decimal::ZERO {
            BookAction::Delete
        } else {
            BookAction::Update
        };

        deltas.push(OrderBookDelta::new(
            instrument_id,
            action,
            BookOrder::new(
                OrderSide::Buy,
                Price::from_decimal_dp(pv.price, BETFAIR_PRICE_PRECISION)?,
                Quantity::from_decimal_dp(pv.volume, BETFAIR_QUANTITY_PRECISION)?,
                0,
            ),
            snapshot_flags,
            sequence,
            ts_event,
            ts_init,
        ));
    }

    // Sell side (ask): atl (available to lay) is price-keyed
    for pv in rc.atl.as_deref().unwrap_or(&[]) {
        let action = if is_snapshot {
            BookAction::Add
        } else if pv.volume == Decimal::ZERO {
            BookAction::Delete
        } else {
            BookAction::Update
        };

        deltas.push(OrderBookDelta::new(
            instrument_id,
            action,
            BookOrder::new(
                OrderSide::Sell,
                Price::from_decimal_dp(pv.price, BETFAIR_PRICE_PRECISION)?,
                Quantity::from_decimal_dp(pv.volume, BETFAIR_QUANTITY_PRECISION)?,
                0,
            ),
            snapshot_flags,
            sequence,
            ts_event,
            ts_init,
        ));
    }

    // Set F_LAST on the final delta
    if let Some(last) = deltas.last_mut() {
        last.flags |= RecordFlag::F_LAST as u8;
    }

    Ok(Some(OrderBookDeltas::new(instrument_id, deltas)))
}

/// Creates a [`TradeTick`] from stream data.
///
/// Betfair does not identify the aggressor side in its stream, so
/// [`AggressorSide::NoAggressor`] is always used.
#[must_use]
pub fn make_trade_tick(
    instrument_id: InstrumentId,
    price: Price,
    size: Quantity,
    trade_id: TradeId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> TradeTick {
    TradeTick::new(
        instrument_id,
        price,
        size,
        AggressorSide::NoAggressor,
        trade_id,
        ts_event,
        ts_init,
    )
}

/// Converts a Betfair [`MarketStatus`] into a Nautilus [`InstrumentStatus`].
#[must_use]
pub fn parse_instrument_status(
    instrument_id: InstrumentId,
    status: MarketStatus,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> InstrumentStatus {
    let action = match status {
        MarketStatus::Open => MarketStatusAction::Trading,
        MarketStatus::Closed => MarketStatusAction::Close,
        MarketStatus::Suspended => MarketStatusAction::Suspend,
        MarketStatus::Inactive => MarketStatusAction::NotAvailableForTrading,
    };

    let is_trading = matches!(status, MarketStatus::Open);

    InstrumentStatus::new(
        instrument_id,
        action,
        ts_event,
        ts_init,
        None,
        None,
        Some(is_trading),
        None,
        None,
    )
}

/// Parses a streaming [`UnmatchedOrder`] into a Nautilus [`OrderStatusReport`].
///
/// Resolves the Nautilus order status from the Betfair streaming status
/// plus matched/cancelled quantities.
///
/// # Errors
///
/// Returns an error if price or quantity values cannot be converted.
pub fn parse_order_status_report(
    uo: &UnmatchedOrder,
    instrument_id: InstrumentId,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let order_side = OrderSide::from(uo.side);
    let order_type = OrderType::from(uo.ot);
    let time_in_force = TimeInForce::from(uo.pt);

    let size_matched = uo.sm.unwrap_or(Decimal::ZERO);
    let size_cancelled = uo.sc.unwrap_or(Decimal::ZERO);
    let size_lapsed = uo.sl.unwrap_or(Decimal::ZERO);
    let size_voided = uo.sv.unwrap_or(Decimal::ZERO);

    // Include lapsed/voided in the closed quantity for status resolution
    let size_closed = size_cancelled + size_lapsed + size_voided;
    let order_status = resolve_streaming_order_status(uo.status, size_matched, size_closed);

    let quantity = Quantity::from_decimal_dp(uo.s, BETFAIR_QUANTITY_PRECISION)?;
    let filled_qty = Quantity::from_decimal_dp(size_matched, BETFAIR_QUANTITY_PRECISION)?;

    let ts_accepted = parse_millis_timestamp(uo.pd);

    // Use the latest lifecycle timestamp, falling back to OCM publish time
    let ts_last = [uo.md, uo.cd, uo.ld]
        .into_iter()
        .flatten()
        .max()
        .map_or(ts_event, parse_millis_timestamp);

    let venue_order_id = VenueOrderId::from(uo.id.as_str());
    let client_order_id = uo.rfo.as_deref().map(ClientOrderId::from);

    let price = Price::from_decimal_dp(uo.p, BETFAIR_PRICE_PRECISION)?;

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_accepted,
        ts_last,
        ts_init,
        None,
    )
    .with_price(price);

    report.avg_px = uo.avp;

    Ok(report)
}

/// Creates a [`FillReport`] for a Betfair order fill.
///
/// Betfair charges commission on net winnings, not per-fill, so commission
/// is set to zero. The `liquidity_side` is unknown from the stream.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn make_fill_report(
    account_id: AccountId,
    instrument_id: InstrumentId,
    venue_order_id: VenueOrderId,
    trade_id: TradeId,
    order_side: OrderSide,
    last_qty: Quantity,
    last_px: Price,
    currency: Currency,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> FillReport {
    FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        Money::new(0.0, currency),
        LiquiditySide::NoLiquiditySide,
        None,
        None,
        ts_event,
        ts_init,
        None,
    )
}

#[cfg(test)]
mod tests {
    use nautilus_model::enums::{MarketStatusAction, OrderStatus};
    use rstest::rstest;

    use super::*;
    use crate::{
        common::{parse::make_instrument_id, testing::load_test_json},
        stream::messages::StreamMessage,
    };

    #[rstest]
    fn test_parse_runner_book_snapshot() {
        let data = load_test_json("stream/mcm_live_IMAGE.json");
        let msg: StreamMessage = serde_json::from_str(&data).unwrap();

        if let StreamMessage::MarketChange(mcm) = msg {
            let mc = mcm.mc.as_ref().unwrap();
            let change = &mc[0];
            let rc = &change.rc.as_ref().unwrap()[0];
            let instrument_id = make_instrument_id(&change.id, rc.id, Decimal::ZERO);

            let deltas = parse_runner_book_deltas(
                instrument_id,
                rc,
                true,
                mcm.pt,
                parse_millis_timestamp(mcm.pt),
                parse_millis_timestamp(mcm.pt),
            )
            .unwrap()
            .expect("should produce deltas");

            // Clear + atb levels + atl levels
            let atb_len = rc.atb.as_ref().unwrap().len();
            let atl_len = rc.atl.as_ref().unwrap().len();
            assert_eq!(deltas.deltas.len(), 1 + atb_len + atl_len);

            // First delta is Clear with F_SNAPSHOT
            assert_eq!(deltas.deltas[0].action, BookAction::Clear);
            assert!(RecordFlag::F_SNAPSHOT.matches(deltas.deltas[0].flags));

            // Subsequent deltas are Add with F_SNAPSHOT
            for delta in &deltas.deltas[1..] {
                assert_eq!(delta.action, BookAction::Add);
                assert!(RecordFlag::F_SNAPSHOT.matches(delta.flags));
            }

            // Last delta has F_LAST
            let last = deltas.deltas.last().unwrap();
            assert!(RecordFlag::F_LAST.matches(last.flags));

            // Verify buy/sell sides
            let buy_count = deltas
                .deltas
                .iter()
                .filter(|d| d.order.side == OrderSide::Buy)
                .count();
            let sell_count = deltas
                .deltas
                .iter()
                .filter(|d| d.order.side == OrderSide::Sell)
                .count();
            assert_eq!(buy_count, atb_len);
            assert_eq!(sell_count, atl_len);
        } else {
            panic!("expected MarketChange");
        }
    }

    #[rstest]
    fn test_parse_runner_book_update() {
        let data = load_test_json("stream/mcm_UPDATE.json");
        let msg: StreamMessage = serde_json::from_str(&data).unwrap();

        if let StreamMessage::MarketChange(mcm) = msg {
            let mc = mcm.mc.as_ref().unwrap();
            let change = &mc[0];
            let rc = &change.rc.as_ref().unwrap()[0];
            let instrument_id = make_instrument_id(&change.id, rc.id, Decimal::ZERO);

            let deltas = parse_runner_book_deltas(
                instrument_id,
                rc,
                false,
                mcm.pt,
                parse_millis_timestamp(mcm.pt),
                parse_millis_timestamp(mcm.pt),
            )
            .unwrap()
            .expect("should produce deltas");

            // No Clear delta for updates
            assert!(deltas.deltas.iter().all(|d| d.action != BookAction::Clear));

            // Last delta has F_LAST
            let last = deltas.deltas.last().unwrap();
            assert!(RecordFlag::F_LAST.matches(last.flags));

            // No snapshot flags
            for delta in &deltas.deltas {
                assert!(!RecordFlag::F_SNAPSHOT.matches(delta.flags));
            }
        } else {
            panic!("expected MarketChange");
        }
    }

    #[rstest]
    fn test_parse_runner_book_update_zero_volume_is_delete() {
        let data = load_test_json("stream/mcm_UPDATE.json");
        let msg: StreamMessage = serde_json::from_str(&data).unwrap();

        if let StreamMessage::MarketChange(mcm) = msg {
            let mc = mcm.mc.as_ref().unwrap();
            let change = &mc[0];
            let rc = &change.rc.as_ref().unwrap()[0];
            let instrument_id = make_instrument_id(&change.id, rc.id, Decimal::ZERO);

            let deltas = parse_runner_book_deltas(
                instrument_id,
                rc,
                false,
                mcm.pt,
                parse_millis_timestamp(mcm.pt),
                parse_millis_timestamp(mcm.pt),
            )
            .unwrap()
            .unwrap();

            // atl has [[4.7, 0]] which should be Delete
            assert!(
                deltas.deltas.iter().any(|d| d.action == BookAction::Delete),
                "zero volume should produce Delete action"
            );
        } else {
            panic!("expected MarketChange");
        }
    }

    #[rstest]
    fn test_parse_runner_book_no_levels_returns_none() {
        let rc = RunnerChange {
            id: 12345,
            hc: None,
            atb: None,
            atl: None,
            batb: None,
            batl: None,
            bdatb: None,
            bdatl: None,
            spb: None,
            spl: None,
            spn: None,
            spf: None,
            trd: None,
            ltp: None,
            tv: None,
        };

        let result = parse_runner_book_deltas(
            make_instrument_id("1.123", 12345, Decimal::ZERO),
            &rc,
            false,
            0,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(result.unwrap().is_none());
    }

    #[rstest]
    fn test_make_trade_tick() {
        let instrument_id = make_instrument_id("1.180737206", 19248890, Decimal::ZERO);
        let tick = make_trade_tick(
            instrument_id,
            Price::new(2.42, BETFAIR_PRICE_PRECISION),
            Quantity::new(100.0, BETFAIR_QUANTITY_PRECISION),
            TradeId::from("test-trade-1"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert_eq!(tick.instrument_id, instrument_id);
        assert_eq!(tick.price.as_f64(), 2.42);
        assert_eq!(tick.size.as_f64(), 100.0);
        assert_eq!(tick.aggressor_side, AggressorSide::NoAggressor);
    }

    #[rstest]
    #[case(MarketStatus::Open, MarketStatusAction::Trading, true)]
    #[case(MarketStatus::Closed, MarketStatusAction::Close, false)]
    #[case(MarketStatus::Suspended, MarketStatusAction::Suspend, false)]
    #[case(
        MarketStatus::Inactive,
        MarketStatusAction::NotAvailableForTrading,
        false
    )]
    fn test_parse_instrument_status(
        #[case] status: MarketStatus,
        #[case] expected_action: MarketStatusAction,
        #[case] expected_is_trading: bool,
    ) {
        let instrument_id = make_instrument_id("1.123", 456, Decimal::ZERO);
        let result = parse_instrument_status(
            instrument_id,
            status,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert_eq!(result.action, expected_action);
        assert_eq!(result.is_trading, Some(expected_is_trading));
    }

    #[rstest]
    fn test_parse_order_status_report_new_order() {
        let data = load_test_json("stream/ocm_NEW_FULL_IMAGE.json");
        let msg: StreamMessage = serde_json::from_str(&data).unwrap();

        if let StreamMessage::OrderChange(ocm) = msg {
            let oc = ocm.oc.as_ref().unwrap();
            let omc = &oc[0];
            let orc = &omc.orc.as_ref().unwrap()[0];
            let uo = &orc.uo.as_ref().unwrap()[0];

            let instrument_id = make_instrument_id(&omc.id, orc.id, Decimal::ZERO);
            let report = parse_order_status_report(
                uo,
                instrument_id,
                AccountId::from("BETFAIR-001"),
                parse_millis_timestamp(ocm.pt),
                parse_millis_timestamp(ocm.pt),
            )
            .unwrap();

            // Partially filled: sm=4.75, sr=0.25, status=E
            assert_eq!(report.order_status, OrderStatus::PartiallyFilled);
            assert_eq!(report.order_side, OrderSide::Sell); // Back → Sell
            assert_eq!(report.order_type, OrderType::Limit);
            assert_eq!(report.filled_qty.as_f64(), 4.75);
            assert_eq!(report.quantity.as_f64(), 5.0);
            assert!(report.price.is_some());
            assert_eq!(report.price.unwrap().as_f64(), 12.0);
        } else {
            panic!("expected OrderChange");
        }
    }

    #[rstest]
    fn test_parse_order_status_report_filled() {
        let data = load_test_json("stream/ocm_FILLED.json");
        let msg: StreamMessage = serde_json::from_str(&data).unwrap();

        if let StreamMessage::OrderChange(ocm) = msg {
            let oc = ocm.oc.as_ref().unwrap();
            let omc = &oc[0];
            let orc = &omc.orc.as_ref().unwrap()[0];
            let uo = &orc.uo.as_ref().unwrap()[0];

            let instrument_id = make_instrument_id(&omc.id, orc.id, Decimal::ZERO);
            let report = parse_order_status_report(
                uo,
                instrument_id,
                AccountId::from("BETFAIR-001"),
                parse_millis_timestamp(ocm.pt),
                parse_millis_timestamp(ocm.pt),
            )
            .unwrap();

            assert_eq!(report.order_status, OrderStatus::Filled);
            assert_eq!(report.order_side, OrderSide::Buy); // Lay → Buy
            assert_eq!(report.filled_qty.as_f64(), 10.0);
            assert_eq!(report.quantity.as_f64(), 10.0);

            // Has client_order_id from rfo field
            assert!(report.client_order_id.is_some());
        } else {
            panic!("expected OrderChange");
        }
    }

    #[rstest]
    fn test_parse_order_status_report_cancelled() {
        let data = load_test_json("stream/ocm_CANCEL.json");
        let msg: StreamMessage = serde_json::from_str(&data).unwrap();

        if let StreamMessage::OrderChange(ocm) = msg {
            let oc = ocm.oc.as_ref().unwrap();
            let omc = &oc[0];
            let orc = &omc.orc.as_ref().unwrap()[0];
            let uo = &orc.uo.as_ref().unwrap()[0];

            let instrument_id = make_instrument_id(&omc.id, orc.id, Decimal::ZERO);
            let report = parse_order_status_report(
                uo,
                instrument_id,
                AccountId::from("BETFAIR-001"),
                parse_millis_timestamp(ocm.pt),
                parse_millis_timestamp(ocm.pt),
            )
            .unwrap();

            assert_eq!(report.order_status, OrderStatus::Canceled);
            assert_eq!(report.order_side, OrderSide::Sell); // Back → Sell
            assert_eq!(report.filled_qty.as_f64(), 0.0);
            assert_eq!(report.quantity.as_f64(), 10.0);
        } else {
            panic!("expected OrderChange");
        }
    }

    #[rstest]
    fn test_make_fill_report() {
        let instrument_id = make_instrument_id("1.180604981", 1209555, Decimal::ZERO);
        let fill = make_fill_report(
            AccountId::from("BETFAIR-001"),
            instrument_id,
            VenueOrderId::from("229430281339"),
            TradeId::from("229430281339-0"),
            OrderSide::Buy,
            Quantity::new(10.0, BETFAIR_QUANTITY_PRECISION),
            Price::new(1.1, BETFAIR_PRICE_PRECISION),
            Currency::GBP(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert_eq!(fill.instrument_id, instrument_id);
        assert_eq!(fill.order_side, OrderSide::Buy);
        assert_eq!(fill.last_qty.as_f64(), 10.0);
        assert_eq!(fill.last_px.as_f64(), 1.1);
        assert_eq!(fill.commission.as_f64(), 0.0);
        assert_eq!(fill.liquidity_side, LiquiditySide::NoLiquiditySide);
    }
}
