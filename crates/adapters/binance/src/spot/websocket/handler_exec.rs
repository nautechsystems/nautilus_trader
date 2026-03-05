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

//! Binance Spot execution WebSocket feed handler.
//!
//! Processes JSON push events from the User Data Stream and correlates them
//! with registered order context (strategy, instrument, trader). Follows the
//! Futures handler pattern with Spot-specific simplifications:
//!
//! - No algo order tracking (Spot has no TWAP/VP/etc. algo orders)
//! - No modify/amendment support (Spot cancel-replace is a separate operation)
//! - No margin call or account config events
//! - Uses `AHashSet<i64>` for trade dedup (no `lru` dependency)
//! - Account positions map to Spot `outboundAccountPosition` events

use std::{
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::{AHashMap, AHashSet};
use nautilus_core::{UUID4, nanos::UnixNanos, time::AtomicTime};
use nautilus_model::{
    enums::{AccountType, LiquiditySide, OrderType},
    events::{AccountState, OrderCanceled, OrderFilled, OrderRejected},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, VenueOrderId,
    },
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use tokio::sync::mpsc::UnboundedReceiver;
use ustr::Ustr;

use crate::common::enums::BinanceOrderStatus;

use super::{
    messages_exec::{BinanceSpotUserDataEvent, NautilusSpotExecWsMessage, SpotExecHandlerCommand},
    types_exec::{BinanceSpotExecutionReport, BinanceSpotExecutionType},
};

/// Maximum number of trade IDs to cache for deduplication.
///
/// When the set exceeds this limit, it is cleared entirely. This provides
/// simple memory-bounded dedup without introducing an LRU dependency.
const MAX_SEEN_TRADES: usize = 10_000;

/// Converts a Binance millisecond timestamp (i64) to [`UnixNanos`].
///
/// Returns the clock's current time if the timestamp is non-positive,
/// preventing nonsensical values from a negative-to-unsigned cast.
fn millis_to_nanos(event_time_ms: i64, clock: &AtomicTime) -> UnixNanos {
    if event_time_ms > 0 {
        UnixNanos::from((event_time_ms as u64) * 1_000_000)
    } else {
        log::warn!("Non-positive event_time={event_time_ms}, using clock time");
        clock.get_time_ns()
    }
}

/// Cached instrument precision for constructing fill events with correct
/// decimal places.
#[derive(Debug, Clone, Copy)]
pub struct InstrumentPrecision {
    /// Number of decimal places for price values.
    pub price_precision: u8,
    /// Number of decimal places for quantity values.
    pub qty_precision: u8,
}

/// Binance Spot execution WebSocket feed handler.
///
/// Processes User Data Stream events and maintains pending order state to
/// correlate WebSocket updates with the original order context (trader,
/// strategy, instrument). Emits normalized Nautilus execution events.
///
/// # Architecture
///
/// The handler receives two streams via unbounded channels:
///
/// - **Commands** (`cmd_rx`): Registration of order/cancel context before HTTP
///   submission, and instrument precision caching.
/// - **Events** (`event_rx`): Deserialized User Data Stream events from the
///   WebSocket client.
///
/// The `next()` method selects between both channels and returns normalized
/// Nautilus events for upstream consumption by the execution client.
pub struct BinanceSpotExecWsFeedHandler {
    clock: &'static AtomicTime,
    trader_id: TraderId,
    account_id: AccountId,
    signal: Arc<AtomicBool>,
    cmd_rx: UnboundedReceiver<SpotExecHandlerCommand>,
    event_rx: UnboundedReceiver<BinanceSpotUserDataEvent>,
    pending_place_requests: AHashMap<ClientOrderId, (TraderId, StrategyId, InstrumentId)>,
    pending_cancel_requests:
        AHashMap<ClientOrderId, (TraderId, StrategyId, InstrumentId, Option<VenueOrderId>)>,
    active_orders: AHashMap<ClientOrderId, (TraderId, StrategyId, InstrumentId)>,
    instruments_cache: AHashMap<String, InstrumentPrecision>,
    seen_trades: AHashSet<i64>,
}

impl Debug for BinanceSpotExecWsFeedHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BinanceSpotExecWsFeedHandler))
            .field("trader_id", &self.trader_id)
            .field("account_id", &self.account_id)
            .field("pending_place_requests", &self.pending_place_requests.len())
            .field(
                "pending_cancel_requests",
                &self.pending_cancel_requests.len(),
            )
            .field("active_orders", &self.active_orders.len())
            .field("instruments_cache", &self.instruments_cache.len())
            .field("seen_trades", &self.seen_trades.len())
            .finish_non_exhaustive()
    }
}

impl BinanceSpotExecWsFeedHandler {
    /// Creates a new [`BinanceSpotExecWsFeedHandler`] instance.
    #[must_use]
    pub fn new(
        clock: &'static AtomicTime,
        trader_id: TraderId,
        account_id: AccountId,
        signal: Arc<AtomicBool>,
        cmd_rx: UnboundedReceiver<SpotExecHandlerCommand>,
        event_rx: UnboundedReceiver<BinanceSpotUserDataEvent>,
    ) -> Self {
        Self {
            clock,
            trader_id,
            account_id,
            signal,
            cmd_rx,
            event_rx,
            pending_place_requests: AHashMap::new(),
            pending_cancel_requests: AHashMap::new(),
            active_orders: AHashMap::new(),
            instruments_cache: AHashMap::new(),
            seen_trades: AHashSet::new(),
        }
    }

    /// Processes commands and events, returning the next normalized execution event.
    ///
    /// Returns `None` when the shutdown signal is set or both channels are closed.
    pub async fn next(&mut self) -> Option<NautilusSpotExecWsMessage> {
        loop {
            if self.signal.load(Ordering::Relaxed) {
                return None;
            }

            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    self.handle_command(cmd);
                }
                Some(event) = self.event_rx.recv() => {
                    if let Some(msg) = self.handle_event(event) {
                        return Some(msg);
                    }
                }
                else => {
                    return None;
                }
            }
        }
    }

    /// Dispatches a command to the appropriate registration handler.
    fn handle_command(&mut self, cmd: SpotExecHandlerCommand) {
        match cmd {
            SpotExecHandlerCommand::RegisterOrder {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            } => {
                self.pending_place_requests
                    .insert(client_order_id, (trader_id, strategy_id, instrument_id));
                self.active_orders
                    .insert(client_order_id, (trader_id, strategy_id, instrument_id));
            }
            SpotExecHandlerCommand::RegisterCancel {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
                venue_order_id,
            } => {
                self.pending_cancel_requests.insert(
                    client_order_id,
                    (trader_id, strategy_id, instrument_id, venue_order_id),
                );
            }
            SpotExecHandlerCommand::CacheInstrument {
                symbol,
                price_precision,
                qty_precision,
            } => {
                self.instruments_cache.insert(
                    symbol,
                    InstrumentPrecision {
                        price_precision,
                        qty_precision,
                    },
                );
            }
        }
    }

    /// Dispatches a User Data Stream event to the appropriate handler.
    fn handle_event(
        &mut self,
        event: BinanceSpotUserDataEvent,
    ) -> Option<NautilusSpotExecWsMessage> {
        match event {
            BinanceSpotUserDataEvent::ExecutionReport(report) => {
                self.handle_execution_report(&report)
            }
            BinanceSpotUserDataEvent::AccountPosition(position) => {
                self.handle_account_position(&position)
            }
            BinanceSpotUserDataEvent::Reconnected => self.handle_reconnect(),
        }
    }

    /// Handles an execution report by dispatching on the execution type.
    fn handle_execution_report(
        &mut self,
        report: &BinanceSpotExecutionReport,
    ) -> Option<NautilusSpotExecWsMessage> {
        match report.execution_type {
            BinanceSpotExecutionType::New => {
                // Move from pending to active on acceptance.
                // HTTP OrderAccepted is already emitted by the execution client,
                // so we don't emit a duplicate here.
                let client_order_id = ClientOrderId::new(&report.client_order_id);
                self.pending_place_requests.remove(&client_order_id);
                None
            }
            BinanceSpotExecutionType::Trade => self.handle_trade_fill(report),
            BinanceSpotExecutionType::Canceled
            | BinanceSpotExecutionType::Expired
            | BinanceSpotExecutionType::TradePrevention => self.handle_cancel(report),
            BinanceSpotExecutionType::Rejected => self.handle_rejection(report),
            BinanceSpotExecutionType::Replaced => {
                log::debug!(
                    "Replaced execution report: symbol={}, client_order_id={}",
                    report.symbol,
                    report.client_order_id
                );
                None
            }
            BinanceSpotExecutionType::Unknown => {
                log::warn!(
                    "Unknown execution type in report: symbol={}, client_order_id={}",
                    report.symbol,
                    report.client_order_id
                );
                None
            }
        }
    }

    /// Handles a trade fill execution report.
    ///
    /// Deduplicates by trade ID, constructs an `OrderFilled` event with
    /// correct precision from the instruments cache, and removes the order
    /// from tracking if fully filled.
    fn handle_trade_fill(
        &mut self,
        report: &BinanceSpotExecutionReport,
    ) -> Option<NautilusSpotExecWsMessage> {
        // Dedup by trade ID — clear set when it grows too large
        if self.seen_trades.len() >= MAX_SEEN_TRADES {
            self.seen_trades.clear();
        }
        if !self.seen_trades.insert(report.trade_id) {
            log::debug!(
                "Duplicate trade_id={}, skipping fill",
                report.trade_id
            );
            return None;
        }

        let client_order_id = ClientOrderId::new(&report.client_order_id);
        let venue_order_id = VenueOrderId::new(report.order_id.to_string());
        let (trader_id, strategy_id, instrument_id) =
            self.get_order_context(&client_order_id, &report.symbol);

        // Look up instrument precision from cache
        let Some(precision) = self.instruments_cache.get(&report.symbol) else {
            log::error!(
                "Instrument not found for fill: {}, skipping to avoid precision mismatch",
                report.symbol
            );
            return None;
        };
        let price_precision = precision.price_precision;
        let qty_precision = precision.qty_precision;

        let ts_event = millis_to_nanos(report.event_time, self.clock);
        let ts_init = self.clock.get_time_ns();

        // Parse critical fill values — skip event on parse failure to avoid
        // emitting a zero-quantity or zero-price fill to the strategy.
        let last_qty: f64 = match report.last_qty.parse() {
            Ok(v) => v,
            Err(e) => {
                log::error!(
                    "Failed to parse last_qty '{}': {e}, skipping fill",
                    report.last_qty
                );
                return None;
            }
        };
        let last_px: f64 = match report.last_price.parse() {
            Ok(v) => v,
            Err(e) => {
                log::error!(
                    "Failed to parse last_price '{}': {e}, skipping fill",
                    report.last_price
                );
                return None;
            }
        };
        let commission: f64 = report.commission.parse().unwrap_or_else(|e| {
            log::warn!("Failed to parse commission '{}': {e}", report.commission);
            0.0
        });

        let commission_currency = report
            .commission_asset
            .as_ref()
            .map_or_else(|| {
                log::debug!("No commission_asset in fill, defaulting to USDT");
                Currency::USDT()
            }, |a| Currency::from(a.as_str()));

        let order_side = report.side.into();
        let order_type: OrderType = report.order_type.into();

        let liquidity_side = if report.is_maker {
            LiquiditySide::Maker
        } else {
            LiquiditySide::Taker
        };

        let event = OrderFilled::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            self.account_id,
            TradeId::new(report.trade_id.to_string()),
            order_side,
            order_type,
            Quantity::new(last_qty, qty_precision),
            Price::new(last_px, price_precision),
            commission_currency,
            liquidity_side,
            UUID4::new(),
            ts_event,
            ts_init,
            false, // reconciliation
            None,  // position_id
            Some(Money::new(commission, commission_currency)),
        );

        // Remove from active_orders if fully filled (use Binance's typed status
        // enum instead of f64 arithmetic to avoid floating-point precision issues)
        if report.order_status == BinanceOrderStatus::Filled {
            self.active_orders.remove(&client_order_id);
            log::debug!(
                "Order fully filled: client_order_id={client_order_id}, \
                 venue_order_id={venue_order_id}"
            );
        }

        Some(NautilusSpotExecWsMessage::OrderFilled(event))
    }

    /// Handles a cancel/expired/trade-prevention execution report.
    ///
    /// For Binance Spot CANCELED events, the `"c"` field contains the cancel
    /// request ID (auto-generated by Binance), while `"C"` (`orig_client_order_id`)
    /// contains the original order's client order ID. Uses `"C"` when non-empty
    /// for correct order context lookup.
    fn handle_cancel(
        &mut self,
        report: &BinanceSpotExecutionReport,
    ) -> Option<NautilusSpotExecWsMessage> {
        // Resolve the original order ID: "C" for cancel-replace/exchange-cancel,
        // "c" for direct user cancels where "C" is empty.
        let client_order_id = if report.orig_client_order_id.is_empty() {
            ClientOrderId::new(&report.client_order_id)
        } else {
            ClientOrderId::new(&report.orig_client_order_id)
        };
        let venue_order_id = VenueOrderId::new(report.order_id.to_string());
        let (trader_id, strategy_id, instrument_id) =
            self.get_order_context(&client_order_id, &report.symbol);

        let ts_event = millis_to_nanos(report.event_time, self.clock);
        let ts_init = self.clock.get_time_ns();

        let event = OrderCanceled::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            UUID4::new(),
            ts_event,
            ts_init,
            false, // reconciliation
            Some(venue_order_id),
            Some(self.account_id),
        );

        // Clean up all tracking maps
        self.pending_place_requests.remove(&client_order_id);
        self.pending_cancel_requests.remove(&client_order_id);
        self.active_orders.remove(&client_order_id);

        Some(NautilusSpotExecWsMessage::OrderCanceled(event))
    }

    /// Handles a rejected execution report.
    fn handle_rejection(
        &mut self,
        report: &BinanceSpotExecutionReport,
    ) -> Option<NautilusSpotExecWsMessage> {
        let client_order_id = ClientOrderId::new(&report.client_order_id);
        let (trader_id, strategy_id, instrument_id) =
            self.get_order_context(&client_order_id, &report.symbol);

        let ts_event = millis_to_nanos(report.event_time, self.clock);
        let ts_init = self.clock.get_time_ns();

        let reject_reason = &report.reject_reason;
        let event = OrderRejected::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            self.account_id,
            Ustr::from(reject_reason),
            UUID4::new(),
            ts_event,
            ts_init,
            false, // reconciliation
            false, // due_post_only
        );

        // Clean up tracking maps
        self.pending_place_requests.remove(&client_order_id);
        self.active_orders.remove(&client_order_id);

        Some(NautilusSpotExecWsMessage::OrderRejected(event))
    }

    /// Looks up order context from pending/active maps.
    ///
    /// Falls back to `EXTERNAL` strategy for untracked orders (e.g., orders
    /// placed before restart or created externally on the exchange). The
    /// instrument ID is constructed from the symbol with `.BINANCE` suffix.
    fn get_order_context(
        &self,
        client_order_id: &ClientOrderId,
        symbol: &str,
    ) -> (TraderId, StrategyId, InstrumentId) {
        // Check pending place requests first
        if let Some((trader_id, strategy_id, instrument_id)) =
            self.pending_place_requests.get(client_order_id)
        {
            return (*trader_id, *strategy_id, *instrument_id);
        }

        // Then check active orders
        if let Some((trader_id, strategy_id, instrument_id)) =
            self.active_orders.get(client_order_id)
        {
            return (*trader_id, *strategy_id, *instrument_id);
        }

        // Fall back to EXTERNAL for untracked orders (restart, external creation)
        let instrument_id = InstrumentId::from(format!("{symbol}.BINANCE").as_str());
        log::debug!(
            "Order context not found for {client_order_id}, \
             using EXTERNAL with {instrument_id}"
        );
        (self.trader_id, StrategyId::new("EXTERNAL"), instrument_id)
    }

    /// Handles an account position update by converting balances to an
    /// `AccountState` event.
    fn handle_account_position(
        &mut self,
        position: &super::types_exec::BinanceSpotAccountPosition,
    ) -> Option<NautilusSpotExecWsMessage> {
        let ts_event = millis_to_nanos(position.event_time, self.clock);
        let ts_init = self.clock.get_time_ns();

        let balances: Vec<AccountBalance> = position
            .balances
            .iter()
            .filter_map(|b| {
                let free: f64 = b.free.parse().unwrap_or_else(|e| {
                    log::warn!("Failed to parse free balance '{}': {e}", b.free);
                    0.0
                });
                let locked: f64 = b.locked.parse().unwrap_or_else(|e| {
                    log::warn!("Failed to parse locked balance '{}': {e}", b.locked);
                    0.0
                });
                let total = free + locked;

                if total == 0.0 {
                    return None;
                }

                let currency = Currency::from(b.asset.as_str());
                Some(AccountBalance::new(
                    Money::new(total, currency),
                    Money::new(locked, currency),
                    Money::new(free, currency),
                ))
            })
            .collect();

        if balances.is_empty() {
            return None;
        }

        let event = AccountState::new(
            self.account_id,
            AccountType::Cash, // Spot accounts are cash accounts
            balances,
            vec![], // No margin balances for spot
            true,   // is_reported (from exchange)
            UUID4::new(),
            ts_event,
            ts_init,
            None, // base_currency
        );

        Some(NautilusSpotExecWsMessage::AccountUpdate(event))
    }

    /// Handles a WebSocket reconnection event.
    ///
    /// Drains pending place requests without emitting false rejections (the
    /// orders may still be live on the exchange). Active orders are preserved
    /// since they may receive future updates.
    fn handle_reconnect(&mut self) -> Option<NautilusSpotExecWsMessage> {
        let pending_count = self.pending_place_requests.len();
        let cancel_count = self.pending_cancel_requests.len();

        if pending_count > 0 || cancel_count > 0 {
            log::warn!(
                "Reconnected: draining {pending_count} pending place requests \
                 and {cancel_count} pending cancel requests without false rejections"
            );
        }

        // Drain pending requests — orders may still be live on the exchange
        self.pending_place_requests.clear();
        self.pending_cancel_requests.clear();
        // Keep active_orders — they may receive future updates

        Some(NautilusSpotExecWsMessage::Reconnected)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::AtomicBool,
    };

    use nautilus_core::{nanos::UnixNanos, time::AtomicTime};
    use nautilus_model::{
        enums::OrderSide,
        identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    };
    use rstest::rstest;
    use tokio::sync::mpsc;

    use super::*;
    use crate::common::enums::{BinanceOrderStatus, BinanceSide, BinanceTimeInForce};
    use crate::spot::enums::BinanceSpotOrderType;
    use crate::spot::websocket::types_exec::{
        BinanceSpotExecutionReport, BinanceSpotExecutionType,
    };

    /// Leaks an `AtomicTime` to get a `&'static` reference for tests.
    fn leaked_clock() -> &'static AtomicTime {
        Box::leak(Box::new(AtomicTime::new(
            false,
            UnixNanos::from(1_000_000_000u64),
        )))
    }

    /// Creates a handler with connected channels for testing.
    fn test_handler() -> (
        BinanceSpotExecWsFeedHandler,
        mpsc::UnboundedSender<SpotExecHandlerCommand>,
        mpsc::UnboundedSender<BinanceSpotUserDataEvent>,
    ) {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let signal = Arc::new(AtomicBool::new(false));

        let handler = BinanceSpotExecWsFeedHandler::new(
            leaked_clock(),
            TraderId::new("TESTER-001"),
            AccountId::new("BINANCE-001"),
            signal,
            cmd_rx,
            event_rx,
        );

        (handler, cmd_tx, event_tx)
    }

    /// Creates a minimal execution report for testing.
    fn make_trade_report(
        client_order_id: &str,
        trade_id: i64,
        symbol: &str,
        last_qty: &str,
        last_price: &str,
        orig_qty: &str,
        cum_qty: &str,
    ) -> BinanceSpotExecutionReport {
        BinanceSpotExecutionReport {
            event_time: 1_772_494_860_000,
            symbol: symbol.to_string(),
            client_order_id: client_order_id.to_string(),
            side: BinanceSide::Buy,
            order_type: BinanceSpotOrderType::Limit,
            time_in_force: BinanceTimeInForce::Gtc,
            orig_qty: orig_qty.to_string(),
            price: "2045.50".to_string(),
            stop_price: "0.0".to_string(),
            iceberg_qty: "0.0".to_string(),
            order_list_id: -1,
            orig_client_order_id: String::new(),
            execution_type: BinanceSpotExecutionType::Trade,
            order_status: BinanceOrderStatus::Filled,
            reject_reason: "NONE".to_string(),  // reject_reason stays String (free-form text)
            order_id: 9_399_999_776,
            last_qty: last_qty.to_string(),
            cumulative_filled_qty: cum_qty.to_string(),
            last_price: last_price.to_string(),
            commission: "0.00001234".to_string(),
            commission_asset: Some("ETH".to_string()),
            transaction_time: 1_772_494_860_000,
            trade_id,
            is_working: false,
            is_maker: true,
            order_creation_time: 1_772_494_856_997,
            cumulative_quote_qty: "20.455".to_string(),
            last_quote_qty: "20.455".to_string(),
            quote_order_qty: "0.0".to_string(),
            working_time: Some(1_772_494_856_997),
            self_trade_prevention_mode: Some("EXPIRE_MAKER".to_string()),
        }
    }

    #[rstest]
    fn test_register_order_populates_pending_and_active() {
        let (mut handler, cmd_tx, _event_tx) = test_handler();

        let client_order_id = ClientOrderId::new("TEST-001");
        let trader_id = TraderId::new("TESTER-001");
        let strategy_id = StrategyId::new("CrossMM-001");
        let instrument_id = InstrumentId::from("ETHUSDC.BINANCE");

        cmd_tx
            .send(SpotExecHandlerCommand::RegisterOrder {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            })
            .expect("send failed");

        // Process the command synchronously via handle_command
        let cmd = handler.cmd_rx.try_recv().expect("no command");
        handler.handle_command(cmd);

        assert!(handler.pending_place_requests.contains_key(&client_order_id));
        assert!(handler.active_orders.contains_key(&client_order_id));
    }

    #[rstest]
    fn test_execution_report_new_moves_pending_to_active() {
        let (mut handler, _cmd_tx, _event_tx) = test_handler();

        let client_order_id = ClientOrderId::new("TEST-001");
        let trader_id = TraderId::new("TESTER-001");
        let strategy_id = StrategyId::new("CrossMM-001");
        let instrument_id = InstrumentId::from("ETHUSDC.BINANCE");

        // Pre-register the order
        handler
            .pending_place_requests
            .insert(client_order_id, (trader_id, strategy_id, instrument_id));
        handler
            .active_orders
            .insert(client_order_id, (trader_id, strategy_id, instrument_id));

        // Create a NEW execution report
        let mut report = make_trade_report("TEST-001", -1, "ETHUSDC", "0", "0", "0.01", "0");
        report.execution_type = BinanceSpotExecutionType::New;
        report.order_status = BinanceOrderStatus::New;

        let result = handler.handle_execution_report(&report);

        // NEW should return None (HTTP already emitted OrderAccepted)
        assert!(result.is_none());
        // Pending should be removed, active should remain
        assert!(!handler.pending_place_requests.contains_key(&client_order_id));
        assert!(handler.active_orders.contains_key(&client_order_id));
    }

    #[rstest]
    fn test_trade_fill_emits_order_filled() {
        let (mut handler, _cmd_tx, _event_tx) = test_handler();

        let client_order_id = ClientOrderId::new("TEST-001");
        let trader_id = TraderId::new("TESTER-001");
        let strategy_id = StrategyId::new("CrossMM-001");
        let instrument_id = InstrumentId::from("ETHUSDC.BINANCE");

        // Register order and cache instrument precision
        handler
            .active_orders
            .insert(client_order_id, (trader_id, strategy_id, instrument_id));
        handler.instruments_cache.insert(
            "ETHUSDC".to_string(),
            InstrumentPrecision {
                price_precision: 2,
                qty_precision: 5,
            },
        );

        let report = make_trade_report(
            "TEST-001",
            123_456_789,
            "ETHUSDC",
            "0.01",
            "2045.50",
            "0.01",
            "0.01",
        );

        let result = handler.handle_execution_report(&report);
        assert!(result.is_some());

        match result.unwrap() {
            NautilusSpotExecWsMessage::OrderFilled(filled) => {
                assert_eq!(filled.client_order_id, client_order_id);
                assert_eq!(filled.strategy_id, strategy_id);
                assert_eq!(filled.instrument_id, instrument_id);
                assert_eq!(filled.order_side, OrderSide::Buy);
                assert_eq!(filled.liquidity_side, LiquiditySide::Maker);
                assert_eq!(filled.order_type, OrderType::Limit);
                assert!(!filled.reconciliation);
            }
            other => panic!("Expected OrderFilled, got {other:?}"),
        }

        // Fully filled → removed from active
        assert!(!handler.active_orders.contains_key(&client_order_id));
    }

    #[rstest]
    fn test_duplicate_trade_id_skipped() {
        let (mut handler, _cmd_tx, _event_tx) = test_handler();

        let client_order_id = ClientOrderId::new("TEST-001");
        let trader_id = TraderId::new("TESTER-001");
        let strategy_id = StrategyId::new("CrossMM-001");
        let instrument_id = InstrumentId::from("ETHUSDC.BINANCE");

        handler
            .active_orders
            .insert(client_order_id, (trader_id, strategy_id, instrument_id));
        handler.instruments_cache.insert(
            "ETHUSDC".to_string(),
            InstrumentPrecision {
                price_precision: 2,
                qty_precision: 5,
            },
        );

        // Two partial fills with the same trade_id
        let report = make_trade_report(
            "TEST-001",
            123_456_789,
            "ETHUSDC",
            "0.005",
            "2045.50",
            "0.01",
            "0.005",
        );

        let result1 = handler.handle_trade_fill(&report);
        assert!(result1.is_some());

        // Re-insert active order for the second attempt
        handler
            .active_orders
            .insert(client_order_id, (trader_id, strategy_id, instrument_id));

        let result2 = handler.handle_trade_fill(&report);
        assert!(result2.is_none(), "Duplicate trade_id should be skipped");
    }

    #[rstest]
    fn test_cancel_emits_order_canceled() {
        let (mut handler, _cmd_tx, _event_tx) = test_handler();

        let client_order_id = ClientOrderId::new("TEST-001");
        let trader_id = TraderId::new("TESTER-001");
        let strategy_id = StrategyId::new("CrossMM-001");
        let instrument_id = InstrumentId::from("ETHUSDC.BINANCE");

        handler
            .active_orders
            .insert(client_order_id, (trader_id, strategy_id, instrument_id));
        handler
            .pending_cancel_requests
            .insert(client_order_id, (trader_id, strategy_id, instrument_id, None));

        let mut report = make_trade_report("TEST-001", -1, "ETHUSDC", "0", "0", "0.01", "0");
        report.execution_type = BinanceSpotExecutionType::Canceled;
        report.order_status = BinanceOrderStatus::Canceled;

        let result = handler.handle_execution_report(&report);
        assert!(result.is_some());

        match result.unwrap() {
            NautilusSpotExecWsMessage::OrderCanceled(canceled) => {
                assert_eq!(canceled.client_order_id, client_order_id);
                assert_eq!(canceled.strategy_id, strategy_id);
                assert!(canceled.venue_order_id.is_some());
                assert!(canceled.account_id.is_some());
            }
            other => panic!("Expected OrderCanceled, got {other:?}"),
        }

        // All maps cleaned up
        assert!(!handler.active_orders.contains_key(&client_order_id));
        assert!(!handler.pending_cancel_requests.contains_key(&client_order_id));
    }

    #[rstest]
    fn test_cancel_with_orig_client_order_id_resolves_original() {
        let (mut handler, _cmd_tx, _event_tx) = test_handler();

        // Register order under the ORIGINAL client_order_id
        let original_id = ClientOrderId::new("MY-ORDER-001");
        let trader_id = TraderId::new("TESTER-001");
        let strategy_id = StrategyId::new("CrossMM-001");
        let instrument_id = InstrumentId::from("ETHUSDC.BINANCE");

        handler
            .active_orders
            .insert(original_id, (trader_id, strategy_id, instrument_id));

        // Binance CANCELED event: "c" = auto-generated cancel ID,
        // "C" = original order ID
        let mut report = make_trade_report(
            "PAyFKkUBxfnY0fqEogcln5", // "c" — auto-generated
            -1,
            "ETHUSDC",
            "0",
            "0",
            "0.01",
            "0",
        );
        report.execution_type = BinanceSpotExecutionType::Canceled;
        report.order_status = BinanceOrderStatus::Canceled;
        report.orig_client_order_id = "MY-ORDER-001".to_string(); // "C" — original

        let result = handler.handle_execution_report(&report);
        assert!(result.is_some());

        match result.unwrap() {
            NautilusSpotExecWsMessage::OrderCanceled(canceled) => {
                // Should use the ORIGINAL order ID, not the auto-generated one
                assert_eq!(canceled.client_order_id, original_id);
                assert_eq!(canceled.strategy_id, strategy_id);
            }
            other => panic!("Expected OrderCanceled, got {other:?}"),
        }

        // Original order should be cleaned from tracking
        assert!(!handler.active_orders.contains_key(&original_id));
    }

    #[rstest]
    fn test_missing_instrument_skips_fill() {
        let (mut handler, _cmd_tx, _event_tx) = test_handler();

        let client_order_id = ClientOrderId::new("TEST-001");
        let trader_id = TraderId::new("TESTER-001");
        let strategy_id = StrategyId::new("CrossMM-001");
        let instrument_id = InstrumentId::from("ETHUSDC.BINANCE");

        handler
            .active_orders
            .insert(client_order_id, (trader_id, strategy_id, instrument_id));
        // Do NOT insert into instruments_cache

        let report = make_trade_report(
            "TEST-001",
            999,
            "ETHUSDC",
            "0.01",
            "2045.50",
            "0.01",
            "0.01",
        );

        let result = handler.handle_trade_fill(&report);
        assert!(result.is_none(), "Should skip fill when instrument not cached");
    }

    #[rstest]
    fn test_unknown_order_uses_external_context() {
        let (mut handler, _cmd_tx, _event_tx) = test_handler();

        // Cache the instrument but do NOT register the order
        handler.instruments_cache.insert(
            "ETHUSDC".to_string(),
            InstrumentPrecision {
                price_precision: 2,
                qty_precision: 5,
            },
        );

        let report = make_trade_report(
            "UNKNOWN-001",
            777,
            "ETHUSDC",
            "0.01",
            "2045.50",
            "0.01",
            "0.01",
        );

        let result = handler.handle_trade_fill(&report);
        assert!(result.is_some());

        match result.unwrap() {
            NautilusSpotExecWsMessage::OrderFilled(filled) => {
                assert_eq!(filled.strategy_id, StrategyId::new("EXTERNAL"));
                assert_eq!(filled.trader_id, TraderId::new("TESTER-001"));
                assert_eq!(
                    filled.instrument_id,
                    InstrumentId::from("ETHUSDC.BINANCE")
                );
            }
            other => panic!("Expected OrderFilled, got {other:?}"),
        }
    }

    #[rstest]
    fn test_reconnect_drains_pending_without_rejections() {
        let (mut handler, _cmd_tx, _event_tx) = test_handler();

        let client_order_id = ClientOrderId::new("TEST-001");
        let trader_id = TraderId::new("TESTER-001");
        let strategy_id = StrategyId::new("CrossMM-001");
        let instrument_id = InstrumentId::from("ETHUSDC.BINANCE");

        handler
            .pending_place_requests
            .insert(client_order_id, (trader_id, strategy_id, instrument_id));
        handler
            .pending_cancel_requests
            .insert(client_order_id, (trader_id, strategy_id, instrument_id, None));
        handler
            .active_orders
            .insert(client_order_id, (trader_id, strategy_id, instrument_id));

        let result = handler.handle_reconnect();
        assert!(matches!(result, Some(NautilusSpotExecWsMessage::Reconnected)));

        // Pending maps drained
        assert!(handler.pending_place_requests.is_empty());
        assert!(handler.pending_cancel_requests.is_empty());
        // Active orders preserved
        assert!(handler.active_orders.contains_key(&client_order_id));
    }

    // --- Integration tests: full command+event pipeline ---
    //
    // These tests use direct handle_command() for deterministic command
    // processing, then handler.next() only for event-driven flows.
    // This avoids tokio::select! ordering races between cmd_rx and event_rx.

    #[tokio::test]
    async fn test_full_flow_register_then_fill_via_next() {
        let (mut handler, _cmd_tx, event_tx) = test_handler();

        // Directly process commands (deterministic, no select! race)
        handler.handle_command(SpotExecHandlerCommand::CacheInstrument {
            symbol: "ETHUSDC".to_string(),
            price_precision: 2,
            qty_precision: 5,
        });
        handler.handle_command(SpotExecHandlerCommand::RegisterOrder {
            client_order_id: ClientOrderId::new("INT-001"),
            trader_id: TraderId::new("TESTER-001"),
            strategy_id: StrategyId::new("CrossMM-001"),
            instrument_id: InstrumentId::from("ETHUSDC.BINANCE"),
        });

        // Send fill event via channel, then retrieve via next()
        event_tx
            .send(BinanceSpotUserDataEvent::ExecutionReport(Box::new(
                make_trade_report(
                    "INT-001",
                    555_666_777,
                    "ETHUSDC",
                    "0.01",
                    "2045.50",
                    "0.01",
                    "0.01",
                ),
            )))
            .expect("send failed");

        let result = handler.next().await;
        assert!(result.is_some());

        match result.unwrap() {
            NautilusSpotExecWsMessage::OrderFilled(filled) => {
                assert_eq!(filled.client_order_id, ClientOrderId::new("INT-001"));
                assert_eq!(filled.strategy_id, StrategyId::new("CrossMM-001"));
            }
            other => panic!("Expected OrderFilled, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_partial_fill_keeps_order_active_via_next() {
        let (mut handler, _cmd_tx, event_tx) = test_handler();

        handler.handle_command(SpotExecHandlerCommand::CacheInstrument {
            symbol: "ETHUSDC".to_string(),
            price_precision: 2,
            qty_precision: 5,
        });
        handler.handle_command(SpotExecHandlerCommand::RegisterOrder {
            client_order_id: ClientOrderId::new("PARTIAL-001"),
            trader_id: TraderId::new("TESTER-001"),
            strategy_id: StrategyId::new("CrossMM-001"),
            instrument_id: InstrumentId::from("ETHUSDC.BINANCE"),
        });

        // Partial fill: 0.005 of 0.01 — status is PARTIALLY_FILLED (not FILLED)
        let mut partial_report = make_trade_report(
            "PARTIAL-001",
            888_001,
            "ETHUSDC",
            "0.005",
            "2045.50",
            "0.01",
            "0.005",
        );
        partial_report.order_status = BinanceOrderStatus::PartiallyFilled;

        event_tx
            .send(BinanceSpotUserDataEvent::ExecutionReport(Box::new(
                partial_report,
            )))
            .expect("send failed");

        let result = handler.next().await;
        assert!(result.is_some());
        assert!(matches!(
            result.unwrap(),
            NautilusSpotExecWsMessage::OrderFilled(_)
        ));

        // Order should still be active (partially filled)
        assert!(handler
            .active_orders
            .contains_key(&ClientOrderId::new("PARTIAL-001")));
    }

    #[tokio::test]
    async fn test_reconnect_then_fill_via_next() {
        let (mut handler, _cmd_tx, event_tx) = test_handler();

        handler.handle_command(SpotExecHandlerCommand::CacheInstrument {
            symbol: "ETHUSDC".to_string(),
            price_precision: 2,
            qty_precision: 5,
        });
        handler.handle_command(SpotExecHandlerCommand::RegisterOrder {
            client_order_id: ClientOrderId::new("RECON-001"),
            trader_id: TraderId::new("TESTER-001"),
            strategy_id: StrategyId::new("CrossMM-001"),
            instrument_id: InstrumentId::from("ETHUSDC.BINANCE"),
        });

        // Reconnection event
        event_tx
            .send(BinanceSpotUserDataEvent::Reconnected)
            .expect("send failed");

        let result = handler.next().await;
        assert!(matches!(
            result,
            Some(NautilusSpotExecWsMessage::Reconnected)
        ));

        // Pending was drained, but active_orders preserved
        assert!(handler.pending_place_requests.is_empty());
        assert!(handler
            .active_orders
            .contains_key(&ClientOrderId::new("RECON-001")));

        // Fill still works via active_orders after reconnect
        event_tx
            .send(BinanceSpotUserDataEvent::ExecutionReport(Box::new(
                make_trade_report(
                    "RECON-001",
                    999_001,
                    "ETHUSDC",
                    "0.01",
                    "2045.50",
                    "0.01",
                    "0.01",
                ),
            )))
            .expect("send failed");

        let result = handler.next().await;
        assert!(result.is_some());
        match result.unwrap() {
            NautilusSpotExecWsMessage::OrderFilled(filled) => {
                assert_eq!(filled.client_order_id, ClientOrderId::new("RECON-001"));
                assert_eq!(filled.strategy_id, StrategyId::new("CrossMM-001"));
            }
            other => panic!("Expected OrderFilled after reconnect, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_account_position_via_next() {
        let (mut handler, _cmd_tx, event_tx) = test_handler();

        let position = super::super::types_exec::BinanceSpotAccountPosition {
            event_time: 1_772_494_856_997,
            update_time: 1_772_494_856_997,
            balances: vec![super::super::types_exec::BinanceSpotBalance {
                asset: "ETH".to_string(),
                free: "0.14741228".to_string(),
                locked: "0.00000000".to_string(),
            }],
        };

        event_tx
            .send(BinanceSpotUserDataEvent::AccountPosition(position))
            .expect("send failed");

        let result = handler.next().await;
        assert!(result.is_some());

        match result.unwrap() {
            NautilusSpotExecWsMessage::AccountUpdate(state) => {
                assert!(!state.balances.is_empty());
            }
            other => panic!("Expected AccountUpdate, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_shutdown_signal_stops_handler() {
        let (mut handler, _cmd_tx, _event_tx) = test_handler();

        // Set shutdown signal
        handler.signal.store(true, Ordering::Relaxed);

        let result = handler.next().await;
        assert!(result.is_none(), "Handler should return None on shutdown");
    }
}
