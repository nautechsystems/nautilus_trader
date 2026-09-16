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

//! Execution client for the Kalshi Trade API.
//!
//! Kalshi reports order state over REST rather than over a trading stream, which decides the shape
//! of this client:
//!
//! - A command is one signed request. The immediate answer to it, the venue order identifier and
//!   the fills it caused, is emitted as order events from the response.
//! - Everything that happens to an order afterwards is learned by polling. One task polls the
//!   orders the client is still tracking, and reports their state as
//!   [`OrderStatusReport`]s and [`FillReport]`s, which is the path every adapter uses for state it
//!   learns asynchronously.
//!
//! The engine deduplicates fills by trade ID and so does this client, so a fill that the submission
//! path reported as an order event is not applied a second time when the poll reports it.

use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use nautilus_common::{
    clients::ExecutionClient,
    live::runner::get_exec_event_sender,
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
};
use nautilus_core::{
    DurationNanos, Params, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter, task::TaskGroup};
use nautilus_model::{
    accounts::AccountAny,
    enums::{OmsType, OrderSide, OrderStatus, OrderType, TimeInForce},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, TradeId, Venue, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Price, Quantity},
};
use parking_lot::Mutex;
use rust_decimal::Decimal;

use crate::{
    common::{
        consts::{
            KALSHI_CURRENCY, KALSHI_PRICE_PRECISION, KALSHI_SIZE_PRECISION, KALSHI_VENUE,
            MAX_PAGE_LIMIT,
        },
        enums::{KalshiBookSide, KalshiOrderStatus, KalshiSelfTradePrevention, KalshiTimeInForce},
    },
    config::KalshiExecClientConfig,
    execution::parse::{
        create_fill_report, create_order_status_report, create_position_status_report,
        fixed_point_precision, liquidity_side_for, order_side_for,
    },
    http::{
        client::KalshiHttpClient,
        error::Error,
        models::{
            KalshiAmendOrderRequest, KalshiBalanceResponse, KalshiCreateOrderRequest, KalshiFill,
            KalshiOrder,
        },
        parse::{
            historical_coverage_floor, instrument_id_for, parse_count_fp, parse_datetime_to_nanos,
            parse_dollars, parse_money_dollars, parse_price_dollars,
        },
    },
};

/// The largest number of order pages one reconciliation pass reads before reporting truncation.
const MAX_ORDER_PAGES: usize = 50;

/// The number of consecutive polls that may report an order as absent before tracking ends.
///
/// The venue's read path lags its write path, so an order the exchange has accepted can be missing
/// from a read for a short while; only a run of misses is evidence that the order is not there.
const MAX_ORDER_MISSES: u32 = 5;

/// The precision facts the client needs about an instrument.
///
/// The client keeps them rather than the instrument itself because a spawned task cannot borrow the
/// cache: it reads them once, when the instrument is registered.
#[derive(Clone, Copy, Debug)]
struct InstrumentMeta {
    price_precision: u8,
    size_precision: u8,
}

impl Default for InstrumentMeta {
    fn default() -> Self {
        Self {
            price_precision: KALSHI_PRICE_PRECISION,
            size_precision: KALSHI_SIZE_PRECISION,
        }
    }
}

/// An order the client submitted or adopted and has not seen reach a terminal state.
#[derive(Clone, Debug)]
struct TrackedOrder {
    /// The client order identifier, absent for an order the member placed outside the platform.
    client_order_id: Option<ClientOrderId>,
    instrument_id: InstrumentId,
    order_side: OrderSide,
    venue_order_id: VenueOrderId,
}

/// Builds the tracked order for an order the exchange reported.
fn tracked_order(order: &KalshiOrder) -> TrackedOrder {
    TrackedOrder {
        client_order_id: order.client_order_id.as_deref().map(ClientOrderId::from),
        instrument_id: instrument_id_for(&order.ticker),
        order_side: order_side_for(order.book_side),
        venue_order_id: VenueOrderId::from(order.order_id.as_str()),
    }
}

/// Returns whether the exchange reports an order in a state it will not leave.
fn order_is_terminal(status: &str) -> bool {
    status
        .trim()
        .parse::<KalshiOrderStatus>()
        .is_ok_and(|status| status.is_terminal())
}

/// What the client knows about the member's working orders and reported fills.
#[derive(Debug, Default)]
struct KalshiState {
    instruments: HashMap<InstrumentId, InstrumentMeta>,
    tracked: HashMap<VenueOrderId, TrackedOrder>,
    reported_fills: HashSet<TradeId>,
    reported_states: HashMap<VenueOrderId, (OrderStatus, Quantity)>,
    misses: HashMap<VenueOrderId, u32>,
}

impl KalshiState {
    /// Registers an instrument's precision facts.
    fn register_instrument(&mut self, instrument: &dyn Instrument) {
        self.instruments.insert(
            instrument.id(),
            InstrumentMeta {
                price_precision: instrument.price_precision(),
                size_precision: instrument.size_precision(),
            },
        );
    }

    /// Returns the facts known about an instrument, or the exchange's conventions.
    ///
    /// The fallback is the exchange's own price and size granularity, which is what an instrument
    /// the client has not been handed declares in practice.
    fn instrument_meta(&self, instrument_id: &InstrumentId) -> InstrumentMeta {
        self.instruments
            .get(instrument_id)
            .copied()
            .unwrap_or_default()
    }

    /// Tracks an order until it reaches a terminal state.
    fn track(&mut self, order: &TrackedOrder) {
        self.tracked.insert(order.venue_order_id, order.clone());
    }

    /// Stops tracking an order.
    fn untrack(&mut self, venue_order_id: VenueOrderId) {
        self.tracked.remove(&venue_order_id);
        self.reported_states.remove(&venue_order_id);
        self.misses.remove(&venue_order_id);
    }

    /// Returns whether a state differs from the one last reported for an order.
    ///
    /// A poll that finds the same state as the last report has nothing to tell the engine, so the
    /// client stays quiet rather than re-reporting every order on every poll. The state is only
    /// committed once it has actually been published, so a read that fails can be retried.
    fn state_changed(
        &self,
        venue_order_id: VenueOrderId,
        status: OrderStatus,
        filled_qty: Quantity,
    ) -> bool {
        self.reported_states
            .get(&venue_order_id)
            .is_none_or(|previous| *previous != (status, filled_qty))
    }

    /// Records the state that has been reported for an order.
    fn commit_reported_state(
        &mut self,
        venue_order_id: VenueOrderId,
        status: OrderStatus,
        filled_qty: Quantity,
    ) {
        self.reported_states
            .insert(venue_order_id, (status, filled_qty));
    }

    /// Records a fill, returning whether it had not been recorded before.
    fn record_fill(&mut self, trade_id: &TradeId) -> bool {
        self.reported_fills.insert(*trade_id)
    }

    /// Returns whether a fill has already been reported to the engine.
    fn fill_reported(&self, trade_id: &TradeId) -> bool {
        self.reported_fills.contains(trade_id)
    }

    /// Records that the exchange did not return an order, returning the consecutive miss count.
    fn record_miss(&mut self, venue_order_id: VenueOrderId) -> u32 {
        let misses = self.misses.entry(venue_order_id).or_insert(0);

        *misses += 1;
        *misses
    }

    /// Clears the consecutive miss count for an order.
    fn clear_misses(&mut self, venue_order_id: VenueOrderId) {
        self.misses.remove(&venue_order_id);
    }
}

/// The venue-facing handles a spawned task needs, all of which are cheap to clone and `Send`.
#[derive(Clone, Debug)]
struct KalshiExecContext {
    http_client: KalshiHttpClient,
    state: Arc<Mutex<KalshiState>>,
    emitter: ExecutionEventEmitter,
    client_id: ClientId,
    account_id: AccountId,
    clock: &'static AtomicTime,
}

impl KalshiExecContext {
    /// Returns the price precision to parse a Kalshi dollar value with.
    ///
    /// A price is parsed at the finest of the instrument's declared precision and the scale of the
    /// value itself, so a price is never rounded down to a coarser scale than it carries. The
    /// exchange quantizes prices to the market's grid, so the value's scale normally equals the
    /// instrument's precision; the two differ only when the instrument is not known, where the value
    /// is the only evidence of its own scale.
    fn price_precision(&self, instrument_id: &InstrumentId, value: &str) -> anyhow::Result<u8> {
        let scale = fixed_point_precision(value)?;

        Ok(match self.state.lock().instruments.get(instrument_id) {
            Some(meta) => meta.price_precision.max(scale),
            None => scale,
        })
    }

    /// Returns the size precision to parse a Kalshi contract count with.
    fn size_precision(&self, instrument_id: &InstrumentId) -> u8 {
        self.state
            .lock()
            .instrument_meta(instrument_id)
            .size_precision
    }

    fn ts_init(&self) -> UnixNanos {
        self.clock.get_time_ns()
    }

    /// Returns every order the member has, following pagination to its end.
    ///
    /// # Errors
    ///
    /// Returns an error if a page cannot be read or the pagination budget is exhausted.
    async fn paged_orders(&self, ticker: Option<&str>) -> anyhow::Result<Vec<KalshiOrder>> {
        let mut cursor: Option<String> = None;
        let mut orders = Vec::new();

        for _ in 0..MAX_ORDER_PAGES {
            let page = self
                .http_client
                .get_orders(ticker, Some(MAX_PAGE_LIMIT), cursor.as_deref())
                .await
                .map_err(|e| anyhow::anyhow!("Failed to fetch Kalshi orders: {e}"))?;

            orders.extend(page.orders);

            match page.cursor.filter(|value| !value.is_empty()) {
                Some(next) => cursor = Some(next),
                None => return Ok(orders),
            }
        }

        // The exchange holds the full order history, so a bounded read that does not terminate means
        // the page ceiling is too small rather than that the history is incomplete.
        anyhow::bail!("Kalshi orders pagination exceeded {MAX_ORDER_PAGES} pages")
    }

    /// Returns the member's working orders.
    ///
    /// # Errors
    ///
    /// Returns an error if the orders cannot be read.
    async fn open_orders(&self) -> anyhow::Result<Vec<KalshiOrder>> {
        Ok(self
            .paged_orders(None)
            .await?
            .into_iter()
            .filter(|order| {
                !order
                    .status
                    .trim()
                    .parse::<KalshiOrderStatus>()
                    .is_ok_and(|status| status.is_terminal())
            })
            .collect())
    }

    /// Returns the order status reports for the member's orders, with the number of orders the
    /// client could not report.
    ///
    /// # Errors
    ///
    /// Returns an error if the orders cannot be read.
    async fn order_status_reports(
        &self,
        instrument_id: Option<InstrumentId>,
        start: Option<UnixNanos>,
        open_only: bool,
    ) -> anyhow::Result<(Vec<OrderStatusReport>, usize)> {
        let ticker = instrument_id.map(|id| id.symbol.as_str().to_string());
        let orders = self.paged_orders(ticker.as_deref()).await?;
        let ts_init = self.ts_init();
        let mut reports = Vec::new();
        let mut skipped = 0;

        for order in &orders {
            let order_instrument_id = instrument_id_for(&order.ticker);

            if instrument_id.is_some_and(|id| id != order_instrument_id) {
                continue;
            }

            if open_only && order_is_terminal(&order.status) {
                continue;
            }
            // A window is bounded by the last change to an order, not by when it was created: an
            // order that was working before the window and reached a terminal state inside it is part
            // of what the window has to report.
            if let Some(start) = start {
                let last_update = order
                    .last_update_time
                    .as_deref()
                    .or(order.created_time.as_deref())
                    .and_then(|value| parse_datetime_to_nanos(value, "last_update_time").ok())
                    .unwrap_or(ts_init);

                if last_update < start {
                    continue;
                }
            }

            match self.order_report(order) {
                Ok(report) => reports.push(report),
                Err(e) => {
                    log::warn!(
                        "Skipping Kalshi order {} that cannot be reported: {e}",
                        order.order_id
                    );
                    skipped += 1;
                }
            }
        }

        Ok((reports, skipped))
    }

    /// Returns the fill reports for the member's fills, with the number of fills the client could not
    /// report.
    ///
    /// # Errors
    ///
    /// Returns an error if the fills cannot be read.
    async fn fill_reports(
        &self,
        instrument_id: Option<InstrumentId>,
        venue_order_id: Option<VenueOrderId>,
        start: Option<UnixNanos>,
    ) -> anyhow::Result<(Vec<FillReport>, usize)> {
        let ticker = instrument_id.map(|id| id.symbol.as_str().to_string());
        let min_ts = start.map(|start| (start.as_u64() / 1_000_000_000).cast_signed());
        let order_id = venue_order_id.as_ref().map(VenueOrderId::as_str);
        let fills = self
            .http_client
            .get_all_fills(ticker.as_deref(), order_id, min_ts, None)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch Kalshi fills: {e}"))?;
        let ts_init = self.ts_init();
        let mut reports = Vec::new();
        let mut skipped = 0;

        for fill in &fills {
            if instrument_id.is_some_and(|id| id != instrument_id_for(&fill.ticker)) {
                continue;
            }

            match self.fill_report(fill, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => {
                    log::warn!("Skipping Kalshi fill {}: {e}", fill.fill_id);
                    skipped += 1;
                }
            }
        }

        Ok((reports, skipped))
    }

    /// Builds the fill report for a venue fill.
    ///
    /// The price precision is derived from the fill's own value, so a fill is never parsed at the
    /// scale of some other price on the order.
    ///
    /// # Errors
    ///
    /// Returns an error if the fill's count, price, fee, or timestamp cannot be parsed.
    fn fill_report(&self, fill: &KalshiFill, ts_init: UnixNanos) -> anyhow::Result<FillReport> {
        let instrument_id = instrument_id_for(&fill.ticker);
        let precision = self.price_precision(&instrument_id, &fill.yes_price_dollars)?;

        Ok(create_fill_report(
            self.account_id,
            fill,
            precision,
            ts_init,
        )?)
    }

    /// Returns the position status reports for the member's positions.
    ///
    /// # Errors
    ///
    /// Returns an error if the positions cannot be read or one cannot be reported.
    async fn position_status_reports(
        &self,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let positions = self
            .http_client
            .get_all_positions()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch Kalshi positions: {e}"))?;
        let ts_init = self.ts_init();
        let mut reports = Vec::new();

        for position in &positions.market_positions {
            if instrument_id.is_some_and(|id| id != instrument_id_for(&position.ticker)) {
                continue;
            }

            reports.push(create_position_status_report(
                self.account_id,
                position,
                ts_init,
            )?);
        }

        Ok(reports)
    }

    /// Builds an execution mass status over an optional lookback window.
    ///
    /// The venue partitions its data into a live tier and a historical tier, and this client reads
    /// only the live tier. A window that reaches past the venue's cutoff is therefore covered only in
    /// part, and a report the client cannot build for any reason leaves the set with a hole in it:
    /// either case marks the report set incomplete, so a consumer does not treat a partial read as a
    /// full picture of the window.
    ///
    /// # Errors
    ///
    /// Returns an error if any of the three reads fails.
    async fn mass_status(
        &self,
        lookback_mins: Option<u64>,
        ts_init: UnixNanos,
    ) -> anyhow::Result<ExecutionMassStatus> {
        let start = lookback_mins
            .map(DurationNanos::try_from_mins)
            .transpose()?
            .map(|lookback| ts_init.saturating_sub(lookback));
        // Terminal orders are included: an order the venue canceled or filled while the client was
        // offline has no other event to transition it, and the live tier retains recently completed
        // orders.
        let (orders, skipped_orders) = self.order_status_reports(None, start, false).await?;
        let (fills, skipped_fills) = self.fill_reports(None, None, start).await?;
        let positions = self.position_status_reports(None).await?;
        let covered = self.covers_window(start).await;
        let complete = covered && skipped_orders == 0 && skipped_fills == 0;
        let mut mass_status = ExecutionMassStatus::new(
            self.client_id,
            self.account_id,
            Venue::from(KALSHI_VENUE),
            ts_init,
            None,
        );

        mass_status.add_order_reports(orders);
        mass_status.add_fill_reports(fills);
        mass_status.add_position_reports(positions);
        mass_status.set_report_window(start, complete);

        if !complete {
            log::warn!(
                "Kalshi mass status reports {skipped_orders} order(s) and {skipped_fills} fill(s) \
                 that could not be read, over a window the venue's live endpoints {} cover; \
                 the report set is marked incomplete",
                if covered { "do" } else { "do not fully" },
            );
        }

        Ok(mass_status)
    }

    /// Returns whether the venue's live endpoints cover the requested window in full.
    ///
    /// Kalshi moves orders and fills older than its cutoffs into a separate historical tier that this
    /// client does not read, so a window starting before the cutoff is only partly covered, as is an
    /// unbounded window that asks for the whole history. A cutoff that cannot be read leaves coverage
    /// unproven.
    async fn covers_window(&self, start: Option<UnixNanos>) -> bool {
        let Some(start) = start else {
            return false;
        };

        let cutoff = match self.http_client.get_historical_cutoff().await {
            Ok(cutoff) => cutoff,
            Err(e) => {
                log::warn!("Failed to read the Kalshi historical cutoffs: {e}");
                return false;
            }
        };
        let Some(floor) = historical_coverage_floor(&cutoff) else {
            log::warn!(
                "Kalshi returned historical cutoffs that cannot be read as timestamps, so mass \
                 status coverage is unproven"
            );
            return false;
        };

        start >= floor
    }

    /// Builds an order status report from an order the exchange reported.
    ///
    /// # Errors
    ///
    /// Returns an error if the order's price cannot be read or the report cannot be built.
    fn order_report(&self, order: &KalshiOrder) -> anyhow::Result<OrderStatusReport> {
        let instrument_id = instrument_id_for(&order.ticker);
        let precision = self.price_precision(&instrument_id, &order.yes_price_dollars)?;

        Ok(create_order_status_report(
            self.account_id,
            order,
            precision,
            self.ts_init(),
        )?)
    }

    /// Reports an order's state and the fills behind it.
    ///
    /// `force` reports the state even when it matches the last report, which the connect-time
    /// adoption needs: the client cannot tell whether the engine accepted an earlier report, so the
    /// first report for an adopted order is never suppressed.
    async fn report_order_state(&self, tracked: &TrackedOrder, order: &KalshiOrder, force: bool) {
        let ts_init = self.ts_init();
        let status_report = match self.order_report(order) {
            Ok(report) => report,
            Err(e) => {
                log::warn!(
                    "Failed to build a status report for Kalshi order {}: {e}",
                    order.order_id
                );
                return;
            }
        };

        if !force
            && !self.state.lock().state_changed(
                tracked.venue_order_id,
                status_report.order_status,
                status_report.filled_qty,
            )
        {
            return;
        }

        // The fills are read before the state is committed: a read that fails must leave the state
        // open, so the next poll retries rather than committing a state whose fills were never seen.
        let fills = match self
            .http_client
            .get_all_fills(Some(&order.ticker), Some(&order.order_id), None, None)
            .await
        {
            Ok(fills) => fills,
            Err(e) => {
                log::warn!(
                    "Failed to read the fills of Kalshi order {}: {e}",
                    order.order_id
                );
                self.emitter.send_order_status_report(status_report);
                return;
            }
        };
        let new_fills = self.covered_fills(&fills, status_report.filled_qty, ts_init);
        let new_trade_ids: Vec<TradeId> = new_fills.iter().map(|fill| fill.trade_id).collect();
        let terminal = matches!(
            status_report.order_status,
            OrderStatus::Canceled
                | OrderStatus::Rejected
                | OrderStatus::Filled
                | OrderStatus::Expired
        );

        self.state.lock().commit_reported_state(
            tracked.venue_order_id,
            status_report.order_status,
            status_report.filled_qty,
        );

        if new_fills.is_empty() {
            self.emitter.send_order_status_report(status_report);
        } else {
            self.emitter.send_order_with_fills(status_report, new_fills);
        }

        // A fill is recorded after it has been published. If the record came first and the task were
        // preempted before publishing, the next poll would suppress the fill and a consumer reading
        // the status would infer a replacement under a different identifier; a repeat published with
        // the same trade identifier is discarded by the consumer instead.
        for trade_id in new_trade_ids {
            self.state.lock().record_fill(&trade_id);
        }

        if terminal {
            self.state.lock().untrack(tracked.venue_order_id);
        }
    }

    /// Returns the fill reports that the reported order state accounts for.
    ///
    /// A status report published together with the fills behind it is read as a snapshot, and a
    /// consumer voids any fill the snapshot's cumulative filled quantity does not cover. Fills that
    /// arrived after the order was read are therefore left out of the bundle: the next poll reports
    /// them against an order read that agrees with them.
    fn covered_fills(
        &self,
        fills: &[KalshiFill],
        filled_qty: Quantity,
        ts_init: UnixNanos,
    ) -> Vec<FillReport> {
        let mut reports = Vec::new();
        let mut covered = Quantity::zero(filled_qty.precision);

        for fill in fills {
            let trade_id = TradeId::from(fill.fill_id.as_str());

            if self.state.lock().fill_reported(&trade_id) {
                continue;
            }
            let report = match self.fill_report(fill, ts_init) {
                Ok(report) => report,
                Err(e) => {
                    log::warn!("Skipping Kalshi fill {}: {e}", fill.fill_id);
                    continue;
                }
            };

            let next = covered + report.last_qty;

            if next > filled_qty {
                break;
            }
            covered = next;
            reports.push(report);
        }

        reports
    }

    /// Polls the exchange once for the state of every tracked order.
    async fn poll_tracked_orders(&self) {
        let tracked: Vec<TrackedOrder> = self.state.lock().tracked.values().cloned().collect();

        for tracked_order in tracked {
            match self
                .http_client
                .get_order(tracked_order.venue_order_id.as_str())
                .await
            {
                Ok(order) => {
                    self.state.lock().clear_misses(tracked_order.venue_order_id);
                    self.report_order_state(&tracked_order, &order, false).await;
                }
                // A 404 is not proof that the order is gone: the venue's read path lags its write
                // path, so an order the exchange has accepted can be missing for a short while. A run
                // of misses ends tracking, and that is reported rather than silent, because the
                // engine keeps the order until something else transitions it.
                Err(Error::Http { status: 404, .. }) => {
                    let misses = self.state.lock().record_miss(tracked_order.venue_order_id);

                    if misses >= MAX_ORDER_MISSES {
                        log::error!(
                            "Kalshi returned no record of order {} for {misses} consecutive polls; \
                             the client stops tracking it and the engine keeps its last state until \
                             an order query or a mass status reports the venue's",
                            tracked_order.venue_order_id
                        );
                        self.state.lock().untrack(tracked_order.venue_order_id);
                    } else {
                        log::warn!(
                            "Kalshi has no record of order {} yet ({misses} of {MAX_ORDER_MISSES} \
                             polls)",
                            tracked_order.venue_order_id
                        );
                    }
                }
                Err(e) => log::warn!(
                    "Failed to poll Kalshi order {}: {e}",
                    tracked_order.venue_order_id
                ),
            }
        }
    }

    /// Reads one order's state and reports it.
    async fn query_order_state(&self, order: &OrderAny, venue_order_id: &VenueOrderId) {
        let tracked = TrackedOrder {
            client_order_id: Some(order.client_order_id()),
            instrument_id: order.instrument_id(),
            order_side: order.order_side(),
            venue_order_id: *venue_order_id,
        };

        match self.http_client.get_order(venue_order_id.as_str()).await {
            Ok(order) => self.report_order_state(&tracked, &order, true).await,
            Err(e) => log::warn!("Failed to query Kalshi order {venue_order_id}: {e}"),
        }
    }

    /// Reads the member's balance and emits it as an account state.
    async fn fetch_and_emit_account_state(&self) {
        match self.http_client.get_balance().await {
            Ok(balance) => self.emit_account_state(&balance),
            Err(e) => log::warn!("Failed to fetch the Kalshi balance: {e}"),
        }
    }

    /// Emits the account state a venue balance describes.
    ///
    /// The fixed-point field carries the venue's own precision, which is finer than the cent field
    /// for a direct member; the portfolio value is only published in cents.
    fn emit_account_state(&self, balance: &KalshiBalanceResponse) {
        let currency = Currency::from(KALSHI_CURRENCY);
        let free = match parse_dollars(&balance.balance_dollars, "balance_dollars") {
            Ok(free) => free,
            Err(e) => {
                log::warn!("Failed to read the Kalshi balance: {e}");
                return;
            }
        };
        // Cash plus the value of the open positions is the account's total; the cash is what can be
        // traded, so the positions are the account's locked part.
        let total = free + Decimal::new(balance.portfolio_value, 2);
        let scale = u32::from(currency.precision);

        if free.normalize().scale() > scale || total.normalize().scale() > scale {
            log::warn!(
                "Kalshi reports a balance finer than {currency} represents ({free}); \
                 the account state carries the rounded amount"
            );
        }

        let account_balance = match AccountBalance::from_total_and_free(total, free, currency) {
            Ok(balance) => balance,
            Err(e) => {
                log::warn!("Failed to build a Kalshi account balance: {e}");
                return;
            }
        };
        let ts_event = UnixNanos::from(
            u64::try_from(balance.updated_ts)
                .unwrap_or_default()
                .saturating_mul(1_000_000_000),
        );

        self.emitter
            .emit_account_state(vec![account_balance], Vec::new(), true, ts_event, None);
    }

    /// Submits one order and reports the exchange's answer.
    async fn submit_order(&self, order: OrderAny, request: KalshiCreateOrderRequest) {
        let ts_event = self.ts_init();

        match self.http_client.create_order(&request).await {
            Ok(response) => {
                let venue_order_id = VenueOrderId::from(response.order_id.as_str());

                self.state.lock().track(&TrackedOrder {
                    client_order_id: Some(order.client_order_id()),
                    instrument_id: order.instrument_id(),
                    order_side: order.order_side(),
                    venue_order_id,
                });
                self.emitter
                    .emit_order_accepted(&order, venue_order_id, ts_event);
                self.emit_fills_for_order(&order, &venue_order_id).await;
            }
            Err(e) => {
                if e.is_retryable() {
                    // The request may have reached the exchange, so a rejection would misreport an
                    // order that can still trade and would clear the engine's reconciliation state for
                    // it. The create request carries the client order id, and the exchange answers a
                    // repeated submission of the same identifier with the original order, so the
                    // order is left to be resolved by a query or a mass status.
                    log::warn!(
                        "Ambiguous Kalshi submission outcome for {}: {e}; awaiting reconciliation",
                        order.client_order_id()
                    );
                } else {
                    log::warn!(
                        "Kalshi rejected submission of {}: {e}",
                        order.client_order_id()
                    );
                    self.emitter
                        .emit_order_rejected(&order, &e.to_string(), ts_event, false);
                }
            }
        }
    }

    /// Reports the fills the exchange holds for one order as order events.
    ///
    /// The submission response carries counts rather than fills, so the fills behind an immediate
    /// trade are read back. A fill is reported once: the trade ID is recorded here and again by the
    /// poll task, and the engine deduplicates it in any case.
    async fn emit_fills_for_order(&self, order: &OrderAny, venue_order_id: &VenueOrderId) {
        let ticker = order.instrument_id().symbol.as_str().to_string();
        let fills = match self
            .http_client
            .get_all_fills(Some(&ticker), Some(venue_order_id.as_str()), None, None)
            .await
        {
            Ok(fills) => fills,
            Err(e) => {
                log::warn!("Failed to fetch the fills of Kalshi order {venue_order_id}: {e}");
                return;
            }
        };

        if fills.is_empty() {
            return;
        }

        let instrument_id = order.instrument_id();
        let size_precision = self.size_precision(&instrument_id);
        let currency = Currency::from(KALSHI_CURRENCY);
        let mut published = Vec::new();

        for fill in &fills {
            let trade_id = TradeId::from(fill.fill_id.as_str());

            if self.state.lock().fill_reported(&trade_id) {
                continue;
            }

            let last_qty = match parse_count_fp(&fill.count_fp, size_precision, "count_fp") {
                Ok(quantity) => quantity,
                Err(e) => {
                    log::warn!("Skipping Kalshi fill {}: {e}", fill.fill_id);
                    continue;
                }
            };
            // The precision comes from the value being parsed, so no fill price is rounded to the
            // scale of another price on the order.
            let price_precision =
                match self.price_precision(&instrument_id, &fill.yes_price_dollars) {
                    Ok(precision) => precision,
                    Err(e) => {
                        log::warn!("Skipping Kalshi fill {}: {e}", fill.fill_id);
                        continue;
                    }
                };
            let last_px = match parse_price_dollars(
                &fill.yes_price_dollars,
                price_precision,
                "yes_price_dollars",
            ) {
                Ok(price) => price,
                Err(e) => {
                    log::warn!("Skipping Kalshi fill {}: {e}", fill.fill_id);
                    continue;
                }
            };
            let commission = fill
                .fee_cost
                .as_deref()
                .map(str::trim)
                .filter(|fee| !fee.is_empty())
                .and_then(|fee| parse_money_dollars(fee, "fee_cost").ok());
            let ts_event = fill
                .created_time
                .as_deref()
                .and_then(|value| parse_datetime_to_nanos(value, "created_time").ok())
                .unwrap_or_else(|| self.ts_init());

            self.emitter.emit_order_filled(
                order,
                *venue_order_id,
                None,
                trade_id,
                last_qty,
                last_px,
                currency,
                commission,
                liquidity_side_for(fill.is_taker),
                ts_event,
            );
            published.push(trade_id);
        }

        // A fill is recorded once it has been published: recording first would let a poll suppress a
        // fill that was never emitted, leaving a consumer to infer a replacement under a different
        // identifier.
        for trade_id in published {
            self.state.lock().record_fill(&trade_id);
        }
    }

    /// Cancels one order and reports the exchange's answer.
    async fn cancel_order(&self, order: OrderAny, venue_order_id: VenueOrderId) {
        let ts_event = self.ts_init();
        let ticker = order.instrument_id().symbol.as_str().to_string();

        // The market ticker is sent with the cancellation so the exchange routes it to the shard the
        // order lives on: an order ID alone identifies no shard, and the default is not necessarily
        // the order's.
        match self
            .http_client
            .cancel_order(venue_order_id.as_str(), &ticker)
            .await
        {
            Ok(_) => {
                // The order stays tracked. The receipt reports only how much the cancellation
                // reduced, so any fill that arrived before it is read back by the next poll, which
                // then reports the terminal state and stops tracking.
                self.emitter
                    .emit_order_canceled(&order, Some(venue_order_id), ts_event);
            }
            Err(e) => {
                log::warn!("Kalshi rejected the cancellation of {venue_order_id}: {e}");
                self.emitter.emit_order_cancel_rejected(
                    &order,
                    Some(venue_order_id),
                    &e.to_string(),
                    ts_event,
                );
            }
        }
    }

    /// Amends one order and reports the exchange's answer.
    async fn amend_order(
        &self,
        order: OrderAny,
        venue_order_id: VenueOrderId,
        quantity: Quantity,
        price: Price,
        request: KalshiAmendOrderRequest,
    ) {
        let ts_event = self.ts_init();

        match self
            .http_client
            .amend_order(venue_order_id.as_str(), &request)
            .await
        {
            Ok(_) => self.emitter.emit_order_updated(
                &order,
                venue_order_id,
                quantity,
                Some(price),
                None,
                None,
                ts_event,
            ),
            Err(e) => {
                log::warn!("Kalshi rejected the amendment of {venue_order_id}: {e}");
                self.emitter.emit_order_modify_rejected(
                    &order,
                    Some(venue_order_id),
                    &e.to_string(),
                    ts_event,
                );
            }
        }
    }
}

/// An execution client for the Kalshi exchange.
#[derive(Debug)]
pub struct KalshiExecutionClient {
    core: ExecutionClientCore,
    context: KalshiExecContext,
    self_trade_prevention: KalshiSelfTradePrevention,
    cancel_order_on_pause: Option<bool>,
    poll_interval: Duration,
    reconciliation: bool,
    tasks: TaskGroup,
    started: bool,
    poll_running: bool,
}

impl KalshiExecutionClient {
    /// Creates a new [`KalshiExecutionClient`].
    ///
    /// The HTTP client is built by the factory, which is where the credential is resolved.
    #[must_use]
    pub fn new(
        core: ExecutionClientCore,
        http_client: KalshiHttpClient,
        config: &KalshiExecClientConfig,
    ) -> Self {
        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            core.account_type,
            core.base_currency,
        );

        Self {
            context: KalshiExecContext {
                http_client,
                state: Arc::new(Mutex::new(KalshiState::default())),
                emitter,
                client_id: core.client_id,
                account_id: core.account_id,
                clock,
            },
            self_trade_prevention: config.self_trade_prevention,
            cancel_order_on_pause: config.cancel_order_on_pause,
            poll_interval: config.poll_interval(),
            reconciliation: config.reconciliation,
            core,
            tasks: TaskGroup::new(),
            started: false,
            poll_running: false,
        }
    }

    /// Returns the interval between order polls.
    #[must_use]
    pub const fn poll_interval(&self) -> Duration {
        self.poll_interval
    }

    /// Returns the number of orders the client is still tracking.
    #[must_use]
    pub fn tracked_orders(&self) -> usize {
        self.context.state.lock().tracked.len()
    }

    /// Spawns a task on the client's task group, reporting a failure to start it.
    fn spawn<F>(&self, description: &str, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        if let Err(e) = self.tasks.spawn(future) {
            log::error!("Failed to start the Kalshi {description}: {e}");
        }
    }

    /// Cancels every spawned task and leaves the client able to start again.
    ///
    /// A task group cannot be reopened once its admission closes, so the group is replaced: without
    /// that, a client that is stopped and started again accepts commands but issues no request, and
    /// its polling and reconciliation never resume.
    fn abort_tasks(&mut self) {
        self.tasks.abort();
        self.tasks = TaskGroup::new();
        self.poll_running = false;
    }

    /// Reports an order the client refused without sending a venue request.
    fn deny(&self, order: &OrderAny, reason: &str) {
        self.context.emitter.emit_order_denied(order, reason);
    }

    /// Spawns the poll task that reports the state of tracked orders.
    fn spawn_poll(&mut self) {
        if self.poll_running {
            return;
        }

        let context = self.context.clone();
        let interval = self.poll_interval;

        self.spawn("order poll", async move {
            loop {
                tokio::time::sleep(interval).await;
                context.poll_tracked_orders().await;
            }
        });
        self.poll_running = true;
    }

    /// Spawns the pass that adopts the member's working orders and positions.
    ///
    /// The exchange holds the state, so a session that starts against an account with resting orders
    /// has to learn about them before it can act: each working order is reported, its fills are read
    /// back, and it is tracked so the poll task keeps it current. An order the member placed outside
    /// the platform carries no client order identifier, so it is reported but not tracked, because
    /// no order event can be routed to it.
    fn spawn_reconciliation(&self) {
        let context = self.context.clone();

        self.spawn("reconciliation", async move {
            let orders = match context.open_orders().await {
                Ok(orders) => orders,
                Err(e) => {
                    log::warn!("Kalshi reconciliation could not read the working orders: {e}");
                    return;
                }
            };
            let mut tracked_count = 0;

            for order in &orders {
                let adopted = tracked_order(order);

                if adopted.client_order_id.is_some() {
                    context.state.lock().track(&adopted);
                    tracked_count += 1;
                }
                context.report_order_state(&adopted, order, true).await;
            }

            match context.position_status_reports(None).await {
                Ok(reports) => {
                    for report in reports {
                        context.emitter.send_position_report(report);
                    }
                }
                Err(e) => log::warn!("Kalshi reconciliation could not read the positions: {e}"),
            }

            log::info!(
                "Reconciled the Kalshi account: {tracked_count} of {} working orders tracked",
                orders.len(),
            );
        });
    }

    /// Builds the create request for a Nautilus order.
    ///
    /// Kalshi takes a book side, a fixed-point count, a fixed-point price, and a time in force, and
    /// has no market order type: a market order is sent as an immediately-or-canceled order at the
    /// far side of the book, which is the price the exchange accepts for it.
    fn build_create_request(&self, order: &OrderAny) -> anyhow::Result<KalshiCreateOrderRequest> {
        let instrument_id = order.instrument_id();
        let meta = self.context.state.lock().instrument_meta(&instrument_id);
        let side = match order.order_side() {
            OrderSide::Buy => KalshiBookSide::Bid,
            OrderSide::Sell => KalshiBookSide::Ask,
        };
        let time_in_force = match order.time_in_force() {
            // A market order is emulated as an order at the far side of the book, so it must not rest:
            // whatever the far side does not fill is canceled rather than left working.
            _ if order.order_type() == OrderType::Market => KalshiTimeInForce::ImmediateOrCancel,
            TimeInForce::Gtc | TimeInForce::Gtd => KalshiTimeInForce::GoodTillCanceled,
            TimeInForce::Ioc => KalshiTimeInForce::ImmediateOrCancel,
            TimeInForce::Fok => KalshiTimeInForce::FillOrKill,
            other => {
                anyhow::bail!(
                    "Kalshi has no {other} order in force: an order rests until it is canceled or \
                     the market closes, so use GTC, or GTD with an explicit expiry"
                );
            }
        };
        let price = match order.order_type() {
            OrderType::Limit => order
                .price()
                .ok_or_else(|| anyhow::anyhow!("A Kalshi limit order requires a price"))?,
            OrderType::Market => self.marketable_price(order)?,
            other => anyhow::bail!("Kalshi has no {other} order type"),
        };
        let expiration_time = match order.time_in_force() {
            TimeInForce::Gtd => order
                .expire_time()
                .map(|ts| (ts.as_u64() / 1_000_000_000).cast_signed()),
            _ => None,
        };

        Ok(KalshiCreateOrderRequest {
            ticker: instrument_id.symbol.as_str().to_string(),
            client_order_id: Some(order.client_order_id().to_string()),
            side,
            // A count and a price keep the exact value the order carries, at the precision the
            // instrument declares.
            count: quantity_string(order.quantity(), meta.size_precision),
            price: price_string(price, meta.price_precision),
            expiration_time,
            time_in_force,
            post_only: order.is_post_only().then_some(true),
            self_trade_prevention_type: self.self_trade_prevention,
            cancel_order_on_pause: self.cancel_order_on_pause,
            reduce_only: order.is_reduce_only().then_some(true),
        })
    }

    /// Returns the crossing price a market order is sent at, taken from the cached order book.
    ///
    /// An empty book leaves the price unknown, and the order is denied rather than sent at a price
    /// the exchange will reject.
    fn marketable_price(&self, order: &OrderAny) -> anyhow::Result<Price> {
        let instrument_id = order.instrument_id();
        let price = self
            .core
            .cache()
            .order_book(&instrument_id)
            .as_ref()
            .and_then(|book| match order.order_side() {
                OrderSide::Buy => book.best_ask_price(),
                OrderSide::Sell => book.best_bid_price(),
            });

        price.ok_or_else(|| {
            anyhow::anyhow!(
                "A Kalshi market order requires a price on the far side of the {instrument_id} book"
            )
        })
    }

    /// Submits one Nautilus order.
    fn submit(&self, order: &OrderAny) {
        let request = match self.build_create_request(order) {
            Ok(request) => request,
            Err(e) => {
                self.deny(order, &e.to_string());
                return;
            }
        };
        self.context.emitter.emit_order_submitted(order);

        let context = self.context.clone();
        let order = order.clone();

        self.spawn("submit task", async move {
            context.submit_order(order, request).await;
        });
    }

    /// Cancels one Nautilus order.
    fn cancel(&self, order: &OrderAny, venue_order_id: Option<VenueOrderId>) {
        let Some(venue_order_id) = venue_order_id.or_else(|| order.venue_order_id()) else {
            self.context.emitter.emit_order_cancel_rejected(
                order,
                None,
                "the order has no venue order id to cancel",
                self.context.ts_init(),
            );
            return;
        };
        let context = self.context.clone();
        let order = order.clone();

        self.spawn("cancel task", async move {
            context.cancel_order(order, venue_order_id).await;
        });
    }
}

/// Formats a quantity as a fixed-point count string.
fn quantity_string(quantity: Quantity, precision: u8) -> String {
    fixed_point_string(quantity.as_decimal(), precision)
}

/// Formats a price as a fixed-point dollar string.
fn price_string(price: Price, precision: u8) -> String {
    fixed_point_string(price.as_decimal(), precision)
}

/// Formats a decimal with exactly `precision` decimal places.
fn fixed_point_string(value: Decimal, precision: u8) -> String {
    let places = usize::from(precision);
    let mut formatted = value.round_dp(u32::from(precision)).to_string();

    match formatted.split_once('.') {
        Some((_, fraction)) => {
            let missing = places.saturating_sub(fraction.len());
            formatted.push_str(&"0".repeat(missing));
        }
        None if places > 0 => {
            formatted.push('.');
            formatted.push_str(&"0".repeat(places));
        }
        None => {}
    }

    formatted
}

#[async_trait(?Send)]
impl ExecutionClient for KalshiExecutionClient {
    fn is_connected(&self) -> bool {
        self.core.is_connected()
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> Venue {
        self.core.venue
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account_owned(&self.core.account_id)
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
        info: Option<Params>,
    ) -> anyhow::Result<()> {
        self.context
            .emitter
            .emit_account_state(balances, margins, reported, ts_event, info);

        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.started {
            return Ok(());
        }

        let sender = get_exec_event_sender();
        self.context.emitter.set_sender(sender);
        self.core.set_started();
        self.started = true;

        log::info!(
            "Started Kalshi execution client: client_id={}, account_id={}, base_url={}",
            self.core.client_id,
            self.core.account_id,
            self.context.http_client.base_url(),
        );

        self.spawn_poll();

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if !self.started {
            return Ok(());
        }

        self.abort_tasks();
        self.core.set_disconnected();
        self.core.set_stopped();
        self.started = false;

        log::info!("Stopped Kalshi execution client: {}", self.core.client_id);

        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.abort_tasks();
        self.started = false;

        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.reset()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
        }

        // Both reads are authenticated: a successful one proves the credential signs and the member
        // is reachable, which is what a connection means for a REST venue.
        let status = self
            .context
            .http_client
            .get_exchange_status()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to reach the Kalshi exchange: {e}"))?;

        self.context
            .http_client
            .get_balance()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to authenticate with Kalshi: {e}"))?;

        self.core.set_connected();

        log::info!(
            "Connected Kalshi execution client: exchange_active={}, trading_active={}",
            status.exchange_active,
            status.trading_active,
        );

        if self.reconciliation {
            self.spawn_reconciliation();
        }

        // The balance was read to prove the credential, and the engine needs it as an account: a
        // strategy trades against the account's balance, and a risk engine that finds no account
        // skips the checks that depend on one.
        self.context.fetch_and_emit_account_state().await;

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.core.is_disconnected() {
            return Ok(());
        }

        self.core.set_disconnected();
        log::info!(
            "Disconnected Kalshi execution client: {}",
            self.core.client_id
        );

        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.get_order(&cmd.client_order_id)?;

        self.submit(&order);

        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        for order in self.core.get_orders_for_list(&cmd.order_list)? {
            self.submit(&order);
        }

        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        let order = self.core.get_order(&cmd.client_order_id)?;
        let Some(venue_order_id) = cmd.venue_order_id.or_else(|| order.venue_order_id()) else {
            self.context.emitter.emit_order_modify_rejected(
                &order,
                None,
                "the order has no venue order id to amend",
                self.context.ts_init(),
            );
            return Ok(());
        };
        // An amendment carries the new total count, which is what the caller asked for when it named
        // a quantity and the order's own total when it did not.
        let quantity = cmd.quantity.unwrap_or_else(|| order.quantity());
        let Some(price) = cmd.price.or_else(|| order.price()) else {
            self.context.emitter.emit_order_modify_rejected(
                &order,
                Some(venue_order_id),
                "a Kalshi amendment requires a price",
                self.context.ts_init(),
            );
            return Ok(());
        };
        let meta = self
            .context
            .state
            .lock()
            .instrument_meta(&order.instrument_id());
        let request = KalshiAmendOrderRequest {
            ticker: order.instrument_id().symbol.as_str().to_string(),
            side: match order.order_side() {
                OrderSide::Buy => KalshiBookSide::Bid,
                OrderSide::Sell => KalshiBookSide::Ask,
            },
            price: price_string(price, meta.price_precision),
            count: quantity_string(quantity, meta.size_precision),
            client_order_id: Some(order.client_order_id().to_string()),
            updated_client_order_id: None,
        };
        let context = self.context.clone();

        self.spawn("amend task", async move {
            context
                .amend_order(order, venue_order_id, quantity, price, request)
                .await;
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        let order = self.core.get_order(&cmd.client_order_id)?;

        self.cancel(&order, cmd.venue_order_id);

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        let tracked: Vec<TrackedOrder> = {
            let state = self.context.state.lock();

            state
                .tracked
                .values()
                .filter(|tracked| tracked.instrument_id == cmd.instrument_id)
                .filter(|tracked| cmd.order_side.is_none_or(|side| side == tracked.order_side))
                .cloned()
                .collect()
        };

        for tracked_order in tracked {
            let Some(client_order_id) = tracked_order.client_order_id else {
                log::warn!(
                    "Cannot cancel Kalshi order {}: the client holds no order for it",
                    tracked_order.venue_order_id
                );
                continue;
            };
            let order = self.core.get_order(&client_order_id)?;
            self.cancel(&order, Some(tracked_order.venue_order_id));
        }

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        // The exchange publishes a batched cancel, but it cancels every working order rather than a
        // named set, so each order is canceled on its own and reports its own outcome.
        for cancel in &cmd.cancels {
            let order = self.core.get_order(&cancel.client_order_id)?;
            self.cancel(&order, cancel.venue_order_id);
        }

        Ok(())
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        let context = self.context.clone();

        self.spawn("account query", async move {
            context.fetch_and_emit_account_state().await;
        });

        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        let order = self.core.get_order(&cmd.client_order_id)?;
        let venue_order_id = cmd
            .venue_order_id
            .or_else(|| order.venue_order_id())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Cannot query Kalshi order {} without a venue order id",
                    cmd.client_order_id
                )
            })?;
        let context = self.context.clone();

        self.spawn("order query", async move {
            context.query_order_state(&order, &venue_order_id).await;
        });

        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        match (cmd.venue_order_id, cmd.client_order_id) {
            (Some(venue_order_id), _) => {
                let order = self
                    .context
                    .http_client
                    .get_order(venue_order_id.as_str())
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!("Failed to fetch Kalshi order {venue_order_id}: {e}")
                    })?;

                Ok(Some(self.context.order_report(&order)?))
            }
            (None, Some(client_order_id)) => {
                let (reports, _) = self
                    .context
                    .order_status_reports(cmd.instrument_id, None, false)
                    .await?;

                Ok(reports
                    .into_iter()
                    .find(|report| report.client_order_id == Some(client_order_id)))
            }
            (None, None) => {
                anyhow::bail!("Cannot query a Kalshi order without an order identifier")
            }
        }
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let (reports, _) = self
            .context
            .order_status_reports(cmd.instrument_id, cmd.start, cmd.open_only)
            .await?;

        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let (reports, _) = self
            .context
            .fill_reports(cmd.instrument_id, cmd.venue_order_id, cmd.start)
            .await?;

        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        self.context
            .position_status_reports(cmd.instrument_id)
            .await
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        let ts_init = self.context.ts_init();

        Ok(Some(
            self.context.mass_status(lookback_mins, ts_init).await?,
        ))
    }

    fn on_instrument(&mut self, instrument: InstrumentAny) {
        self.context.state.lock().register_instrument(&instrument);
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{enums::AccountType, identifiers::TraderId};
    use rstest::rstest;

    use super::*;

    const FILL_TEMPLATE: &str = r#"{
        "fill_id": "FILL_ID",
        "trade_id": "FILL_ID",
        "order_id": "order-1",
        "ticker": "KXHIGHNY-25JAN01-T50",
        "outcome_side": "yes",
        "book_side": "bid",
        "count_fp": "COUNT",
        "yes_price_dollars": "PRICE",
        "no_price_dollars": "0.6600",
        "is_taker": true,
        "fee_cost": "0.0100",
        "created_time": "2025-01-01T12:00:05Z"
    }"#;

    fn fill(fill_id: &str, count: &str, price: &str) -> KalshiFill {
        let json = FILL_TEMPLATE
            .replace("FILL_ID", fill_id)
            .replace("COUNT", count)
            .replace("PRICE", price);

        serde_json::from_str(&json).expect("fill fixture decodes")
    }

    fn context() -> KalshiExecContext {
        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TESTER-001"),
            AccountId::from("KALSHI-001"),
            AccountType::Cash,
            Some(Currency::from(KALSHI_CURRENCY)),
        );

        KalshiExecContext {
            http_client: KalshiHttpClient::new(None, None, None, None)
                .expect("a client without a credential"),
            state: Arc::new(Mutex::new(KalshiState::default())),
            emitter,
            client_id: ClientId::from("KALSHI-EXEC"),
            account_id: AccountId::from("KALSHI-001"),
            clock,
        }
    }

    #[rstest]
    #[case("0.4700", 2)]
    #[case("0.4000", 1)]
    #[case("0.3400", 2)]
    #[case("0.4701", 4)]
    fn test_price_precision_comes_from_the_value(#[case] value: &str, #[case] expected: u8) {
        // An instrument the client was not handed has no declared precision, and another price on the
        // same order is not evidence of this one's: parsing at a coarser scale would round it.
        let context = context();
        let precision = context
            .price_precision(&instrument_id_for("KXHIGHNY-25JAN01-T50"), value)
            .unwrap();

        assert_eq!(precision, expected);
    }

    #[rstest]
    fn test_price_precision_is_never_coarser_than_the_instrument() {
        let context = context();
        let market = crate::http::fixtures::market();
        let instrument = crate::http::parse::create_instrument_from_market(&market, ts_init())
            .expect("the market fixture is an instrument");
        let instrument_id = instrument.id();
        let instrument_precision = instrument.price_precision();
        context.state.lock().register_instrument(&instrument);

        // A value inside the instrument's grid is parsed at the instrument's precision, so prices of
        // one instrument stay comparable, and a value finer than the grid keeps its own scale.
        assert_eq!(
            context.price_precision(&instrument_id, "0.47").unwrap(),
            instrument_precision
        );
        assert_eq!(
            context.price_precision(&instrument_id, "0.4701").unwrap(),
            4
        );
    }

    fn ts_init() -> UnixNanos {
        UnixNanos::from(1_735_732_800_000_000_000u64)
    }

    #[rstest]
    fn test_covered_fills_stop_at_the_reported_filled_quantity() {
        let context = context();
        let fills = vec![
            fill("fill-1", "50.00", "0.3400"),
            fill("fill-2", "100.00", "0.3400"),
        ];
        let reports = context.covered_fills(&fills, Quantity::from("50.00"), UnixNanos::default());

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].trade_id, TradeId::from("fill-1"));
        // A fill is reported only once it has been published, so the bundle leaves nothing recorded:
        // recording here would let a poll suppress a fill that was never emitted.
        assert!(!context.state.lock().fill_reported(&TradeId::from("fill-1")));
        assert!(!context.state.lock().fill_reported(&TradeId::from("fill-2")));
    }

    #[rstest]
    fn test_covered_fills_are_empty_when_the_order_read_shows_no_fill() {
        let context = context();
        let fills = vec![fill("fill-1", "100.00", "0.3400")];
        let reports = context.covered_fills(&fills, Quantity::zero(2), UnixNanos::default());

        assert!(reports.is_empty());
    }
}
