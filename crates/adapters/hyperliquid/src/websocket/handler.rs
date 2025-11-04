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

//! WebSocket message handler for Hyperliquid.

use ahash::AHashMap;
use nautilus_core::{nanos::UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    identifiers::AccountId,
    instruments::{Instrument, InstrumentAny},
};
use ustr::Ustr;

use super::{
    messages::{ExecutionReport, HyperliquidWsMessage, NautilusWsMessage, WsUserEventData},
    parse::{parse_ws_fill_report, parse_ws_order_status_report},
};

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
pub enum HandlerCommand {
    /// Initialize the instruments cache with the given instruments.
    InitializeInstruments(Vec<InstrumentAny>),
    /// Update a single instrument in the cache.
    UpdateInstrument(InstrumentAny),
}

pub(super) struct FeedHandler {
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    msg_rx: tokio::sync::mpsc::UnboundedReceiver<HyperliquidWsMessage>,
    out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
    instruments_cache: AHashMap<Ustr, InstrumentAny>,
    account_id: Option<AccountId>,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`] instance.
    pub(super) fn new(
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        msg_rx: tokio::sync::mpsc::UnboundedReceiver<HyperliquidWsMessage>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusWsMessage>,
        account_id: Option<AccountId>,
    ) -> Self {
        Self {
            cmd_rx,
            msg_rx,
            out_tx,
            instruments_cache: AHashMap::new(),
            account_id,
        }
    }

    pub(super) async fn next(&mut self) -> Option<()> {
        let clock = get_atomic_clock_realtime();

        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::InitializeInstruments(instruments) => {
                            for inst in instruments {
                                self.instruments_cache.insert(inst.symbol().inner(), inst);
                            }
                        }
                        HandlerCommand::UpdateInstrument(inst) => {
                            self.instruments_cache.insert(inst.symbol().inner(), inst);
                        }
                    }
                    continue;
                }

                Some(msg) = self.msg_rx.recv() => {
                    let ts_init = clock.get_time_ns();
                    let nautilus_messages = Self::parse_to_nautilus_messages(
                        msg,
                        &self.instruments_cache,
                        self.account_id,
                        ts_init,
                    );

                    for nautilus_msg in nautilus_messages {
                        if self.out_tx.send(nautilus_msg).is_err() {
                            tracing::debug!("Receiver dropped, stopping handler");
                            return None;
                        }
                    }
                }

                else => {
                    tracing::debug!("Handler shutting down: stream ended or command channel closed");
                    return None;
                }
            }
        }
    }

    fn parse_to_nautilus_messages(
        msg: HyperliquidWsMessage,
        instruments: &AHashMap<Ustr, InstrumentAny>,
        account_id: Option<AccountId>,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let mut result = Vec::new();

        match msg {
            HyperliquidWsMessage::OrderUpdates { data } => {
                if let Some(account_id) = account_id {
                    let mut exec_reports = Vec::new();

                    for order_update in &data {
                        if let Some(instrument) = instruments.get(&order_update.order.coin) {
                            match parse_ws_order_status_report(
                                order_update,
                                instrument,
                                account_id,
                                ts_init,
                            ) {
                                Ok(report) => {
                                    exec_reports.push(ExecutionReport::Order(report));
                                }
                                Err(e) => {
                                    tracing::error!("Error parsing order update: {e}");
                                }
                            }
                        } else {
                            tracing::warn!(
                                "No instrument found for coin: {}",
                                order_update.order.coin
                            );
                        }
                    }

                    if !exec_reports.is_empty() {
                        result.push(NautilusWsMessage::ExecutionReports(exec_reports));
                    }
                }
            }
            HyperliquidWsMessage::UserEvents { data } => {
                if let Some(account_id) = account_id
                    && let WsUserEventData::Fills { fills } = data
                {
                    let mut exec_reports = Vec::new();

                    for fill in &fills {
                        if let Some(instrument) = instruments.get(&fill.coin) {
                            match parse_ws_fill_report(fill, instrument, account_id, ts_init) {
                                Ok(report) => {
                                    exec_reports.push(ExecutionReport::Fill(report));
                                }
                                Err(e) => {
                                    tracing::error!("Error parsing fill: {e}");
                                }
                            }
                        } else {
                            tracing::warn!("No instrument found for coin: {}", fill.coin);
                        }
                    }

                    if !exec_reports.is_empty() {
                        result.push(NautilusWsMessage::ExecutionReports(exec_reports));
                    }
                }
            }
            _ => {}
        }

        result
    }
}
