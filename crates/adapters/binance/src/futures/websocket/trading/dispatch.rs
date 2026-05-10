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

//! Trading API response dispatch for the Binance Futures adapter.
//!
//! Maps responses from the WebSocket Trading API (order accepted, rejected,
//! canceled, modified) into Nautilus order events. Accepted/canceled/modified
//! responses only clear the pending-request state because the corresponding
//! order events arrive via the user data stream. Rejections emit events
//! directly since the stream never reports them.

use nautilus_core::{UUID4, time::AtomicTime};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    events::{OrderCancelRejected, OrderEventAny, OrderModifyRejected, OrderRejected},
    identifiers::AccountId,
};

use super::messages::BinanceFuturesWsTradingMessage;
use crate::common::{consts::BINANCE_GTX_ORDER_REJECT_CODE, dispatch::WsDispatchState};

pub(crate) fn dispatch_ws_trading_message(
    msg: BinanceFuturesWsTradingMessage,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
) {
    match msg {
        BinanceFuturesWsTradingMessage::OrderAccepted {
            request_id,
            response,
        } => {
            dispatch_state.pending_requests.remove(&request_id);
            log::debug!(
                "WS order accepted: request_id={request_id}, order_id={}",
                response.order_id
            );
            // OrderAccepted event comes from user data stream (ORDER_TRADE_UPDATE)
        }
        BinanceFuturesWsTradingMessage::OrderRejected {
            request_id,
            code,
            msg,
        } => {
            log::debug!("WS order rejected: request_id={request_id}, code={code}, msg={msg}");

            if let Some((_, pending)) = dispatch_state.pending_requests.remove(&request_id) {
                // Clone to drop the DashMap read guard before cleanup_terminal
                let identity = dispatch_state
                    .order_identities
                    .get(&pending.client_order_id)
                    .map(|r| r.clone());

                if let Some(identity) = identity {
                    let due_post_only = i64::from(code) == BINANCE_GTX_ORDER_REJECT_CODE;
                    let ts_now = clock.get_time_ns();
                    let rejected = OrderRejected::new(
                        emitter.trader_id(),
                        identity.strategy_id,
                        identity.instrument_id,
                        pending.client_order_id,
                        account_id,
                        ustr::Ustr::from(&format!("code={code}: {msg}")),
                        UUID4::new(),
                        ts_now,
                        ts_now,
                        false,
                        due_post_only,
                    );
                    dispatch_state.cleanup_terminal(pending.client_order_id);
                    emitter.send_order_event(OrderEventAny::Rejected(rejected));
                } else {
                    log::warn!(
                        "No order identity for {}, cannot emit OrderRejected",
                        pending.client_order_id
                    );
                }
            } else {
                log::warn!("No pending request for {request_id}, cannot emit OrderRejected");
            }
        }
        BinanceFuturesWsTradingMessage::OrderCanceled {
            request_id,
            response,
        } => {
            dispatch_state.pending_requests.remove(&request_id);
            log::debug!(
                "WS order canceled: request_id={request_id}, order_id={}",
                response.order_id
            );
            // OrderCanceled event comes from user data stream (ORDER_TRADE_UPDATE)
        }
        BinanceFuturesWsTradingMessage::CancelRejected {
            request_id,
            code,
            msg,
        } => {
            log::warn!("WS cancel rejected: request_id={request_id}, code={code}, msg={msg}");

            if let Some((_, pending)) = dispatch_state.pending_requests.remove(&request_id)
                && let Some(identity) = dispatch_state
                    .order_identities
                    .get(&pending.client_order_id)
            {
                let ts_now = clock.get_time_ns();
                let rejected = OrderCancelRejected::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    identity.instrument_id,
                    pending.client_order_id,
                    ustr::Ustr::from(&format!("code={code}: {msg}")),
                    UUID4::new(),
                    ts_now,
                    ts_now,
                    false,
                    pending.venue_order_id,
                    Some(account_id),
                );
                emitter.send_order_event(OrderEventAny::CancelRejected(rejected));
            }
        }
        BinanceFuturesWsTradingMessage::OrderModified {
            request_id,
            response,
        } => {
            dispatch_state.pending_requests.remove(&request_id);
            log::debug!(
                "WS order modified: request_id={request_id}, order_id={}",
                response.order_id
            );
            // OrderUpdated event comes from user data stream (ORDER_TRADE_UPDATE)
        }
        BinanceFuturesWsTradingMessage::ModifyRejected {
            request_id,
            code,
            msg,
        } => {
            log::warn!("WS modify rejected: request_id={request_id}, code={code}, msg={msg}");

            if let Some((_, pending)) = dispatch_state.pending_requests.remove(&request_id)
                && let Some(identity) = dispatch_state
                    .order_identities
                    .get(&pending.client_order_id)
            {
                let ts_now = clock.get_time_ns();
                let rejected = OrderModifyRejected::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    identity.instrument_id,
                    pending.client_order_id,
                    ustr::Ustr::from(&format!("code={code}: {msg}")),
                    UUID4::new(),
                    ts_now,
                    ts_now,
                    false,
                    pending.venue_order_id,
                    Some(account_id),
                );
                emitter.send_order_event(OrderEventAny::ModifyRejected(rejected));
            }
        }
        BinanceFuturesWsTradingMessage::Connected => {
            log::info!("WS trading API connected");
        }
        BinanceFuturesWsTradingMessage::Reconnected => {
            log::info!("WS trading API reconnected");
        }
        BinanceFuturesWsTradingMessage::Error(err) => {
            log::error!("WS trading API error: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::ExecutionEvent;
    use nautilus_core::time::get_atomic_clock_realtime;
    use nautilus_model::{
        enums::{AccountType, OrderSide, OrderType},
        identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    };
    use rstest::rstest;

    use super::*;
    use crate::common::dispatch::{OrderIdentity, PendingOperation, PendingRequest};

    #[rstest]
    fn test_dispatch_ws_trading_message_emits_cancel_rejected_and_clears_pending_request() {
        let clock = get_atomic_clock_realtime();
        let (emitter, mut rx) = create_test_emitter(clock);
        let dispatch_state = create_tracked_dispatch_state(
            ClientOrderId::from("TEST"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        );
        dispatch_state.pending_requests.insert(
            "req-cancel".to_string(),
            PendingRequest {
                client_order_id: ClientOrderId::from("TEST"),
                venue_order_id: Some(VenueOrderId::from("12345")),
                operation: PendingOperation::Cancel,
            },
        );

        dispatch_ws_trading_message(
            BinanceFuturesWsTradingMessage::CancelRejected {
                request_id: "req-cancel".to_string(),
                code: -2011,
                msg: "Unknown order sent".to_string(),
            },
            &emitter,
            AccountId::from("BINANCE-001"),
            clock,
            &dispatch_state,
        );

        assert!(dispatch_state.pending_requests.get("req-cancel").is_none());

        match rx
            .try_recv()
            .expect("Cancel rejection event should be emitted")
        {
            ExecutionEvent::Order(OrderEventAny::CancelRejected(event)) => {
                assert_eq!(event.client_order_id, ClientOrderId::from("TEST"));
                assert_eq!(event.account_id, Some(AccountId::from("BINANCE-001")));
                assert!(event.reason.as_str().contains("code=-2011"));
            }
            other => panic!("Expected CancelRejected event, was {other:?}"),
        }
    }

    #[rstest]
    fn test_dispatch_ws_trading_message_emits_modify_rejected_and_clears_pending_request() {
        let clock = get_atomic_clock_realtime();
        let (emitter, mut rx) = create_test_emitter(clock);
        let dispatch_state = create_tracked_dispatch_state(
            ClientOrderId::from("TEST"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        );
        dispatch_state.pending_requests.insert(
            "req-modify".to_string(),
            PendingRequest {
                client_order_id: ClientOrderId::from("TEST"),
                venue_order_id: Some(VenueOrderId::from("12345")),
                operation: PendingOperation::Modify,
            },
        );

        dispatch_ws_trading_message(
            BinanceFuturesWsTradingMessage::ModifyRejected {
                request_id: "req-modify".to_string(),
                code: -4028,
                msg: "Price or quantity not changed".to_string(),
            },
            &emitter,
            AccountId::from("BINANCE-001"),
            clock,
            &dispatch_state,
        );

        assert!(dispatch_state.pending_requests.get("req-modify").is_none());

        match rx
            .try_recv()
            .expect("Modify rejection event should be emitted")
        {
            ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)) => {
                assert_eq!(event.client_order_id, ClientOrderId::from("TEST"));
                assert_eq!(event.account_id, Some(AccountId::from("BINANCE-001")));
                assert!(event.reason.as_str().contains("code=-4028"));
            }
            other => panic!("Expected ModifyRejected event, was {other:?}"),
        }
    }

    fn create_test_emitter(
        clock: &'static AtomicTime,
    ) -> (
        ExecutionEventEmitter,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TESTER-001"),
            AccountId::from("BINANCE-001"),
            AccountType::Margin,
            None,
        );
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(tx);
        (emitter, rx)
    }

    fn create_tracked_dispatch_state(
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
    ) -> WsDispatchState {
        let dispatch_state = WsDispatchState::default();
        dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id: StrategyId::from("TEST-STRATEGY"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
                price: None,
            },
        );
        dispatch_state
    }
}
