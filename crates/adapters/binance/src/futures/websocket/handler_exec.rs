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

//! Binance Futures execution WebSocket handler.
//!
//! Implements the two-tier architecture with pending order maps for correlating
//! WebSocket order updates with the original order context (strategy_id, etc.).

use std::{
    collections::VecDeque,
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use nautilus_core::{nanos::UnixNanos, time::AtomicTime};
use nautilus_model::{
    enums::{AccountType, LiquiditySide},
    events::{AccountState, OrderAccepted, OrderCanceled, OrderFilled, OrderUpdated},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, VenueOrderId,
    },
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use ustr::Ustr;

use super::messages::{
    BinanceExecutionType, BinanceFuturesAccountUpdateMsg, BinanceFuturesExecWsMessage,
    BinanceFuturesOrderUpdateMsg, ExecHandlerCommand, NautilusExecWsMessage, NautilusWsMessage,
};
use crate::{
    common::{enums::BinanceProductType, symbol::format_instrument_id},
    futures::http::BinanceFuturesInstrument,
};

/// Data cached for pending place requests to correlate with responses.
pub type PlaceRequestData = (ClientOrderId, TraderId, StrategyId, InstrumentId);

/// Data cached for pending cancel requests to correlate with responses.
pub type CancelRequestData = (
    ClientOrderId,
    TraderId,
    StrategyId,
    InstrumentId,
    Option<VenueOrderId>,
);

/// Data cached for pending modify requests to correlate with responses.
pub type ModifyRequestData = (
    ClientOrderId,
    TraderId,
    StrategyId,
    InstrumentId,
    Option<VenueOrderId>,
);

/// Binance Futures execution WebSocket handler.
///
/// Processes user data stream messages and maintains pending order state
/// to correlate WebSocket updates with the original order context.
pub struct BinanceFuturesExecWsFeedHandler {
    clock: &'static AtomicTime,
    trader_id: TraderId,
    account_id: AccountId,
    account_type: AccountType,
    product_type: BinanceProductType,
    signal: Arc<AtomicBool>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<ExecHandlerCommand>,
    msg_rx: tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>,
    pending_place_requests: AHashMap<ClientOrderId, PlaceRequestData>,
    pending_cancel_requests: AHashMap<ClientOrderId, CancelRequestData>,
    pending_modify_requests: AHashMap<ClientOrderId, ModifyRequestData>,
    active_orders: AHashMap<ClientOrderId, (TraderId, StrategyId, InstrumentId)>,
    instruments_cache: AHashMap<Ustr, BinanceFuturesInstrument>,
    message_queue: VecDeque<NautilusExecWsMessage>,
}

impl Debug for BinanceFuturesExecWsFeedHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BinanceFuturesExecHandler))
            .field("trader_id", &self.trader_id)
            .field("account_id", &self.account_id)
            .field("pending_place_requests", &self.pending_place_requests.len())
            .field(
                "pending_cancel_requests",
                &self.pending_cancel_requests.len(),
            )
            .field("active_orders", &self.active_orders.len())
            .field("instruments_cache", &self.instruments_cache.len())
            .finish_non_exhaustive()
    }
}

impl BinanceFuturesExecWsFeedHandler {
    /// Creates a new [`BinanceFuturesExecWsFeedHandler`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        clock: &'static AtomicTime,
        trader_id: TraderId,
        account_id: AccountId,
        account_type: AccountType,
        product_type: BinanceProductType,
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<ExecHandlerCommand>,
        msg_rx: tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>,
    ) -> Self {
        Self {
            clock,
            trader_id,
            account_id,
            account_type,
            product_type,
            signal,
            cmd_rx,
            msg_rx,
            pending_place_requests: AHashMap::new(),
            pending_cancel_requests: AHashMap::new(),
            pending_modify_requests: AHashMap::new(),
            active_orders: AHashMap::new(),
            instruments_cache: AHashMap::new(),
            message_queue: VecDeque::new(),
        }
    }

    /// Processes commands and messages, returning the next output event.
    pub async fn next(&mut self) -> Option<NautilusExecWsMessage> {
        loop {
            if self.signal.load(Ordering::Relaxed) {
                return None;
            }

            // Return queued messages first
            if let Some(msg) = self.message_queue.pop_front() {
                return Some(msg);
            }

            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    self.handle_command(cmd);
                }
                Some(msg) = self.msg_rx.recv() => {
                    if let Some(event) = self.handle_message(msg) {
                        return Some(event);
                    }
                }
                else => {
                    return None;
                }
            }
        }
    }

    fn handle_command(&mut self, cmd: ExecHandlerCommand) {
        match cmd {
            ExecHandlerCommand::SetClient(_) => {
                // WebSocket client is managed by the outer layer
            }
            ExecHandlerCommand::Disconnect => {
                // WebSocket client is managed by the outer layer
            }
            ExecHandlerCommand::InitializeInstruments(instruments) => {
                for inst in instruments {
                    self.instruments_cache.insert(inst.symbol(), inst);
                }
            }
            ExecHandlerCommand::UpdateInstrument(instrument) => {
                self.instruments_cache
                    .insert(instrument.symbol(), instrument);
            }
            ExecHandlerCommand::Subscribe { .. } => {
                // Subscriptions are managed by the outer layer
            }
            ExecHandlerCommand::RegisterOrder {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
            } => {
                let data = (client_order_id, trader_id, strategy_id, instrument_id);
                self.pending_place_requests.insert(client_order_id, data);
                self.active_orders
                    .insert(client_order_id, (trader_id, strategy_id, instrument_id));
            }
            ExecHandlerCommand::RegisterCancel {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
                venue_order_id,
            } => {
                let data = (
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                    venue_order_id,
                );
                self.pending_cancel_requests.insert(client_order_id, data);
            }
            ExecHandlerCommand::RegisterModify {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
                venue_order_id,
            } => {
                let data = (
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                    venue_order_id,
                );
                self.pending_modify_requests.insert(client_order_id, data);
            }
        }
    }

    fn handle_message(&mut self, msg: NautilusWsMessage) -> Option<NautilusExecWsMessage> {
        match msg {
            NautilusWsMessage::ExecRaw(exec_msg) => self.handle_exec_message(exec_msg),
            NautilusWsMessage::Reconnected => Some(NautilusExecWsMessage::Reconnected),
            NautilusWsMessage::Error(err) => {
                log::error!(
                    "User data stream WebSocket error: code={}, msg={}",
                    err.code,
                    err.msg
                );
                None
            }
            NautilusWsMessage::Data(_) | NautilusWsMessage::Exec(_) => None,
        }
    }

    fn handle_exec_message(
        &mut self,
        msg: BinanceFuturesExecWsMessage,
    ) -> Option<NautilusExecWsMessage> {
        match msg {
            BinanceFuturesExecWsMessage::OrderUpdate(update) => self.handle_order_update(&update),
            BinanceFuturesExecWsMessage::AccountUpdate(update) => {
                self.handle_account_update(&update)
            }
            BinanceFuturesExecWsMessage::MarginCall(mc) => {
                log::warn!(
                    "Margin call: cross_wallet_balance={}, positions_at_risk={}",
                    mc.cross_wallet_balance,
                    mc.positions.len()
                );
                None
            }
            BinanceFuturesExecWsMessage::AccountConfigUpdate(cfg) => {
                if let Some(ref lc) = cfg.leverage_config {
                    log::info!(
                        "Account config update: symbol={}, leverage={}",
                        lc.symbol,
                        lc.leverage
                    );
                }
                None
            }
            BinanceFuturesExecWsMessage::ListenKeyExpired => {
                log::warn!("Listen key expired");
                Some(NautilusExecWsMessage::ListenKeyExpired)
            }
        }
    }

    fn handle_order_update(
        &mut self,
        msg: &BinanceFuturesOrderUpdateMsg,
    ) -> Option<NautilusExecWsMessage> {
        let order_data = &msg.order;
        let ts_event = UnixNanos::from((msg.event_time * 1_000_000) as u64);
        let ts_init = self.clock.get_time_ns();

        let client_order_id = ClientOrderId::new(&order_data.client_order_id);
        let venue_order_id = VenueOrderId::new(order_data.order_id.to_string());

        // Look up order context from pending/active maps, falling back to EXTERNAL
        let (trader_id, strategy_id, instrument_id) =
            self.get_order_context(&client_order_id, &order_data.symbol);

        match order_data.execution_type {
            BinanceExecutionType::New => {
                // Move from pending to active on acceptance
                self.pending_place_requests.remove(&client_order_id);

                let event = OrderAccepted::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    self.account_id,
                    nautilus_core::UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                );

                Some(NautilusExecWsMessage::OrderAccepted(event))
            }
            BinanceExecutionType::Canceled | BinanceExecutionType::Expired => {
                // Clean up tracking maps
                self.pending_cancel_requests.remove(&client_order_id);
                self.active_orders.remove(&client_order_id);

                let event = OrderCanceled::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    nautilus_core::UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(self.account_id),
                );

                Some(NautilusExecWsMessage::OrderCanceled(event))
            }
            BinanceExecutionType::Trade => self.handle_trade_fill(
                msg,
                trader_id,
                strategy_id,
                instrument_id,
                ts_event,
                ts_init,
            ),
            BinanceExecutionType::Amendment => {
                self.pending_modify_requests.remove(&client_order_id);

                // Look up precision from instrument cache
                let symbol_key = Ustr::from(&order_data.symbol);
                let (price_precision, size_precision) =
                    if let Some(inst) = self.instruments_cache.get(&symbol_key) {
                        (inst.price_precision(), inst.quantity_precision())
                    } else {
                        log::warn!(
                            "Instrument not found for amendment: {}, using default precision",
                            order_data.symbol
                        );
                        (8, 8)
                    };

                let quantity: f64 = order_data.original_qty.parse().unwrap_or(0.0);
                let price: f64 = order_data.original_price.parse().unwrap_or(0.0);

                let event = OrderUpdated::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    Quantity::new(quantity, size_precision as u8),
                    nautilus_core::UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(self.account_id),
                    Some(Price::new(price, price_precision as u8)),
                    None,
                    None,
                );

                Some(NautilusExecWsMessage::OrderUpdated(event))
            }
            BinanceExecutionType::Calculated => {
                log::warn!(
                    "Calculated execution (liquidation/ADL): symbol={}, client_order_id={}",
                    order_data.symbol,
                    order_data.client_order_id
                );
                None
            }
        }
    }

    fn handle_trade_fill(
        &mut self,
        msg: &BinanceFuturesOrderUpdateMsg,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Option<NautilusExecWsMessage> {
        let order_data = &msg.order;
        let client_order_id = ClientOrderId::new(&order_data.client_order_id);
        let venue_order_id = VenueOrderId::new(order_data.order_id.to_string());

        // Look up precision from instrument cache
        let symbol_key = Ustr::from(&order_data.symbol);
        let Some(inst) = self.instruments_cache.get(&symbol_key) else {
            log::error!(
                "Instrument not found for fill: {}, skipping to avoid precision mismatch",
                order_data.symbol
            );
            return None;
        };
        let price_precision = inst.price_precision();
        let size_precision = inst.quantity_precision();

        let last_qty: f64 = order_data.last_filled_qty.parse().unwrap_or(0.0);
        let last_px: f64 = order_data.last_filled_price.parse().unwrap_or(0.0);
        let cum_qty: f64 = order_data.cumulative_filled_qty.parse().unwrap_or(0.0);
        let original_qty: f64 = order_data.original_qty.parse().unwrap_or(0.0);
        let leaves_qty = original_qty - cum_qty;
        let commission: f64 = order_data
            .commission
            .as_deref()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0.0);

        let commission_currency = order_data
            .commission_asset
            .as_ref()
            .map_or_else(Currency::USDT, |a| Currency::from(a.as_str()));

        let liquidity_side = if order_data.is_maker {
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
            TradeId::new(order_data.trade_id.to_string()),
            order_data.side.into(),
            order_data.order_type.into(),
            Quantity::new(last_qty, size_precision as u8),
            Price::new(last_px, price_precision as u8),
            commission_currency,
            liquidity_side,
            nautilus_core::UUID4::new(),
            ts_event,
            ts_init,
            false,
            None,
            Some(Money::new(commission, commission_currency)),
        );

        // Clean up if fully filled
        if leaves_qty <= 0.0 {
            self.active_orders.remove(&client_order_id);
            log::debug!(
                "Order fully filled: client_order_id={client_order_id}, venue_order_id={venue_order_id}"
            );
        }

        Some(NautilusExecWsMessage::OrderFilled(event))
    }

    fn handle_account_update(
        &mut self,
        msg: &BinanceFuturesAccountUpdateMsg,
    ) -> Option<NautilusExecWsMessage> {
        let ts_event = UnixNanos::from((msg.event_time * 1_000_000) as u64);

        let balances: Vec<AccountBalance> = msg
            .account
            .balances
            .iter()
            .filter_map(|b| {
                let wallet_balance: f64 = b.wallet_balance.parse().unwrap_or(0.0);
                let cross_wallet: f64 = b.cross_wallet_balance.parse().unwrap_or(0.0);
                let locked = wallet_balance - cross_wallet;

                if wallet_balance == 0.0 {
                    return None;
                }

                let currency = Currency::from(&b.asset);
                Some(AccountBalance::new(
                    Money::new(wallet_balance, currency),
                    Money::new(locked.max(0.0), currency),
                    Money::new(cross_wallet, currency),
                ))
            })
            .collect();

        if balances.is_empty() {
            return None;
        }

        let event = AccountState::new(
            self.account_id,
            self.account_type,
            balances,
            vec![], // Margins handled separately
            true,   // is_reported
            nautilus_core::UUID4::new(),
            ts_event,
            self.clock.get_time_ns(),
            None, // base_currency
        );

        Some(NautilusExecWsMessage::AccountUpdate(event))
    }

    /// Looks up order context from pending/active maps.
    ///
    /// Falls back to EXTERNAL strategy for untracked orders (e.g., orders from before
    /// restart or created externally). Uses instruments cache when available, otherwise
    /// constructs instrument ID from the symbol using the configured product type.
    fn get_order_context(
        &self,
        client_order_id: &ClientOrderId,
        symbol: &str,
    ) -> (TraderId, StrategyId, InstrumentId) {
        // First check pending place requests
        if let Some((_, trader_id, strategy_id, instrument_id)) =
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
        let symbol_ustr = Ustr::from(symbol);

        // Prefer instruments cache for correct instrument ID
        let instrument_id = if let Some(instrument) = self.instruments_cache.get(&symbol_ustr) {
            instrument.id()
        } else {
            // Ultimate fallback: construct from symbol using product type
            log::warn!("Instrument not in cache for {symbol}, constructing ID from product type");
            format_instrument_id(&symbol_ustr, self.product_type)
        };

        log::debug!(
            "Order context not found for {client_order_id}, using EXTERNAL with {instrument_id}"
        );
        (self.trader_id, StrategyId::new("EXTERNAL"), instrument_id)
    }
}
