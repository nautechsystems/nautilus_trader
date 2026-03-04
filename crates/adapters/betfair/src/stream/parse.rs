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

use ahash::{AHashMap, AHashSet};
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
        enums::{MarketStatus, StreamingOrderStatus, resolve_streaming_order_status},
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

/// Converts a Betfair [`MarketStatus`] and `in_play` flag into a Nautilus [`InstrumentStatus`].
///
/// The `in_play` flag distinguishes pre-open (Open + not in play) from active
/// trading (Open + in play), matching Betfair's market lifecycle.
#[must_use]
pub fn parse_instrument_status(
    instrument_id: InstrumentId,
    status: MarketStatus,
    in_play: bool,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> InstrumentStatus {
    let action = match (status, in_play) {
        (MarketStatus::Inactive, _) => MarketStatusAction::Close,
        (MarketStatus::Open, false) => MarketStatusAction::PreOpen,
        (MarketStatus::Open, true) => MarketStatusAction::Trading,
        (MarketStatus::Suspended, _) => MarketStatusAction::Pause,
        (MarketStatus::Closed, _) => MarketStatusAction::Close,
    };

    let is_trading = matches!(status, MarketStatus::Open) && in_play;

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

/// Generates a deterministic [`TradeId`] for a Betfair fill.
///
/// Uses `bet_id` and cumulative `sm` (size matched) which together uniquely
/// identify each fill state, since `sm` increases monotonically with each fill.
pub fn make_trade_id(uo: &UnmatchedOrder) -> TradeId {
    let sm = uo.sm.unwrap_or(Decimal::ZERO);
    TradeId::new(format!("{}-{sm}", uo.id))
}

/// Tracks cumulative fill state per bet to compute incremental fills from the
/// Betfair OCM stream.
///
/// Betfair provides cumulative `sm` (size matched) and `avp` (average price
/// matched) on each order update. This tracker maintains per-bet state to
/// derive individual fill quantities and prices for each update.
#[derive(Debug, Default)]
pub struct FillTracker {
    filled_qty: AHashMap<String, Decimal>,
    avg_px: AHashMap<String, Decimal>,
    published_trade_ids: AHashSet<String>,
}

impl FillTracker {
    /// Creates a new [`FillTracker`] instance.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Computes an incremental [`FillReport`] for an unmatched order update.
    ///
    /// Returns `None` if no new fill occurred (size matched unchanged,
    /// duplicate trade ID, or overfill detected).
    #[allow(clippy::too_many_arguments)]
    pub fn maybe_fill_report(
        &mut self,
        uo: &UnmatchedOrder,
        order_qty: Decimal,
        instrument_id: InstrumentId,
        account_id: AccountId,
        currency: Currency,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Option<FillReport> {
        let sm = uo.sm?;

        if sm <= Decimal::ZERO {
            return None;
        }

        let prev_filled = self
            .filled_qty
            .get(&uo.id)
            .copied()
            .unwrap_or(Decimal::ZERO);

        if sm <= prev_filled {
            return None;
        }

        // Overfill guard
        if sm > order_qty {
            log::warn!(
                "Rejecting potential overfill for bet_id={}: order_qty={order_qty}, sm={sm}",
                uo.id,
            );
            return None;
        }

        let trade_id = make_trade_id(uo);

        if self.published_trade_ids.contains(trade_id.as_str()) {
            return None;
        }

        let fill_qty_dec = sm - prev_filled;
        let fill_price = self.compute_fill_price(uo, prev_filled);

        let last_qty = Quantity::from_decimal_dp(fill_qty_dec, BETFAIR_QUANTITY_PRECISION).ok()?;
        let last_px = Price::from_decimal_dp(fill_price, BETFAIR_PRICE_PRECISION).ok()?;

        // Update state before emitting
        self.filled_qty.insert(uo.id.clone(), sm);

        if let Some(avp) = uo.avp {
            self.avg_px.insert(uo.id.clone(), avp);
        }

        self.published_trade_ids.insert(trade_id.to_string());

        let venue_order_id = VenueOrderId::from(uo.id.as_str());
        let order_side = OrderSide::from(uo.side);
        let client_order_id = uo.rfo.as_deref().map(ClientOrderId::from);
        let ts_fill = uo.md.map_or(ts_event, parse_millis_timestamp);

        Some(make_fill_report(
            account_id,
            instrument_id,
            venue_order_id,
            trade_id,
            order_side,
            last_qty,
            last_px,
            currency,
            client_order_id,
            ts_fill,
            ts_init,
        ))
    }

    /// Back-calculates the individual fill price from Betfair's cumulative
    /// average price matched (`avp`).
    ///
    /// For the first fill, the average price IS the fill price. For subsequent
    /// fills, the individual price is derived from:
    /// `fill_price = (avp * sm - prev_avp * prev_sm) / fill_size`
    fn compute_fill_price(&self, uo: &UnmatchedOrder, prev_filled: Decimal) -> Decimal {
        let Some(avp) = uo.avp else {
            return uo.p;
        };

        if prev_filled == Decimal::ZERO {
            return avp;
        }

        let Some(prev_avg) = self.avg_px.get(&uo.id).copied() else {
            return avp;
        };

        if prev_avg == avp {
            return avp;
        }

        let sm = uo.sm.unwrap_or(Decimal::ZERO);
        let fill_size = sm - prev_filled;

        if fill_size == Decimal::ZERO {
            return prev_avg;
        }

        let fill_price = (avp * sm - prev_avg * prev_filled) / fill_size;

        if fill_price <= Decimal::ZERO {
            log::warn!(
                "Calculated fill price {fill_price} is invalid for bet_id={}, falling back to avp={avp}",
                uo.id,
            );
            return avp;
        }

        fill_price
    }

    /// Removes state for a completed bet to prevent unbounded growth.
    pub fn prune(&mut self, bet_id: &str) {
        self.filled_qty.remove(bet_id);
        self.avg_px.remove(bet_id);

        let prefix = format!("{bet_id}-");
        self.published_trade_ids
            .retain(|id| !id.starts_with(&prefix));
    }
}

/// Returns `true` if the unmatched order has cancel, lapse, or void quantities.
#[must_use]
pub fn has_cancel_quantity(uo: &UnmatchedOrder) -> bool {
    let sc = uo.sc.unwrap_or(Decimal::ZERO);
    let sl = uo.sl.unwrap_or(Decimal::ZERO);
    let sv = uo.sv.unwrap_or(Decimal::ZERO);
    (sc + sl + sv) > Decimal::ZERO
}

/// Returns `true` if the order is execution-complete and has lapsed.
#[must_use]
pub fn is_lapsed(uo: &UnmatchedOrder) -> bool {
    uo.status == StreamingOrderStatus::ExecutionComplete && uo.lsrc.is_some()
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
    client_order_id: Option<ClientOrderId>,
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
        client_order_id,
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
    #[case(MarketStatus::Open, false, MarketStatusAction::PreOpen, false)]
    #[case(MarketStatus::Open, true, MarketStatusAction::Trading, true)]
    #[case(MarketStatus::Closed, false, MarketStatusAction::Close, false)]
    #[case(MarketStatus::Closed, true, MarketStatusAction::Close, false)]
    #[case(MarketStatus::Suspended, false, MarketStatusAction::Pause, false)]
    #[case(MarketStatus::Suspended, true, MarketStatusAction::Pause, false)]
    #[case(MarketStatus::Inactive, false, MarketStatusAction::Close, false)]
    fn test_parse_instrument_status(
        #[case] status: MarketStatus,
        #[case] in_play: bool,
        #[case] expected_action: MarketStatusAction,
        #[case] expected_is_trading: bool,
    ) {
        let instrument_id = make_instrument_id("1.123", 456, Decimal::ZERO);
        let result = parse_instrument_status(
            instrument_id,
            status,
            in_play,
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
    fn test_parse_order_status_report_duplicate_execution() {
        let data = load_test_json("stream/ocm_DUPLICATE_EXECUTION.json");
        let msgs: Vec<StreamMessage> = serde_json::from_str(&data).unwrap();

        if let StreamMessage::OrderChange(ocm) = &msgs[0] {
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

            // Partially filled: sm=1.12, status=E
            assert_eq!(report.order_status, OrderStatus::PartiallyFilled);
            assert_eq!(report.order_side, OrderSide::Buy); // Lay → Buy
            assert_eq!(report.filled_qty.as_f64(), 1.12);
        } else {
            panic!("expected OrderChange");
        }
    }

    #[rstest]
    fn test_parse_order_status_report_multiple_fills() {
        let data = load_test_json("stream/ocm_multiple_fills.json");
        let msgs: Vec<StreamMessage> = serde_json::from_str(&data).unwrap();

        if let StreamMessage::OrderChange(ocm) = &msgs[0] {
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

            // Partially filled: sm=16.19, status=E, has rfo
            assert_eq!(report.order_status, OrderStatus::PartiallyFilled);
            assert_eq!(report.order_side, OrderSide::Sell); // Back → Sell
            assert_eq!(report.filled_qty.as_f64(), 16.19);
            assert!(report.client_order_id.is_some());
            assert!(report.avg_px.is_some());
        } else {
            panic!("expected OrderChange");
        }
    }

    #[rstest]
    fn test_parse_runner_book_snapshot_empty_book() {
        // A snapshot with no levels should still produce a clear delta
        let rc = RunnerChange {
            id: 12345,
            hc: None,
            atb: Some(vec![]),
            atl: Some(vec![]),
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
            true,
            1000,
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap()
        .expect("should produce snapshot deltas");

        // Just the Clear delta with F_LAST
        assert_eq!(result.deltas.len(), 1);
        assert_eq!(result.deltas[0].action, BookAction::Clear);
        assert!(RecordFlag::F_LAST.matches(result.deltas[0].flags));
        assert!(RecordFlag::F_SNAPSHOT.matches(result.deltas[0].flags));
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
            None,
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

    #[rstest]
    fn test_fill_tracker_single_full_fill() {
        let data = load_test_json("stream/ocm_FILLED.json");
        let msg: StreamMessage = serde_json::from_str(&data).unwrap();

        if let StreamMessage::OrderChange(ocm) = msg {
            let oc = ocm.oc.as_ref().unwrap();
            let omc = &oc[0];
            let orc = &omc.orc.as_ref().unwrap()[0];
            let uo = &orc.uo.as_ref().unwrap()[0];
            let instrument_id = make_instrument_id(&omc.id, orc.id, Decimal::ZERO);
            let ts = parse_millis_timestamp(ocm.pt);

            let mut tracker = FillTracker::new();
            let fill = tracker
                .maybe_fill_report(
                    uo,
                    uo.s,
                    instrument_id,
                    AccountId::from("BETFAIR-001"),
                    Currency::GBP(),
                    ts,
                    ts,
                )
                .expect("should produce fill");

            assert_eq!(fill.last_qty.as_f64(), 10.0);
            assert_eq!(fill.last_px.as_f64(), 1.1);
            assert_eq!(fill.order_side, OrderSide::Buy);
            assert!(fill.client_order_id.is_some());
        } else {
            panic!("expected OrderChange");
        }
    }

    #[rstest]
    fn test_fill_tracker_incremental_fills() {
        let data = load_test_json("stream/ocm_multiple_fills.json");
        let msgs: Vec<StreamMessage> = serde_json::from_str(&data).unwrap();
        let instrument_id = make_instrument_id("1.179082386", 50210, Decimal::ZERO);

        let mut tracker = FillTracker::new();
        let account_id = AccountId::from("BETFAIR-001");
        let currency = Currency::GBP();

        // First fill: sm=16.19 (from zero)
        let uo1 = extract_uo(&msgs[0]);
        let ts1 = extract_ts(&msgs[0]);
        let fill1 = tracker
            .maybe_fill_report(uo1, uo1.s, instrument_id, account_id, currency, ts1, ts1)
            .expect("should produce first fill");
        assert_eq!(fill1.last_qty.as_f64(), 16.19);
        assert_eq!(fill1.last_px.as_f64(), 5.8);

        // Second fill: sm=16.96 (delta=0.77)
        let uo2 = extract_uo(&msgs[1]);
        let ts2 = extract_ts(&msgs[1]);
        let fill2 = tracker
            .maybe_fill_report(uo2, uo2.s, instrument_id, account_id, currency, ts2, ts2)
            .expect("should produce second fill");
        assert_eq!(fill2.last_qty.as_f64(), 0.77);
        assert_eq!(fill2.last_px.as_f64(), 5.8);

        // Third fill: sm=17.73 (delta=0.77)
        let uo3 = extract_uo(&msgs[2]);
        let ts3 = extract_ts(&msgs[2]);
        let fill3 = tracker
            .maybe_fill_report(uo3, uo3.s, instrument_id, account_id, currency, ts3, ts3)
            .expect("should produce third fill");
        assert_eq!(fill3.last_qty.as_f64(), 0.77);
    }

    #[rstest]
    fn test_fill_tracker_different_price() {
        let data = load_test_json("stream/ocm_filled_different_price.json");
        let msg: StreamMessage = serde_json::from_str(&data).unwrap();

        if let StreamMessage::OrderChange(ocm) = msg {
            let oc = ocm.oc.as_ref().unwrap();
            let omc = &oc[0];
            let orc = &omc.orc.as_ref().unwrap()[0];
            let uo = &orc.uo.as_ref().unwrap()[0];
            let instrument_id = make_instrument_id(&omc.id, orc.id, Decimal::ZERO);
            let ts = parse_millis_timestamp(ocm.pt);

            let mut tracker = FillTracker::new();
            let fill = tracker
                .maybe_fill_report(
                    uo,
                    uo.s,
                    instrument_id,
                    AccountId::from("BETFAIR-001"),
                    Currency::GBP(),
                    ts,
                    ts,
                )
                .expect("should produce fill");

            // Order placed at 1.3 but average fill price is 1.2
            assert_eq!(fill.last_qty.as_f64(), 20.0);
            assert_eq!(fill.last_px.as_f64(), 1.2);
        } else {
            panic!("expected OrderChange");
        }
    }

    #[rstest]
    fn test_fill_tracker_cancel_no_fill() {
        let data = load_test_json("stream/ocm_CANCEL.json");
        let msg: StreamMessage = serde_json::from_str(&data).unwrap();

        if let StreamMessage::OrderChange(ocm) = msg {
            let oc = ocm.oc.as_ref().unwrap();
            let omc = &oc[0];
            let orc = &omc.orc.as_ref().unwrap()[0];
            let uo = &orc.uo.as_ref().unwrap()[0];
            let instrument_id = make_instrument_id(&omc.id, orc.id, Decimal::ZERO);
            let ts = parse_millis_timestamp(ocm.pt);

            let mut tracker = FillTracker::new();
            let result = tracker.maybe_fill_report(
                uo,
                uo.s,
                instrument_id,
                AccountId::from("BETFAIR-001"),
                Currency::GBP(),
                ts,
                ts,
            );
            assert!(result.is_none(), "cancelled order should not produce fill");
        } else {
            panic!("expected OrderChange");
        }
    }

    #[rstest]
    fn test_fill_tracker_lapsed_no_fill() {
        let data = load_test_json("stream/ocm_error_fill.json");
        let msg: StreamMessage = serde_json::from_str(&data).unwrap();

        if let StreamMessage::OrderChange(ocm) = msg {
            let oc = ocm.oc.as_ref().unwrap();
            let omc = &oc[0];
            let orc = &omc.orc.as_ref().unwrap()[0];
            let uo = &orc.uo.as_ref().unwrap()[0];
            let instrument_id = make_instrument_id(&omc.id, orc.id, Decimal::ZERO);
            let ts = parse_millis_timestamp(ocm.pt);

            let mut tracker = FillTracker::new();
            let result = tracker.maybe_fill_report(
                uo,
                uo.s,
                instrument_id,
                AccountId::from("BETFAIR-001"),
                Currency::GBP(),
                ts,
                ts,
            );
            assert!(result.is_none(), "lapsed order should not produce fill");
        } else {
            panic!("expected OrderChange");
        }
    }

    #[rstest]
    fn test_fill_tracker_duplicate_dedup() {
        let data = load_test_json("stream/ocm_FILLED.json");
        let msg: StreamMessage = serde_json::from_str(&data).unwrap();

        if let StreamMessage::OrderChange(ocm) = msg {
            let oc = ocm.oc.as_ref().unwrap();
            let omc = &oc[0];
            let orc = &omc.orc.as_ref().unwrap()[0];
            let uo = &orc.uo.as_ref().unwrap()[0];
            let instrument_id = make_instrument_id(&omc.id, orc.id, Decimal::ZERO);
            let ts = parse_millis_timestamp(ocm.pt);
            let account_id = AccountId::from("BETFAIR-001");
            let currency = Currency::GBP();

            let mut tracker = FillTracker::new();

            // First call produces fill
            let fill1 =
                tracker.maybe_fill_report(uo, uo.s, instrument_id, account_id, currency, ts, ts);
            assert!(fill1.is_some());

            // Second call with same data produces nothing (dedup)
            let fill2 =
                tracker.maybe_fill_report(uo, uo.s, instrument_id, account_id, currency, ts, ts);
            assert!(fill2.is_none(), "duplicate fill should be suppressed");
        } else {
            panic!("expected OrderChange");
        }
    }

    #[rstest]
    fn test_fill_tracker_price_back_calculation() {
        let data = load_test_json("stream/ocm_multiple_fills.json");
        let msgs: Vec<StreamMessage> = serde_json::from_str(&data).unwrap();
        let instrument_id = make_instrument_id("1.179082386", 50210, Decimal::ZERO);
        let account_id = AccountId::from("BETFAIR-001");
        let currency = Currency::GBP();
        let mut tracker = FillTracker::new();

        // Process first fill at avp=5.8
        let uo1 = extract_uo(&msgs[0]);
        let ts1 = extract_ts(&msgs[0]);
        let fill1 = tracker
            .maybe_fill_report(uo1, uo1.s, instrument_id, account_id, currency, ts1, ts1)
            .unwrap();
        assert_eq!(fill1.last_px.as_f64(), 5.8);

        // Second fill also at avp=5.8 (same price, avg unchanged)
        let uo2 = extract_uo(&msgs[1]);
        let ts2 = extract_ts(&msgs[1]);
        let fill2 = tracker
            .maybe_fill_report(uo2, uo2.s, instrument_id, account_id, currency, ts2, ts2)
            .unwrap();
        assert_eq!(fill2.last_px.as_f64(), 5.8);
    }

    #[rstest]
    fn test_has_cancel_quantity() {
        let data = load_test_json("stream/ocm_CANCEL.json");
        let msg: StreamMessage = serde_json::from_str(&data).unwrap();

        if let StreamMessage::OrderChange(ocm) = msg {
            let uo = &ocm.oc.as_ref().unwrap()[0].orc.as_ref().unwrap()[0]
                .uo
                .as_ref()
                .unwrap()[0];
            assert!(has_cancel_quantity(uo));
        } else {
            panic!("expected OrderChange");
        }
    }

    #[rstest]
    fn test_has_cancel_quantity_filled_order() {
        let data = load_test_json("stream/ocm_FILLED.json");
        let msg: StreamMessage = serde_json::from_str(&data).unwrap();

        if let StreamMessage::OrderChange(ocm) = msg {
            let uo = &ocm.oc.as_ref().unwrap()[0].orc.as_ref().unwrap()[0]
                .uo
                .as_ref()
                .unwrap()[0];
            assert!(!has_cancel_quantity(uo));
        } else {
            panic!("expected OrderChange");
        }
    }

    fn extract_uo(msg: &StreamMessage) -> &UnmatchedOrder {
        if let StreamMessage::OrderChange(ocm) = msg {
            &ocm.oc.as_ref().unwrap()[0].orc.as_ref().unwrap()[0]
                .uo
                .as_ref()
                .unwrap()[0]
        } else {
            panic!("expected OrderChange")
        }
    }

    fn extract_ts(msg: &StreamMessage) -> UnixNanos {
        if let StreamMessage::OrderChange(ocm) = msg {
            parse_millis_timestamp(ocm.pt)
        } else {
            panic!("expected OrderChange")
        }
    }
}
