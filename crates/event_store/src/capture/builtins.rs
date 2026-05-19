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
//! wrapper enums production code actually pushes through `send_trading_command`,
//! `publish_order_event`, `send_execution_report`, and `publish_position_event`
//! ([`TradingCommand`], [`OrderEventAny`], [`ExecutionReport`], [`PositionEvent`]).
//! The same pattern covers `send_data_command` and `send_data_response` ([`DataCommand`],
//! [`DataResponse`]). These reach the bus tap as their wrapper [`std::any::TypeId`] and
//! the bare-type registrations would miss them. Each dispatcher unwraps its variant,
//! runs the inner-typed encode, and stamps the inner-variant's canonical `payload_type`
//! tag so forensics scans see entries identical to the bare-type capture path.
//!
//! The payload serialization format is MessagePack via `rmp-serde`. The on-disk envelope
//! codec stays bincode (positional, non-self-describing); MessagePack inside the payload
//! handles the upstream Nautilus types that carry `#[serde(tag = "type")]` internal
//! tagging, which a non-self-describing format like bincode cannot round-trip.

use std::collections::HashSet;

use bytes::Bytes;
use nautilus_common::messages::{
    data::{
        BarsResponse, BookResponse, CustomDataResponse, DataCommand, DataResponse,
        ForwardPricesResponse, FundingRatesResponse, InstrumentResponse, InstrumentsResponse,
        QuotesResponse, TradesResponse,
    },
    execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, ExecutionReport, ModifyOrder,
        QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList, TradingCommand,
    },
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::DataType,
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied,
        OrderEmulated, OrderEventAny, OrderExpired, OrderFilled, OrderInitialized,
        OrderModifyRejected, OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderReleased,
        OrderSubmitted, OrderTriggered, OrderUpdated, PositionAdjusted, PositionChanged,
        PositionClosed, PositionEvent, PositionOpened,
    },
    identifiers::{ClientId, InstrumentId, Venue},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
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
    headers::Headers,
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
/// The canonical `payload_type` tag for [`FillReport`].
pub const PAYLOAD_TYPE_FILL_REPORT: &str = "FillReport";
/// The canonical `payload_type` tag for the [`ExecutionReport::OrderWithFills`] bundle.
pub const PAYLOAD_TYPE_ORDER_WITH_FILLS: &str = "OrderWithFills";
/// The canonical `payload_type` tag for [`PositionStatusReport`].
pub const PAYLOAD_TYPE_POSITION_STATUS_REPORT: &str = "PositionStatusReport";
/// The canonical `payload_type` tag for [`ExecutionMassStatus`].
pub const PAYLOAD_TYPE_EXECUTION_MASS_STATUS: &str = "ExecutionMassStatus";
/// The canonical `payload_type` tag for [`PositionOpened`].
pub const PAYLOAD_TYPE_POSITION_OPENED: &str = "PositionOpened";
/// The canonical `payload_type` tag for [`PositionChanged`].
pub const PAYLOAD_TYPE_POSITION_CHANGED: &str = "PositionChanged";
/// The canonical `payload_type` tag for [`PositionClosed`].
pub const PAYLOAD_TYPE_POSITION_CLOSED: &str = "PositionClosed";
/// The canonical `payload_type` tag for [`PositionAdjusted`].
pub const PAYLOAD_TYPE_POSITION_ADJUSTED: &str = "PositionAdjusted";
/// The canonical `payload_type` tag for [`AccountState`].
pub const PAYLOAD_TYPE_ACCOUNT_STATE: &str = "AccountState";

/// The canonical `payload_type` tag for `RequestCommand`.
pub const PAYLOAD_TYPE_REQUEST_COMMAND: &str = "RequestCommand";
/// The canonical `payload_type` tag for `SubscribeCommand`.
pub const PAYLOAD_TYPE_SUBSCRIBE_COMMAND: &str = "SubscribeCommand";
/// The canonical `payload_type` tag for `UnsubscribeCommand`.
pub const PAYLOAD_TYPE_UNSUBSCRIBE_COMMAND: &str = "UnsubscribeCommand";
#[cfg(feature = "defi")]
/// The canonical `payload_type` tag for `DefiRequestCommand`.
pub const PAYLOAD_TYPE_DEFI_REQUEST_COMMAND: &str = "DefiRequestCommand";
#[cfg(feature = "defi")]
/// The canonical `payload_type` tag for `DefiSubscribeCommand`.
pub const PAYLOAD_TYPE_DEFI_SUBSCRIBE_COMMAND: &str = "DefiSubscribeCommand";
#[cfg(feature = "defi")]
/// The canonical `payload_type` tag for `DefiUnsubscribeCommand`.
pub const PAYLOAD_TYPE_DEFI_UNSUBSCRIBE_COMMAND: &str = "DefiUnsubscribeCommand";

/// The canonical `payload_type` tag for [`CustomDataResponse`].
pub const PAYLOAD_TYPE_CUSTOM_DATA_RESPONSE: &str = "CustomDataResponse";
/// The canonical `payload_type` tag for [`InstrumentResponse`].
pub const PAYLOAD_TYPE_INSTRUMENT_RESPONSE: &str = "InstrumentResponse";
/// The canonical `payload_type` tag for [`InstrumentsResponse`].
pub const PAYLOAD_TYPE_INSTRUMENTS_RESPONSE: &str = "InstrumentsResponse";
/// The canonical `payload_type` tag for [`BookResponse`].
pub const PAYLOAD_TYPE_BOOK_RESPONSE: &str = "BookResponse";
/// The canonical `payload_type` tag for [`QuotesResponse`].
pub const PAYLOAD_TYPE_QUOTES_RESPONSE: &str = "QuotesResponse";
/// The canonical `payload_type` tag for [`TradesResponse`].
pub const PAYLOAD_TYPE_TRADES_RESPONSE: &str = "TradesResponse";
/// The canonical `payload_type` tag for [`FundingRatesResponse`].
pub const PAYLOAD_TYPE_FUNDING_RATES_RESPONSE: &str = "FundingRatesResponse";
/// The canonical `payload_type` tag for [`ForwardPricesResponse`].
pub const PAYLOAD_TYPE_FORWARD_PRICES_RESPONSE: &str = "ForwardPricesResponse";
/// The canonical `payload_type` tag for [`BarsResponse`].
pub const PAYLOAD_TYPE_BARS_RESPONSE: &str = "BarsResponse";

// Wrapper-level fallback tag reached only when a dispatcher returns an
// `EncodedPayload` without an override. Every current variant stamps its own
// inner tag, so this is a sentinel for a future variant that forgets the
// override rather than a tag the writer is expected to commit.
const PAYLOAD_TYPE_TRADING_COMMAND: &str = "TradingCommand";

const PAYLOAD_TYPE_ORDER_EVENT_ANY: &str = "OrderEventAny";

const PAYLOAD_TYPE_EXECUTION_REPORT: &str = "ExecutionReport";

const PAYLOAD_TYPE_POSITION_EVENT: &str = "PositionEvent";

const PAYLOAD_TYPE_DATA_COMMAND: &str = "DataCommand";

const PAYLOAD_TYPE_DATA_RESPONSE: &str = "DataResponse";

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
/// reaches the tap as [`TradingCommand`], `publish_order_event` reaches it as
/// [`OrderEventAny`], `send_execution_report` reaches it as [`ExecutionReport`],
/// `publish_position_event` reaches it as [`PositionEvent`], and `send_data_response`
/// reaches it as [`DataResponse`]. Without these wrapper-aware dispatchers the tap looks
/// up the wrapper's [`std::any::TypeId`], finds no encoder, and silently drops the
/// capture.
///
/// [`AccountState`] is registered as a bare type: `publish_account_state` and
/// `send_account_state` both reach the tap as the same `AccountState` `TypeId`, so a
/// single registration covers both dispatch paths.
///
/// [`OrderStatusReport`], [`FillReport`], and [`PositionStatusReport`] are registered
/// as bare types because the execution engine publishes raw venue reports through
/// `publish_any` on `reconciliation.raw.*` topics before any state mutation. The
/// bare-type registration is what captures those raw inputs for forensic replay.
pub fn register_default(registry: &mut EncoderRegistry) {
    registry
        .register::<SubmitOrder, _>(payload_type(PAYLOAD_TYPE_SUBMIT_ORDER), encode_submit_order);
    registry
        .register::<OrderFilled, _>(payload_type(PAYLOAD_TYPE_ORDER_FILLED), encode_order_filled);
    registry.register::<OrderStatusReport, _>(
        payload_type(PAYLOAD_TYPE_ORDER_STATUS_REPORT),
        encode_order_status_report,
    );
    registry.register::<FillReport, _>(payload_type(PAYLOAD_TYPE_FILL_REPORT), encode_fill_report);
    registry.register::<PositionStatusReport, _>(
        payload_type(PAYLOAD_TYPE_POSITION_STATUS_REPORT),
        encode_position_status_report,
    );
    registry.register::<TradingCommand, _>(
        payload_type(PAYLOAD_TYPE_TRADING_COMMAND),
        encode_trading_command,
    );
    registry.register::<OrderEventAny, _>(
        payload_type(PAYLOAD_TYPE_ORDER_EVENT_ANY),
        encode_order_event_any,
    );
    registry.register::<ExecutionReport, _>(
        payload_type(PAYLOAD_TYPE_EXECUTION_REPORT),
        encode_execution_report,
    );
    registry.register::<PositionEvent, _>(
        payload_type(PAYLOAD_TYPE_POSITION_EVENT),
        encode_position_event,
    );
    registry.register::<AccountState, _>(
        payload_type(PAYLOAD_TYPE_ACCOUNT_STATE),
        encode_account_state,
    );
    registry
        .register::<DataCommand, _>(payload_type(PAYLOAD_TYPE_DATA_COMMAND), encode_data_command);
    registry.register::<DataResponse, _>(
        payload_type(PAYLOAD_TYPE_DATA_RESPONSE),
        encode_data_response,
    );

    register_default_headers(registry);
}

/// Attaches header extractors for every type that carries `correlation_id` or
/// `causation_id` today.
///
/// Header propagation lands incrementally per the SPEC's workstream A: extractors only
/// exist for types whose underlying struct has grown the fields. Other types fall back to
/// the registry's no-op extractor, which yields [`Headers::empty`]; capture still works
/// for them, the entry just carries no correlation metadata until the field set arrives.
fn register_default_headers(registry: &mut EncoderRegistry) {
    registry.register_headers::<SubmitOrder, _>(extract_submit_order_headers);
    registry.register_headers::<SubmitOrderList, _>(extract_submit_order_list_headers);
    registry.register_headers::<ModifyOrder, _>(extract_modify_order_headers);
    registry.register_headers::<CancelOrder, _>(extract_cancel_order_headers);
    registry.register_headers::<CancelAllOrders, _>(extract_cancel_all_orders_headers);
    registry.register_headers::<BatchCancelOrders, _>(extract_batch_cancel_orders_headers);
    registry.register_headers::<QueryOrder, _>(extract_query_order_headers);
    registry.register_headers::<QueryAccount, _>(extract_query_account_headers);
    registry.register_headers::<TradingCommand, _>(extract_trading_command_headers);
    registry.register_headers::<DataCommand, _>(extract_data_command_headers);
    registry.register_headers::<DataResponse, _>(extract_data_response_headers);
}

fn headers_from_fields(correlation_id: Option<UUID4>, causation_id: Option<UUID4>) -> Headers {
    Headers {
        correlation_id,
        causation_id,
    }
}

fn extract_submit_order_headers(cmd: &SubmitOrder) -> Headers {
    headers_from_fields(cmd.correlation_id, cmd.causation_id)
}

fn extract_submit_order_list_headers(cmd: &SubmitOrderList) -> Headers {
    headers_from_fields(cmd.correlation_id, cmd.causation_id)
}

fn extract_modify_order_headers(cmd: &ModifyOrder) -> Headers {
    headers_from_fields(cmd.correlation_id, cmd.causation_id)
}

fn extract_cancel_order_headers(cmd: &CancelOrder) -> Headers {
    headers_from_fields(cmd.correlation_id, cmd.causation_id)
}

fn extract_cancel_all_orders_headers(cmd: &CancelAllOrders) -> Headers {
    headers_from_fields(cmd.correlation_id, cmd.causation_id)
}

fn extract_batch_cancel_orders_headers(cmd: &BatchCancelOrders) -> Headers {
    headers_from_fields(cmd.correlation_id, cmd.causation_id)
}

fn extract_query_order_headers(cmd: &QueryOrder) -> Headers {
    headers_from_fields(cmd.correlation_id, cmd.causation_id)
}

fn extract_query_account_headers(cmd: &QueryAccount) -> Headers {
    headers_from_fields(cmd.correlation_id, cmd.causation_id)
}

// `send_trading_command` reaches the bus tap with the wrapper's `TypeId`, so the
// extractor must mirror the encoder's variant dispatch to surface the inner command's
// correlation metadata on the captured entry.
fn extract_trading_command_headers(command: &TradingCommand) -> Headers {
    match command {
        TradingCommand::SubmitOrder(cmd) => extract_submit_order_headers(cmd),
        TradingCommand::SubmitOrderList(cmd) => extract_submit_order_list_headers(cmd),
        TradingCommand::ModifyOrder(cmd) => extract_modify_order_headers(cmd),
        TradingCommand::CancelOrder(cmd) => extract_cancel_order_headers(cmd),
        TradingCommand::CancelAllOrders(cmd) => extract_cancel_all_orders_headers(cmd),
        TradingCommand::BatchCancelOrders(cmd) => extract_batch_cancel_orders_headers(cmd),
        TradingCommand::QueryOrder(cmd) => extract_query_order_headers(cmd),
        TradingCommand::QueryAccount(cmd) => extract_query_account_headers(cmd),
    }
}

// `send_data_command` reaches the bus tap with the wrapper's `TypeId`. The data engine
// keys RPC request/response pairs by the request's `request_id`: the response's
// `correlation_id` echoes that uuid back. Surfacing `request_id` as the captured entry's
// `correlation_id` therefore lines a request entry up with its eventual response entry
// under the same chain key. Subscribe / Unsubscribe variants carry an explicit
// `correlation_id` field, which we forward as-is. DeFi variants are not yet wired through
// header propagation.
fn extract_data_command_headers(command: &DataCommand) -> Headers {
    match command {
        DataCommand::Request(cmd) => headers_from_fields(Some(*cmd.request_id()), None),
        DataCommand::Subscribe(cmd) => headers_from_fields(cmd.correlation_id(), None),
        DataCommand::Unsubscribe(cmd) => headers_from_fields(cmd.correlation_id(), None),
        // `DataCommand` is `#[non_exhaustive]` and the defi variants do not yet carry
        // header propagation; future variants drop through this arm with empty headers
        // until their correlation field shape lands.
        _ => Headers::empty(),
    }
}

// Every `DataResponse` variant carries a required `correlation_id` that pairs the
// response with its originating request; the captured entry mirrors that value.
fn extract_data_response_headers(response: &DataResponse) -> Headers {
    headers_from_fields(Some(*response.correlation_id()), None)
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

/// Encodes an [`ExecutionReport`] envelope by dispatching on the variant.
///
/// `send_execution_report` hands the bus tap an [`ExecutionReport`] wrapper, so the tap
/// dispatches by the wrapper's [`std::any::TypeId`] and the inner variants never reach
/// their bare-type encoders. The dispatcher unwraps each variant, encodes the inner type
/// with its own index keys, and stamps the inner-variant tag so forensics scans see
/// entries identical to a bare capture path.
///
/// The [`ExecutionReport::Order`] arm reuses [`encode_order_status_report`] because the
/// bare-type encoder already exists; the remaining variants delegate to private inner
/// encoders that index the report's identifiers individually.
///
/// # Errors
///
/// Returns the inner encoder's [`EncodeError`] when MessagePack rejects the inner
/// payload.
pub fn encode_execution_report(report: &ExecutionReport) -> Result<EncodedPayload, EncodeError> {
    match report {
        ExecutionReport::Order(r) => Ok(retag(
            encode_order_status_report(r)?,
            PAYLOAD_TYPE_ORDER_STATUS_REPORT,
        )),
        ExecutionReport::Fill(r) => encode_fill_report(r),
        ExecutionReport::OrderWithFills(order, fills) => encode_order_with_fills(order, fills),
        ExecutionReport::Position(r) => encode_position_status_report(r),
        ExecutionReport::MassStatus(s) => encode_execution_mass_status(s),
    }
}

/// Encodes a [`FillReport`] into canonical bytes plus its `venue_order_id` index and,
/// when present, its `client_order_id` index.
///
/// # Errors
///
/// Returns [`EncodeError::Serialize`] when MessagePack rejects the payload.
pub fn encode_fill_report(report: &FillReport) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(report)?;
    let mut index_keys = Vec::with_capacity(2);
    index_keys.push(IndexKey::new(
        IndexKind::VenueOrderId,
        report.venue_order_id.to_string(),
    ));

    if let Some(client_order_id) = &report.client_order_id {
        index_keys.push(IndexKey::new(
            IndexKind::ClientOrderId,
            client_order_id.to_string(),
        ));
    }

    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_FILL_REPORT),
        payload,
        index_keys,
    ))
}

/// Encodes a [`PositionStatusReport`] into canonical bytes with no sidecar indices.
///
/// `PositionStatusReport` carries only `AccountId`, `InstrumentId`, and `PositionId`;
/// none of those have a matching [`IndexKind`] variant today. Capture with no sidecar
/// indices so the entry is forensics-discoverable by sequential scan rather than
/// synthesising an index against an identifier the reader cannot query.
///
/// # Errors
///
/// Returns [`EncodeError::Serialize`] when MessagePack rejects the payload.
pub fn encode_position_status_report(
    report: &PositionStatusReport,
) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(report)?;
    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_POSITION_STATUS_REPORT),
        payload,
        Vec::new(),
    ))
}

fn encode_order_with_fills(
    order: &OrderStatusReport,
    fills: &[FillReport],
) -> Result<EncodedPayload, EncodeError> {
    #[derive(Serialize)]
    struct OrderWithFillsRef<'a> {
        order_report: &'a OrderStatusReport,
        fill_reports: &'a [FillReport],
    }

    let payload = encode_serde(&OrderWithFillsRef {
        order_report: order,
        fill_reports: fills,
    })?;
    let mut index_keys = Vec::new();
    let mut seen = HashSet::new();
    push_unique_index_key(
        &mut index_keys,
        &mut seen,
        IndexKind::VenueOrderId,
        order.venue_order_id.to_string(),
    );

    if let Some(client_order_id) = &order.client_order_id {
        push_unique_index_key(
            &mut index_keys,
            &mut seen,
            IndexKind::ClientOrderId,
            client_order_id.to_string(),
        );
    }

    for fill in fills {
        push_unique_index_key(
            &mut index_keys,
            &mut seen,
            IndexKind::VenueOrderId,
            fill.venue_order_id.to_string(),
        );

        if let Some(client_order_id) = &fill.client_order_id {
            push_unique_index_key(
                &mut index_keys,
                &mut seen,
                IndexKind::ClientOrderId,
                client_order_id.to_string(),
            );
        }
    }

    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_ORDER_WITH_FILLS),
        payload,
        index_keys,
    ))
}

fn encode_execution_mass_status(
    status: &ExecutionMassStatus,
) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(status)?;
    let mut index_keys = Vec::new();
    let mut seen = HashSet::new();

    let order_reports = status.order_reports();

    for (venue_order_id, report) in &order_reports {
        push_unique_index_key(
            &mut index_keys,
            &mut seen,
            IndexKind::VenueOrderId,
            venue_order_id.to_string(),
        );

        if let Some(client_order_id) = &report.client_order_id {
            push_unique_index_key(
                &mut index_keys,
                &mut seen,
                IndexKind::ClientOrderId,
                client_order_id.to_string(),
            );
        }
    }

    let fill_reports = status.fill_reports();

    for (venue_order_id, fills) in &fill_reports {
        push_unique_index_key(
            &mut index_keys,
            &mut seen,
            IndexKind::VenueOrderId,
            venue_order_id.to_string(),
        );

        for fill in fills {
            if let Some(client_order_id) = &fill.client_order_id {
                push_unique_index_key(
                    &mut index_keys,
                    &mut seen,
                    IndexKind::ClientOrderId,
                    client_order_id.to_string(),
                );
            }
        }
    }
    // PositionStatusReport identifiers are not indexable today, see
    // `encode_position_status_report`.
    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_EXECUTION_MASS_STATUS),
        payload,
        index_keys,
    ))
}

fn push_unique_index_key(
    index_keys: &mut Vec<IndexKey>,
    seen: &mut HashSet<(IndexKind, String)>,
    kind: IndexKind,
    key: String,
) {
    if seen.insert((kind, key.clone())) {
        index_keys.push(IndexKey::new(kind, key));
    }
}

/// Encodes a [`PositionEvent`] envelope by dispatching on the variant.
///
/// `publish_position_event` hands the bus tap a [`PositionEvent`] wrapper, so the tap
/// dispatches by the wrapper's [`std::any::TypeId`] and the inner variants never reach
/// their bare-type encoders. The dispatcher unwraps each variant, encodes the inner
/// struct, and stamps the inner-variant tag so forensics scans see entries identical
/// to the bare-type capture path.
///
/// # Errors
///
/// Returns the inner encoder's [`EncodeError`] when MessagePack rejects the inner
/// payload.
pub fn encode_position_event(event: &PositionEvent) -> Result<EncodedPayload, EncodeError> {
    match event {
        PositionEvent::PositionOpened(e) => encode_position_opened(e),
        PositionEvent::PositionChanged(e) => encode_position_changed(e),
        PositionEvent::PositionClosed(e) => encode_position_closed(e),
        PositionEvent::PositionAdjusted(e) => encode_position_adjusted(e),
    }
}

fn encode_position_opened(event: &PositionOpened) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(event)?;
    let index_keys = vec![IndexKey::new(
        IndexKind::ClientOrderId,
        event.opening_order_id.to_string(),
    )];
    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_POSITION_OPENED),
        payload,
        index_keys,
    ))
}

fn encode_position_changed(event: &PositionChanged) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(event)?;
    let index_keys = vec![IndexKey::new(
        IndexKind::ClientOrderId,
        event.opening_order_id.to_string(),
    )];
    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_POSITION_CHANGED),
        payload,
        index_keys,
    ))
}

fn encode_position_closed(event: &PositionClosed) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(event)?;
    let mut index_keys = Vec::new();
    let mut seen = HashSet::new();

    push_unique_index_key(
        &mut index_keys,
        &mut seen,
        IndexKind::ClientOrderId,
        event.opening_order_id.to_string(),
    );

    // Opening and closing client_order_ids are distinct in normal operation; dedup
    // guards the rare case where a single order both opens and closes the position.
    if let Some(closing_order_id) = &event.closing_order_id {
        push_unique_index_key(
            &mut index_keys,
            &mut seen,
            IndexKind::ClientOrderId,
            closing_order_id.to_string(),
        );
    }

    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_POSITION_CLOSED),
        payload,
        index_keys,
    ))
}

fn encode_position_adjusted(event: &PositionAdjusted) -> Result<EncodedPayload, EncodeError> {
    // PositionAdjusted carries no client_order_id; identifiers are PositionId,
    // AccountId, and InstrumentId, none of which have a matching IndexKind today.
    let payload = encode_serde(event)?;
    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_POSITION_ADJUSTED),
        payload,
        Vec::new(),
    ))
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

/// Encodes an [`AccountState`] into canonical bytes with no sidecar indices.
///
/// `AccountState` carries `AccountId` and `event_id` (UUID4); neither matches an
/// [`IndexKind`] variant today, so the encoder emits no sidecar keys and forensics
/// scans rely on sequential range over `seq`. This mirrors the [`PositionStatusReport`]
/// precedent.
///
/// # Errors
///
/// Returns [`EncodeError::Serialize`] when MessagePack rejects the payload.
pub fn encode_account_state(message: &AccountState) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(message)?;
    Ok(EncodedPayload::new(payload, Vec::new()))
}

/// Encodes a [`DataCommand`] envelope by dispatching on its command category.
///
/// `send_data_command` hands the bus tap a [`DataCommand`] wrapper, so the tap
/// dispatches by the wrapper's [`std::any::TypeId`] and the inner command category
/// never reaches a bare-type encoder. The dispatcher unwraps the category, encodes the
/// serializable inner enum (`RequestCommand`, `SubscribeCommand`, or
/// `UnsubscribeCommand`), and stamps that category's canonical `payload_type` tag.
///
/// Request IDs, command IDs, and correlation IDs do not have a matching [`IndexKind`]
/// today, so data commands emit no sidecar indices. Correlation is recovered from the
/// captured payload and, once header propagation lands, propagated headers.
///
/// # Errors
///
/// Returns [`EncodeError::Serialize`] when MessagePack rejects the inner payload, or
/// when a future non-exhaustive [`DataCommand`] variant has no encoder yet.
pub fn encode_data_command(command: &DataCommand) -> Result<EncodedPayload, EncodeError> {
    match command {
        DataCommand::Request(cmd) => {
            encode_data_command_category(cmd, PAYLOAD_TYPE_REQUEST_COMMAND)
        }
        DataCommand::Subscribe(cmd) => {
            encode_data_command_category(cmd, PAYLOAD_TYPE_SUBSCRIBE_COMMAND)
        }
        DataCommand::Unsubscribe(cmd) => {
            encode_data_command_category(cmd, PAYLOAD_TYPE_UNSUBSCRIBE_COMMAND)
        }
        #[cfg(feature = "defi")]
        DataCommand::DefiRequest(cmd) => {
            encode_data_command_category(cmd, PAYLOAD_TYPE_DEFI_REQUEST_COMMAND)
        }
        #[cfg(feature = "defi")]
        DataCommand::DefiSubscribe(cmd) => {
            encode_data_command_category(cmd, PAYLOAD_TYPE_DEFI_SUBSCRIBE_COMMAND)
        }
        #[cfg(feature = "defi")]
        DataCommand::DefiUnsubscribe(cmd) => {
            encode_data_command_category(cmd, PAYLOAD_TYPE_DEFI_UNSUBSCRIBE_COMMAND)
        }
        _ => Err(EncodeError::Serialize(
            "unsupported DataCommand variant".to_string(),
        )),
    }
}

fn encode_data_command_category<T: Serialize>(
    command: &T,
    tag: &str,
) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(command)?;
    Ok(EncodedPayload::with_payload_type(
        payload_type(tag),
        payload,
        Vec::new(),
    ))
}

/// Encodes a [`DataResponse`] envelope by dispatching on the variant.
///
/// `send_data_response` hands the bus tap a [`DataResponse`] wrapper, so the tap
/// dispatches by the wrapper's [`std::any::TypeId`] and the inner variants never reach
/// their bare-type encoders. The dispatcher unwraps each variant, encodes the inner
/// struct, and stamps the inner-variant tag so forensics scans see entries identical
/// to a bare-type capture path.
///
/// Each variant carries a `correlation_id` (UUID4) pairing the response with the
/// originating `RequestCommand::request_id`. [`IndexKind`] has no matching variant
/// today, so every variant emits zero sidecar indices, mirroring the
/// [`PositionStatusReport`] and [`AccountState`] precedents.
///
/// The [`DataResponse::Data`] and [`DataResponse::Book`] variants carry payloads that
/// are not directly serializable: [`CustomDataResponse`] holds an `Arc<dyn Any>` and
/// [`BookResponse`] holds a [`nautilus_model::orderbook::OrderBook`] without serde
/// derives. The dispatcher serializes the audit-relevant metadata for those two
/// variants via local borrowed wrapper structs (the `encode_order_with_fills`
/// precedent) and omits the opaque payload. `BookResponse` is state-affecting on
/// the data engine path (`handle_book_response` clones the book into the cache); a
/// follow-up that adds serde to `OrderBook`/`BookLadder` can replace the metadata
/// wrapper with full payload capture without changing the dispatcher contract.
///
/// # Errors
///
/// Returns the inner encoder's [`EncodeError`] when MessagePack rejects the inner
/// payload.
pub fn encode_data_response(response: &DataResponse) -> Result<EncodedPayload, EncodeError> {
    match response {
        DataResponse::Data(resp) => encode_custom_data_response(resp),
        DataResponse::Instrument(resp) => encode_instrument_response(resp),
        DataResponse::Instruments(resp) => encode_instruments_response(resp),
        DataResponse::Book(resp) => encode_book_response(resp),
        DataResponse::Quotes(resp) => encode_quotes_response(resp),
        DataResponse::Trades(resp) => encode_trades_response(resp),
        DataResponse::FundingRates(resp) => encode_funding_rates_response(resp),
        DataResponse::ForwardPrices(resp) => encode_forward_prices_response(resp),
        DataResponse::Bars(resp) => encode_bars_response(resp),
    }
}

fn encode_custom_data_response(
    response: &CustomDataResponse,
) -> Result<EncodedPayload, EncodeError> {
    // `data: Arc<dyn Any + Send + Sync>` is type-erased at the dispatcher; the
    // payload is captured via a metadata-only wrapper so the audit entry pairs
    // with the originating request without depending on per-registration
    // serializers for the inner Any payload.
    #[derive(Serialize)]
    struct CustomDataResponseRef<'a> {
        correlation_id: &'a UUID4,
        client_id: &'a ClientId,
        venue: &'a Option<Venue>,
        data_type: &'a DataType,
        start: &'a Option<UnixNanos>,
        end: &'a Option<UnixNanos>,
        ts_init: &'a UnixNanos,
        params: &'a Option<Params>,
    }

    let payload = encode_serde(&CustomDataResponseRef {
        correlation_id: &response.correlation_id,
        client_id: &response.client_id,
        venue: &response.venue,
        data_type: &response.data_type,
        start: &response.start,
        end: &response.end,
        ts_init: &response.ts_init,
        params: &response.params,
    })?;
    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_CUSTOM_DATA_RESPONSE),
        payload,
        Vec::new(),
    ))
}

fn encode_instrument_response(
    response: &InstrumentResponse,
) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(response)?;
    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_INSTRUMENT_RESPONSE),
        payload,
        Vec::new(),
    ))
}

fn encode_instruments_response(
    response: &InstrumentsResponse,
) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(response)?;
    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_INSTRUMENTS_RESPONSE),
        payload,
        Vec::new(),
    ))
}

fn encode_book_response(response: &BookResponse) -> Result<EncodedPayload, EncodeError> {
    // `data: OrderBook` is not serde-derived today (BookLadder/BookLevel chain), and
    // the full book state is rarely the audit value at this level. Capture
    // response-level metadata via a borrowed wrapper so the entry pairs with the
    // originating request.
    #[derive(Serialize)]
    struct BookResponseRef<'a> {
        correlation_id: &'a UUID4,
        client_id: &'a ClientId,
        instrument_id: &'a InstrumentId,
        start: &'a Option<UnixNanos>,
        end: &'a Option<UnixNanos>,
        ts_init: &'a UnixNanos,
        params: &'a Option<Params>,
    }

    let payload = encode_serde(&BookResponseRef {
        correlation_id: &response.correlation_id,
        client_id: &response.client_id,
        instrument_id: &response.instrument_id,
        start: &response.start,
        end: &response.end,
        ts_init: &response.ts_init,
        params: &response.params,
    })?;
    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_BOOK_RESPONSE),
        payload,
        Vec::new(),
    ))
}

fn encode_quotes_response(response: &QuotesResponse) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(response)?;
    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_QUOTES_RESPONSE),
        payload,
        Vec::new(),
    ))
}

fn encode_trades_response(response: &TradesResponse) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(response)?;
    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_TRADES_RESPONSE),
        payload,
        Vec::new(),
    ))
}

fn encode_funding_rates_response(
    response: &FundingRatesResponse,
) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(response)?;
    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_FUNDING_RATES_RESPONSE),
        payload,
        Vec::new(),
    ))
}

fn encode_forward_prices_response(
    response: &ForwardPricesResponse,
) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(response)?;
    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_FORWARD_PRICES_RESPONSE),
        payload,
        Vec::new(),
    ))
}

fn encode_bars_response(response: &BarsResponse) -> Result<EncodedPayload, EncodeError> {
    let payload = encode_serde(response)?;
    Ok(EncodedPayload::with_payload_type(
        payload_type(PAYLOAD_TYPE_BARS_RESPONSE),
        payload,
        Vec::new(),
    ))
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::data::{
        RequestCommand, RequestQuotes, SubscribeCommand, SubscribeQuotes, UnsubscribeCommand,
        UnsubscribeQuotes,
    };
    #[cfg(feature = "defi")]
    use nautilus_common::messages::defi::{
        DefiRequestCommand, DefiSubscribeCommand, DefiUnsubscribeCommand, RequestPoolSnapshot,
        SubscribeBlocks, UnsubscribeBlocks,
    };
    use nautilus_core::{UUID4, UnixNanos};
    #[cfg(feature = "defi")]
    use nautilus_model::defi::Blockchain;
    use nautilus_model::{
        data::{Bar, BarType},
        enums::{
            AccountType, BookType, LiquiditySide, OrderSide, OrderStatus, OrderType,
            PositionAdjustmentType, PositionSide, PositionSideSpecified, TimeInForce,
        },
        events::{PositionAdjusted, PositionChanged, PositionClosed, PositionOpened},
        identifiers::{
            AccountId, ClientId, ClientOrderId, InstrumentId, OrderListId, PositionId, StrategyId,
            TradeId, TraderId, Venue, VenueOrderId,
        },
        instruments::{InstrumentAny, stubs::currency_pair_ethusdt},
        orderbook::OrderBook,
        orders::OrderList,
        reports::{ExecutionMassStatus, FillReport, PositionStatusReport},
        types::{AccountBalance, Currency, Money, Price, Quantity},
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
            None, // correlation_id
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

        assert_eq!(registry.len(), 12);
        assert!(registry.contains::<SubmitOrder>());
        assert!(registry.contains::<OrderFilled>());
        assert!(registry.contains::<OrderStatusReport>());
        assert!(registry.contains::<FillReport>());
        assert!(registry.contains::<PositionStatusReport>());
        assert!(registry.contains::<TradingCommand>());
        assert!(registry.contains::<OrderEventAny>());
        assert!(registry.contains::<ExecutionReport>());
        assert!(registry.contains::<PositionEvent>());
        assert!(registry.contains::<AccountState>());
        assert!(registry.contains::<DataCommand>());
        assert!(registry.contains::<DataResponse>());
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
            None, // correlation_id
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
            None, // correlation_id
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
            None, // correlation_id
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
            None, // correlation_id
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
            None, // correlation_id
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
            None, // correlation_id
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
            None, // correlation_id
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

    fn make_fill_report() -> FillReport {
        FillReport::new(
            AccountId::from("BINANCE-001"),
            instrument_id(),
            venue_order_id(),
            TradeId::from("T-1111"),
            OrderSide::Buy,
            Quantity::from("1"),
            Price::from("100.00"),
            Money::new(0.10, Currency::USDT()),
            LiquiditySide::Taker,
            Some(client_order_id()),
            None,
            UnixNanos::from(40),
            UnixNanos::from(41),
            None,
        )
    }

    fn make_position_status_report() -> PositionStatusReport {
        PositionStatusReport::new(
            AccountId::from("BINANCE-001"),
            instrument_id(),
            PositionSideSpecified::Long,
            Quantity::from("1"),
            UnixNanos::from(50),
            UnixNanos::from(51),
            None,
            Some(PositionId::from("P-001")),
            None,
        )
    }

    fn make_execution_mass_status_with_reports() -> ExecutionMassStatus {
        let mut status = ExecutionMassStatus::new(
            ClientId::from("BINANCE"),
            AccountId::from("BINANCE-001"),
            Venue::from("BINANCE"),
            UnixNanos::from(60),
            None,
        );
        status.add_order_reports(vec![make_order_status_report()]);
        status.add_fill_reports(vec![make_fill_report()]);
        status.add_position_reports(vec![make_position_status_report()]);
        status
    }

    #[rstest]
    fn execution_report_order_envelope_reuses_bare_status_encoder() {
        // ExecutionReport::Order maps onto the existing OrderStatusReport bare-type
        // encoder; the dispatcher must produce identical bytes and indices and stamp
        // the OrderStatusReport tag so forensics scans pair the entry with the same
        // decoder as the bare-type capture path.
        let report = make_order_status_report();
        let bare = encode_order_status_report(&report).expect("bare");

        let envelope = ExecutionReport::Order(Box::new(report));
        let wrapped = encode_execution_report(&envelope).expect("envelope");

        assert_eq!(wrapped.payload, bare.payload);
        assert_eq!(wrapped.index_keys, bare.index_keys);
        assert_eq!(
            wrapped.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_ORDER_STATUS_REPORT,
        );
    }

    #[rstest]
    fn execution_report_fill_envelope_emits_venue_and_client_order_id_indices() {
        let fill = make_fill_report();
        let envelope = ExecutionReport::Fill(Box::new(fill.clone()));
        let encoded = encode_execution_report(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_FILL_REPORT,
        );
        assert_eq!(encoded.index_keys.len(), 2);
        assert_eq!(encoded.index_keys[0].kind, IndexKind::VenueOrderId);
        assert_eq!(encoded.index_keys[0].key, fill.venue_order_id.to_string());
        assert_eq!(encoded.index_keys[1].kind, IndexKind::ClientOrderId);
        assert_eq!(
            encoded.index_keys[1].key,
            fill.client_order_id.expect("set").to_string(),
        );

        let decoded: FillReport = rmp_serde::from_slice(&encoded.payload).expect("decode");
        assert_eq!(decoded, fill);
    }

    #[rstest]
    fn execution_report_fill_envelope_omits_client_order_id_when_absent() {
        let mut fill = make_fill_report();
        fill.client_order_id = None;
        let envelope = ExecutionReport::Fill(Box::new(fill));
        let encoded = encode_execution_report(&envelope).expect("encode");

        assert_eq!(encoded.index_keys.len(), 1);
        assert_eq!(encoded.index_keys[0].kind, IndexKind::VenueOrderId);
    }

    #[rstest]
    fn execution_report_position_envelope_records_no_indices() {
        // PositionStatusReport identifiers (AccountId, InstrumentId, PositionId) have
        // no matching IndexKind today; the dispatcher must not invent sidecar indices
        // pointing at an identifier the reader cannot query.
        let position = make_position_status_report();
        let envelope = ExecutionReport::Position(Box::new(position.clone()));
        let encoded = encode_execution_report(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_POSITION_STATUS_REPORT,
        );
        assert!(encoded.index_keys.is_empty());

        let decoded: PositionStatusReport =
            rmp_serde::from_slice(&encoded.payload).expect("decode");
        assert_eq!(decoded, position);
    }

    #[rstest]
    fn execution_report_order_with_fills_envelope_dedupes_shared_order_ids() {
        // The order and its fills semantically carry the same venue_order_id and
        // client_order_id, so the dispatcher must dedupe rather than emit a duplicate
        // (kind, key) pair the backend would silently drop.
        let order = make_order_status_report();
        let fills = vec![make_fill_report()];
        let envelope = ExecutionReport::OrderWithFills(Box::new(order.clone()), fills);
        let encoded = encode_execution_report(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_ORDER_WITH_FILLS,
        );
        assert_eq!(encoded.index_keys.len(), 2);
        assert_eq!(encoded.index_keys[0].kind, IndexKind::VenueOrderId);
        assert_eq!(encoded.index_keys[0].key, order.venue_order_id.to_string());
        assert_eq!(encoded.index_keys[1].kind, IndexKind::ClientOrderId);
        assert_eq!(
            encoded.index_keys[1].key,
            order.client_order_id.expect("set").to_string(),
        );
    }

    #[rstest]
    fn execution_report_order_with_fills_envelope_indexes_distinct_fill_ids() {
        // A bundled OrderWithFills can carry a fill whose client_order_id differs
        // from the order's (rare, but real: external orders observed via a fill
        // before the venue confirms the canonical id). The dispatcher must index
        // both client_order_ids so forensics can resolve either to the same seq.
        let order = make_order_status_report();
        let mut fill = make_fill_report();
        fill.client_order_id = Some(ClientOrderId::from("O-EXTRA-001"));
        let envelope = ExecutionReport::OrderWithFills(Box::new(order), vec![fill.clone()]);
        let encoded = encode_execution_report(&envelope).expect("encode");

        assert_eq!(encoded.index_keys.len(), 3);
        assert_eq!(encoded.index_keys[2].kind, IndexKind::ClientOrderId);
        assert_eq!(
            encoded.index_keys[2].key,
            fill.client_order_id.expect("set").to_string(),
        );
    }

    #[rstest]
    fn execution_report_order_with_fills_payload_round_trips() {
        #[derive(serde::Deserialize)]
        struct OrderWithFillsOwned {
            order_report: OrderStatusReport,
            fill_reports: Vec<FillReport>,
        }

        let order = make_order_status_report();
        let fills = vec![make_fill_report()];
        let envelope = ExecutionReport::OrderWithFills(Box::new(order.clone()), fills.clone());
        let encoded = encode_execution_report(&envelope).expect("encode");

        let decoded: OrderWithFillsOwned = rmp_serde::from_slice(&encoded.payload).expect("decode");
        assert_eq!(decoded.order_report, order);
        assert_eq!(decoded.fill_reports, fills);
    }

    #[rstest]
    fn execution_report_mass_status_envelope_indexes_orders_and_fills() {
        let status = make_execution_mass_status_with_reports();
        let envelope = ExecutionReport::MassStatus(Box::new(status.clone()));
        let encoded = encode_execution_report(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_EXECUTION_MASS_STATUS,
        );
        // One order with venue+client ids, one fill sharing both ids, one position
        // (unindexable). The dispatcher must dedupe the shared ids.
        assert_eq!(encoded.index_keys.len(), 2);
        assert_eq!(encoded.index_keys[0].kind, IndexKind::VenueOrderId);
        assert_eq!(encoded.index_keys[0].key, venue_order_id().to_string());
        assert_eq!(encoded.index_keys[1].kind, IndexKind::ClientOrderId);
        assert_eq!(encoded.index_keys[1].key, client_order_id().to_string());

        let decoded: ExecutionMassStatus = rmp_serde::from_slice(&encoded.payload).expect("decode");
        assert_eq!(decoded, status);
    }

    #[rstest]
    fn execution_report_mass_status_envelope_indexes_distinct_children() {
        // Two distinct orders + a fill for a third venue_order_id with its own
        // client_order_id. The dispatcher must record an index for each unique id
        // so forensics can resolve any child of the status report.
        let mut status = ExecutionMassStatus::new(
            ClientId::from("BINANCE"),
            AccountId::from("BINANCE-001"),
            Venue::from("BINANCE"),
            UnixNanos::from(60),
            None,
        );
        let order_a = make_order_status_report();
        let order_b = OrderStatusReport {
            client_order_id: Some(ClientOrderId::from("O-B")),
            venue_order_id: VenueOrderId::from("V-B"),
            ..make_order_status_report()
        };
        let fill_c = FillReport {
            venue_order_id: VenueOrderId::from("V-C"),
            client_order_id: Some(ClientOrderId::from("O-C")),
            ..make_fill_report()
        };
        status.add_order_reports(vec![order_a, order_b]);
        status.add_fill_reports(vec![fill_c]);

        let encoded = encode_execution_report(&ExecutionReport::MassStatus(Box::new(status)))
            .expect("encode");

        // Three venue_order_ids + three client_order_ids = 6 distinct keys
        assert_eq!(encoded.index_keys.len(), 6);
        let venue_keys: Vec<&str> = encoded
            .index_keys
            .iter()
            .filter(|k| k.kind == IndexKind::VenueOrderId)
            .map(|k| k.key.as_str())
            .collect();
        let client_keys: Vec<&str> = encoded
            .index_keys
            .iter()
            .filter(|k| k.kind == IndexKind::ClientOrderId)
            .map(|k| k.key.as_str())
            .collect();
        assert!(venue_keys.contains(&venue_order_id().to_string().as_str()));
        assert!(venue_keys.contains(&"V-B"));
        assert!(venue_keys.contains(&"V-C"));
        assert!(client_keys.contains(&client_order_id().to_string().as_str()));
        assert!(client_keys.contains(&"O-B"));
        assert!(client_keys.contains(&"O-C"));
    }

    // Walks every ExecutionReport variant and asserts the dispatcher stamps the
    // inner-variant tag. Catches a swapped match arm or a forgotten override that
    // would otherwise fall back to the wrapper sentinel tag.
    #[rstest]
    #[case::order(
        ExecutionReport::Order(Box::new(make_order_status_report())),
        PAYLOAD_TYPE_ORDER_STATUS_REPORT
    )]
    #[case::fill(
        ExecutionReport::Fill(Box::new(make_fill_report())),
        PAYLOAD_TYPE_FILL_REPORT
    )]
    #[case::order_with_fills(
        ExecutionReport::OrderWithFills(
            Box::new(make_order_status_report()),
            vec![make_fill_report()],
        ),
        PAYLOAD_TYPE_ORDER_WITH_FILLS,
    )]
    #[case::position(
        ExecutionReport::Position(Box::new(make_position_status_report())),
        PAYLOAD_TYPE_POSITION_STATUS_REPORT
    )]
    #[case::mass_status(
        ExecutionReport::MassStatus(Box::new(make_execution_mass_status_with_reports())),
        PAYLOAD_TYPE_EXECUTION_MASS_STATUS
    )]
    fn execution_report_envelope_stamps_inner_tag_for_every_variant(
        #[case] report: ExecutionReport,
        #[case] expected_tag: &str,
    ) {
        let encoded = encode_execution_report(&report).expect("encode");
        let tag = encoded.payload_type.expect("override").as_str().to_string();

        assert_eq!(tag, expected_tag);
        assert_ne!(
            tag, PAYLOAD_TYPE_EXECUTION_REPORT,
            "wrapper fallback tag must never reach the writer",
        );
    }

    fn opening_order_id() -> ClientOrderId {
        ClientOrderId::from("O-OPEN-001")
    }

    fn closing_order_id() -> ClientOrderId {
        ClientOrderId::from("O-CLOSE-001")
    }

    fn position_id() -> PositionId {
        PositionId::from("P-001")
    }

    fn make_position_opened() -> PositionOpened {
        PositionOpened {
            trader_id: trader_id(),
            strategy_id: strategy_id(),
            instrument_id: instrument_id(),
            position_id: position_id(),
            account_id: AccountId::from("BINANCE-001"),
            opening_order_id: opening_order_id(),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 1.0,
            quantity: Quantity::from("1"),
            last_qty: Quantity::from("1"),
            last_px: Price::from("100.00"),
            currency: Currency::USDT(),
            avg_px_open: 100.0,
            event_id: UUID4::new(),
            ts_event: UnixNanos::from(70),
            ts_init: UnixNanos::from(71),
        }
    }

    fn make_position_changed() -> PositionChanged {
        PositionChanged {
            trader_id: trader_id(),
            strategy_id: strategy_id(),
            instrument_id: instrument_id(),
            position_id: position_id(),
            account_id: AccountId::from("BINANCE-001"),
            opening_order_id: opening_order_id(),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 2.0,
            quantity: Quantity::from("2"),
            peak_quantity: Quantity::from("2"),
            last_qty: Quantity::from("1"),
            last_px: Price::from("101.00"),
            currency: Currency::USDT(),
            avg_px_open: 100.5,
            avg_px_close: None,
            realized_return: 0.0,
            realized_pnl: None,
            unrealized_pnl: Money::new(1.0, Currency::USDT()),
            event_id: UUID4::new(),
            ts_opened: UnixNanos::from(70),
            ts_event: UnixNanos::from(80),
            ts_init: UnixNanos::from(81),
        }
    }

    fn make_position_closed() -> PositionClosed {
        PositionClosed {
            trader_id: trader_id(),
            strategy_id: strategy_id(),
            instrument_id: instrument_id(),
            position_id: position_id(),
            account_id: AccountId::from("BINANCE-001"),
            opening_order_id: opening_order_id(),
            closing_order_id: Some(closing_order_id()),
            entry: OrderSide::Buy,
            side: PositionSide::Flat,
            signed_qty: 0.0,
            quantity: Quantity::from("0"),
            peak_quantity: Quantity::from("2"),
            last_qty: Quantity::from("2"),
            last_px: Price::from("102.00"),
            currency: Currency::USDT(),
            avg_px_open: 100.5,
            avg_px_close: Some(102.0),
            realized_return: 0.015,
            realized_pnl: Some(Money::new(3.0, Currency::USDT())),
            unrealized_pnl: Money::new(0.0, Currency::USDT()),
            duration: 3_600_000_000_000,
            event_id: UUID4::new(),
            ts_opened: UnixNanos::from(70),
            ts_closed: Some(UnixNanos::from(90)),
            ts_event: UnixNanos::from(90),
            ts_init: UnixNanos::from(91),
        }
    }

    fn make_position_adjusted() -> PositionAdjusted {
        PositionAdjusted::new(
            trader_id(),
            strategy_id(),
            instrument_id(),
            position_id(),
            AccountId::from("BINANCE-001"),
            PositionAdjustmentType::Commission,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::from(100),
            UnixNanos::from(101),
        )
    }

    #[rstest]
    fn position_event_opened_envelope_emits_opening_order_id_index() {
        let opened = make_position_opened();
        let envelope = PositionEvent::PositionOpened(opened.clone());
        let encoded = encode_position_event(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_POSITION_OPENED,
        );
        assert_eq!(encoded.index_keys.len(), 1);
        assert_eq!(encoded.index_keys[0].kind, IndexKind::ClientOrderId);
        assert_eq!(
            encoded.index_keys[0].key,
            opened.opening_order_id.to_string(),
        );

        let decoded: PositionOpened = rmp_serde::from_slice(&encoded.payload).expect("decode");
        assert_eq!(decoded, opened);
    }

    #[rstest]
    fn position_event_changed_envelope_emits_opening_order_id_index() {
        let changed = make_position_changed();
        let envelope = PositionEvent::PositionChanged(changed.clone());
        let encoded = encode_position_event(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_POSITION_CHANGED,
        );
        assert_eq!(encoded.index_keys.len(), 1);
        assert_eq!(encoded.index_keys[0].kind, IndexKind::ClientOrderId);
        assert_eq!(
            encoded.index_keys[0].key,
            changed.opening_order_id.to_string(),
        );

        let decoded: PositionChanged = rmp_serde::from_slice(&encoded.payload).expect("decode");
        assert_eq!(decoded, changed);
    }

    #[rstest]
    fn position_event_closed_envelope_indexes_both_opening_and_closing_order_ids() {
        let closed = make_position_closed();
        let envelope = PositionEvent::PositionClosed(closed.clone());
        let encoded = encode_position_event(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_POSITION_CLOSED,
        );
        assert_eq!(encoded.index_keys.len(), 2);
        assert_eq!(encoded.index_keys[0].kind, IndexKind::ClientOrderId);
        assert_eq!(
            encoded.index_keys[0].key,
            closed.opening_order_id.to_string(),
        );
        assert_eq!(encoded.index_keys[1].kind, IndexKind::ClientOrderId);
        assert_eq!(
            encoded.index_keys[1].key,
            closed.closing_order_id.expect("set").to_string(),
        );

        let decoded: PositionClosed = rmp_serde::from_slice(&encoded.payload).expect("decode");
        assert_eq!(decoded, closed);
    }

    #[rstest]
    fn position_event_closed_envelope_omits_closing_order_id_when_absent() {
        let mut closed = make_position_closed();
        closed.closing_order_id = None;
        let envelope = PositionEvent::PositionClosed(closed);
        let encoded = encode_position_event(&envelope).expect("encode");

        assert_eq!(encoded.index_keys.len(), 1);
        assert_eq!(encoded.index_keys[0].kind, IndexKind::ClientOrderId);
        assert_eq!(encoded.index_keys[0].key, opening_order_id().to_string());
    }

    #[rstest]
    fn position_event_closed_envelope_dedupes_when_open_and_close_match() {
        // Rare but real: a single order both opens and closes the position (e.g.,
        // reduce-only fills against a stale position). The dispatcher must dedupe
        // rather than insert the same (kind, key) twice.
        let mut closed = make_position_closed();
        closed.closing_order_id = Some(closed.opening_order_id);
        let envelope = PositionEvent::PositionClosed(closed);
        let encoded = encode_position_event(&envelope).expect("encode");

        assert_eq!(encoded.index_keys.len(), 1);
        assert_eq!(encoded.index_keys[0].key, opening_order_id().to_string());
    }

    #[rstest]
    fn position_event_adjusted_envelope_records_no_indices() {
        // PositionAdjusted has no ClientOrderId field; PositionId/AccountId/
        // InstrumentId have no matching IndexKind today, so the dispatcher must
        // not invent sidecar indices pointing at an identifier the reader cannot
        // query.
        let adjusted = make_position_adjusted();
        let envelope = PositionEvent::PositionAdjusted(adjusted);
        let encoded = encode_position_event(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_POSITION_ADJUSTED,
        );
        assert!(encoded.index_keys.is_empty());

        let decoded: PositionAdjusted = rmp_serde::from_slice(&encoded.payload).expect("decode");
        assert_eq!(decoded, adjusted);
    }

    #[rstest]
    #[case::opened(
        PositionEvent::PositionOpened(make_position_opened()),
        PAYLOAD_TYPE_POSITION_OPENED
    )]
    #[case::changed(
        PositionEvent::PositionChanged(make_position_changed()),
        PAYLOAD_TYPE_POSITION_CHANGED
    )]
    #[case::closed(
        PositionEvent::PositionClosed(make_position_closed()),
        PAYLOAD_TYPE_POSITION_CLOSED
    )]
    #[case::adjusted(
        PositionEvent::PositionAdjusted(make_position_adjusted()),
        PAYLOAD_TYPE_POSITION_ADJUSTED
    )]
    fn position_event_envelope_stamps_inner_tag_for_every_variant(
        #[case] event: PositionEvent,
        #[case] expected_tag: &str,
    ) {
        let encoded = encode_position_event(&event).expect("encode");
        let tag = encoded.payload_type.expect("override").as_str().to_string();

        assert_eq!(tag, expected_tag);
        assert_ne!(
            tag, PAYLOAD_TYPE_POSITION_EVENT,
            "wrapper fallback tag must never reach the writer",
        );
    }

    fn make_account_state() -> AccountState {
        AccountState::new(
            AccountId::from("BINANCE-001"),
            AccountType::Cash,
            vec![AccountBalance::new(
                Money::from("1000000 USD"),
                Money::from("0 USD"),
                Money::from("1000000 USD"),
            )],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::from(110),
            UnixNanos::from(111),
            Some(Currency::USD()),
        )
    }

    #[rstest]
    fn account_state_encoder_records_no_indices() {
        // AccountState carries AccountId and event_id (UUID4); neither matches an
        // IndexKind variant today. The encoder must capture the payload without
        // synthesising sidecar indices pointing at identifiers the reader cannot
        // query, mirroring the PositionStatusReport precedent.
        let state = make_account_state();
        let encoded = encode_account_state(&state).expect("encode");

        assert!(!encoded.payload.is_empty());
        assert!(encoded.index_keys.is_empty());
        assert!(
            encoded.payload_type.is_none(),
            "bare-type encoders inherit the registry's registered tag",
        );
    }

    #[rstest]
    fn account_state_payload_round_trips_through_msgpack() {
        let state = make_account_state();
        let encoded = encode_account_state(&state).expect("encode");

        let decoded: AccountState = rmp_serde::from_slice(&encoded.payload).expect("decode");
        assert_eq!(decoded, state);
    }

    #[rstest]
    fn account_state_registered_under_canonical_payload_type() {
        // The default registry must dispatch AccountState through encode_account_state
        // and stamp PAYLOAD_TYPE_ACCOUNT_STATE so both `publish_account_state` and
        // `send_account_state` capture under the same canonical tag.
        let registry = default_registry();
        let state = make_account_state();
        let (tag, encoded) = registry
            .encode(&state)
            .expect("encode")
            .expect("registered");

        assert_eq!(tag.as_str(), PAYLOAD_TYPE_ACCOUNT_STATE);
        assert!(encoded.index_keys.is_empty());
    }

    #[rstest]
    fn data_command_request_envelope_stamps_request_command_payload_type() {
        // DataCommand reaches the bus tap as the wrapper TypeId. The dispatcher must
        // unwrap one level, encode RequestCommand, and stamp the category tag so the
        // reader pairs the bytes with the RequestCommand decoder.
        let request = make_request_command();
        let envelope = DataCommand::Request(request.clone());
        let encoded = encode_data_command(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_REQUEST_COMMAND,
        );
        assert!(encoded.index_keys.is_empty());

        let decoded: RequestCommand = rmp_serde::from_slice(&encoded.payload).expect("decode");
        match (decoded, request) {
            (RequestCommand::Quotes(decoded), RequestCommand::Quotes(expected)) => {
                assert_eq!(decoded.request_id, expected.request_id);
                assert_eq!(decoded.instrument_id, expected.instrument_id);
            }
            other => panic!("expected RequestCommand::Quotes round trip, was {other:?}"),
        }
    }

    #[rstest]
    fn data_command_subscribe_envelope_stamps_subscribe_command_payload_type() {
        let subscribe = make_subscribe_command();
        let envelope = DataCommand::Subscribe(subscribe.clone());
        let encoded = encode_data_command(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_SUBSCRIBE_COMMAND,
        );
        assert!(encoded.index_keys.is_empty());

        let decoded: SubscribeCommand = rmp_serde::from_slice(&encoded.payload).expect("decode");
        match (decoded, subscribe) {
            (SubscribeCommand::Quotes(decoded), SubscribeCommand::Quotes(expected)) => {
                assert_eq!(decoded.command_id, expected.command_id);
                assert_eq!(decoded.instrument_id, expected.instrument_id);
            }
            other => panic!("expected SubscribeCommand::Quotes round trip, was {other:?}"),
        }
    }

    #[rstest]
    fn data_command_unsubscribe_envelope_stamps_unsubscribe_command_payload_type() {
        let unsubscribe = make_unsubscribe_command();
        let envelope = DataCommand::Unsubscribe(unsubscribe.clone());
        let encoded = encode_data_command(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_UNSUBSCRIBE_COMMAND,
        );
        assert!(encoded.index_keys.is_empty());

        let decoded: UnsubscribeCommand = rmp_serde::from_slice(&encoded.payload).expect("decode");
        match (decoded, unsubscribe) {
            (UnsubscribeCommand::Quotes(decoded), UnsubscribeCommand::Quotes(expected)) => {
                assert_eq!(decoded.command_id, expected.command_id);
                assert_eq!(decoded.instrument_id, expected.instrument_id);
            }
            other => panic!("expected UnsubscribeCommand::Quotes round trip, was {other:?}"),
        }
    }

    #[rstest]
    fn data_command_registered_under_category_payload_type() {
        // The default registry must dispatch DataCommand through encode_data_command and
        // stamp the inner category tag, not the wrapper sentinel tag.
        let registry = default_registry();
        let envelope = DataCommand::Subscribe(make_subscribe_command());
        let (tag, encoded) = registry
            .encode(&envelope)
            .expect("encode")
            .expect("registered");

        assert_eq!(tag.as_str(), PAYLOAD_TYPE_SUBSCRIBE_COMMAND);
        assert!(encoded.index_keys.is_empty());
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn data_command_defi_request_envelope_stamps_defi_request_command_payload_type() {
        let request = make_defi_request_command();
        let envelope = DataCommand::DefiRequest(request.clone());
        let encoded = encode_data_command(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_DEFI_REQUEST_COMMAND,
        );
        assert!(encoded.index_keys.is_empty());

        let decoded: DefiRequestCommand = rmp_serde::from_slice(&encoded.payload).expect("decode");
        match (decoded, request) {
            (
                DefiRequestCommand::PoolSnapshot(decoded),
                DefiRequestCommand::PoolSnapshot(expected),
            ) => {
                assert_eq!(decoded.request_id, expected.request_id);
                assert_eq!(decoded.instrument_id, expected.instrument_id);
                assert_eq!(decoded.client_id, expected.client_id);
                assert_eq!(decoded.ts_init, expected.ts_init);
            }
        }
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn data_command_defi_subscribe_envelope_stamps_defi_subscribe_command_payload_type() {
        let subscribe = make_defi_subscribe_command();
        let envelope = DataCommand::DefiSubscribe(subscribe.clone());
        let encoded = encode_data_command(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_DEFI_SUBSCRIBE_COMMAND,
        );
        assert!(encoded.index_keys.is_empty());

        let decoded: DefiSubscribeCommand =
            rmp_serde::from_slice(&encoded.payload).expect("decode");

        match (decoded, subscribe) {
            (DefiSubscribeCommand::Blocks(decoded), DefiSubscribeCommand::Blocks(expected)) => {
                assert_eq!(decoded.command_id, expected.command_id);
                assert_eq!(decoded.chain, expected.chain);
                assert_eq!(decoded.client_id, expected.client_id);
                assert_eq!(decoded.ts_init, expected.ts_init);
            }
            other => panic!("expected DefiSubscribeCommand::Blocks round trip, was {other:?}"),
        }
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn data_command_defi_unsubscribe_envelope_stamps_defi_unsubscribe_command_payload_type() {
        let unsubscribe = make_defi_unsubscribe_command();
        let envelope = DataCommand::DefiUnsubscribe(unsubscribe.clone());
        let encoded = encode_data_command(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_DEFI_UNSUBSCRIBE_COMMAND,
        );
        assert!(encoded.index_keys.is_empty());

        let decoded: DefiUnsubscribeCommand =
            rmp_serde::from_slice(&encoded.payload).expect("decode");

        match (decoded, unsubscribe) {
            (DefiUnsubscribeCommand::Blocks(decoded), DefiUnsubscribeCommand::Blocks(expected)) => {
                assert_eq!(decoded.command_id, expected.command_id);
                assert_eq!(decoded.chain, expected.chain);
                assert_eq!(decoded.client_id, expected.client_id);
                assert_eq!(decoded.ts_init, expected.ts_init);
            }
            other => panic!("expected DefiUnsubscribeCommand::Blocks round trip, was {other:?}"),
        }
    }

    fn client_id() -> ClientId {
        ClientId::from("BINANCE")
    }

    fn venue() -> Venue {
        Venue::from("BINANCE")
    }

    fn correlation_id() -> UUID4 {
        UUID4::new()
    }

    fn bar_type() -> BarType {
        BarType::from("ETHUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL")
    }

    fn data_type() -> DataType {
        DataType::new("Bar", None, None)
    }

    fn make_request_command() -> RequestCommand {
        RequestCommand::Quotes(RequestQuotes::new(
            instrument_id(),
            None,
            None,
            None,
            Some(client_id()),
            correlation_id(),
            UnixNanos::from(197),
            None,
        ))
    }

    fn make_subscribe_command() -> SubscribeCommand {
        SubscribeCommand::Quotes(SubscribeQuotes::new(
            instrument_id(),
            Some(client_id()),
            Some(venue()),
            correlation_id(),
            UnixNanos::from(198),
            Some(correlation_id()),
            None,
        ))
    }

    fn make_unsubscribe_command() -> UnsubscribeCommand {
        UnsubscribeCommand::Quotes(UnsubscribeQuotes::new(
            instrument_id(),
            Some(client_id()),
            Some(venue()),
            correlation_id(),
            UnixNanos::from(199),
            Some(correlation_id()),
            None,
        ))
    }

    #[cfg(feature = "defi")]
    fn make_defi_request_command() -> DefiRequestCommand {
        DefiRequestCommand::PoolSnapshot(RequestPoolSnapshot::new(
            instrument_id(),
            Some(client_id()),
            correlation_id(),
            UnixNanos::from(196),
            None,
        ))
    }

    #[cfg(feature = "defi")]
    fn make_defi_subscribe_command() -> DefiSubscribeCommand {
        DefiSubscribeCommand::Blocks(SubscribeBlocks::new(
            Blockchain::Ethereum,
            Some(client_id()),
            correlation_id(),
            UnixNanos::from(195),
            None,
        ))
    }

    #[cfg(feature = "defi")]
    fn make_defi_unsubscribe_command() -> DefiUnsubscribeCommand {
        DefiUnsubscribeCommand::Blocks(UnsubscribeBlocks::new(
            Blockchain::Ethereum,
            Some(client_id()),
            correlation_id(),
            UnixNanos::from(194),
            None,
        ))
    }

    fn make_custom_data_response() -> CustomDataResponse {
        CustomDataResponse::new(
            correlation_id(),
            client_id(),
            Some(venue()),
            data_type(),
            (),
            None,
            None,
            UnixNanos::from(200),
            None,
        )
    }

    fn make_instrument_response() -> InstrumentResponse {
        InstrumentResponse::new(
            correlation_id(),
            client_id(),
            instrument_id(),
            InstrumentAny::CurrencyPair(currency_pair_ethusdt()),
            None,
            None,
            UnixNanos::from(201),
            None,
        )
    }

    fn make_instruments_response() -> InstrumentsResponse {
        InstrumentsResponse::new(
            correlation_id(),
            client_id(),
            venue(),
            vec![InstrumentAny::CurrencyPair(currency_pair_ethusdt())],
            None,
            None,
            UnixNanos::from(202),
            None,
        )
    }

    fn make_book_response() -> BookResponse {
        BookResponse::new(
            correlation_id(),
            client_id(),
            instrument_id(),
            OrderBook::new(instrument_id(), BookType::L2_MBP),
            None,
            None,
            UnixNanos::from(203),
            None,
        )
    }

    fn make_quotes_response() -> QuotesResponse {
        QuotesResponse::new(
            correlation_id(),
            client_id(),
            instrument_id(),
            Vec::new(),
            None,
            None,
            UnixNanos::from(204),
            None,
        )
    }

    fn make_trades_response() -> TradesResponse {
        TradesResponse::new(
            correlation_id(),
            client_id(),
            instrument_id(),
            Vec::new(),
            None,
            None,
            UnixNanos::from(205),
            None,
        )
    }

    fn make_funding_rates_response() -> FundingRatesResponse {
        FundingRatesResponse::new(
            correlation_id(),
            client_id(),
            instrument_id(),
            Vec::new(),
            None,
            None,
            UnixNanos::from(206),
            None,
        )
    }

    fn make_forward_prices_response() -> ForwardPricesResponse {
        ForwardPricesResponse::new(
            correlation_id(),
            client_id(),
            venue(),
            Vec::new(),
            UnixNanos::from(207),
            None,
        )
    }

    fn make_bars_response() -> BarsResponse {
        BarsResponse::new(
            correlation_id(),
            client_id(),
            bar_type(),
            Vec::<Bar>::new(),
            None,
            None,
            UnixNanos::from(208),
            None,
        )
    }

    #[rstest]
    fn data_response_custom_data_envelope_stamps_inner_tag_with_no_indices() {
        // CustomDataResponse holds `data: Arc<dyn Any>`; the dispatcher captures
        // metadata via a borrowed wrapper. Correlation_id has no matching IndexKind
        // today, so the encoder emits no sidecar keys.
        let response = make_custom_data_response();
        let envelope = DataResponse::Data(response);
        let encoded = encode_data_response(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_CUSTOM_DATA_RESPONSE,
        );
        assert!(encoded.index_keys.is_empty());
        assert!(!encoded.payload.is_empty());
    }

    #[rstest]
    fn data_response_custom_data_payload_round_trips_metadata() {
        // The CustomDataResponseRef wrapper omits `data` (Arc<dyn Any>); the audit
        // entry captures correlation_id, client_id, venue, data_type, timing, and
        // params so forensics can pair the response with its request.
        #[derive(serde::Deserialize)]
        struct CustomDataResponseOwned {
            correlation_id: UUID4,
            client_id: ClientId,
            venue: Option<Venue>,
            ts_init: UnixNanos,
        }

        let response = make_custom_data_response();
        let envelope = DataResponse::Data(response.clone());
        let encoded = encode_data_response(&envelope).expect("encode");

        let decoded: CustomDataResponseOwned =
            rmp_serde::from_slice(&encoded.payload).expect("decode");
        assert_eq!(decoded.correlation_id, response.correlation_id);
        assert_eq!(decoded.client_id, response.client_id);
        assert_eq!(decoded.venue, response.venue);
        assert_eq!(decoded.ts_init, response.ts_init);
    }

    #[rstest]
    fn data_response_book_envelope_stamps_inner_tag_with_no_indices() {
        let response = make_book_response();
        let envelope = DataResponse::Book(response);
        let encoded = encode_data_response(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_BOOK_RESPONSE,
        );
        assert!(encoded.index_keys.is_empty());
        assert!(!encoded.payload.is_empty());
    }

    #[rstest]
    fn data_response_book_payload_round_trips_metadata() {
        // BookResponseRef omits `data: OrderBook` (not serde-derived). The audit
        // entry captures the correlation and addressing metadata.
        #[derive(serde::Deserialize)]
        struct BookResponseOwned {
            correlation_id: UUID4,
            instrument_id: InstrumentId,
        }

        let response = make_book_response();
        let envelope = DataResponse::Book(response.clone());
        let encoded = encode_data_response(&envelope).expect("encode");

        let decoded: BookResponseOwned = rmp_serde::from_slice(&encoded.payload).expect("decode");
        assert_eq!(decoded.correlation_id, response.correlation_id);
        assert_eq!(decoded.instrument_id, response.instrument_id);
    }

    #[rstest]
    fn data_response_quotes_payload_round_trips() {
        let response = make_quotes_response();
        let envelope = DataResponse::Quotes(response.clone());
        let encoded = encode_data_response(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_QUOTES_RESPONSE,
        );
        assert!(encoded.index_keys.is_empty());

        let decoded: QuotesResponse = rmp_serde::from_slice(&encoded.payload).expect("decode");
        assert_eq!(decoded.correlation_id, response.correlation_id);
        assert_eq!(decoded.instrument_id, response.instrument_id);
        assert_eq!(decoded.data, response.data);
    }

    #[rstest]
    fn data_response_trades_payload_round_trips() {
        let response = make_trades_response();
        let envelope = DataResponse::Trades(response.clone());
        let encoded = encode_data_response(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_TRADES_RESPONSE,
        );

        let decoded: TradesResponse = rmp_serde::from_slice(&encoded.payload).expect("decode");
        assert_eq!(decoded.correlation_id, response.correlation_id);
    }

    #[rstest]
    fn data_response_bars_payload_round_trips() {
        let response = make_bars_response();
        let envelope = DataResponse::Bars(response.clone());
        let encoded = encode_data_response(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_BARS_RESPONSE,
        );

        let decoded: BarsResponse = rmp_serde::from_slice(&encoded.payload).expect("decode");
        assert_eq!(decoded.correlation_id, response.correlation_id);
        assert_eq!(decoded.bar_type, response.bar_type);
    }

    #[rstest]
    fn data_response_instrument_payload_round_trips() {
        let response = make_instrument_response();
        let envelope = DataResponse::Instrument(Box::new(response.clone()));
        let encoded = encode_data_response(&envelope).expect("encode");

        assert_eq!(
            encoded.payload_type.expect("override").as_str(),
            PAYLOAD_TYPE_INSTRUMENT_RESPONSE,
        );

        let decoded: InstrumentResponse = rmp_serde::from_slice(&encoded.payload).expect("decode");
        assert_eq!(decoded.correlation_id, response.correlation_id);
        assert_eq!(decoded.instrument_id, response.instrument_id);
    }

    // Walks every DataResponse variant and asserts the dispatcher stamps the
    // inner-variant tag and emits zero sidecar indices. Catches a swapped match
    // arm or a forgotten `with_payload_type` override that would otherwise fall
    // back to the wrapper sentinel tag.
    #[rstest]
    #[case::data(
        DataResponse::Data(make_custom_data_response()),
        PAYLOAD_TYPE_CUSTOM_DATA_RESPONSE
    )]
    #[case::instrument(
        DataResponse::Instrument(Box::new(make_instrument_response())),
        PAYLOAD_TYPE_INSTRUMENT_RESPONSE
    )]
    #[case::instruments(
        DataResponse::Instruments(make_instruments_response()),
        PAYLOAD_TYPE_INSTRUMENTS_RESPONSE
    )]
    #[case::book(DataResponse::Book(make_book_response()), PAYLOAD_TYPE_BOOK_RESPONSE)]
    #[case::quotes(
        DataResponse::Quotes(make_quotes_response()),
        PAYLOAD_TYPE_QUOTES_RESPONSE
    )]
    #[case::trades(
        DataResponse::Trades(make_trades_response()),
        PAYLOAD_TYPE_TRADES_RESPONSE
    )]
    #[case::funding_rates(
        DataResponse::FundingRates(make_funding_rates_response()),
        PAYLOAD_TYPE_FUNDING_RATES_RESPONSE
    )]
    #[case::forward_prices(
        DataResponse::ForwardPrices(make_forward_prices_response()),
        PAYLOAD_TYPE_FORWARD_PRICES_RESPONSE
    )]
    #[case::bars(DataResponse::Bars(make_bars_response()), PAYLOAD_TYPE_BARS_RESPONSE)]
    fn data_response_envelope_stamps_inner_tag_for_every_variant(
        #[case] response: DataResponse,
        #[case] expected_tag: &str,
    ) {
        let encoded = encode_data_response(&response).expect("encode");
        let tag = encoded.payload_type.expect("override").as_str().to_string();

        assert_eq!(tag, expected_tag);
        assert_ne!(
            tag, PAYLOAD_TYPE_DATA_RESPONSE,
            "wrapper fallback tag must never reach the writer",
        );
        assert!(
            encoded.index_keys.is_empty(),
            "DataResponse correlation_id has no matching IndexKind today",
        );
    }

    #[rstest]
    fn data_response_registered_under_canonical_payload_type() {
        // The default registry must dispatch DataResponse through encode_data_response
        // and stamp the inner-variant tag, so `send_data_response` captures under the
        // same canonical tag as a bare-type capture path.
        let registry = default_registry();
        let envelope = DataResponse::Quotes(make_quotes_response());
        let (tag, encoded) = registry
            .encode(&envelope)
            .expect("encode")
            .expect("registered");

        assert_eq!(tag.as_str(), PAYLOAD_TYPE_QUOTES_RESPONSE);
        assert!(encoded.index_keys.is_empty());
    }

    #[rstest]
    fn data_response_headers_extractor_surfaces_correlation_id() {
        // The data engine pairs RPC requests and responses by correlation_id (see
        // `crates/data/src/engine/mod.rs` `send_response`). Captured DataResponse entries
        // must carry that uuid in `Headers::correlation_id` so forensics can join a
        // captured response to its request.
        let registry = default_registry();
        let response = make_quotes_response();
        let expected = response.correlation_id;
        let envelope = DataResponse::Quotes(response);

        let headers = registry
            .headers_for_any(&envelope as &dyn std::any::Any)
            .expect("registered");
        assert_eq!(headers.correlation_id, Some(expected));
        assert_eq!(headers.causation_id, None);
    }

    #[rstest]
    fn data_command_request_headers_use_request_id_as_correlation() {
        // The request_id of an outbound RequestCommand IS the chain root: the eventual
        // DataResponse echoes the same uuid back as its correlation_id. Surfacing
        // request_id in `Headers::correlation_id` lines the request entry up with its
        // response entry under one chain key.
        let registry = default_registry();
        let request = make_quotes_request();
        let expected = request.request_id;
        let envelope = DataCommand::Request(RequestCommand::Quotes(request));

        let headers = registry
            .headers_for_any(&envelope as &dyn std::any::Any)
            .expect("registered");
        assert_eq!(headers.correlation_id, Some(expected));
    }

    #[rstest]
    fn data_command_subscribe_headers_surface_correlation_id() {
        // Subscribe variants carry an optional correlation_id field; the extractor
        // must forward whatever the inner command reports so captured subscribe
        // traffic joins its acknowledgements under one chain key.
        let registry = default_registry();
        let subscribe = make_subscribe_command();
        let expected = subscribe.correlation_id();
        let envelope = DataCommand::Subscribe(subscribe);

        let headers = registry
            .headers_for_any(&envelope as &dyn std::any::Any)
            .expect("registered");
        assert_eq!(headers.correlation_id, expected);
    }

    #[rstest]
    fn data_command_unsubscribe_headers_surface_correlation_id() {
        let registry = default_registry();
        let unsubscribe = make_unsubscribe_command();
        let expected = unsubscribe.correlation_id();
        let envelope = DataCommand::Unsubscribe(unsubscribe);

        let headers = registry
            .headers_for_any(&envelope as &dyn std::any::Any)
            .expect("registered");
        assert_eq!(headers.correlation_id, expected);
    }

    #[rstest]
    #[case::submit_order(trading_command_submit_order)]
    #[case::submit_order_list(trading_command_submit_order_list)]
    #[case::modify_order(trading_command_modify_order)]
    #[case::cancel_order(trading_command_cancel_order)]
    #[case::cancel_all_orders(trading_command_cancel_all_orders)]
    #[case::batch_cancel_orders(trading_command_batch_cancel_orders)]
    #[case::query_order(trading_command_query_order)]
    #[case::query_account(trading_command_query_account)]
    fn trading_command_extractor_surfaces_both_headers(
        #[case] builder: fn() -> (TradingCommand, UUID4, UUID4),
    ) {
        // The TradingCommand envelope dispatch must route every variant to the matching
        // per-type extractor and forward both correlation_id and causation_id intact.
        // A swap of args inside any extract_*_headers helper or a misrouted wrapper arm
        // is caught by exercising each variant with distinct populated values.
        let (envelope, corr, caus) = builder();
        let registry = default_registry();

        let headers = registry
            .headers_for_any(&envelope as &dyn std::any::Any)
            .expect("registered");
        assert_eq!(headers.correlation_id, Some(corr));
        assert_eq!(headers.causation_id, Some(caus));
    }

    fn trading_command_submit_order() -> (TradingCommand, UUID4, UUID4) {
        let corr = UUID4::new();
        let caus = UUID4::new();
        let mut cmd = make_submit_order();
        cmd.correlation_id = Some(corr);
        cmd.causation_id = Some(caus);
        (TradingCommand::SubmitOrder(cmd), corr, caus)
    }

    fn trading_command_submit_order_list() -> (TradingCommand, UUID4, UUID4) {
        let corr = UUID4::new();
        let caus = UUID4::new();
        let mut cmd = make_submit_order_list(vec![client_order_id()]);
        cmd.correlation_id = Some(corr);
        cmd.causation_id = Some(caus);
        (TradingCommand::SubmitOrderList(cmd), corr, caus)
    }

    fn trading_command_modify_order() -> (TradingCommand, UUID4, UUID4) {
        let corr = UUID4::new();
        let caus = UUID4::new();
        let mut cmd = make_modify_order(Some(venue_order_id()));
        cmd.correlation_id = Some(corr);
        cmd.causation_id = Some(caus);
        (TradingCommand::ModifyOrder(cmd), corr, caus)
    }

    fn trading_command_cancel_order() -> (TradingCommand, UUID4, UUID4) {
        let corr = UUID4::new();
        let caus = UUID4::new();
        let mut cmd = make_cancel_order();
        cmd.correlation_id = Some(corr);
        cmd.causation_id = Some(caus);
        (TradingCommand::CancelOrder(cmd), corr, caus)
    }

    fn trading_command_cancel_all_orders() -> (TradingCommand, UUID4, UUID4) {
        let corr = UUID4::new();
        let caus = UUID4::new();
        let mut cmd = make_cancel_all_orders();
        cmd.correlation_id = Some(corr);
        cmd.causation_id = Some(caus);
        (TradingCommand::CancelAllOrders(cmd), corr, caus)
    }

    fn trading_command_batch_cancel_orders() -> (TradingCommand, UUID4, UUID4) {
        let corr = UUID4::new();
        let caus = UUID4::new();
        let mut cmd = make_batch_cancel_orders(vec![make_cancel_order()]);
        cmd.correlation_id = Some(corr);
        cmd.causation_id = Some(caus);
        (TradingCommand::BatchCancelOrders(cmd), corr, caus)
    }

    fn trading_command_query_order() -> (TradingCommand, UUID4, UUID4) {
        let corr = UUID4::new();
        let caus = UUID4::new();
        let mut cmd = make_query_order(Some(venue_order_id()));
        cmd.correlation_id = Some(corr);
        cmd.causation_id = Some(caus);
        (TradingCommand::QueryOrder(cmd), corr, caus)
    }

    fn trading_command_query_account() -> (TradingCommand, UUID4, UUID4) {
        let corr = UUID4::new();
        let caus = UUID4::new();
        let mut cmd = make_query_account();
        cmd.correlation_id = Some(corr);
        cmd.causation_id = Some(caus);
        (TradingCommand::QueryAccount(cmd), corr, caus)
    }

    #[rstest]
    #[case::data(data_response_data())]
    #[case::instrument(data_response_instrument())]
    #[case::instruments(data_response_instruments())]
    #[case::book(data_response_book())]
    #[case::quotes(data_response_quotes())]
    #[case::trades(data_response_trades())]
    #[case::funding_rates(data_response_funding_rates())]
    #[case::forward_prices(data_response_forward_prices())]
    #[case::bars(data_response_bars())]
    fn data_response_extractor_surfaces_correlation_id_for_every_variant(
        #[case] envelope_with_expected: (DataResponse, UUID4),
    ) {
        // Every DataResponse variant carries a required correlation_id paired with its
        // originating request; the extractor must forward that uuid intact regardless
        // of which variant is captured.
        let (envelope, expected) = envelope_with_expected;
        let registry = default_registry();

        let headers = registry
            .headers_for_any(&envelope as &dyn std::any::Any)
            .expect("registered");
        assert_eq!(headers.correlation_id, Some(expected));
        assert_eq!(headers.causation_id, None);
    }

    fn data_response_data() -> (DataResponse, UUID4) {
        let resp = make_custom_data_response();
        let expected = resp.correlation_id;
        (DataResponse::Data(resp), expected)
    }

    fn data_response_instrument() -> (DataResponse, UUID4) {
        let resp = make_instrument_response();
        let expected = resp.correlation_id;
        (DataResponse::Instrument(Box::new(resp)), expected)
    }

    fn data_response_instruments() -> (DataResponse, UUID4) {
        let resp = make_instruments_response();
        let expected = resp.correlation_id;
        (DataResponse::Instruments(resp), expected)
    }

    fn data_response_book() -> (DataResponse, UUID4) {
        let resp = make_book_response();
        let expected = resp.correlation_id;
        (DataResponse::Book(resp), expected)
    }

    fn data_response_quotes() -> (DataResponse, UUID4) {
        let resp = make_quotes_response();
        let expected = resp.correlation_id;
        (DataResponse::Quotes(resp), expected)
    }

    fn data_response_trades() -> (DataResponse, UUID4) {
        let resp = make_trades_response();
        let expected = resp.correlation_id;
        (DataResponse::Trades(resp), expected)
    }

    fn data_response_funding_rates() -> (DataResponse, UUID4) {
        let resp = make_funding_rates_response();
        let expected = resp.correlation_id;
        (DataResponse::FundingRates(resp), expected)
    }

    fn data_response_forward_prices() -> (DataResponse, UUID4) {
        let resp = make_forward_prices_response();
        let expected = resp.correlation_id;
        (DataResponse::ForwardPrices(resp), expected)
    }

    fn data_response_bars() -> (DataResponse, UUID4) {
        let resp = make_bars_response();
        let expected = resp.correlation_id;
        (DataResponse::Bars(resp), expected)
    }

    fn make_quotes_request() -> RequestQuotes {
        RequestQuotes {
            instrument_id: InstrumentId::from("EUR/USD.SIM"),
            start: None,
            end: None,
            limit: None,
            client_id: None,
            request_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
        }
    }
}
