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
    data::{
        BookOrder, InstrumentClose, InstrumentStatus, OrderBookDelta, OrderBookDeltas, TradeTick,
    },
    enums::{
        AggressorSide, BookAction, InstrumentCloseType, LiquiditySide, MarketStatusAction,
        OrderSide, OrderType, RecordFlag, TimeInForce,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::{
        consts::{BETFAIR_PRICE_PRECISION, BETFAIR_QUANTITY_PRECISION},
        enums::{MarketStatus, RunnerStatus, StreamingOrderStatus, resolve_streaming_order_status},
        parse::{make_instrument_id, parse_millis_timestamp},
    },
    data_types::{
        BetfairBspBookDelta, BetfairRaceProgress, BetfairRaceRunnerData, BetfairStartingPrice,
        BetfairTicker,
    },
    stream::messages::{
        MarketDefinition, RaceProgressChange, RaceRunnerChange, RunnerChange, UnmatchedOrder,
    },
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

/// Produces per-runner [`InstrumentStatus`] events from a market definition.
///
/// Iterates `def.runners` and maps each runner's lifecycle to a Nautilus status.
/// Scratched runners (`Removed`, `RemovedVacant`) close immediately regardless
/// of market-level state. The `in_play` flag distinguishes pre-open (Open + not
/// in play) from active trading (Open + in play).
///
/// Returns an empty vector when `def.status` or `def.runners` is missing.
#[must_use]
pub fn parse_instrument_statuses(
    market_id: &str,
    def: &MarketDefinition,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Vec<InstrumentStatus> {
    let Some(status) = def.status else {
        return Vec::new();
    };
    let Some(runners) = &def.runners else {
        return Vec::new();
    };
    let in_play = def.in_play.unwrap_or(false);

    runners
        .iter()
        .map(|rd| {
            let handicap = rd.hc.unwrap_or(Decimal::ZERO);
            let instrument_id = make_instrument_id(market_id, rd.id, handicap);
            let action = match rd.status {
                Some(RunnerStatus::Removed | RunnerStatus::RemovedVacant) => {
                    MarketStatusAction::Close
                }
                _ => match (status, in_play) {
                    (MarketStatus::Inactive, _) => MarketStatusAction::Close,
                    (MarketStatus::Open, false) => MarketStatusAction::PreOpen,
                    (MarketStatus::Open, true) => MarketStatusAction::Trading,
                    (MarketStatus::Suspended, _) => MarketStatusAction::Pause,
                    (MarketStatus::Closed, _) => MarketStatusAction::Close,
                },
            };
            let is_trading = matches!(status, MarketStatus::Open)
                && in_play
                && !matches!(
                    rd.status,
                    Some(RunnerStatus::Removed | RunnerStatus::RemovedVacant)
                );
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
        })
        .collect()
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
    #[expect(clippy::too_many_arguments)]
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

        let order_qty = resolve_stream_order_quantity(order_qty, uo);

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

    /// Pre-populates state for a bet from existing order data.
    ///
    /// Called during reconnect sync so that the first stream update
    /// computes a correct incremental fill instead of treating the
    /// cumulative matched size as a new fill.
    pub fn sync_order(&mut self, bet_id: &str, filled_qty: Decimal, avg_px: Decimal) {
        self.filled_qty.insert(bet_id.to_string(), filled_qty);

        if avg_px > Decimal::ZERO {
            self.avg_px.insert(bet_id.to_string(), avg_px);
        }
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
    let time_in_force = parse_stream_time_in_force(uo)?;

    let size_matched = uo.sm.unwrap_or(Decimal::ZERO);
    let size_cancelled = uo.sc.unwrap_or(Decimal::ZERO);
    let size_lapsed = uo.sl.unwrap_or(Decimal::ZERO);
    let size_voided = uo.sv.unwrap_or(Decimal::ZERO);

    // Include lapsed/voided in the closed quantity for status resolution
    let size_closed = size_cancelled + size_lapsed + size_voided;
    let order_status = resolve_streaming_order_status(uo.status, size_matched, size_closed);

    let quantity_decimal = stream_order_quantity(uo);
    anyhow::ensure!(
        quantity_decimal > Decimal::ZERO,
        "failed to resolve positive quantity for stream order update {} \
         (order_type={:?}, persistence_type={:?}, size={}, bsp_liability={:?}, \
         size_matched={:?}, size_remaining={:?}, size_cancelled={:?}, size_lapsed={:?}, size_voided={:?})",
        uo.id,
        uo.ot,
        uo.pt,
        uo.s,
        uo.bsp,
        uo.sm,
        uo.sr,
        uo.sc,
        uo.sl,
        uo.sv,
    );
    let quantity = Quantity::from_decimal_dp(quantity_decimal, BETFAIR_QUANTITY_PRECISION)?;
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
    if let Some(lsrc) = uo.lsrc {
        report.cancel_reason = Some(lsrc.to_string());
    }

    Ok(report)
}

fn parse_stream_time_in_force(uo: &UnmatchedOrder) -> anyhow::Result<TimeInForce> {
    match uo.pt {
        Some(persistence_type) => Ok(TimeInForce::from(persistence_type)),
        None if matches!(
            uo.ot,
            crate::common::enums::StreamingOrderType::LimitOnClose
                | crate::common::enums::StreamingOrderType::MarketOnClose
        ) =>
        {
            Ok(TimeInForce::AtTheClose)
        }
        None => anyhow::bail!("missing persistence type for order update {}", uo.id),
    }
}

fn stream_order_quantity(uo: &UnmatchedOrder) -> Decimal {
    if uo.s > Decimal::ZERO {
        return uo.s;
    }

    let lifecycle_qty = uo.sm.unwrap_or(Decimal::ZERO)
        + uo.sr.unwrap_or(Decimal::ZERO)
        + uo.sc.unwrap_or(Decimal::ZERO)
        + uo.sl.unwrap_or(Decimal::ZERO)
        + uo.sv.unwrap_or(Decimal::ZERO);

    if lifecycle_qty > Decimal::ZERO {
        return lifecycle_qty;
    }

    if uses_liability_based_stream_quantity(uo) {
        return uo.bsp.unwrap_or(Decimal::ZERO);
    }

    Decimal::ZERO
}

fn resolve_stream_order_quantity(order_qty: Decimal, uo: &UnmatchedOrder) -> Decimal {
    if order_qty > Decimal::ZERO {
        order_qty
    } else {
        stream_order_quantity(uo)
    }
}

fn uses_liability_based_stream_quantity(uo: &UnmatchedOrder) -> bool {
    matches!(
        uo.ot,
        crate::common::enums::StreamingOrderType::LimitOnClose
            | crate::common::enums::StreamingOrderType::MarketOnClose
    )
}

/// Creates a [`FillReport`] for a Betfair order fill.
///
/// Betfair charges commission on net winnings, not per-fill, so commission
/// is set to zero. The `liquidity_side` is unknown from the stream.
#[must_use]
#[expect(clippy::too_many_arguments)]
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

/// Extracts a [`BetfairTicker`] from a runner change if any ticker fields are present.
///
/// Returns `None` when the runner change contains no ltp, tv, spn, or spf data.
#[must_use]
pub fn parse_betfair_ticker(
    instrument_id: InstrumentId,
    rc: &RunnerChange,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Option<BetfairTicker> {
    if rc.ltp.is_none() && rc.tv.is_none() && rc.spn.is_none() && rc.spf.is_none() {
        return None;
    }

    Some(BetfairTicker::new(
        instrument_id,
        rc.ltp.map_or(f64::NAN, |d| {
            d.to_string().parse::<f64>().unwrap_or(f64::NAN)
        }),
        rc.tv.map_or(f64::NAN, |d| {
            d.to_string().parse::<f64>().unwrap_or(f64::NAN)
        }),
        rc.spn.map_or(f64::NAN, |d| {
            d.to_string().parse::<f64>().unwrap_or(f64::NAN)
        }),
        rc.spf.map_or(f64::NAN, |d| {
            d.to_string().parse::<f64>().unwrap_or(f64::NAN)
        }),
        ts_event,
        ts_init,
    ))
}

/// Extracts [`BetfairStartingPrice`] values from a market definition's runners.
///
/// Returns one entry per runner that has a non-None BSP value.
#[must_use]
pub fn parse_betfair_starting_prices(
    market_id: &str,
    def: &MarketDefinition,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Vec<BetfairStartingPrice> {
    let Some(runners) = &def.runners else {
        return Vec::new();
    };

    runners
        .iter()
        .filter_map(|rd| {
            let bsp = rd.bsp?;
            let handicap = rd.hc.unwrap_or(Decimal::ZERO);
            let instrument_id = make_instrument_id(market_id, rd.id, handicap);
            let bsp_f64 = bsp.to_string().parse::<f64>().unwrap_or(f64::NAN);
            Some(BetfairStartingPrice::new(
                instrument_id,
                bsp_f64,
                ts_event,
                ts_init,
            ))
        })
        .collect()
}

/// Extracts BSP order book deltas from a runner change's `spb`/`spl` fields.
///
/// Returns an empty vec when neither `spb` nor `spl` data is present.
/// The `side` field uses `OrderSide::Sell` for `spb` (back) and
/// `OrderSide::Buy` for `spl` (lay), following Betfair's inverted convention.
#[must_use]
pub fn parse_bsp_book_deltas(
    instrument_id: InstrumentId,
    rc: &RunnerChange,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Vec<BetfairBspBookDelta> {
    let spb_len = rc.spb.as_ref().map_or(0, Vec::len);
    let spl_len = rc.spl.as_ref().map_or(0, Vec::len);

    if spb_len + spl_len == 0 {
        return Vec::new();
    }

    let mut result = Vec::with_capacity(spb_len + spl_len);

    // spb (starting price back) -> Sell side (Betfair convention)
    for pv in rc.spb.as_deref().unwrap_or(&[]) {
        let action = if pv.volume == Decimal::ZERO {
            BookAction::Delete as u32
        } else {
            BookAction::Update as u32
        };

        result.push(BetfairBspBookDelta::new(
            instrument_id,
            action,
            OrderSide::Sell as u32,
            pv.price.to_string().parse::<f64>().unwrap_or(f64::NAN),
            pv.volume.to_string().parse::<f64>().unwrap_or(0.0),
            ts_event,
            ts_init,
        ));
    }

    // spl (starting price lay) -> Buy side (Betfair convention)
    for pv in rc.spl.as_deref().unwrap_or(&[]) {
        let action = if pv.volume == Decimal::ZERO {
            BookAction::Delete as u32
        } else {
            BookAction::Update as u32
        };

        result.push(BetfairBspBookDelta::new(
            instrument_id,
            action,
            OrderSide::Buy as u32,
            pv.price.to_string().parse::<f64>().unwrap_or(f64::NAN),
            pv.volume.to_string().parse::<f64>().unwrap_or(0.0),
            ts_event,
            ts_init,
        ));
    }

    result
}

/// Produces [`InstrumentClose`] events from a market definition's runner statuses.
///
/// Winners and placed runners get close price 1.0; losers and removed runners
/// get close price 0.0. Active runners produce no close event.
#[must_use]
pub fn parse_instrument_closes(
    market_id: &str,
    def: &MarketDefinition,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Vec<InstrumentClose> {
    let Some(runners) = &def.runners else {
        return Vec::new();
    };

    runners
        .iter()
        .filter_map(|rd| {
            let status = rd.status.as_ref()?;
            let close_price = match status {
                RunnerStatus::Winner | RunnerStatus::Placed => Price::from("1.00"),
                RunnerStatus::Loser | RunnerStatus::Removed | RunnerStatus::RemovedVacant => {
                    Price::from("0.00")
                }
                RunnerStatus::Active | RunnerStatus::Hidden => return None,
            };

            let handicap = rd.hc.unwrap_or(Decimal::ZERO);
            let instrument_id = make_instrument_id(market_id, rd.id, handicap);

            Some(InstrumentClose::new(
                instrument_id,
                close_price,
                InstrumentCloseType::ContractExpired,
                ts_event,
                ts_init,
            ))
        })
        .collect()
}

/// Parses a single [`RaceRunnerChange`] into a [`BetfairRaceRunnerData`].
///
/// Returns `None` if the runner change has no selection ID.
#[must_use]
pub fn parse_race_runner_data(
    race_id: &str,
    market_id: &str,
    rrc: &RaceRunnerChange,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Option<BetfairRaceRunnerData> {
    let selection_id = rrc.id?;

    Some(BetfairRaceRunnerData::new(
        race_id.to_string(),
        market_id.to_string(),
        selection_id,
        rrc.lat.unwrap_or(f64::NAN),
        rrc.lng.unwrap_or(f64::NAN),
        rrc.spd.unwrap_or(f64::NAN),
        rrc.prg.unwrap_or(f64::NAN),
        rrc.sfq.unwrap_or(f64::NAN),
        ts_event,
        ts_init,
    ))
}

/// Parses a [`RaceProgressChange`] into a [`BetfairRaceProgress`].
#[must_use]
pub fn parse_race_progress(
    race_id: &str,
    market_id: &str,
    rpc: &RaceProgressChange,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> BetfairRaceProgress {
    let order_json = rpc
        .ord
        .as_ref()
        .map(|v| serde_json::to_string(v).unwrap_or_default())
        .unwrap_or_default();

    let jumps_json = rpc
        .jumps
        .as_ref()
        .map(|v| serde_json::to_string(v).unwrap_or_default())
        .unwrap_or_default();

    BetfairRaceProgress::new(
        race_id.to_string(),
        market_id.to_string(),
        rpc.g.clone().unwrap_or_default(),
        rpc.st.unwrap_or(f64::NAN),
        rpc.rt.unwrap_or(f64::NAN),
        rpc.spd.unwrap_or(f64::NAN),
        rpc.prg.unwrap_or(f64::NAN),
        order_json,
        jumps_json,
        ts_event,
        ts_init,
    )
}

#[cfg(test)]
mod tests {
    use nautilus_model::enums::{MarketStatusAction, OrderStatus, TimeInForce};
    use rstest::rstest;

    use super::*;
    use crate::{
        common::{
            enums::{StreamingOrderType, StreamingPersistenceType, StreamingSide},
            testing::load_test_json,
        },
        stream::messages::{PV, RunnerDefinition, StreamMessage, stream_decode},
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

    fn make_status_def(
        status: MarketStatus,
        in_play: bool,
        runner_status: RunnerStatus,
    ) -> MarketDefinition {
        MarketDefinition {
            runners: Some(vec![RunnerDefinition {
                id: 456,
                hc: None,
                sort_priority: None,
                name: None,
                status: Some(runner_status),
                adjustment_factor: None,
                bsp: None,
                removal_date: None,
            }]),
            bet_delay: None,
            betting_type: None,
            bsp_market: None,
            bsp_reconciled: None,
            competition_id: None,
            competition_name: None,
            complete: None,
            country_code: None,
            cross_matching: None,
            discount_allowed: None,
            each_way_divisor: None,
            event_id: None,
            event_name: None,
            event_type_id: None,
            event_type_name: None,
            in_play: Some(in_play),
            line_interval: None,
            line_max_unit: None,
            line_min_unit: None,
            market_base_rate: None,
            market_id: None,
            market_name: None,
            market_time: None,
            market_type: None,
            number_of_active_runners: None,
            number_of_winners: None,
            open_date: None,
            persistence_enabled: None,
            price_ladder_definition: None,
            race_type: None,
            regulators: None,
            runners_voidable: None,
            settled_time: None,
            status: Some(status),
            suspend_time: None,
            timezone: None,
            turn_in_play_enabled: None,
            venue: None,
            version: None,
        }
    }

    #[rstest]
    #[case(MarketStatus::Open, false, MarketStatusAction::PreOpen, false)]
    #[case(MarketStatus::Open, true, MarketStatusAction::Trading, true)]
    #[case(MarketStatus::Closed, false, MarketStatusAction::Close, false)]
    #[case(MarketStatus::Closed, true, MarketStatusAction::Close, false)]
    #[case(MarketStatus::Suspended, false, MarketStatusAction::Pause, false)]
    #[case(MarketStatus::Suspended, true, MarketStatusAction::Pause, false)]
    #[case(MarketStatus::Inactive, false, MarketStatusAction::Close, false)]
    fn test_parse_instrument_statuses_market_state(
        #[case] status: MarketStatus,
        #[case] in_play: bool,
        #[case] expected_action: MarketStatusAction,
        #[case] expected_is_trading: bool,
    ) {
        let def = make_status_def(status, in_play, RunnerStatus::Active);
        let results =
            parse_instrument_statuses("1.123", &def, UnixNanos::default(), UnixNanos::default());

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, expected_action);
        assert_eq!(results[0].is_trading, Some(expected_is_trading));
    }

    #[rstest]
    #[case(RunnerStatus::Removed)]
    #[case(RunnerStatus::RemovedVacant)]
    fn test_parse_instrument_statuses_scratched_runner_closes(#[case] runner_status: RunnerStatus) {
        // Even with Open + in_play the runner must close when scratched
        let def = make_status_def(MarketStatus::Open, true, runner_status);
        let results =
            parse_instrument_statuses("1.123", &def, UnixNanos::default(), UnixNanos::default());

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action, MarketStatusAction::Close);
        assert_eq!(results[0].is_trading, Some(false));
    }

    #[rstest]
    #[case::missing_runners("runners")]
    #[case::missing_status("status")]
    fn test_parse_instrument_statuses_returns_empty(#[case] drop_field: &str) {
        let mut def = make_status_def(MarketStatus::Open, true, RunnerStatus::Active);
        match drop_field {
            "runners" => def.runners = None,
            "status" => def.status = None,
            _ => unreachable!(),
        }

        let results =
            parse_instrument_statuses("1.123", &def, UnixNanos::default(), UnixNanos::default());

        assert!(results.is_empty());
    }

    #[rstest]
    fn test_parse_instrument_statuses_mixed_runners() {
        // Three runners in one market definition: an Active runner should follow the
        // market-level mapping while Removed/RemovedVacant override to Close. Verify
        // that selection id and handicap propagate into the emitted instrument id.
        let mut def = make_status_def(MarketStatus::Open, true, RunnerStatus::Active);
        def.runners = Some(vec![
            RunnerDefinition {
                id: 101,
                hc: None,
                sort_priority: Some(1),
                name: None,
                status: Some(RunnerStatus::Active),
                adjustment_factor: None,
                bsp: None,
                removal_date: None,
            },
            RunnerDefinition {
                id: 202,
                hc: Some(Decimal::new(25, 1)), // 2.5 handicap
                sort_priority: Some(2),
                name: None,
                status: Some(RunnerStatus::Removed),
                adjustment_factor: None,
                bsp: None,
                removal_date: None,
            },
            RunnerDefinition {
                id: 303,
                hc: None,
                sort_priority: Some(3),
                name: None,
                status: Some(RunnerStatus::RemovedVacant),
                adjustment_factor: None,
                bsp: None,
                removal_date: None,
            },
        ]);

        let results =
            parse_instrument_statuses("1.999", &def, UnixNanos::default(), UnixNanos::default());

        assert_eq!(results.len(), 3);

        assert_eq!(results[0].action, MarketStatusAction::Trading);
        assert_eq!(results[0].is_trading, Some(true));

        assert_eq!(results[1].action, MarketStatusAction::Close);
        assert_eq!(results[1].is_trading, Some(false));

        assert_eq!(results[2].action, MarketStatusAction::Close);
        assert_eq!(results[2].is_trading, Some(false));

        // Each runner produces a distinct instrument id (selection id + handicap)
        assert_ne!(results[0].instrument_id, results[1].instrument_id);
        assert_ne!(results[1].instrument_id, results[2].instrument_id);
        assert_ne!(results[0].instrument_id, results[2].instrument_id);
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

    #[rstest]
    fn test_parse_race_runner_data_from_fixture() {
        let data = load_test_json("stream/rcm_single.json");
        let msg = stream_decode(data.as_bytes()).unwrap();

        let StreamMessage::RaceChange(rcm) = msg else {
            panic!("expected RaceChange");
        };

        let race = &rcm.rc.as_ref().unwrap()[0];
        let rrc = &race.rrc.as_ref().unwrap()[0];
        let ts = parse_millis_timestamp(rcm.pt);

        let runner = parse_race_runner_data(
            race.id.as_deref().unwrap(),
            race.mid.as_deref().unwrap(),
            rrc,
            ts,
            ts,
        )
        .unwrap();

        assert_eq!(runner.race_id, "28587288.1650");
        assert_eq!(runner.market_id, "1.1234567");
        assert_eq!(runner.selection_id, 7390417);
        assert!((runner.latitude - 51.4189543).abs() < 1e-6);
        assert!((runner.longitude - (-0.4058491)).abs() < 1e-6);
        assert!((runner.speed - 17.8).abs() < 1e-6);
        assert!((runner.progress - 2051.0).abs() < 1e-6);
        assert!((runner.stride_frequency - 2.07).abs() < 1e-6);
    }

    #[rstest]
    fn test_parse_race_progress_from_fixture() {
        let data = load_test_json("stream/rcm_single.json");
        let msg = stream_decode(data.as_bytes()).unwrap();

        let StreamMessage::RaceChange(rcm) = msg else {
            panic!("expected RaceChange");
        };

        let race = &rcm.rc.as_ref().unwrap()[0];
        let rpc = race.rpc.as_ref().unwrap();
        let ts = parse_millis_timestamp(rcm.pt);

        let progress = parse_race_progress(
            race.id.as_deref().unwrap(),
            race.mid.as_deref().unwrap(),
            rpc,
            ts,
            ts,
        );

        assert_eq!(progress.race_id, "28587288.1650");
        assert_eq!(progress.market_id, "1.1234567");
        assert_eq!(progress.gate_name, "1f");
        assert!((progress.sectional_time - 10.6).abs() < 1e-6);
        assert!((progress.running_time - 46.7).abs() < 1e-6);
        assert!((progress.speed - 17.8).abs() < 1e-6);
        assert!((progress.progress - 87.5).abs() < 1e-6);

        let order: Vec<i64> = serde_json::from_str(&progress.order).unwrap();
        assert_eq!(order, vec![7390417, 5600338, 11527189, 6395118, 8706072]);

        let jumps: Vec<serde_json::Value> = serde_json::from_str(&progress.jumps).unwrap();
        assert_eq!(jumps.len(), 2);
        assert_eq!(jumps[0]["J"], 2);
    }

    #[rstest]
    fn test_parse_race_runner_data_multi_runner() {
        let data = load_test_json("stream/rcm_multi_runner.json");
        let msg = stream_decode(data.as_bytes()).unwrap();

        let StreamMessage::RaceChange(rcm) = msg else {
            panic!("expected RaceChange");
        };

        let race = &rcm.rc.as_ref().unwrap()[0];
        let ts = parse_millis_timestamp(rcm.pt);
        let race_id = race.id.as_deref().unwrap();
        let market_id = race.mid.as_deref().unwrap();

        let runners: Vec<_> = race
            .rrc
            .as_ref()
            .unwrap()
            .iter()
            .filter_map(|rrc| parse_race_runner_data(race_id, market_id, rrc, ts, ts))
            .collect();

        assert_eq!(runners.len(), 5);
        assert_eq!(runners[0].selection_id, 35467839);
        assert_eq!(runners[4].selection_id, 41694785);
        assert!((runners[0].speed - 16.33).abs() < 1e-6);
        assert!((runners[4].speed - 17.11).abs() < 1e-6);
    }

    #[rstest]
    fn test_parse_race_runner_data_missing_id_returns_none() {
        let rrc = RaceRunnerChange {
            ft: Some(1000),
            id: None,
            lat: Some(51.0),
            lng: Some(-0.4),
            spd: Some(15.0),
            prg: Some(500.0),
            sfq: Some(2.0),
        };
        let ts = UnixNanos::from(1_000_000_000u64);
        let result = parse_race_runner_data("race1", "market1", &rrc, ts, ts);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_parse_race_runner_data_absent_fields_are_nan() {
        let rrc = RaceRunnerChange {
            ft: None,
            id: Some(12345),
            lat: None,
            lng: None,
            spd: None,
            prg: None,
            sfq: None,
        };
        let ts = UnixNanos::from(1_000_000_000u64);
        let runner = parse_race_runner_data("race1", "market1", &rrc, ts, ts).unwrap();
        assert!(runner.latitude.is_nan());
        assert!(runner.longitude.is_nan());
        assert!(runner.speed.is_nan());
        assert!(runner.progress.is_nan());
        assert!(runner.stride_frequency.is_nan());
    }

    #[rstest]
    fn test_parse_race_progress_absent_fields() {
        let rpc = RaceProgressChange {
            ft: None,
            g: None,
            st: None,
            rt: None,
            spd: None,
            prg: None,
            ord: None,
            jumps: None,
        };
        let ts = UnixNanos::from(1_000_000_000u64);
        let progress = parse_race_progress("race1", "market1", &rpc, ts, ts);
        assert_eq!(progress.gate_name, "");
        assert!(progress.sectional_time.is_nan());
        assert!(progress.running_time.is_nan());
        assert_eq!(progress.order, "");
        assert_eq!(progress.jumps, "");
    }

    fn runner_change_with_ticker(
        id: u64,
        ltp: Option<Decimal>,
        tv: Option<Decimal>,
        spn: Option<Decimal>,
        spf: Option<Decimal>,
    ) -> RunnerChange {
        RunnerChange {
            id,
            hc: None,
            atb: None,
            atl: None,
            batb: None,
            batl: None,
            bdatb: None,
            bdatl: None,
            spb: None,
            spl: None,
            spn,
            spf,
            trd: None,
            ltp,
            tv,
        }
    }

    #[rstest]
    fn test_parse_betfair_ticker_all_fields() {
        let rc = runner_change_with_ticker(
            9249757,
            Some(Decimal::new(55, 1)),
            Some(Decimal::new(189032, 2)),
            Some(Decimal::new(568, 2)),
            Some(Decimal::new(573, 2)),
        );
        let ts = UnixNanos::from(1_000_000_000u64);
        let instrument_id = make_instrument_id("1.185781465", 9249757, Decimal::ZERO);

        let ticker = parse_betfair_ticker(instrument_id, &rc, ts, ts).unwrap();

        assert_eq!(ticker.instrument_id, instrument_id);
        assert!((ticker.last_traded_price - 5.5).abs() < f64::EPSILON);
        assert!((ticker.traded_volume - 1890.32).abs() < f64::EPSILON);
        assert!((ticker.starting_price_near - 5.68).abs() < f64::EPSILON);
        assert!((ticker.starting_price_far - 5.73).abs() < f64::EPSILON);
    }

    #[rstest]
    fn test_parse_betfair_ticker_partial_fields() {
        let rc = runner_change_with_ticker(
            9249757,
            Some(Decimal::new(55, 1)),
            Some(Decimal::new(189032, 2)),
            None,
            None,
        );
        let ts = UnixNanos::from(1_000_000_000u64);
        let instrument_id = make_instrument_id("1.185781465", 9249757, Decimal::ZERO);

        let ticker = parse_betfair_ticker(instrument_id, &rc, ts, ts).unwrap();

        assert!((ticker.last_traded_price - 5.5).abs() < f64::EPSILON);
        assert!((ticker.traded_volume - 1890.32).abs() < f64::EPSILON);
        assert!(ticker.starting_price_near.is_nan());
        assert!(ticker.starting_price_far.is_nan());
    }

    #[rstest]
    fn test_parse_betfair_ticker_no_fields_returns_none() {
        let rc = runner_change_with_ticker(9249757, None, None, None, None);
        let ts = UnixNanos::from(1_000_000_000u64);
        let instrument_id = make_instrument_id("1.185781465", 9249757, Decimal::ZERO);

        assert!(parse_betfair_ticker(instrument_id, &rc, ts, ts).is_none());
    }

    #[rstest]
    fn test_parse_betfair_ticker_only_tv() {
        let rc =
            runner_change_with_ticker(40273293, None, Some(Decimal::new(320115, 2)), None, None);
        let ts = UnixNanos::from(1_000_000_000u64);
        let instrument_id = make_instrument_id("1.185781465", 40273293, Decimal::ZERO);

        let ticker = parse_betfair_ticker(instrument_id, &rc, ts, ts).unwrap();

        assert!(ticker.last_traded_price.is_nan());
        assert!((ticker.traded_volume - 3201.15).abs() < f64::EPSILON);
        assert!(ticker.starting_price_near.is_nan());
        assert!(ticker.starting_price_far.is_nan());
    }

    #[rstest]
    fn test_parse_betfair_ticker_from_fixture() {
        let data = load_test_json("stream/mcm_BSP_settled.json");
        let msg: StreamMessage = serde_json::from_str(&data).unwrap();

        if let StreamMessage::MarketChange(mcm) = msg {
            let mc = mcm.mc.as_ref().unwrap();
            let change = &mc[0];
            let rc_list = change.rc.as_ref().unwrap();

            // Runner 9249757 has ltp=5.5, tv=1890.32, spn=5.68, spf=5.73
            let rc = rc_list.iter().find(|r| r.id == 9249757).unwrap();
            let instrument_id = make_instrument_id(&change.id, rc.id, Decimal::ZERO);
            let ts = parse_millis_timestamp(mcm.pt);

            let ticker = parse_betfair_ticker(instrument_id, rc, ts, ts).unwrap();

            assert!((ticker.last_traded_price - 5.5).abs() < f64::EPSILON);
            assert!((ticker.traded_volume - 1890.32).abs() < f64::EPSILON);
            assert!((ticker.starting_price_near - 5.68).abs() < f64::EPSILON);
            assert!((ticker.starting_price_far - 5.73).abs() < f64::EPSILON);

            // Runner 40273293 has ltp=2.1, tv=3201.15 but no spn/spf
            let rc2 = rc_list.iter().find(|r| r.id == 40273293).unwrap();
            let instrument_id2 = make_instrument_id(&change.id, rc2.id, Decimal::ZERO);
            let ticker2 = parse_betfair_ticker(instrument_id2, rc2, ts, ts).unwrap();

            assert!((ticker2.last_traded_price - 2.1).abs() < f64::EPSILON);
            assert!((ticker2.traded_volume - 3201.15).abs() < f64::EPSILON);
            assert!(ticker2.starting_price_near.is_nan());
            assert!(ticker2.starting_price_far.is_nan());

            // Runner 23678734 has only tv=0, no ltp
            let rc3 = rc_list.iter().find(|r| r.id == 23678734).unwrap();
            let instrument_id3 = make_instrument_id(&change.id, rc3.id, Decimal::ZERO);
            let ticker3 = parse_betfair_ticker(instrument_id3, rc3, ts, ts).unwrap();

            assert!(ticker3.last_traded_price.is_nan());
            assert!((ticker3.traded_volume - 0.0).abs() < f64::EPSILON);
        } else {
            panic!("Expected MarketChange");
        }
    }

    #[rstest]
    fn test_parse_betfair_starting_prices_from_fixture() {
        let data = load_test_json("stream/mcm_BSP_settled.json");
        let msg: StreamMessage = serde_json::from_str(&data).unwrap();

        if let StreamMessage::MarketChange(mcm) = msg {
            let mc = mcm.mc.as_ref().unwrap();
            let def = mc[0].market_definition.as_ref().unwrap();
            let ts = parse_millis_timestamp(mcm.pt);

            let prices = parse_betfair_starting_prices(&mc[0].id, def, ts, ts);

            // 3 runners have bsp values, 1 (REMOVED) does not
            assert_eq!(prices.len(), 3);

            let bsp_map: std::collections::HashMap<String, f64> = prices
                .iter()
                .map(|p| (p.instrument_id.to_string(), p.bsp))
                .collect();

            let id_winner = make_instrument_id("1.185781465", 9249757, Decimal::ZERO).to_string();
            let id_placed = make_instrument_id("1.185781465", 40273293, Decimal::ZERO).to_string();
            let id_loser = make_instrument_id("1.185781465", 11120000, Decimal::ZERO).to_string();

            assert!((bsp_map[&id_winner] - 5.73).abs() < f64::EPSILON);
            assert!((bsp_map[&id_placed] - 2.14).abs() < f64::EPSILON);
            assert!((bsp_map[&id_loser] - 28.56).abs() < f64::EPSILON);
        } else {
            panic!("Expected MarketChange");
        }
    }

    #[rstest]
    fn test_parse_betfair_starting_prices_no_runners() {
        let def = MarketDefinition {
            runners: None,
            bet_delay: None,
            betting_type: None,
            bsp_market: None,
            bsp_reconciled: None,
            competition_id: None,
            competition_name: None,
            complete: None,
            country_code: None,
            cross_matching: None,
            discount_allowed: None,
            each_way_divisor: None,
            event_id: None,
            event_name: None,
            event_type_id: None,
            event_type_name: None,
            in_play: None,
            line_interval: None,
            line_max_unit: None,
            line_min_unit: None,
            market_base_rate: None,
            market_id: None,
            market_name: None,
            market_time: None,
            market_type: None,
            number_of_active_runners: None,
            number_of_winners: None,
            open_date: None,
            persistence_enabled: None,
            price_ladder_definition: None,
            race_type: None,
            regulators: None,
            runners_voidable: None,
            settled_time: None,
            status: None,
            suspend_time: None,
            timezone: None,
            turn_in_play_enabled: None,
            venue: None,
            version: None,
        };
        let ts = UnixNanos::from(1_000_000_000u64);

        let prices = parse_betfair_starting_prices("1.12345", &def, ts, ts);

        assert!(prices.is_empty());
    }

    #[rstest]
    fn test_parse_bsp_book_deltas_from_fixture() {
        let data = load_test_json("stream/mcm_BSP.json");
        let messages: Vec<StreamMessage> = serde_json::from_str(&data).unwrap();

        // Find the MCM with runner changes containing spb/spl data
        let mcm = messages
            .iter()
            .find_map(|m| match m {
                StreamMessage::MarketChange(mcm) => {
                    let mc = mcm.mc.as_ref()?;
                    let has_spb = mc.iter().any(|c| {
                        c.rc.as_ref()
                            .is_some_and(|rcs| rcs.iter().any(|r| r.spb.is_some()))
                    });

                    if has_spb { Some(mcm) } else { None }
                }
                _ => None,
            })
            .expect("fixture should contain MCM with spb data");

        let mc = mcm.mc.as_ref().unwrap();
        let change = &mc[0];
        let rc_list = change.rc.as_ref().unwrap();

        // Runner 9249757 has spb and spl arrays
        let rc = rc_list.iter().find(|r| r.id == 9249757).unwrap();
        let instrument_id = make_instrument_id(&change.id, rc.id, Decimal::ZERO);
        let ts = parse_millis_timestamp(mcm.pt);

        let deltas = parse_bsp_book_deltas(instrument_id, rc, ts, ts);

        let spb_count = rc.spb.as_ref().unwrap().len();
        let spl_count = rc.spl.as_ref().unwrap().len();
        assert_eq!(deltas.len(), spb_count + spl_count);

        // SPB entries are Sell side
        assert_eq!(deltas[0].side, OrderSide::Sell as u32);
        assert!((deltas[0].price - 1000.0).abs() < f64::EPSILON);
        assert!((deltas[0].size - 33.38).abs() < f64::EPSILON);
        assert_eq!(deltas[0].action, BookAction::Update as u32);

        // SPL entries are Buy side
        let spl_start = spb_count;
        assert_eq!(deltas[spl_start].side, OrderSide::Buy as u32);
        assert!((deltas[spl_start].price - 7.0).abs() < f64::EPSILON);
        assert!((deltas[spl_start].size - 10.0).abs() < f64::EPSILON);
    }

    #[rstest]
    fn test_parse_bsp_book_deltas_zero_volume_is_delete() {
        let rc = RunnerChange {
            id: 12345,
            hc: None,
            atb: None,
            atl: None,
            batb: None,
            batl: None,
            bdatb: None,
            bdatl: None,
            spb: Some(vec![PV {
                price: Decimal::new(50, 1),
                volume: Decimal::ZERO,
            }]),
            spl: None,
            spn: None,
            spf: None,
            trd: None,
            ltp: None,
            tv: None,
        };
        let ts = UnixNanos::from(1_000_000_000u64);
        let instrument_id = make_instrument_id("1.12345", 12345, Decimal::ZERO);

        let deltas = parse_bsp_book_deltas(instrument_id, &rc, ts, ts);

        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].action, BookAction::Delete as u32);
        assert!((deltas[0].price - 5.0).abs() < f64::EPSILON);
        assert!((deltas[0].size - 0.0).abs() < f64::EPSILON);
    }

    #[rstest]
    fn test_parse_bsp_book_deltas_no_spb_spl_returns_empty() {
        let rc = runner_change_with_ticker(12345, None, None, None, None);
        let ts = UnixNanos::from(1_000_000_000u64);
        let instrument_id = make_instrument_id("1.12345", 12345, Decimal::ZERO);

        let deltas = parse_bsp_book_deltas(instrument_id, &rc, ts, ts);

        assert!(deltas.is_empty());
    }

    #[rstest]
    fn test_parse_instrument_closes_from_fixture() {
        let data = load_test_json("stream/mcm_BSP_settled.json");
        let msg: StreamMessage = serde_json::from_str(&data).unwrap();

        if let StreamMessage::MarketChange(mcm) = msg {
            let mc = mcm.mc.as_ref().unwrap();
            let def = mc[0].market_definition.as_ref().unwrap();
            let ts = parse_millis_timestamp(mcm.pt);

            let closes = parse_instrument_closes(&mc[0].id, def, ts, ts);

            // 4 runners: WINNER, PLACED, LOSER, REMOVED - all produce close events
            assert_eq!(closes.len(), 4);

            let close_map: std::collections::HashMap<String, Price> = closes
                .iter()
                .map(|c| (c.instrument_id.to_string(), c.close_price))
                .collect();

            let id_winner = make_instrument_id("1.185781465", 9249757, Decimal::ZERO).to_string();
            let id_placed = make_instrument_id("1.185781465", 40273293, Decimal::ZERO).to_string();
            let id_loser = make_instrument_id("1.185781465", 11120000, Decimal::ZERO).to_string();
            let id_removed = make_instrument_id("1.185781465", 37433527, Decimal::ZERO).to_string();

            assert_eq!(close_map[&id_winner], Price::from("1.00"));
            assert_eq!(close_map[&id_placed], Price::from("1.00"));
            assert_eq!(close_map[&id_loser], Price::from("0.00"));
            assert_eq!(close_map[&id_removed], Price::from("0.00"));
        } else {
            panic!("Expected MarketChange");
        }
    }

    #[rstest]
    fn test_parse_instrument_closes_active_runners_excluded() {
        let data = load_test_json("stream/mcm_BSP.json");
        let messages: Vec<StreamMessage> = serde_json::from_str(&data).unwrap();

        // Find the MCM with a market definition containing ACTIVE runners
        let mcm = messages
            .iter()
            .find_map(|m| match m {
                StreamMessage::MarketChange(mcm) => {
                    let mc = mcm.mc.as_ref()?;
                    mc.iter()
                        .find(|c| c.market_definition.is_some())
                        .map(|_| mcm)
                }
                _ => None,
            })
            .expect("fixture should contain MCM with market definition");

        let mc = mcm.mc.as_ref().unwrap();
        let change = mc.iter().find(|c| c.market_definition.is_some()).unwrap();
        let def = change.market_definition.as_ref().unwrap();
        let ts = parse_millis_timestamp(mcm.pt);

        let closes = parse_instrument_closes(&change.id, def, ts, ts);

        assert!(
            closes.is_empty(),
            "Active runners should not produce close events, found {}",
            closes.len()
        );
    }

    fn make_test_uo(
        bet_id: &str,
        size: Decimal,
        sm: Option<Decimal>,
        avp: Option<Decimal>,
    ) -> UnmatchedOrder {
        UnmatchedOrder {
            id: bet_id.to_string(),
            p: Decimal::new(25, 1),
            s: size,
            side: StreamingSide::Back,
            status: StreamingOrderStatus::Executable,
            pt: Some(StreamingPersistenceType::Lapse),
            ot: StreamingOrderType::Limit,
            pd: 1616568581000,
            bsp: None,
            rfo: None,
            rfs: None,
            rc: None,
            rac: None,
            md: None,
            cd: None,
            ld: None,
            avp,
            sm,
            sr: None,
            sl: None,
            sc: None,
            sv: None,
            lsrc: None,
        }
    }

    #[rstest]
    fn test_fill_tracker_sync_order_prevents_duplicate_fill() {
        let mut tracker = FillTracker::new();

        // Sync existing fill state: 10 matched at 2.5
        tracker.sync_order("123456", Decimal::new(10, 0), Decimal::new(25, 1));

        let uo = make_test_uo(
            "123456",
            Decimal::new(20, 0),
            Some(Decimal::new(10, 0)),
            Some(Decimal::new(25, 1)),
        );

        let instrument_id = InstrumentId::from("1.234567-123456-0.0.BETFAIR");
        let account_id = AccountId::from("BETFAIR-001");
        let currency = Currency::from("GBP");
        let ts = UnixNanos::default();

        // Same sm=10 as synced, should not emit a fill
        let result =
            tracker.maybe_fill_report(&uo, uo.s, instrument_id, account_id, currency, ts, ts);
        assert!(
            result.is_none(),
            "should not emit fill for already-synced qty"
        );
    }

    #[rstest]
    fn test_fill_tracker_sync_order_allows_incremental_fill() {
        let mut tracker = FillTracker::new();

        // Sync: 10 matched at 2.5
        tracker.sync_order("123456", Decimal::new(10, 0), Decimal::new(25, 1));

        let uo = make_test_uo(
            "123456",
            Decimal::new(20, 0),
            Some(Decimal::new(15, 0)),
            Some(Decimal::new(26, 1)),
        );

        let instrument_id = InstrumentId::from("1.234567-123456-0.0.BETFAIR");
        let account_id = AccountId::from("BETFAIR-001");
        let currency = Currency::from("GBP");
        let ts = UnixNanos::default();

        // sm=15 vs synced 10, should emit incremental fill of 5
        let result =
            tracker.maybe_fill_report(&uo, uo.s, instrument_id, account_id, currency, ts, ts);
        assert!(result.is_some(), "should emit fill for new matched qty");
        let fill = result.unwrap();
        assert_eq!(fill.last_qty, Quantity::from("5.00"));
    }

    #[rstest]
    fn test_fill_tracker_overfill_rejected() {
        let mut tracker = FillTracker::new();

        // sm=30 exceeds order size s=20
        let uo = make_test_uo(
            "999001",
            Decimal::new(20, 0),
            Some(Decimal::new(30, 0)),
            Some(Decimal::new(25, 1)),
        );

        let instrument_id = InstrumentId::from("1.234567-999001-0.0.BETFAIR");
        let account_id = AccountId::from("BETFAIR-001");
        let currency = Currency::from("GBP");
        let ts = UnixNanos::default();

        let result =
            tracker.maybe_fill_report(&uo, uo.s, instrument_id, account_id, currency, ts, ts);
        assert!(
            result.is_none(),
            "overfill (sm > order_qty) should be rejected"
        );
    }

    #[rstest]
    fn test_fill_tracker_zero_sm_returns_none() {
        let mut tracker = FillTracker::new();

        let uo = make_test_uo("999002", Decimal::new(10, 0), Some(Decimal::ZERO), None);

        let instrument_id = InstrumentId::from("1.234567-999002-0.0.BETFAIR");
        let result = tracker.maybe_fill_report(
            &uo,
            uo.s,
            instrument_id,
            AccountId::from("BETFAIR-001"),
            Currency::from("GBP"),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        assert!(result.is_none(), "zero sm should not produce a fill");
    }

    #[rstest]
    fn test_fill_tracker_no_avp_uses_order_price() {
        let data = load_test_json("stream/ocm_FILLED_no_avp.json");
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
                .expect("should produce fill even without avp");

            // No avp field, so fill price falls back to order price (p=3.5)
            assert_eq!(fill.last_qty.as_f64(), 25.0);
            assert_eq!(fill.last_px.as_f64(), 3.5);
        } else {
            panic!("expected OrderChange");
        }
    }

    #[rstest]
    fn test_fill_tracker_weighted_avg_back_calculation() {
        let mut tracker = FillTracker::new();
        let instrument_id = InstrumentId::from("1.234567-999003-0.0.BETFAIR");
        let account_id = AccountId::from("BETFAIR-001");
        let currency = Currency::from("GBP");
        let ts = UnixNanos::default();

        // First fill: 10 @ avp=2.0
        let uo1 = make_test_uo(
            "999003",
            Decimal::new(30, 0),
            Some(Decimal::new(10, 0)),
            Some(Decimal::new(20, 1)),
        );
        let fill1 = tracker
            .maybe_fill_report(&uo1, uo1.s, instrument_id, account_id, currency, ts, ts)
            .expect("first fill");
        assert_eq!(fill1.last_px.as_f64(), 2.0);
        assert_eq!(fill1.last_qty.as_f64(), 10.0);

        // Second fill: sm=20, avp=2.5
        // Back-calc: (2.5*20 - 2.0*10) / 10 = (50-20)/10 = 3.0
        let uo2 = make_test_uo(
            "999003",
            Decimal::new(30, 0),
            Some(Decimal::new(20, 0)),
            Some(Decimal::new(25, 1)),
        );
        let fill2 = tracker
            .maybe_fill_report(&uo2, uo2.s, instrument_id, account_id, currency, ts, ts)
            .expect("second fill");
        assert_eq!(fill2.last_qty.as_f64(), 10.0);
        assert_eq!(fill2.last_px.as_f64(), 3.0);
    }

    #[rstest]
    fn test_fill_tracker_negative_fill_price_falls_back_to_avp() {
        let mut tracker = FillTracker::new();
        let instrument_id = InstrumentId::from("1.234567-999004-0.0.BETFAIR");
        let account_id = AccountId::from("BETFAIR-001");
        let currency = Currency::from("GBP");
        let ts = UnixNanos::default();

        // First fill: 10 @ avp=5.0
        let uo1 = make_test_uo(
            "999004",
            Decimal::new(20, 0),
            Some(Decimal::new(10, 0)),
            Some(Decimal::new(50, 1)),
        );
        tracker
            .maybe_fill_report(&uo1, uo1.s, instrument_id, account_id, currency, ts, ts)
            .expect("first fill");

        // Second fill: sm=15, avp=1.0
        // Back-calc: (1.0*15 - 5.0*10) / 5 = (15-50)/5 = -7.0
        // Negative price should fall back to avp=1.0
        let uo2 = make_test_uo(
            "999004",
            Decimal::new(20, 0),
            Some(Decimal::new(15, 0)),
            Some(Decimal::new(10, 1)),
        );
        let fill2 = tracker
            .maybe_fill_report(&uo2, uo2.s, instrument_id, account_id, currency, ts, ts)
            .expect("second fill should use avp fallback");
        assert_eq!(fill2.last_qty.as_f64(), 5.0);
        assert_eq!(fill2.last_px.as_f64(), 1.0);
    }

    #[rstest]
    fn test_fill_tracker_prune_clears_state() {
        let mut tracker = FillTracker::new();
        let instrument_id = InstrumentId::from("1.234567-999005-0.0.BETFAIR");
        let account_id = AccountId::from("BETFAIR-001");
        let currency = Currency::from("GBP");
        let ts = UnixNanos::default();

        // Fill order fully
        let uo = make_test_uo(
            "999005",
            Decimal::new(10, 0),
            Some(Decimal::new(10, 0)),
            Some(Decimal::new(25, 1)),
        );
        let fill1 =
            tracker.maybe_fill_report(&uo, uo.s, instrument_id, account_id, currency, ts, ts);
        assert!(fill1.is_some());

        // Same data again - deduplicated
        let fill2 =
            tracker.maybe_fill_report(&uo, uo.s, instrument_id, account_id, currency, ts, ts);
        assert!(fill2.is_none(), "should be deduplicated");

        // Prune the bet
        tracker.prune("999005");

        // After prune, same data can produce a fill again (simulates re-processing)
        let fill3 =
            tracker.maybe_fill_report(&uo, uo.s, instrument_id, account_id, currency, ts, ts);
        assert!(fill3.is_some(), "after prune, should produce fill again");
    }

    #[rstest]
    fn test_fill_tracker_sm_none_returns_none() {
        let mut tracker = FillTracker::new();

        // sm=None (no matched quantity field at all)
        let uo = make_test_uo("999006", Decimal::new(10, 0), None, None);

        let instrument_id = InstrumentId::from("1.234567-999006-0.0.BETFAIR");
        let result = tracker.maybe_fill_report(
            &uo,
            uo.s,
            instrument_id,
            AccountId::from("BETFAIR-001"),
            Currency::from("GBP"),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        assert!(result.is_none(), "None sm should not produce a fill");
    }

    #[rstest]
    fn test_parse_order_status_report_missing_persistence_type_for_market_on_close() {
        let uo = UnmatchedOrder {
            s: Decimal::ZERO,
            pt: None,
            ot: StreamingOrderType::MarketOnClose,
            sr: Some(Decimal::new(10, 0)),
            ..make_test_uo("999007", Decimal::new(10, 0), Some(Decimal::ZERO), None)
        };

        let report = parse_order_status_report(
            &uo,
            InstrumentId::from("1.234567-123456-0.0.BETFAIR"),
            AccountId::from("BETFAIR-001"),
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(report.quantity, Quantity::from("10.00"));
        assert_eq!(report.time_in_force, TimeInForce::AtTheClose);
    }

    #[rstest]
    fn test_parse_order_status_report_missing_persistence_type_for_limit_on_close() {
        let uo = UnmatchedOrder {
            s: Decimal::ZERO,
            pt: None,
            ot: StreamingOrderType::LimitOnClose,
            sr: Some(Decimal::new(10, 0)),
            ..make_test_uo("999013", Decimal::new(10, 0), Some(Decimal::ZERO), None)
        };

        let report = parse_order_status_report(
            &uo,
            InstrumentId::from("1.234567-123456-0.0.BETFAIR"),
            AccountId::from("BETFAIR-001"),
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(report.quantity, Quantity::from("10.00"));
        assert_eq!(report.time_in_force, TimeInForce::AtTheClose);
    }

    #[rstest]
    fn test_parse_order_status_report_market_on_close_uses_bsp_liability() {
        let uo = UnmatchedOrder {
            s: Decimal::ZERO,
            bsp: Some(Decimal::new(20, 1)),
            pt: None,
            ot: StreamingOrderType::MarketOnClose,
            ..make_test_uo("999010", Decimal::new(10, 0), Some(Decimal::ZERO), None)
        };

        let report = parse_order_status_report(
            &uo,
            InstrumentId::from("1.234567-123456-0.0.BETFAIR"),
            AccountId::from("BETFAIR-001"),
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(report.quantity, Quantity::from("2.00"));
        assert_eq!(report.time_in_force, TimeInForce::AtTheClose);
    }

    #[rstest]
    fn test_parse_order_status_report_fails_for_non_positive_quantity() {
        let uo = UnmatchedOrder {
            s: Decimal::ZERO,
            sm: Some(Decimal::ZERO),
            sr: Some(Decimal::ZERO),
            sc: Some(Decimal::ZERO),
            sl: Some(Decimal::ZERO),
            sv: Some(Decimal::ZERO),
            ..make_test_uo("999014", Decimal::ZERO, Some(Decimal::ZERO), None)
        };

        let result = parse_order_status_report(
            &uo,
            InstrumentId::from("1.234567-123456-0.0.BETFAIR"),
            AccountId::from("BETFAIR-001"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("failed to resolve positive quantity for stream order update 999014")
        );
    }

    #[rstest]
    fn test_parse_order_status_report_includes_lapse_reason() {
        let uo = UnmatchedOrder {
            status: StreamingOrderStatus::ExecutionComplete,
            sl: Some(Decimal::ONE),
            lsrc: Some(crate::common::enums::LapseStatusReasonCode::SpInPlay),
            ..make_test_uo("999012", Decimal::new(10, 0), Some(Decimal::ZERO), None)
        };

        let report = parse_order_status_report(
            &uo,
            InstrumentId::from("1.234567-123456-0.0.BETFAIR"),
            AccountId::from("BETFAIR-001"),
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(report.order_status, OrderStatus::Canceled);
        assert_eq!(report.cancel_reason.as_deref(), Some("SP_IN_PLAY"));
    }

    #[rstest]
    fn test_parse_order_status_report_missing_persistence_type_fails_for_limit_order() {
        let uo = UnmatchedOrder {
            pt: None,
            ..make_test_uo("999008", Decimal::new(10, 0), Some(Decimal::ZERO), None)
        };

        let result = parse_order_status_report(
            &uo,
            InstrumentId::from("1.234567-123456-0.0.BETFAIR"),
            AccountId::from("BETFAIR-001"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "missing persistence type for order update 999008"
        );
    }

    #[rstest]
    fn test_fill_tracker_uses_lifecycle_quantity_when_stream_size_is_zero() {
        let mut tracker = FillTracker::new();
        let uo = UnmatchedOrder {
            s: Decimal::ZERO,
            sr: Some(Decimal::new(10, 0)),
            sm: Some(Decimal::new(5, 0)),
            avp: Some(Decimal::new(20, 1)),
            ..make_test_uo("999009", Decimal::new(10, 0), Some(Decimal::ZERO), None)
        };

        let fill = tracker
            .maybe_fill_report(
                &uo,
                uo.s,
                InstrumentId::from("1.234567-123456-0.0.BETFAIR"),
                AccountId::from("BETFAIR-001"),
                Currency::from("GBP"),
                UnixNanos::default(),
                UnixNanos::default(),
            )
            .expect("zero stream size should fall back to lifecycle quantities");

        assert_eq!(fill.last_qty, Quantity::from("5.00"));
    }

    #[rstest]
    fn test_fill_tracker_uses_bsp_liability_when_stream_size_is_zero() {
        let mut tracker = FillTracker::new();
        let uo = UnmatchedOrder {
            s: Decimal::ZERO,
            bsp: Some(Decimal::new(20, 1)),
            pt: None,
            ot: StreamingOrderType::MarketOnClose,
            sm: Some(Decimal::new(10, 1)),
            avp: Some(Decimal::new(20, 1)),
            ..make_test_uo("999011", Decimal::new(10, 0), Some(Decimal::ZERO), None)
        };

        let fill = tracker
            .maybe_fill_report(
                &uo,
                uo.s,
                InstrumentId::from("1.234567-123456-0.0.BETFAIR"),
                AccountId::from("BETFAIR-001"),
                Currency::from("GBP"),
                UnixNanos::default(),
                UnixNanos::default(),
            )
            .expect("zero stream size should fall back to bsp liability");

        assert_eq!(fill.last_qty, Quantity::from("1.00"));
    }

    #[rstest]
    fn test_fill_tracker_partial_void_still_emits_fill() {
        let data = load_test_json("stream/ocm_VOIDED_partial.json");
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
                .expect("should produce fill for matched portion");

            // Order: s=100, sm=60, sv=40 -> fill qty should be 60
            assert_eq!(fill.last_qty.as_f64(), 60.0);
            assert_eq!(fill.last_px.as_f64(), 1.5);
            assert_eq!(fill.order_side, OrderSide::Sell);
        } else {
            panic!("expected OrderChange");
        }
    }

    #[rstest]
    fn test_fill_tracker_no_fill_when_sv_zero_and_fully_filled() {
        let data = load_test_json("stream/ocm_FILLED_sv_zero.json");
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
                .expect("fully filled order should produce fill");

            assert_eq!(fill.last_qty.as_f64(), 50.0);
            assert_eq!(fill.last_px.as_f64(), 2.0);

            // sv=0, so no void event should be generated (tested separately)
            assert_eq!(uo.sv, Some(Decimal::ZERO));
        } else {
            panic!("expected OrderChange");
        }
    }
}
