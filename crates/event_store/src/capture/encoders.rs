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
//! Phase 6 ships a sample triple — `SubmitOrder` (command), `OrderFilled` (generated
//! event), `OrderStatusReport` (raw venue report) — so the bus capture adapter has a
//! working allow-list end-to-end. The remaining encoders land alongside the broader
//! capture surface in later phases; the registry shape stays the same.
//!
//! The payload serialization format is MessagePack via `rmp-serde`. The on-disk envelope
//! codec stays bincode (positional, non-self-describing); MessagePack inside the payload
//! handles the upstream Nautilus types that carry `#[serde(tag = "type")]` internal
//! tagging, which a non-self-describing format like bincode cannot round-trip.
//!
//! These encoders register against the inner Rust types (e.g., [`SubmitOrder`]). Bus
//! APIs that carry envelope enums such as `TradingCommand` or `OrderEventAny` reach the
//! adapter as their wrapper [`std::any::TypeId`], so the dispatch wrapper either
//! unwraps the variant before calling [`crate::BusCaptureAdapter::capture`] or registers
//! a wrapper-aware encoder via [`crate::EncoderRegistry::register`]. The wiring choice
//! lands with the bus dispatch instrumentation in a later phase.

use bytes::Bytes;
use nautilus_common::messages::execution::SubmitOrder;
use nautilus_model::{events::OrderFilled, reports::OrderStatusReport};
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
/// The canonical `payload_type` tag for [`OrderFilled`].
pub const PAYLOAD_TYPE_ORDER_FILLED: &str = "OrderFilled";
/// The canonical `payload_type` tag for [`OrderStatusReport`].
pub const PAYLOAD_TYPE_ORDER_STATUS_REPORT: &str = "OrderStatusReport";

/// Returns an [`EncoderRegistry`] preloaded with the Phase 6 representative encoders.
///
/// Callers can extend the returned registry with additional encoders before constructing
/// the [`crate::capture::BusCaptureAdapter`].
#[must_use]
pub fn default_registry() -> EncoderRegistry {
    let mut registry = EncoderRegistry::new();
    register_default(&mut registry);
    registry
}

/// Adds the Phase 6 representative encoders to `registry`.
pub fn register_default(registry: &mut EncoderRegistry) {
    registry
        .register::<SubmitOrder, _>(payload_type(PAYLOAD_TYPE_SUBMIT_ORDER), encode_submit_order);
    registry
        .register::<OrderFilled, _>(payload_type(PAYLOAD_TYPE_ORDER_FILLED), encode_order_filled);
    registry.register::<OrderStatusReport, _>(
        payload_type(PAYLOAD_TYPE_ORDER_STATUS_REPORT),
        encode_order_status_report,
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
        events::OrderInitialized,
        identifiers::{
            AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId,
            VenueOrderId,
        },
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
    fn default_registry_contains_phase6_encoders() {
        let registry = default_registry();

        assert_eq!(registry.len(), 3);
        assert!(registry.contains::<SubmitOrder>());
        assert!(registry.contains::<OrderFilled>());
        assert!(registry.contains::<OrderStatusReport>());
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
}
