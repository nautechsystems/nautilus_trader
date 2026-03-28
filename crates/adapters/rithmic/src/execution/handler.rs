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

//! Message handler for Rithmic execution events.

use rithmic_rs::{
    OrderStatus,
    api::RithmicResponse,
    rithmic_to_unix_nanos,
    rti::{
        ExchangeOrderNotification, RithmicOrderNotification,
        exchange_order_notification::BracketType as ExchangeBracketType,
        exchange_order_notification::NotifyType as ExchangeNotifyType, messages::RithmicMessage,
        rithmic_order_notification::BracketType as RithmicBracketType,
        rithmic_order_notification::NotifyType as RithmicNotifyType,
    },
};

use super::client::{
    ExecutionEvent, OrderAccepted, OrderCancelled, OrderContext, OrderFilled, OrderModified,
    OrderRejected, OrderSubmitted,
};
use super::parse::{parse_order_side, parse_order_type, parse_time_in_force};

fn parse_linked_basket_ids(value: Option<&str>) -> Vec<String> {
    value
        .into_iter()
        .flat_map(|text| text.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn rithmic_bracket_type_text(value: Option<i32>) -> Option<String> {
    value
        .and_then(|raw| RithmicBracketType::try_from(raw).ok())
        .map(|kind| kind.as_str_name().to_string())
}

fn exchange_bracket_type_text(value: Option<i32>) -> Option<String> {
    value
        .and_then(|raw| ExchangeBracketType::try_from(raw).ok())
        .map(|kind| kind.as_str_name().to_string())
}

fn rithmic_order_context(notif: &RithmicOrderNotification) -> OrderContext {
    OrderContext {
        is_snapshot: notif.is_snapshot.unwrap_or(false),
        symbol: notif.symbol.clone(),
        exchange: notif.exchange.clone(),
        side: notif
            .transaction_type
            .and_then(|value| parse_order_side(value).ok()),
        order_type: notif
            .price_type
            .and_then(|value| parse_order_type(value).ok()),
        time_in_force: notif
            .duration
            .and_then(|value| parse_time_in_force(value).ok()),
        quantity: notif.quantity.map(|value| value as f64),
        filled_qty: notif.total_fill_size.map(|value| value as f64),
        leaves_qty: notif.total_unfilled_size.map(|value| value as f64),
        price: notif.price,
        trigger_price: notif.trigger_price,
        avg_price: notif.avg_fill_price,
        original_basket_id: notif.original_basket_id.clone(),
        linked_basket_ids: parse_linked_basket_ids(notif.linked_basket_ids.as_deref()),
        bracket_type: rithmic_bracket_type_text(notif.bracket_type),
    }
}

fn exchange_order_context(notif: &ExchangeOrderNotification) -> OrderContext {
    OrderContext {
        is_snapshot: notif.is_snapshot.unwrap_or(false),
        symbol: notif.symbol.clone(),
        exchange: notif.exchange.clone(),
        side: notif
            .transaction_type
            .and_then(|value| parse_order_side(value).ok()),
        order_type: notif
            .price_type
            .and_then(|value| parse_order_type(value).ok()),
        time_in_force: notif
            .duration
            .and_then(|value| parse_time_in_force(value).ok()),
        quantity: notif.quantity.map(|value| value as f64),
        filled_qty: notif
            .total_fill_size
            .or(notif.fill_size)
            .map(|value| value as f64),
        leaves_qty: notif.total_unfilled_size.map(|value| value as f64),
        price: notif.price,
        trigger_price: notif.trigger_price,
        avg_price: notif.avg_fill_price,
        original_basket_id: notif.original_basket_id.clone(),
        linked_basket_ids: parse_linked_basket_ids(notif.linked_basket_ids.as_deref()),
        bracket_type: exchange_bracket_type_text(notif.bracket_type),
    }
}

/// Handles incoming execution messages from Rithmic and converts them to
/// internal `ExecutionEvent`s used by the adapter.
#[derive(Default)]
pub struct ExecutionHandler;

impl ExecutionHandler {
    /// Creates a new execution handler.
    pub fn new() -> Self {
        Self {}
    }

    /// Converts a generic `RithmicResponse` into an `ExecutionEvent` if the
    /// message represents an execution update.
    pub fn handle_response(&self, response: &RithmicResponse) -> Option<ExecutionEvent> {
        self.handle_message(&response.message)
    }

    fn handle_message(&self, message: &RithmicMessage) -> Option<ExecutionEvent> {
        match message {
            RithmicMessage::RithmicOrderNotification(notif) => {
                tracing::debug!(
                    user_tag = ?notif.user_tag,
                    basket_id = ?notif.basket_id,
                    notify_type = ?notif.notify_type,
                    status = ?notif.status,
                    completion_reason = ?notif.completion_reason,
                    text = ?notif.text,
                    "received raw rithmic order notification"
                );
                self.handle_rithmic_order_notification(notif)
            }
            RithmicMessage::ExchangeOrderNotification(notif) => {
                tracing::debug!(
                    user_tag = ?notif.user_tag,
                    basket_id = ?notif.basket_id,
                    notify_type = ?notif.notify_type,
                    status = ?notif.status,
                    report_text = ?notif.report_text,
                    text = ?notif.text,
                    fill_size = ?notif.fill_size,
                    fill_price = ?notif.fill_price,
                    "received raw exchange order notification"
                );
                self.handle_exchange_order_notification(notif)
            }
            RithmicMessage::ForcedLogout(_) => {
                tracing::warn!("Forced logout received from order plant");
                Some(ExecutionEvent::Error("Forced logout".to_string()))
            }
            other => {
                tracing::debug!(
                    kind = ?std::mem::discriminant(other),
                    "ignoring non-execution order plant response"
                );
                None
            }
        }
    }

    fn handle_rithmic_order_notification(
        &self,
        notif: &RithmicOrderNotification,
    ) -> Option<ExecutionEvent> {
        let context = rithmic_order_context(notif);
        let client_order_id = match notif
            .user_tag
            .clone()
            .or_else(|| context.original_basket_id.clone())
            .or_else(|| notif.basket_id.clone())
        {
            Some(tag) => tag,
            None => {
                tracing::debug!(
                    "RithmicOrderNotification missing user_tag/original_basket_id/basket_id"
                );
                return None;
            }
        };

        let notify_type = RithmicNotifyType::try_from(notif.notify_type?).ok()?;
        let ts_event = rithmic_to_unix_nanos(notif.ssboe.unwrap_or(0), notif.usecs.unwrap_or(0));
        let account_id = notif.account_id.clone().unwrap_or_default();

        tracing::debug!(
            client_order_id = %client_order_id,
            ?notify_type,
            basket_id = ?notif.basket_id,
            status = ?notif.status,
            "rithmic order notification"
        );

        match notify_type {
            RithmicNotifyType::OrderRcvdFromClnt
            | RithmicNotifyType::OrderRcvdByExchGtwy
            | RithmicNotifyType::OrderSentToExch => {
                Some(ExecutionEvent::Submitted(OrderSubmitted {
                    client_order_id,
                    venue_order_id: notif.basket_id.clone(),
                    account_id,
                    ts_event,
                    context,
                }))
            }
            RithmicNotifyType::Open => Some(ExecutionEvent::Accepted(OrderAccepted {
                client_order_id,
                venue_order_id: notif.basket_id.clone().unwrap_or_default(),
                account_id,
                ts_event,
                context,
            })),
            RithmicNotifyType::Modified => Some(ExecutionEvent::Modified(OrderModified {
                client_order_id,
                venue_order_id: notif.basket_id.clone().unwrap_or_default(),
                new_price: notif.price,
                new_qty: notif.quantity.map(|q| q as f64),
                ts_event,
                context,
            })),
            RithmicNotifyType::Complete => {
                let status: OrderStatus = notif
                    .status
                    .as_deref()
                    .unwrap_or("")
                    .parse()
                    .unwrap_or(OrderStatus::Unknown);

                if status == OrderStatus::Cancelled {
                    let venue_order_id = notif.basket_id.clone().unwrap_or_default();
                    Some(ExecutionEvent::Cancelled(OrderCancelled {
                        client_order_id,
                        venue_order_id,
                        ts_event,
                        context,
                    }))
                } else {
                    // Completion without a clear cancellation is typically followed by an
                    // ExchangeOrderNotification::Fill. Avoid emitting a duplicate event here.
                    tracing::debug!(
                        ?status,
                        "order complete without cancellation – waiting for fill detail"
                    );
                    None
                }
            }
            RithmicNotifyType::ModificationFailed
            | RithmicNotifyType::CancellationFailed
            | RithmicNotifyType::LinkOrdersFailed => {
                let reason = notif
                    .completion_reason
                    .clone()
                    .or_else(|| notif.text.clone())
                    .unwrap_or_else(|| "Order rejected".to_string());
                Some(ExecutionEvent::Rejected(OrderRejected {
                    client_order_id,
                    reason,
                    ts_event,
                    context,
                }))
            }
            RithmicNotifyType::ModifyPending
            | RithmicNotifyType::CancelPending
            | RithmicNotifyType::OpenPending
            | RithmicNotifyType::TriggerPending
            | RithmicNotifyType::ModifyRcvdFromClnt
            | RithmicNotifyType::CancelRcvdFromClnt
            | RithmicNotifyType::ModifyRcvdByExchGtwy
            | RithmicNotifyType::CancelRcvdByExchGtwy
            | RithmicNotifyType::ModifySentToExch
            | RithmicNotifyType::CancelSentToExch
            | RithmicNotifyType::Generic => {
                tracing::debug!("Ignoring pending/generic order notification");
                None
            }
        }
    }

    fn handle_exchange_order_notification(
        &self,
        notif: &ExchangeOrderNotification,
    ) -> Option<ExecutionEvent> {
        let context = exchange_order_context(notif);
        let client_order_id = match notif
            .user_tag
            .clone()
            .or_else(|| context.original_basket_id.clone())
            .or_else(|| notif.basket_id.clone())
        {
            Some(tag) => tag,
            None => {
                tracing::debug!(
                    "ExchangeOrderNotification missing user_tag/original_basket_id/basket_id"
                );
                return None;
            }
        };
        let notify_type = ExchangeNotifyType::try_from(notif.notify_type?).ok()?;
        let ts_event = rithmic_to_unix_nanos(notif.ssboe.unwrap_or(0), notif.usecs.unwrap_or(0));
        let venue_order_id = notif.basket_id.clone().unwrap_or_default();

        tracing::debug!(
            client_order_id = %client_order_id,
            ?notify_type,
            basket_id = ?notif.basket_id,
            status = ?notif.status,
            "exchange order notification"
        );

        match notify_type {
            ExchangeNotifyType::Fill => {
                let fill_price = match notif.fill_price {
                    Some(p) => p,
                    None => {
                        tracing::debug!("Fill notification missing price");
                        return None;
                    }
                };
                let fill_qty = match notif.fill_size {
                    Some(q) => q as f64,
                    None => {
                        tracing::debug!("Fill notification missing size");
                        return None;
                    }
                };
                let leaves_qty = notif.total_unfilled_size.unwrap_or(0) as f64;

                Some(ExecutionEvent::Filled(OrderFilled {
                    client_order_id,
                    venue_order_id,
                    fill_price,
                    fill_qty,
                    leaves_qty,
                    commission: 0.0,
                    ts_event,
                    trade_id: notif.fill_id.clone(),
                    currency: notif.currency.clone(),
                    context,
                }))
            }
            ExchangeNotifyType::Cancel => Some(ExecutionEvent::Cancelled(OrderCancelled {
                client_order_id,
                venue_order_id,
                ts_event,
                context,
            })),
            ExchangeNotifyType::Reject
            | ExchangeNotifyType::NotCancelled
            | ExchangeNotifyType::NotModified => {
                let reason = notif
                    .text
                    .clone()
                    .or_else(|| notif.report_text.clone())
                    .unwrap_or_else(|| "Order rejected by exchange".to_string());
                Some(ExecutionEvent::Rejected(OrderRejected {
                    client_order_id,
                    reason,
                    ts_event,
                    context,
                }))
            }
            ExchangeNotifyType::Modify => Some(ExecutionEvent::Modified(OrderModified {
                client_order_id,
                venue_order_id,
                new_price: notif.price,
                new_qty: notif.modified_size.map(|q| q as f64),
                ts_event,
                context,
            })),
            ExchangeNotifyType::Status
            | ExchangeNotifyType::Trigger
            | ExchangeNotifyType::Generic => {
                tracing::debug!("Ignoring status/trigger/generic exchange notification");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    fn submitted_from_rithmic_notification() {
        let notif = RithmicOrderNotification {
            notify_type: Some(RithmicNotifyType::OrderRcvdFromClnt as i32),
            user_tag: Some("C1".to_string()),
            basket_id: Some("B1".to_string()),
            account_id: Some("ACCT".to_string()),
            is_snapshot: Some(true),
            symbol: Some("ESZ4".to_string()),
            exchange: Some("CME".to_string()),
            transaction_type: Some(1),
            price_type: Some(2),
            duration: Some(2),
            quantity: Some(3),
            price: Some(5025.25),
            total_unfilled_size: Some(3),
            ssboe: Some(1),
            usecs: Some(2),
            ..Default::default()
        };

        let handler = ExecutionHandler::new();
        let event = handler
            .handle_message(&RithmicMessage::RithmicOrderNotification(notif))
            .expect("expected submitted event");

        match event {
            ExecutionEvent::Submitted(s) => {
                assert_eq!(s.client_order_id, "C1");
                assert_eq!(s.venue_order_id.as_deref(), Some("B1"));
                assert_eq!(s.account_id, "ACCT");
                assert_eq!(s.ts_event, rithmic_to_unix_nanos(1, 2));
                assert_eq!(s.context.symbol.as_deref(), Some("ESZ4"));
                assert_eq!(s.context.exchange.as_deref(), Some("CME"));
                assert!(s.context.is_snapshot);
                assert_eq!(s.context.quantity, Some(3.0));
                assert_eq!(s.context.price, Some(5025.25));
                assert_eq!(s.context.leaves_qty, Some(3.0));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[rstest::rstest]
    fn fill_from_exchange_notification() {
        let notif = ExchangeOrderNotification {
            notify_type: Some(ExchangeNotifyType::Fill as i32),
            user_tag: Some("C2".to_string()),
            basket_id: Some("B2".to_string()),
            is_snapshot: Some(true),
            symbol: Some("ESZ4".to_string()),
            exchange: Some("CME".to_string()),
            transaction_type: Some(1),
            price_type: Some(2),
            duration: Some(1),
            quantity: Some(5),
            fill_price: Some(4500.25),
            fill_size: Some(2),
            fill_id: Some("FILL1".to_string()),
            currency: Some("USD".to_string()),
            total_unfilled_size: Some(3),
            ssboe: Some(10),
            usecs: Some(20),
            ..Default::default()
        };

        let handler = ExecutionHandler::new();
        let event = handler
            .handle_message(&RithmicMessage::ExchangeOrderNotification(notif))
            .expect("expected filled event");

        match event {
            ExecutionEvent::Filled(f) => {
                assert_eq!(f.client_order_id, "C2");
                assert_eq!(f.venue_order_id, "B2");
                assert_eq!(f.fill_price, 4500.25);
                assert_eq!(f.fill_qty, 2.0);
                assert_eq!(f.leaves_qty, 3.0);
                assert_eq!(f.ts_event, rithmic_to_unix_nanos(10, 20));
                assert_eq!(f.trade_id.as_deref(), Some("FILL1"));
                assert_eq!(f.currency.as_deref(), Some("USD"));
                assert!(f.context.is_snapshot);
                assert_eq!(f.context.symbol.as_deref(), Some("ESZ4"));
                assert_eq!(f.context.exchange.as_deref(), Some("CME"));
                assert_eq!(f.context.quantity, Some(5.0));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[rstest::rstest]
    fn reject_from_exchange_notification() {
        let notif = ExchangeOrderNotification {
            notify_type: Some(ExchangeNotifyType::Reject as i32),
            user_tag: Some("C3".to_string()),
            basket_id: Some("B3".to_string()),
            text: Some("Reason".to_string()),
            ssboe: Some(5),
            usecs: Some(6),
            ..Default::default()
        };

        let handler = ExecutionHandler::new();
        let event = handler
            .handle_message(&RithmicMessage::ExchangeOrderNotification(notif))
            .expect("expected rejected event");

        match event {
            ExecutionEvent::Rejected(r) => {
                assert_eq!(r.client_order_id, "C3");
                assert_eq!(r.reason, "Reason");
                assert_eq!(r.ts_event, rithmic_to_unix_nanos(5, 6));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[rstest::rstest]
    fn bracket_child_notification_uses_original_basket_id_when_user_tag_missing() {
        let notif = ExchangeOrderNotification {
            notify_type: Some(ExchangeNotifyType::Fill as i32),
            user_tag: None,
            original_basket_id: Some("PB1".to_string()),
            linked_basket_ids: Some("TB1".to_string()),
            bracket_type: Some(ExchangeBracketType::StopOnlyStatic as i32),
            basket_id: Some("SB1".to_string()),
            symbol: Some("ESZ4".to_string()),
            exchange: Some("CME".to_string()),
            fill_price: Some(4500.25),
            fill_size: Some(1),
            total_unfilled_size: Some(0),
            ssboe: Some(10),
            usecs: Some(20),
            ..Default::default()
        };

        let handler = ExecutionHandler::new();
        let event = handler
            .handle_message(&RithmicMessage::ExchangeOrderNotification(notif))
            .expect("expected filled event");

        match event {
            ExecutionEvent::Filled(f) => {
                assert_eq!(f.client_order_id, "PB1");
                assert_eq!(f.context.original_basket_id.as_deref(), Some("PB1"));
                assert_eq!(f.context.linked_basket_ids, vec!["TB1".to_string()]);
                assert_eq!(f.context.bracket_type.as_deref(), Some("STOP_ONLY_STATIC"));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
