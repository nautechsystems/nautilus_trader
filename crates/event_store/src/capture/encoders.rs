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

//! Representative encoders for the SPEC's allow-listed message surface.
//!
//! Phase 6 shipped a sample triple (`SubmitOrder` command, `OrderFilled` generated event,
//! `OrderStatusReport` raw venue report) so the bus capture adapter had a working
//! allow-list end-to-end. Phase 7 adds envelope-aware dispatchers for the
//! wrapper enums production code actually pushes through `send_trading_command` and
//! `publish_order_event` ([`TradingCommand`], [`OrderEventAny`]); these reach the bus
//! tap as their wrapper [`std::any::TypeId`] and the bare-type registrations would miss
//! them. Each dispatcher unwraps its variant, runs the inner-typed encode, and stamps
//! the inner-variant's canonical `payload_type` tag so forensics scans see entries
//! identical to the bare-type capture path.
//!
//! The payload serialization format is MessagePack via `rmp-serde`. The on-disk envelope
//! codec stays bincode (positional, non-self-describing); MessagePack inside the payload
//! handles the upstream Nautilus types that carry `#[serde(tag = "type")]` internal
//! tagging, which a non-self-describing format like bincode cannot round-trip.

use bytes::Bytes;
use nautilus_common::messages::execution::{
    BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder, QueryAccount, QueryOrder,
    SubmitOrder, SubmitOrderList, TradingCommand,
};
use nautilus_model::{
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEmulated,
        OrderEventAny, OrderExpired, OrderFilled, OrderInitialized, OrderModifyRejected,
        OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderReleased, OrderSubmitted,
        OrderTriggered, OrderUpdated,
    },
    reports::OrderStatusReport,
};
use serde::Serialize;
use ustr::Ustr;

use crate::{
    backend::{IndexKey, IndexKind},
    capture::{
        encoder::{EncodeError, EncodedPayload},
        registry::EncoderRegistry,
    },
    entry::PayloadType,
};

/// The canonical `payload_type` tag for [`SubmitOrder`].
pub const PAYLOAD_TYPE_SUBMIT_ORDER: &str = "SubmitOrder";
/// The canonical `payload_type` tag for [`SubmitOrderList`].
pub const PAYLOAD_TYPE_SUBMIT_ORDER_LIST: &str = "SubmitOrderList";
/// The canonical `payload_type` tag for [`ModifyOrder`].
pub const PAYLOAD_TYPE_MODIFY_ORDER: &str = "ModifyOrder";
/// The canonical `payload_type` tag for [`CancelOrder`].
pub const PAYLOAD_TYPE_CANCEL_ORDER: &str = "CancelOrder";
/// The canonical `payload_type` tag for [`CancelAllOrders`].
pub const PAYLOAD_TYPE_CANCEL_ALL_ORDERS: &str = "CancelAllOrders";
/// The canonical `payload_type` tag for [`BatchCancelOrders`].
pub const PAYLOAD_TYPE_BATCH_CANCEL_ORDERS: &str = "BatchCancelOrders";
/// The canonical `payload_type` tag for [`QueryOrder`].
pub const PAYLOAD_TYPE_QUERY_ORDER: &str = "QueryOrder";
/// The canonical `payload_type` tag for [`QueryAccount`].
pub const PAYLOAD_TYPE_QUERY_ACCOUNT: &str = "QueryAccount";

/// The canonical `payload_type` tag for [`OrderInitialized`].
pub const PAYLOAD_TYPE_ORDER_INITIALIZED: &str = "OrderInitialized";
/// The canonical `payload_type` tag for [`OrderDenied`].
pub const PAYLOAD_TYPE_ORDER_DENIED: &str = "OrderDenied";
/// The canonical `payload_type` tag for [`OrderEmulated`].
pub const PAYLOAD_TYPE_ORDER_EMULATED: &str = "OrderEmulated";
/// The canonical `payload_type` tag for [`OrderReleased`].
pub const PAYLOAD_TYPE_ORDER_RELEASED: &str = "OrderReleased";
/// The canonical `payload_type` tag for [`OrderSubmitted`].
pub const PAYLOAD_TYPE_ORDER_SUBMITTED: &str = "OrderSubmitted";
/// The canonical `payload_type` tag for [`OrderAccepted`].
pub const PAYLOAD_TYPE_ORDER_ACCEPTED: &str = "OrderAccepted";
/// The canonical `payload_type` tag for [`OrderRejected`].
pub const PAYLOAD_TYPE_ORDER_REJECTED: &str = "OrderRejected";
/// The canonical `payload_type` tag for [`OrderCanceled`].
pub const PAYLOAD_TYPE_ORDER_CANCELED: &str = "OrderCanceled";
/// The canonical `payload_type` tag for [`OrderExpired`].
pub const PAYLOAD_TYPE_ORDER_EXPIRED: &str = "OrderExpired";
/// The canonical `payload_type` tag for [`OrderTriggered`].
pub const PAYLOAD_TYPE_ORDER_TRIGGERED: &str = "OrderTriggered";
/// The canonical `payload_type` tag for [`OrderPendingUpdate`].
pub const PAYLOAD_TYPE_ORDER_PENDING_UPDATE: &str = "OrderPendingUpdate";
/// The canonical `payload_type` tag for [`OrderPendingCancel`].
pub const PAYLOAD_TYPE_ORDER_PENDING_CANCEL: &str = "OrderPendingCancel";
/// The canonical `payload_type` tag for [`OrderModifyRejected`].
pub const PAYLOAD_TYPE_ORDER_MODIFY_REJECTED: &str = "OrderModifyRejected";
/// The canonical `payload_type` tag for [`OrderCancelRejected`].
pub const PAYLOAD_TYPE_ORDER_CANCEL_REJECTED: &str = "OrderCancelRejected";
/// The canonical `payload_type` tag for [`OrderUpdated`].
pub const PAYLOAD_TYPE_ORDER_UPDATED: &str = "OrderUpdated";
/// The canonical `payload_type` tag for [`OrderFilled`].
pub const PAYLOAD_TYPE_ORDER_FILLED: &str = "OrderFilled";
/// The canonical `payload_type` tag for [`OrderStatusReport`].
pub const PAYLOAD_TYPE_ORDER_STATUS_REPORT: &str = "OrderStatusReport";

// Wrapper-level fallback tag reached only when a dispatcher returns an
// `EncodedPayload` without an override. Every current variant stamps its own
// inner tag, so this is a sentinel for a future variant that forgets the
// override rather than a tag the writer is expected to commit.
const PAYLOAD_TYPE_TRADING_COMMAND: &str = "TradingCommand";

const PAYLOAD_TYPE_ORDER_EVENT_ANY: &str = "OrderEventAny";

/// Returns an [`EncoderRegistry`] preloaded with the default encoders.
///
/// Callers can extend the returned registry with additional encoders before constructing
/// the [`crate::capture::BusCaptureAdapter`].
#[must_use]
pub fn default_registry() -> EncoderRegistry {
    let mut registry = EncoderRegistry::new();
    register_default(&mut registry);
    registry
}

/// Adds the default encoders to `registry`.
///
/// The bare-type registrations remain for capture sites that already submit the inner
/// type directly (the kernel's `RunStarted` path and a few internal tests). The envelope
/// registrations are what production bus traffic actually hits: `send_trading_command`
/// reaches the tap as [`TradingCommand`], and `publish_order_event` reaches it as
/// [`OrderEventAny`]. Without these wrapper-aware dispatchers the tap looks up the
/// wrapper's [`std::any::TypeId`], finds no encoder, and silently drops the capture.
pub fn register_default(registry: &mut EncoderRegistry) {
    registry
        .register::<SubmitOrder, _>(payload_type(PAYLOAD_TYPE_SUBMIT_ORDER), encode_submit_order);
    registry
        .register::<OrderFilled, _>(payload_type(PAYLOAD_TYPE_ORDER_FILLED), encode_order_filled);
    registry.register::<OrderStatusReport, _>(
        payload_type(PAYLOAD_TYPE_ORDER_STATUS_REPORT),
        encode_order_status_report,
    );
    registry.register::<TradingCommand, _>(
        payload_type(PAYLOAD_TYPE_TRADING_COMMAND),
        encode_trading_command,
    );
    registry.register::<OrderEventAny, _>(
        payload_type(PAYLOAD_TYPE_ORDER_EVENT_ANY),
        encode_order_event_any,
    );
}

fn payload_type(tag: &str) -> PayloadType {
    Ustr::from(tag)
}

fn encode_serde<T: Serialize>(value: &T) -> Result<Bytes, EncodeError> {
    rmp_serde::to_vec_named(value)
        .map(Bytes::from)
        .map_err(|e| EncodeError::Serialize(e.to_string()))
}

/// Encodes a [`SubmitOrder`] command into canonical bytes plus its `client_order_id` index.
///
/// # Errors
///
/// Returns [`EncodeError::Serialize`] when MessagePack rejects the payload (a malformed
/// value the type system should make unrepresentable; surfaced rather than swallowed
/// because the audit contract refuses to drop captured commands).
pub fn encode_submit_order(message: &SubmitOrder) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(message)?;
    let index_keys = vec![IndexKey::new(
        IndexKind::ClientOrderId,
        message.client_order_id.to_string(),
    )];
    Ok(EncodedPayload::new(payload, index_keys))
}

/// Encodes an [`OrderFilled`] event into canonical bytes plus its `client_order_id` and
/// `venue_order_id` indices.
///
/// # Errors
///
/// Returns [`EncodeError::Serialize`] when MessagePack rejects the payload.
pub fn encode_order_filled(message: &OrderFilled) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(message)?;
    let index_keys = vec![
        IndexKey::new(
            IndexKind::ClientOrderId,
            message.client_order_id.to_string(),
        ),
        IndexKey::new(IndexKind::VenueOrderId, message.venue_order_id.to_string()),
    ];
    Ok(EncodedPayload::new(payload, index_keys))
}

/// Encodes a [`TradingCommand`] envelope by dispatching on the variant.
///
/// The captured entry's `payload_type` matches the inner-variant tag (e.g. `SubmitOrder`
/// rather than `TradingCommand`) so forensics scans pair with the same decoder as the
/// bare-type capture path. The serialized payload is the inner variant; the wrapper enum
/// is never written to disk.
///
/// # Errors
///
/// Returns the inner encoder's [`EncodeError`] for the [`TradingCommand::SubmitOrder`]
/// variant; other variants return [`EncodeError::Serialize`] when MessagePack rejects the
/// inner payload.
pub fn encode_trading_command(command: &TradingCommand) -> Result<EncodedPayload, EncodeError> {
    match command {
        TradingCommand::SubmitOrder(cmd) => {
            Ok(retag(encode_submit_order(cmd)?, PAYLOAD_TYPE_SUBMIT_ORDER))
        }
        TradingCommand::SubmitOrderList(cmd) => encode_submit_order_list(cmd),
        TradingCommand::ModifyOrder(cmd) => encode_modify_order(cmd),
        TradingCommand::CancelOrder(cmd) => encode_cancel_order(cmd),
        TradingCommand::CancelAllOrders(cmd) => encode_cancel_all_orders(cmd),
        TradingCommand::BatchCancelOrders(cmd) => encode_batch_cancel_orders(cmd),
        TradingCommand::QueryOrder(cmd) => encode_query_order(cmd),
        TradingCommand::QueryAccount(cmd) => encode_query_account(cmd),
    }
}

/// Encodes an [`OrderEventAny`] envelope by dispatching on the inner variant.
///
/// The captured entry's `payload_type` matches the inner-variant tag (e.g. `OrderFilled`
/// rather than `OrderEventAny`); the serialized payload is the inner variant.
///
/// # Errors
///
/// Returns the inner encoder's [`EncodeError`] for the [`OrderEventAny::Filled`] variant;
/// other variants return [`EncodeError::Serialize`] when MessagePack rejects the inner
/// payload.
pub fn encode_order_event_any(event: &OrderEventAny) -> Result<EncodedPayload, EncodeError> {
    match event {
        OrderEventAny::Initialized(e) => encode_order_initialized(e),
        OrderEventAny::Denied(e) => encode_order_denied(e),
        OrderEventAny::Emulated(e) => encode_order_emulated(e),
        OrderEventAny::Released(e) => encode_order_released(e),
        OrderEventAny::Submitted(e) => encode_order_submitted(e),
        OrderEventAny::Accepted(e) => encode_order_accepted(e),
        OrderEventAny::Rejected(e) => encode_order_rejected(e),
        OrderEventAny::Canceled(e) => encode_order_canceled(e),
        OrderEventAny::Expired(e) => encode_order_expired(e),
        OrderEventAny::Triggered(e) => encode_order_triggered(e),
        OrderEventAny::PendingUpdate(e) => encode_order_pending_update(e),
        OrderEventAny::PendingCancel(e) => encode_order_pending_cancel(e),
        OrderEventAny::ModifyRejected(e) => encode_order_modify_rejected(e),
        OrderEventAny::CancelRejected(e) => encode_order_cancel_rejected(e),
        OrderEventAny::Updated(e) => encode_order_updated(e),
        OrderEventAny::Filled(e) => Ok(retag(encode_order_filled(e)?, PAYLOAD_TYPE_ORDER_FILLED)),
    }
}

fn encode_submit_order_list(cmd: &SubmitOrderList) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(cmd)?;
    let index_keys = cmd
        .order_list
        .client_order_ids
        .iter()
        .map(|cid| IndexKey::new(IndexKind::ClientOrderId, cid.to_string()))
        .collect();
    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_SUBMIT_ORDER_LIST),
        payload,
        index_keys,
    ))
}

fn encode_modify_order(cmd: &ModifyOrder) -> Result<EncodedPayload, EncodeError> {
    encode_with_order_ids(
        cmd,
        PAYLOAD_TYPE_MODIFY_ORDER,
        cmd.client_order_id.to_string(),
        cmd.venue_order_id.map(|v| v.to_string()),
    )
}

fn encode_cancel_order(cmd: &CancelOrder) -> Result<EncodedPayload, EncodeError> {
    encode_with_order_ids(
        cmd,
        PAYLOAD_TYPE_CANCEL_ORDER,
        cmd.client_order_id.to_string(),
        cmd.venue_order_id.map(|v| v.to_string()),
    )
}

fn encode_cancel_all_orders(cmd: &CancelAllOrders) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(cmd)?;
    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_CANCEL_ALL_ORDERS),
        payload,
        Vec::new(),
    ))
}

fn encode_batch_cancel_orders(cmd: &BatchCancelOrders) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(cmd)?;
    let mut index_keys = Vec::with_capacity(cmd.cancels.len() * 2);
    for c in &cmd.cancels {
        index_keys.push(IndexKey::new(
            IndexKind::ClientOrderId,
            c.client_order_id.to_string(),
        ));

        if let Some(venue) = c.venue_order_id {
            index_keys.push(IndexKey::new(IndexKind::VenueOrderId, venue.to_string()));
        }
    }
    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_BATCH_CANCEL_ORDERS),
        payload,
        index_keys,
    ))
}

fn encode_query_order(cmd: &QueryOrder) -> Result<EncodedPayload, EncodeError> {
    encode_with_order_ids(
        cmd,
        PAYLOAD_TYPE_QUERY_ORDER,
        cmd.client_order_id.to_string(),
        cmd.venue_order_id.map(|v| v.to_string()),
    )
}

fn encode_query_account(cmd: &QueryAccount) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(cmd)?;
    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_QUERY_ACCOUNT),
        payload,
        Vec::new(),
    ))
}

fn encode_order_initialized(e: &OrderInitialized) -> Result<EncodedPayload, EncodeError> {
    encode_with_order_ids(
        e,
        PAYLOAD_TYPE_ORDER_INITIALIZED,
        e.client_order_id.to_string(),
        None,
    )
}

fn encode_order_denied(e: &OrderDenied) -> Result<EncodedPayload, EncodeError> {
    encode_with_order_ids(
        e,
        PAYLOAD_TYPE_ORDER_DENIED,
        e.client_order_id.to_string(),
        None,
    )
}

fn encode_order_emulated(e: &OrderEmulated) -> Result<EncodedPayload, EncodeError> {
    encode_with_order_ids(
        e,
        PAYLOAD_TYPE_ORDER_EMULATED,
        e.client_order_id.to_string(),
        None,
    )
}

fn encode_order_released(e: &OrderReleased) -> Result<EncodedPayload, EncodeError> {
    encode_with_order_ids(
        e,
        PAYLOAD_TYPE_ORDER_RELEASED,
        e.client_order_id.to_string(),
        None,
    )
}

fn encode_order_submitted(e: &OrderSubmitted) -> Result<EncodedPayload, EncodeError> {
    encode_with_order_ids(
        e,
        PAYLOAD_TYPE_ORDER_SUBMITTED,
        e.client_order_id.to_string(),
        None,
    )
}

fn encode_order_accepted(e: &OrderAccepted) -> Result<EncodedPayload, EncodeError> {
    encode_with_order_ids(
        e,
        PAYLOAD_TYPE_ORDER_ACCEPTED,
        e.client_order_id.to_string(),
        Some(e.venue_order_id.to_string()),
    )
}

fn encode_order_rejected(e: &OrderRejected) -> Result<EncodedPayload, EncodeError> {
    encode_with_order_ids(
        e,
        PAYLOAD_TYPE_ORDER_REJECTED,
        e.client_order_id.to_string(),
        None,
    )
}

fn encode_order_canceled(e: &OrderCanceled) -> Result<EncodedPayload, EncodeError> {
    encode_with_order_ids(
        e,
        PAYLOAD_TYPE_ORDER_CANCELED,
        e.client_order_id.to_string(),
        e.venue_order_id.map(|v| v.to_string()),
    )
}

fn encode_order_expired(e: &OrderExpired) -> Result<EncodedPayload, EncodeError> {
    encode_with_order_ids(
        e,
        PAYLOAD_TYPE_ORDER_EXPIRED,
        e.client_order_id.to_string(),
        e.venue_order_id.map(|v| v.to_string()),
    )
}

fn encode_order_triggered(e: &OrderTriggered) -> Result<EncodedPayload, EncodeError> {
    encode_with_order_ids(
        e,
        PAYLOAD_TYPE_ORDER_TRIGGERED,
        e.client_order_id.to_string(),
        e.venue_order_id.map(|v| v.to_string()),
    )
}

fn encode_order_pending_update(e: &OrderPendingUpdate) -> Result<EncodedPayload, EncodeError> {
    encode_with_order_ids(
        e,
        PAYLOAD_TYPE_ORDER_PENDING_UPDATE,
        e.client_order_id.to_string(),
        e.venue_order_id.map(|v| v.to_string()),
    )
}

fn encode_order_pending_cancel(e: &OrderPendingCancel) -> Result<EncodedPayload, EncodeError> {
    encode_with_order_ids(
        e,
        PAYLOAD_TYPE_ORDER_PENDING_CANCEL,
        e.client_order_id.to_string(),
        e.venue_order_id.map(|v| v.to_string()),
    )
}

fn encode_order_modify_rejected(e: &OrderModifyRejected) -> Result<EncodedPayload, EncodeError> {
    encode_with_order_ids(
        e,
        PAYLOAD_TYPE_ORDER_MODIFY_REJECTED,
        e.client_order_id.to_string(),
        e.venue_order_id.map(|v| v.to_string()),
    )
}

fn encode_order_cancel_rejected(e: &OrderCancelRejected) -> Result<EncodedPayload, EncodeError> {
    encode_with_order_ids(
        e,
        PAYLOAD_TYPE_ORDER_CANCEL_REJECTED,
        e.client_order_id.to_string(),
        e.venue_order_id.map(|v| v.to_string()),
    )
}

fn encode_order_updated(e: &OrderUpdated) -> Result<EncodedPayload, EncodeError> {
    encode_with_order_ids(
        e,
        PAYLOAD_TYPE_ORDER_UPDATED,
        e.client_order_id.to_string(),
        e.venue_order_id.map(|v| v.to_string()),
    )
}

fn encode_with_order_ids<T: Serialize>(
    value: &T,
    tag: &str,
    client_order_id: String,
    venue_order_id: Option<String>,
) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(value)?;
    let mut index_keys = Vec::with_capacity(2);
    index_keys.push(IndexKey::new(IndexKind::ClientOrderId, client_order_id));
    if let Some(venue) = venue_order_id {
        index_keys.push(IndexKey::new(IndexKind::VenueOrderId, venue));
    }
    Ok(EncodedPayload::with_payload_type(
        payload_type(tag),
        payload,
        index_keys,
    ))
}

fn retag(mut encoded: EncodedPayload, tag: &str) -> EncodedPayload {
    encoded.payload_type = Some(payload_type(tag));
    encoded
}

/// Encodes an [`OrderStatusReport`] into canonical bytes plus its `venue_order_id` index
/// and, when present, its `client_order_id` index.
///
/// External orders observed only at the venue may not carry a `client_order_id`; the
/// index is omitted in that case so the secondary index never records an empty key.
///
/// # Errors
///
/// Returns [`EncodeError::Serialize`] when MessagePack rejects the payload.
pub fn encode_order_status_report(
    message: &OrderStatusReport,
) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(message)?;
    let mut index_keys = Vec::with_capacity(2);
    index_keys.push(IndexKey::new(
        IndexKind::VenueOrderId,
        message.venue_order_id.to_string(),
    ));

    if let Some(client_order_id) = &message.client_order_id {
        index_keys.push(IndexKey::new(
            IndexKind::ClientOrderId,
            client_order_id.to_string(),
        ));
    }
    Ok(EncodedPayload::new(payload, index_keys))
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
        identifiers::{
            AccountId, ClientId, ClientOrderId, InstrumentId, OrderListId, StrategyId, TradeId,
            TraderId, VenueOrderId,
        },
        orders::OrderList,
        types::{Currency, Money, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    fn trader_id() -> TraderId {
        TraderId::from("TRADER-001")
    }

    fn strategy_id() -> StrategyId {
        StrategyId::from("S-001")
    }

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("ETHUSDT-PERP.BINANCE")
    }

    fn client_order_id() -> ClientOrderId {
        ClientOrderId::from("O-20260510-000001")
    }

    fn venue_order_id() -> VenueOrderId {
        VenueOrderId::from("V-12345")
    }

    fn make_submit_order() -> SubmitOrder {
        let order_init = OrderInitialized::new(
            trader_id(),
            strategy_id(),
            instrument_id(),
            client_order_id(),
            OrderSide::Buy,
            OrderType::Market,
            Quantity::from("1"),
            TimeInForce::Gtc,
            false,
            false,
            false,
            false,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
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
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        SubmitOrder::new(
            trader_id(),
            Some(ClientId::from("BINANCE")),
            strategy_id(),
            instrument_id(),
            client_order_id(),
            order_init,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::from(3),
        )
    }

    fn make_order_filled() -> OrderFilled {
        OrderFilled::new(
            trader_id(),
            strategy_id(),
            instrument_id(),
            client_order_id(),
            venue_order_id(),
            AccountId::from("BINANCE-001"),
            TradeId::from("T-9999"),
            OrderSide::Buy,
            OrderType::Market,
            Quantity::from("1"),
            Price::from("100.00"),
            Currency::USDT(),
            LiquiditySide::Taker,
            UUID4::new(),
            UnixNanos::from(10),
            UnixNanos::from(11),
            false,
            None,
            Some(Money::new(0.10, Currency::USDT())),
        )
    }

    fn make_order_status_report() -> OrderStatusReport {
        OrderStatusReport::new(
            AccountId::from("BINANCE-001"),
            instrument_id(),
            Some(client_order_id()),
            venue_order_id(),
            OrderSide::Buy,
            OrderType::Market,
            TimeInForce::Gtc,
            OrderStatus::Filled,
            Quantity::from("1"),
            Quantity::from("1"),
            UnixNanos::from(20),
            UnixNanos::from(21),
            UnixNanos::from(22),
            Some(UUID4::new()),
        )
    }

    #[rstest]
    fn submit_order_encoder_emits_client_order_id_index() {
        let cmd = make_submit_order();
        let encoded = encode_submit_order(&cmd).expect("encode");

        assert!(!encoded.payload.is_empty());
        assert_eq!(encoded.index_keys.len(), 1);
        assert_eq!(encoded.index_keys[0].kind, IndexKind::ClientOrderId);
        assert_eq!(encoded.index_keys[0].key, cmd.client_order_id.to_string());
    }

    #[rstest]
    fn order_filled_encoder_emits_client_and_venue_order_id_indices() {
        let event = make_order_filled();
        let encoded = encode_order_filled(&event).expect("encode");

        assert!(!encoded.payload.is_empty());
        assert_eq!(encoded.index_keys.len(), 2);
        assert_eq!(encoded.index_keys[0].kind, IndexKind::ClientOrderId);
        assert_eq!(encoded.index_keys[0].key, event.client_order_id.to_string());
        assert_eq!(encoded.index_keys[1].kind, IndexKind::VenueOrderId);
        assert_eq!(encoded.index_keys[1].key, event.venue_order_id.to_string());
    }

    #[rstest]
    fn order_status_report_encoder_includes_client_order_id_when_present() {
        let report = make_order_status_report();
        let encoded = encode_order_status_report(&report).expect("encode");

        assert_eq!(encoded.index_keys.len(), 2);
        assert_eq!(encoded.index_keys[0].kind, IndexKind::VenueOrderId);
        assert_eq!(encoded.index_keys[1].kind, IndexKind::ClientOrderId);
    }

    #[rstest]
    fn order_status_report_encoder_omits_client_order_id_when_absent() {
        let mut report = make_order_status_report();
        report.client_order_id = None;
        let encoded = encode_order_status_report(&report).expect("encode");

        assert_eq!(encoded.index_keys.len(), 1);
        assert_eq!(encoded.index_keys[0].kind, IndexKind::VenueOrderId);
    }

    #[rstest]
    fn default_registry_contains_bare_and_envelope_encoders() {
        let registry = default_registry();

        assert_eq!(registry.len(), 5);
        assert!(registry.contains::<SubmitOrder>());
        assert!(registry.contains::<OrderFilled>());
        assert!(registry.contains::<OrderStatusReport>());
        assert!(registry.contains::<TradingCommand>());
        assert!(registry.contains::<OrderEventAny>());
    }

    #[rstest]
    fn submit_order_payload_round_trips_through_msgpack() {
        let cmd = make_submit_order();
        let encoded = encode_submit_order(&cmd).expect("encode");

        let decoded: SubmitOrder = rmp_serde::from_slice(&encoded.payload).expect("decode");
        assert_eq!(decoded, cmd);
    }

    #[rstest]
    fn order_filled_payload_round_trips_through_msgpack() {
        let event = make_order_filled();
        let encoded = encode_order_filled(&event).expect("encode");

        let decoded: OrderFilled = rmp_serde::from_slice(&encoded.payload).expect("decode");
        assert_eq!(decoded, event);
    }

    #[rstest]
    fn order_status_report_payload_round_trips_through_msgpack() {
        let report = make_order_status_report();
        let encoded = encode_order_status_report(&report).expect("encode");

        let decoded: OrderStatusReport = rmp_serde::from_slice(&encoded.payload).expect("decode");
        assert_eq!(decoded, report);
    }

    fn make_cancel_order() -> CancelOrder {
        CancelOrder::new(
            trader_id(),
            Some(ClientId::from("BINANCE")),
            strategy_id(),
            instrument_id(),
            client_order_id(),
            Some(venue_order_id()),
            UUID4::new(),
            UnixNanos::from(4),
            None,
        )
    }

    fn make_query_account() -> QueryAccount {
        QueryAccount::new(
            trader_id(),
            Some(ClientId::from("BINANCE")),
            AccountId::from("BINANCE-001"),
            UUID4::new(),
            UnixNanos::from(5),
            None,
        )
    }

    fn make_order_submitted() -> OrderSubmitted {
        OrderSubmitted::new(
            trader_id(),
            strategy_id(),
            instrument_id(),
            client_order_id(),
            AccountId::from("BINANCE-001"),
            UUID4::new(),
            UnixNanos::from(30),
            UnixNanos::from(31),
        )
    }

    fn make_modify_order(venue: Option<VenueOrderId>) -> ModifyOrder {
        ModifyOrder::new(
            trader_id(),
            Some(ClientId::from("BINANCE")),
            strategy_id(),
            instrument_id(),
            client_order_id(),
            venue,
            Some(Quantity::from("2")),
            Some(Price::from("100.00")),
            None,
            UUID4::new(),
            UnixNanos::from(6),
            None,
        )
    }

    fn make_cancel_all_orders() -> CancelAllOrders {
        CancelAllOrders::new(
            trader_id(),
            Some(ClientId::from("BINANCE")),
            strategy_id(),
            instrument_id(),
            OrderSide::Buy,
            UUID4::new(),
            UnixNanos::from(7),
            None,
        )
    }

    fn make_query_order(venue: Option<VenueOrderId>) -> QueryOrder {
        QueryOrder::new(
            trader_id(),
            Some(ClientId::from("BINANCE")),
            strategy_id(),
            instrument_id(),
            client_order_id(),
            venue,
            UUID4::new(),
            UnixNanos::from(8),
            None,
        )
    }

    fn make_batch_cancel_orders(cancels: Vec<CancelOrder>) -> BatchCancelOrders {
        BatchCancelOrders::new(
            trader_id(),
            Some(ClientId::from("BINANCE")),
            strategy_id(),
            instrument_id(),
            cancels,
            UUID4::new(),
            UnixNanos::from(9),
            None,
        )
    }

    fn make_submit_order_list(client_order_ids: Vec<ClientOrderId>) -> SubmitOrderList {
        // OrderList::new asserts that order_inits' client_order_ids match the list's,
        // so we mint one OrderInitialized per id with the matching client_order_id.
        let order_inits: Vec<OrderInitialized> = client_order_ids
            .iter()
            .copied()
            .map(make_order_initialized_with_id)
            .collect();
        let order_list = OrderList::new(
            OrderListId::from("OL-1"),
            instrument_id(),
            strategy_id(),
            client_order_ids,
            UnixNanos::from(10),
        );
        SubmitOrderList::new(
            trader_id(),
            Some(ClientId::from("BINANCE")),
            strategy_id(),
            order_list,
            order_inits,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::from(11),
        )
    }

    fn make_order_initialized_with_id(client_order_id: ClientOrderId) -> OrderInitialized {
        OrderInitialized {
            client_order_id,
            ..OrderInitialized::default()
        }
    }

    fn ev_initialized() -> OrderEventAny {
        OrderEventAny::Initialized(make_order_initialized_with_id(client_order_id()))
    }

    fn ev_denied() -> OrderEventAny {
        OrderEventAny::Denied(OrderDenied {
            client_order_id: client_order_id(),
            ..Default::default()
        })
    }

    fn ev_emulated() -> OrderEventAny {
        OrderEventAny::Emulated(OrderEmulated {
            client_order_id: client_order_id(),
            ..Default::default()
        })
    }

    fn ev_released() -> OrderEventAny {
        OrderEventAny::Released(OrderReleased {
            client_order_id: client_order_id(),
            ..Default::default()
        })
    }

    fn ev_submitted() -> OrderEventAny {
        OrderEventAny::Submitted(make_order_submitted())
    }

    fn ev_accepted_with_venue(venue: VenueOrderId) -> OrderEventAny {
        OrderEventAny::Accepted(OrderAccepted {
            client_order_id: client_order_id(),
            venue_order_id: venue,
            ..Default::default()
        })
    }

    fn ev_rejected() -> OrderEventAny {
        OrderEventAny::Rejected(OrderRejected {
            client_order_id: client_order_id(),
            ..Default::default()
        })
    }

    fn ev_canceled(venue: Option<VenueOrderId>) -> OrderEventAny {
        OrderEventAny::Canceled(OrderCanceled {
            client_order_id: client_order_id(),
            venue_order_id: venue,
            ..Default::default()
        })
    }

    fn ev_expired(venue: Option<VenueOrderId>) -> OrderEventAny {
        OrderEventAny::Expired(OrderExpired {
            client_order_id: client_order_id(),
            venue_order_id: venue,
            ..Default::default()
        })
    }

    fn ev_triggered(venue: Option<VenueOrderId>) -> OrderEventAny {
        OrderEventAny::Triggered(OrderTriggered {
            client_order_id: client_order_id(),
            venue_order_id: venue,
            ..Default::default()
        })
    }

    fn ev_pending_update(venue: Option<VenueOrderId>) -> OrderEventAny {
        OrderEventAny::PendingUpdate(OrderPendingUpdate {
            client_order_id: client_order_id(),
            venue_order_id: venue,
            ..Default::default()
        })
    }

    fn ev_pending_cancel(venue: Option<VenueOrderId>) -> OrderEventAny {
        OrderEventAny::PendingCancel(OrderPendingCancel {
            client_order_id: client_order_id(),
            venue_order_id: venue,
            ..Default::default()
        })
    }

    fn ev_modify_rejected(venue: Option<VenueOrderId>) -> OrderEventAny {
        OrderEventAny::ModifyRejected(OrderModifyRejected {
            client_order_id: client_order_id(),
            venue_order_id: venue,
            ..Default::default()
        })
    }

    fn ev_cancel_rejected(venue: Option<VenueOrderId>) -> OrderEventAny {
        OrderEventAny::CancelRejected(OrderCancelRejected {
            client_order_id: client_order_id(),
            venue_order_id: venue,
            ..Default::default()
        })
    }

    fn ev_updated(venue: Option<VenueOrderId>) -> OrderEventAny {
        OrderEventAny::Updated(OrderUpdated {
            client_order_id: client_order_id(),
            venue_order_id: venue,
            ..Default::default()
        })
    }

    fn ev_filled() -> OrderEventAny {
        OrderEventAny::Filled(make_order_filled())
    }

    #[rstest]
    fn trading_command_envelope_stamps_inner_submit_order_payload_type() {
        // TradingCommand reaches the bus tap as the wrapper TypeId; the dispatcher must
        // unwrap to SubmitOrder, produce the same bytes and indices as the bare-type
        // encoder, and stamp the inner payload_type so forensics scans pair the entry
        // with the SubmitOrder decoder.
        let cmd = make_submit_order();
        let bare = encode_submit_order(&cmd).expect("bare");

        let envelope = TradingCommand::SubmitOrder(cmd);
        let wrapped = encode_trading_command(&envelope).expect("envelope");

        assert_eq!(wrapped.payload, bare.payload);
        assert_eq!(wrapped.index_keys, bare.index_keys);
        assert_eq!(
            wrapped.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_SUBMIT_ORDER,
        );
    }

    #[rstest]
    fn trading_command_cancel_order_envelope_emits_client_and_venue_indices() {
        let cancel = make_cancel_order();
        let envelope = TradingCommand::CancelOrder(cancel.clone());
        let wrapped = encode_trading_command(&envelope).expect("envelope");

        assert_eq!(
            wrapped.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_CANCEL_ORDER,
        );
        assert_eq!(wrapped.index_keys.len(), 2);
        assert_eq!(wrapped.index_keys[0].kind, IndexKind::ClientOrderId);
        assert_eq!(
            wrapped.index_keys[0].key,
            cancel.client_order_id.to_string(),
        );
        assert_eq!(wrapped.index_keys[1].kind, IndexKind::VenueOrderId);
        assert_eq!(
            wrapped.index_keys[1].key,
            cancel.venue_order_id.expect("set").to_string(),
        );

        let decoded: CancelOrder = rmp_serde::from_slice(&wrapped.payload).expect("decode");
        assert_eq!(decoded, cancel);
    }

    #[rstest]
    fn trading_command_query_account_envelope_records_no_order_indices() {
        // QueryAccount carries no client_order_id or venue_order_id; the dispatcher
        // must not invent empty index keys.
        let envelope = TradingCommand::QueryAccount(make_query_account());
        let wrapped = encode_trading_command(&envelope).expect("envelope");

        assert_eq!(
            wrapped.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_QUERY_ACCOUNT,
        );
        assert!(wrapped.index_keys.is_empty());
    }

    #[rstest]
    fn order_event_any_envelope_stamps_inner_filled_payload_type() {
        let filled = make_order_filled();
        let bare = encode_order_filled(&filled).expect("bare");

        let envelope = OrderEventAny::Filled(filled);
        let wrapped = encode_order_event_any(&envelope).expect("envelope");

        assert_eq!(wrapped.payload, bare.payload);
        assert_eq!(wrapped.index_keys, bare.index_keys);
        assert_eq!(
            wrapped.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_ORDER_FILLED,
        );
    }

    #[rstest]
    fn order_event_any_submitted_envelope_emits_client_order_id_index() {
        let submitted = make_order_submitted();
        let envelope = OrderEventAny::Submitted(submitted);
        let wrapped = encode_order_event_any(&envelope).expect("envelope");

        assert_eq!(
            wrapped.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_ORDER_SUBMITTED,
        );
        assert_eq!(wrapped.index_keys.len(), 1);
        assert_eq!(wrapped.index_keys[0].kind, IndexKind::ClientOrderId);
        assert_eq!(
            wrapped.index_keys[0].key,
            submitted.client_order_id.to_string(),
        );

        let decoded: OrderSubmitted = rmp_serde::from_slice(&wrapped.payload).expect("decode");
        assert_eq!(decoded, submitted);
    }

    // Walks every TradingCommand variant and asserts the dispatcher stamps the inner-variant
    // payload_type tag and emits the expected number of index keys. Catches a swapped match
    // arm or a forgotten `with_payload_type` override that would otherwise fall back to the
    // wrapper sentinel tag.
    #[rstest]
    #[case::submit_order(
        TradingCommand::SubmitOrder(make_submit_order()),
        PAYLOAD_TYPE_SUBMIT_ORDER,
        1
    )]
    #[case::submit_order_list(
        TradingCommand::SubmitOrderList(make_submit_order_list(vec![
            ClientOrderId::from("O-A"),
            ClientOrderId::from("O-B"),
        ])),
        PAYLOAD_TYPE_SUBMIT_ORDER_LIST,
        2,
    )]
    #[case::modify_order(
        TradingCommand::ModifyOrder(make_modify_order(Some(venue_order_id()))),
        PAYLOAD_TYPE_MODIFY_ORDER,
        2
    )]
    #[case::cancel_order(
        TradingCommand::CancelOrder(make_cancel_order()),
        PAYLOAD_TYPE_CANCEL_ORDER,
        2
    )]
    #[case::cancel_all_orders(
        TradingCommand::CancelAllOrders(make_cancel_all_orders()),
        PAYLOAD_TYPE_CANCEL_ALL_ORDERS,
        0
    )]
    #[case::batch_cancel_orders(
        TradingCommand::BatchCancelOrders(make_batch_cancel_orders(vec![make_cancel_order()])),
        PAYLOAD_TYPE_BATCH_CANCEL_ORDERS,
        2,
    )]
    #[case::query_order(
        TradingCommand::QueryOrder(make_query_order(Some(venue_order_id()))),
        PAYLOAD_TYPE_QUERY_ORDER,
        2
    )]
    #[case::query_account(
        TradingCommand::QueryAccount(make_query_account()),
        PAYLOAD_TYPE_QUERY_ACCOUNT,
        0
    )]
    fn trading_command_envelope_stamps_inner_tag_for_every_variant(
        #[case] command: TradingCommand,
        #[case] expected_tag: &str,
        #[case] expected_index_count: usize,
    ) {
        let encoded = encode_trading_command(&command).expect("encode");
        let tag = encoded.payload_type.expect("override").as_str().to_string();

        assert_eq!(tag, expected_tag);
        assert_ne!(
            tag, PAYLOAD_TYPE_TRADING_COMMAND,
            "wrapper fallback tag must never reach the writer",
        );
        assert_eq!(encoded.index_keys.len(), expected_index_count);
    }

    // Walks every OrderEventAny variant. Builds each variant from `Default::default()`
    // (gated by the `stubs` feature on `nautilus-model`) with the test client_order_id
    // patched in, so the assertion can verify the index value alongside the tag.
    #[rstest]
    #[case::initialized(ev_initialized(), PAYLOAD_TYPE_ORDER_INITIALIZED, false)]
    #[case::denied(ev_denied(), PAYLOAD_TYPE_ORDER_DENIED, false)]
    #[case::emulated(ev_emulated(), PAYLOAD_TYPE_ORDER_EMULATED, false)]
    #[case::released(ev_released(), PAYLOAD_TYPE_ORDER_RELEASED, false)]
    #[case::submitted(ev_submitted(), PAYLOAD_TYPE_ORDER_SUBMITTED, false)]
    #[case::accepted(
        ev_accepted_with_venue(venue_order_id()),
        PAYLOAD_TYPE_ORDER_ACCEPTED,
        true
    )]
    #[case::rejected(ev_rejected(), PAYLOAD_TYPE_ORDER_REJECTED, false)]
    #[case::canceled(ev_canceled(Some(venue_order_id())), PAYLOAD_TYPE_ORDER_CANCELED, true)]
    #[case::expired(ev_expired(Some(venue_order_id())), PAYLOAD_TYPE_ORDER_EXPIRED, true)]
    #[case::triggered(
        ev_triggered(Some(venue_order_id())),
        PAYLOAD_TYPE_ORDER_TRIGGERED,
        true
    )]
    #[case::pending_update(
        ev_pending_update(Some(venue_order_id())),
        PAYLOAD_TYPE_ORDER_PENDING_UPDATE,
        true
    )]
    #[case::pending_cancel(
        ev_pending_cancel(Some(venue_order_id())),
        PAYLOAD_TYPE_ORDER_PENDING_CANCEL,
        true
    )]
    #[case::modify_rejected(
        ev_modify_rejected(Some(venue_order_id())),
        PAYLOAD_TYPE_ORDER_MODIFY_REJECTED,
        true
    )]
    #[case::cancel_rejected(
        ev_cancel_rejected(Some(venue_order_id())),
        PAYLOAD_TYPE_ORDER_CANCEL_REJECTED,
        true
    )]
    #[case::updated(ev_updated(Some(venue_order_id())), PAYLOAD_TYPE_ORDER_UPDATED, true)]
    #[case::filled(ev_filled(), PAYLOAD_TYPE_ORDER_FILLED, true)]
    fn order_event_any_envelope_stamps_inner_tag_for_every_variant(
        #[case] event: OrderEventAny,
        #[case] expected_tag: &str,
        #[case] expects_venue_index: bool,
    ) {
        let encoded = encode_order_event_any(&event).expect("encode");
        let tag = encoded.payload_type.expect("override").as_str().to_string();

        assert_eq!(tag, expected_tag);
        assert_ne!(
            tag, PAYLOAD_TYPE_ORDER_EVENT_ANY,
            "wrapper fallback tag must never reach the writer",
        );
        assert_eq!(
            encoded.index_keys[0].kind,
            IndexKind::ClientOrderId,
            "first index must always be ClientOrderId for every order event",
        );
        assert_eq!(encoded.index_keys[0].key, client_order_id().to_string(),);
        if expects_venue_index {
            assert_eq!(encoded.index_keys.len(), 2);
            assert_eq!(encoded.index_keys[1].kind, IndexKind::VenueOrderId);
        } else {
            assert_eq!(encoded.index_keys.len(), 1);
        }
    }

    // Optional venue_order_id branch coverage for variants that carry one. None must skip
    // the VenueOrderId index entirely; Some must push it second.
    #[rstest]
    #[case::cancel_order_some(TradingCommand::CancelOrder(make_cancel_order()), 2)]
    #[case::cancel_order_none(
        TradingCommand::CancelOrder(CancelOrder {
            venue_order_id: None,
            ..make_cancel_order()
        }),
        1,
    )]
    #[case::modify_order_some(
        TradingCommand::ModifyOrder(make_modify_order(Some(venue_order_id()))),
        2
    )]
    #[case::modify_order_none(TradingCommand::ModifyOrder(make_modify_order(None)), 1)]
    #[case::query_order_some(
        TradingCommand::QueryOrder(make_query_order(Some(venue_order_id()))),
        2
    )]
    #[case::query_order_none(TradingCommand::QueryOrder(make_query_order(None)), 1)]
    fn trading_command_envelope_index_count_matches_venue_optionality(
        #[case] command: TradingCommand,
        #[case] expected_index_count: usize,
    ) {
        let encoded = encode_trading_command(&command).expect("encode");
        assert_eq!(encoded.index_keys.len(), expected_index_count);
        assert_eq!(encoded.index_keys[0].kind, IndexKind::ClientOrderId);
        if expected_index_count == 2 {
            assert_eq!(encoded.index_keys[1].kind, IndexKind::VenueOrderId);
        }
    }

    #[rstest]
    #[case::canceled_some(ev_canceled(Some(venue_order_id())), 2)]
    #[case::canceled_none(ev_canceled(None), 1)]
    #[case::updated_some(ev_updated(Some(venue_order_id())), 2)]
    #[case::updated_none(ev_updated(None), 1)]
    #[case::pending_update_some(ev_pending_update(Some(venue_order_id())), 2)]
    #[case::pending_update_none(ev_pending_update(None), 1)]
    fn order_event_any_envelope_index_count_matches_venue_optionality(
        #[case] event: OrderEventAny,
        #[case] expected_index_count: usize,
    ) {
        let encoded = encode_order_event_any(&event).expect("encode");
        assert_eq!(encoded.index_keys.len(), expected_index_count);
    }

    #[rstest]
    fn batch_cancel_orders_envelope_indexes_each_child_with_optional_venue() {
        // Two cancels: one with a venue_order_id (contributes 2 indices), one without
        // (contributes 1). The dispatcher must preserve both children's identifiers in
        // the same order they appear in the batch.
        let with_venue = make_cancel_order();
        let mut without_venue = make_cancel_order();
        without_venue.venue_order_id = None;
        without_venue.client_order_id = ClientOrderId::from("O-NOVENUE");
        let batch = make_batch_cancel_orders(vec![with_venue.clone(), without_venue.clone()]);

        let encoded =
            encode_trading_command(&TradingCommand::BatchCancelOrders(batch)).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_BATCH_CANCEL_ORDERS,
        );
        assert_eq!(encoded.index_keys.len(), 3);
        assert_eq!(encoded.index_keys[0].kind, IndexKind::ClientOrderId);
        assert_eq!(
            encoded.index_keys[0].key,
            with_venue.client_order_id.to_string(),
        );
        assert_eq!(encoded.index_keys[1].kind, IndexKind::VenueOrderId);
        assert_eq!(
            encoded.index_keys[1].key,
            with_venue.venue_order_id.expect("set").to_string(),
        );
        assert_eq!(encoded.index_keys[2].kind, IndexKind::ClientOrderId);
        assert_eq!(
            encoded.index_keys[2].key,
            without_venue.client_order_id.to_string(),
        );
    }

    #[rstest]
    fn submit_order_list_envelope_indexes_each_client_order_id() {
        // SubmitOrderList carries N orders; the dispatcher must emit a ClientOrderId
        // index per child so forensics can resolve any of the list's intents to the
        // captured seq.
        let ids = vec![
            ClientOrderId::from("O-LIST-1"),
            ClientOrderId::from("O-LIST-2"),
            ClientOrderId::from("O-LIST-3"),
        ];
        let cmd = make_submit_order_list(ids.clone());
        let encoded =
            encode_trading_command(&TradingCommand::SubmitOrderList(cmd)).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_SUBMIT_ORDER_LIST,
        );
        assert_eq!(encoded.index_keys.len(), ids.len());
        for (idx, expected_id) in ids.iter().enumerate() {
            assert_eq!(encoded.index_keys[idx].kind, IndexKind::ClientOrderId);
            assert_eq!(encoded.index_keys[idx].key, expected_id.to_string());
        }
    }
}
