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

//! External message bus stream encoding and republishing.

use std::{any::Any, cell::Cell};

use anyhow::Context;
use nautilus_model::{
    data::{CustomData, Data, deserialize_custom_from_json},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
};
use serde::de::DeserializeOwned;
use ustr::Ustr;

pub(crate) mod codec;

use self::codec::PayloadCodecError;
use super::{
    BusMessage, BusMessageSource, BusPayloadType, HAS_EXTERNAL_EGRESS, SUPPRESS_EXTERNAL_DEPTH,
    SuppressExternalGuard,
    api::{
        publish_account_state, publish_any, publish_bar, publish_deltas, publish_depth,
        publish_funding_rate, publish_index_price, publish_instrument, publish_mark_price,
        publish_option_greeks, publish_order_event, publish_portfolio_snapshot,
        publish_position_event, publish_quote, publish_trade,
    },
    get_message_bus,
    mstr::{MStr, Topic},
};
use crate::{
    enums::SerializationEncoding,
    messages::{
        data::{
            DataResponse, DataResponseEnvelope, RequestCommand, SubscribeCommand,
            UnsubscribeCommand,
        },
        execution::{GenerateExecutionMassStatus, TradingCommand},
    },
};

#[inline(always)]
pub(super) fn forward_to_external_egress<T>(
    topic: MStr<Topic>,
    payload_type: BusPayloadType,
    message: &T,
) where
    T: serde::Serialize + Any,
{
    if !HAS_EXTERNAL_EGRESS.with(Cell::get) {
        return;
    }

    forward_external_message(topic, payload_type, message);
}

#[inline]
pub(super) fn forward_any_to_external_egress(topic: MStr<Topic>, message: &dyn Any) {
    if !HAS_EXTERNAL_EGRESS.with(Cell::get) {
        return;
    }

    if let Some(custom) = message.downcast_ref::<CustomData>() {
        forward_external_message(
            topic,
            BusPayloadType::Custom(Ustr::from(custom.data.type_name())),
            custom,
        );
        return;
    }

    if forward_downcast::<SubscribeCommand>(topic, BusPayloadType::SubscribeCommand, message)
        || forward_downcast::<UnsubscribeCommand>(
            topic,
            BusPayloadType::UnsubscribeCommand,
            message,
        )
        || forward_downcast::<DataResponse>(topic, BusPayloadType::DataResponse, message)
        || forward_downcast::<DataResponseEnvelope>(
            topic,
            BusPayloadType::DataResponseEnvelope,
            message,
        )
        || forward_downcast::<RequestCommand>(topic, BusPayloadType::RequestCommand, message)
        || forward_downcast::<TradingCommand>(topic, BusPayloadType::TradingCommand, message)
        || forward_downcast::<GenerateExecutionMassStatus>(
            topic,
            BusPayloadType::GenerateExecutionMassStatus,
            message,
        )
        || forward_downcast::<OrderStatusReport>(topic, BusPayloadType::OrderStatusReport, message)
        || forward_downcast::<FillReport>(topic, BusPayloadType::FillReport, message)
        || forward_downcast::<PositionStatusReport>(
            topic,
            BusPayloadType::PositionStatusReport,
            message,
        )
    {
        return;
    }

    forward_downcast::<ExecutionMassStatus>(topic, BusPayloadType::ExecutionMassStatus, message);
}

fn forward_downcast<T>(topic: MStr<Topic>, payload_type: BusPayloadType, message: &dyn Any) -> bool
where
    T: serde::Serialize + Any,
{
    let Some(message) = message.downcast_ref::<T>() else {
        return false;
    };

    forward_external_message(topic, payload_type, message);
    true
}

#[cold]
#[inline(never)]
fn forward_external_message<T>(topic: MStr<Topic>, payload_type: BusPayloadType, message: &T)
where
    T: serde::Serialize + Any,
{
    if SUPPRESS_EXTERNAL_DEPTH.with(Cell::get) > 0 {
        return;
    }

    let (external_egress, encoding) = {
        let bus_rc = get_message_bus();
        let bus = bus_rc.borrow();

        let Some(external_egress) = bus.external_egress() else {
            return;
        };

        if payload_type.is_typed_message() && !bus.has_external_streams() {
            return;
        }

        if bus.types_filter().contains(&payload_type) {
            return;
        }

        (external_egress, bus.encoding_for(payload_type))
    };

    if external_egress.borrow().is_closed() {
        return;
    }

    let payload = match codec::serialize_payload(encoding, payload_type, message) {
        Ok(payload) => payload,
        Err(PayloadCodecError::Dropped(e)) => {
            log::debug!("{e}");
            return;
        }
        Err(PayloadCodecError::Failed(e)) => {
            log::error!("{e}");
            return;
        }
    };

    let external_egress = external_egress.borrow();
    if external_egress.is_closed() {
        return;
    }

    // Serialization can close the original destination or replace it on the bus
    external_egress.publish(BusMessage::new(*topic, payload_type, payload, encoding));
}

thread_local! {
    static EXTERNAL_SOURCE: Cell<Option<BusMessageSource>> = const { Cell::new(None) };
}

/// Returns the source of the external message currently being dispatched on this thread.
#[must_use]
pub fn current_external_source() -> Option<BusMessageSource> {
    EXTERNAL_SOURCE.get()
}

struct ExternalSourceGuard(Option<BusMessageSource>);

impl ExternalSourceGuard {
    fn new(source: Option<BusMessageSource>) -> Self {
        Self(EXTERNAL_SOURCE.replace(source))
    }
}

impl Drop for ExternalSourceGuard {
    fn drop(&mut self) {
        EXTERNAL_SOURCE.set(self.0);
    }
}

/// Republishes a native data reply only when its correlation handler is pending.
///
/// Returns `false` without dispatching when the message is not a streaming-registered
/// `DataResponse` or `DataResponseEnvelope`, or no handler is pending for its correlation ID.
/// `LiveNode` uses this during data-client connection, before actors and strategies subscribe.
/// The reply dispatches through application topic handlers, with external provenance and egress
/// suppression active during dispatch. Startup replies bypass stream processors and can overtake
/// buffered messages from the same ingress.
///
/// # Errors
///
/// Returns an error if a native data reply cannot be decoded or its topic is invalid.
pub fn republish_pending_external_response(message: &BusMessage) -> anyhow::Result<bool> {
    if !matches!(
        message.payload_type,
        BusPayloadType::DataResponse | BusPayloadType::DataResponseEnvelope
    ) || !is_registered_streaming_type(message)
    {
        return Ok(false);
    }

    match message.payload_type {
        BusPayloadType::DataResponseEnvelope => republish_pending_response::<DataResponseEnvelope>(
            message,
            DataResponseEnvelope::correlation_id,
        ),
        _ => republish_pending_response::<DataResponse>(message, DataResponse::correlation_id),
    }
}

fn republish_pending_response<T: serde::de::DeserializeOwned + 'static>(
    message: &BusMessage,
    correlation_id: fn(&T) -> &nautilus_core::UUID4,
) -> anyhow::Result<bool> {
    let Some(response) =
        codec::deserialize_payload::<T>(message.payload_type, message.encoding, &message.payload)?
    else {
        return Ok(false);
    };

    if get_message_bus()
        .borrow()
        .get_response_handler(correlation_id(&response))
        .is_none()
    {
        return Ok(false);
    }

    let topic =
        MStr::<Topic>::topic_from_ustr(message.topic).context("invalid external message topic")?;
    let _guard = SuppressExternalGuard::new();
    let _source = ExternalSourceGuard::new(message.source);
    publish_any(topic, &response);
    Ok(true)
}

/// Decodes an externally-received [`BusMessage`] and republishes it onto the internal bus.
///
/// The message `payload_type` header selects the concrete type and the message `encoding` selects
/// the wire codec, so the message is decoded with the producer's encoding rather than the local
/// configuration. Republishing runs under a [`SuppressExternalGuard`] so the message is not
/// forwarded straight back out through external egress, which would create an echo loop on a node
/// that has both external ingress and egress.
///
/// # Errors
///
/// Returns an error if the topic is invalid or a supported payload cannot be decoded. Unsupported
/// encodings for native data requests and responses also return an error. Other unsupported
/// type/encoding pairs are skipped with a warning.
pub fn republish_external_message(message: &BusMessage) -> anyhow::Result<()> {
    let topic =
        MStr::<Topic>::topic_from_ustr(message.topic).context("invalid external message topic")?;

    if !is_registered_streaming_type(message) {
        return Ok(());
    }

    let _guard = SuppressExternalGuard::new();
    let _source = ExternalSourceGuard::new(message.source);

    match message.payload_type {
        BusPayloadType::Custom(_) => {
            handle_custom_data(
                topic,
                message.payload_type,
                message.encoding,
                &message.payload,
            )?;
        }
        BusPayloadType::Instrument => {
            handle_payload(
                topic,
                message.payload_type,
                message.encoding,
                &message.payload,
                publish_instrument,
            )?;
        }
        BusPayloadType::OrderBookDeltas => handle_market_data(
            topic,
            message.encoding,
            &message.payload,
            codec::deserialize_order_book_deltas,
            publish_deltas,
        )?,
        BusPayloadType::OrderBookDepth => handle_market_data(
            topic,
            message.encoding,
            &message.payload,
            codec::deserialize_order_book_depth,
            publish_depth,
        )?,
        BusPayloadType::QuoteTick => handle_market_data(
            topic,
            message.encoding,
            &message.payload,
            codec::deserialize_quote,
            publish_quote,
        )?,
        BusPayloadType::TradeTick => handle_market_data(
            topic,
            message.encoding,
            &message.payload,
            codec::deserialize_trade,
            publish_trade,
        )?,
        BusPayloadType::Bar => handle_market_data(
            topic,
            message.encoding,
            &message.payload,
            codec::deserialize_bar,
            publish_bar,
        )?,
        BusPayloadType::MarkPriceUpdate => handle_market_data(
            topic,
            message.encoding,
            &message.payload,
            codec::deserialize_mark_price,
            publish_mark_price,
        )?,
        BusPayloadType::IndexPriceUpdate => handle_market_data(
            topic,
            message.encoding,
            &message.payload,
            codec::deserialize_index_price,
            publish_index_price,
        )?,
        BusPayloadType::FundingRateUpdate => handle_market_data(
            topic,
            message.encoding,
            &message.payload,
            codec::deserialize_funding_rate,
            publish_funding_rate,
        )?,
        BusPayloadType::OptionGreeks => {
            handle_market_data(
                topic,
                message.encoding,
                &message.payload,
                codec::deserialize_option_greeks,
                publish_option_greeks,
            )?;
        }
        BusPayloadType::AccountState => {
            handle_payload(
                topic,
                message.payload_type,
                message.encoding,
                &message.payload,
                publish_account_state,
            )?;
        }
        BusPayloadType::OrderEvent => {
            handle_payload(
                topic,
                message.payload_type,
                message.encoding,
                &message.payload,
                publish_order_event,
            )?;
        }
        BusPayloadType::PositionEvent => {
            handle_payload(
                topic,
                message.payload_type,
                message.encoding,
                &message.payload,
                publish_position_event,
            )?;
        }
        BusPayloadType::PortfolioSnapshot => {
            handle_payload(
                topic,
                message.payload_type,
                message.encoding,
                &message.payload,
                publish_portfolio_snapshot,
            )?;
        }
        BusPayloadType::SubscribeCommand => {
            handle_payload_any::<SubscribeCommand>(topic, message)?;
        }
        BusPayloadType::UnsubscribeCommand => {
            handle_payload_any::<UnsubscribeCommand>(topic, message)?;
        }
        BusPayloadType::DataResponse => {
            handle_payload_any::<DataResponse>(topic, message)?;
        }
        BusPayloadType::DataResponseEnvelope => {
            handle_payload_any::<DataResponseEnvelope>(topic, message)?;
        }
        BusPayloadType::RequestCommand => {
            handle_payload_any::<RequestCommand>(topic, message)?;
        }
        BusPayloadType::TradingCommand => {
            handle_payload_any::<TradingCommand>(topic, message)?;
        }
        BusPayloadType::GenerateExecutionMassStatus => {
            handle_payload_any::<GenerateExecutionMassStatus>(topic, message)?;
        }
        BusPayloadType::OrderStatusReport => {
            handle_payload_any::<OrderStatusReport>(topic, message)?;
        }
        BusPayloadType::FillReport => {
            handle_payload_any::<FillReport>(topic, message)?;
        }
        BusPayloadType::PositionStatusReport => {
            handle_payload_any::<PositionStatusReport>(topic, message)?;
        }
        BusPayloadType::ExecutionMassStatus => {
            handle_payload_any::<ExecutionMassStatus>(topic, message)?;
        }
        #[cfg(feature = "defi")]
        BusPayloadType::Block
        | BusPayloadType::Pool
        | BusPayloadType::PoolLiquidityUpdate
        | BusPayloadType::PoolFeeCollect
        | BusPayloadType::PoolFlash => {
            crate::defi::msgbus::republish_external_message(
                topic,
                message.payload_type,
                message.encoding,
                &message.payload,
            )?;
        }
    }

    Ok(())
}

/// Decodes a supported typed payload and passes it to `processor` before applying normal inbound
/// republishing rules.
///
/// Supported typed JSON or MessagePack payloads reach `processor` regardless of internal streaming
/// registration. Internal republishing still requires registration. Native requests and replies
/// use the shared JSON/MessagePack codecs; unsupported encodings return an error.
/// Other unsupported type/encoding pairs are skipped with a warning. The processor mapping
/// includes a `payload_type` field containing the [`BusPayloadType`] name. External egress is
/// suppressed for the duration of processing, including processor callbacks. Other payloads bypass
/// the processor and follow normal inbound republishing. A processor error stops processing and
/// skips internal republishing.
///
/// # Errors
///
/// Returns an error if:
/// - The topic is invalid.
/// - A native data request or response uses an unsupported encoding.
/// - The typed payload cannot be decoded or mapped to an object.
/// - The mapping already contains the reserved `payload_type` field.
/// - The processor fails.
/// - Normal inbound republishing fails.
pub fn process_external_typed_message(
    message: &BusMessage,
    processor: &mut dyn FnMut(&dyn Any, &serde_json::Value) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let topic =
        MStr::<Topic>::topic_from_ustr(message.topic).context("invalid external message topic")?;
    let _guard = SuppressExternalGuard::new();
    let _source = ExternalSourceGuard::new(message.source);

    #[rustfmt::skip]
    let result = match message.payload_type {
        BusPayloadType::SubscribeCommand => process_typed_payload::<SubscribeCommand>(topic, message, processor),
        BusPayloadType::UnsubscribeCommand => process_typed_payload::<UnsubscribeCommand>(topic, message, processor),
        BusPayloadType::DataResponse => process_typed_payload::<DataResponse>(topic, message, processor),
        BusPayloadType::DataResponseEnvelope => process_typed_payload::<DataResponseEnvelope>(topic, message, processor),
        BusPayloadType::RequestCommand => process_typed_payload::<RequestCommand>(topic, message, processor),
        BusPayloadType::TradingCommand => process_typed_payload::<TradingCommand>(topic, message, processor),
        BusPayloadType::GenerateExecutionMassStatus => process_typed_payload::<GenerateExecutionMassStatus>(topic, message, processor),
        BusPayloadType::OrderStatusReport => process_typed_payload::<OrderStatusReport>(topic, message, processor),
        BusPayloadType::FillReport => process_typed_payload::<FillReport>(topic, message, processor),
        BusPayloadType::PositionStatusReport => process_typed_payload::<PositionStatusReport>(topic, message, processor),
        BusPayloadType::ExecutionMassStatus => process_typed_payload::<ExecutionMassStatus>(topic, message, processor),
        _ => republish_external_message(message),
    };
    result
}

fn handle_payload_any<T>(topic: MStr<Topic>, message: &BusMessage) -> anyhow::Result<()>
where
    T: DeserializeOwned + Any,
{
    handle_payload(
        topic,
        message.payload_type,
        message.encoding,
        &message.payload,
        |topic, value: &T| publish_any(topic, value),
    )
}

fn process_typed_payload<T>(
    topic: MStr<Topic>,
    message: &BusMessage,
    processor: &mut dyn FnMut(&dyn Any, &serde_json::Value) -> anyhow::Result<()>,
) -> anyhow::Result<()>
where
    T: DeserializeOwned + serde::Serialize + Any,
{
    let Some(value) =
        codec::deserialize_payload::<T>(message.payload_type, message.encoding, &message.payload)?
    else {
        return Ok(());
    };

    let mut mapping = serde_json::to_value(&value).with_context(|| {
        format!(
            "failed to map decoded {} stream payload",
            message.payload_type
        )
    })?;

    let mapping_object = mapping.as_object_mut().with_context(|| {
        format!(
            "decoded {} stream payload did not map to an object",
            message.payload_type
        )
    })?;

    anyhow::ensure!(
        !mapping_object.contains_key("payload_type"),
        "decoded {} stream payload contains reserved payload_type field",
        message.payload_type
    );
    mapping_object.insert(
        "payload_type".to_string(),
        serde_json::Value::String(message.payload_type.as_str().to_string()),
    );

    processor(&value, &mapping)?;
    if is_registered_streaming_type(message) {
        publish_any(topic, &value);
    }

    Ok(())
}

fn is_registered_streaming_type(message: &BusMessage) -> bool {
    if get_message_bus()
        .borrow()
        .is_streaming_type(message.payload_type)
    {
        return true;
    }

    let type_name = message.payload_type.as_str();
    if type_name.is_empty() {
        log::debug!(
            "Skipping external message on topic '{}' with no payload type for inbound republishing",
            message.topic
        );
    } else {
        log::debug!(
            "Skipping external {type_name} message on topic '{}' because the type is not registered for streaming",
            message.topic
        );
    }

    false
}

pub(crate) fn handle_payload<T>(
    topic: MStr<Topic>,
    payload_type: BusPayloadType,
    encoding: SerializationEncoding,
    payload: &[u8],
    publish: impl FnOnce(MStr<Topic>, &T),
) -> anyhow::Result<()>
where
    T: DeserializeOwned + 'static,
{
    let Some(value) = codec::deserialize_payload(payload_type, encoding, payload)? else {
        return Ok(());
    };

    publish(topic, &value);
    Ok(())
}

fn handle_market_data<T>(
    topic: MStr<Topic>,
    encoding: SerializationEncoding,
    payload: &[u8],
    deserialize: fn(SerializationEncoding, &[u8]) -> anyhow::Result<Option<T>>,
    publish: impl FnOnce(MStr<Topic>, &T),
) -> anyhow::Result<()> {
    let Some(value) = deserialize(encoding, payload)? else {
        return Ok(());
    };

    publish(topic, &value);
    Ok(())
}

fn handle_custom_data(
    topic: MStr<Topic>,
    payload_type: BusPayloadType,
    encoding: SerializationEncoding,
    payload: &[u8],
) -> anyhow::Result<()> {
    let Some(custom) = decode_custom_data_payload(payload_type, encoding, payload)? else {
        return Ok(());
    };

    publish_any(topic, &custom);
    Ok(())
}

fn decode_custom_data_payload(
    payload_type: BusPayloadType,
    encoding: SerializationEncoding,
    payload: &[u8],
) -> anyhow::Result<Option<CustomData>> {
    let BusPayloadType::Custom(custom_type_name) = payload_type else {
        unreachable!("custom data payload decoding requires a custom payload type");
    };

    if custom_type_name.is_empty() {
        log::warn!("External payload has no type for inbound republishing");
        return Ok(None);
    } else if !payload_type.supports(encoding) {
        codec::warn_unsupported_inbound(payload_type, encoding);
        return Ok(None);
    }

    match encoding {
        SerializationEncoding::Json => {
            let value =
                codec::deserialize_json_payload::<serde_json::Value>(payload, "CustomData")?;
            decode_custom_data_value(custom_type_name, &value)
                .context("failed to decode JSON CustomData")
        }
        SerializationEncoding::MsgPack => {
            let value =
                codec::deserialize_msgpack_payload::<serde_json::Value>(payload, "CustomData")?;
            decode_custom_data_value(custom_type_name, &value)
                .context("failed to decode MsgPack CustomData")
        }
        SerializationEncoding::Sbe => {
            codec::warn_unsupported_inbound(payload_type, encoding);
            Ok(None)
        }
    }
}

fn decode_custom_data_value(
    custom_type_name: Ustr,
    value: &serde_json::Value,
) -> anyhow::Result<Option<CustomData>> {
    let Some(data) = deserialize_custom_from_json(custom_type_name.as_str(), value)? else {
        log::warn!(
            "External custom payload type '{custom_type_name}' is not registered for inbound republishing"
        );
        return Ok(None);
    };

    let envelope_type_name = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .context("CustomData JSON missing 'type' field")?;
    anyhow::ensure!(
        envelope_type_name == custom_type_name.as_str(),
        "CustomData envelope type '{envelope_type_name}' does not match message type '{custom_type_name}'"
    );

    let Data::Custom(custom) = data else {
        anyhow::bail!("CustomData registry returned non-custom data");
    };

    Ok(Some(custom))
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use bytes::Bytes;
    use rstest::rstest;
    use serde::{Serialize, Serializer};

    use super::*;
    use crate::msgbus::{
        MessageBus, TypedHandler, backing::MessageBusExternalEgress, set_message_bus,
    };

    struct RecordingEgress {
        messages: Rc<RefCell<Vec<BusMessage>>>,
        closed: bool,
    }

    impl MessageBusExternalEgress for RecordingEgress {
        fn is_closed(&self) -> bool {
            self.closed
        }

        fn publish(&self, message: BusMessage) {
            self.messages.borrow_mut().push(message);
        }

        fn close(&mut self) {
            self.closed = true;
        }
    }

    struct ReentrantPayload<F>(F);

    impl<F: Fn()> Serialize for ReentrantPayload<F> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            (self.0)();
            serializer.serialize_u64(37)
        }
    }

    #[rstest]
    fn serialization_can_publish_before_original_egress_delivery() {
        let messages = install_egress();

        let payload = ReentrantPayload(|| {
            forward_external_message("nested".into(), BusPayloadType::QuoteTick, &19_u64);
        });

        forward_external_message("outer".into(), BusPayloadType::QuoteTick, &payload);

        let messages = messages.borrow();
        assert_eq!(messages.len(), 2);
        assert_message(&messages[0], "nested", b"19");
        assert_message(&messages[1], "outer", b"37");
    }

    #[rstest]
    #[case::close(false)]
    #[case::dispose(true)]
    fn serialization_can_close_original_egress(#[case] dispose: bool) {
        let messages = install_egress();

        let payload = ReentrantPayload(move || {
            let bus = get_message_bus();
            let mut bus = bus.borrow_mut();
            if dispose {
                bus.dispose();
            } else {
                bus.close().unwrap();
            }
        });

        forward_external_message("outer".into(), BusPayloadType::QuoteTick, &payload);

        assert!(messages.borrow().is_empty());
        assert!(!get_message_bus().borrow().has_external_egress());
    }

    #[rstest]
    fn serialization_keeps_original_destination_when_egress_is_replaced() {
        let original = install_egress();
        let replacement = Rc::new(RefCell::new(Vec::new()));
        let replacement_clone = replacement.clone();

        let payload = ReentrantPayload(move || {
            get_message_bus().borrow_mut().set_external_egress(
                Box::new(RecordingEgress {
                    messages: replacement_clone.clone(),
                    closed: false,
                }),
                SerializationEncoding::Json,
            );
        });

        forward_external_message("outer".into(), BusPayloadType::QuoteTick, &payload);
        forward_external_message("later".into(), BusPayloadType::QuoteTick, &23_u64);

        let original = original.borrow();
        let replacement = replacement.borrow();
        assert_eq!(original.len(), 1);
        assert_message(&original[0], "outer", b"37");
        assert_eq!(replacement.len(), 1);
        assert_message(&replacement[0], "later", b"23");
    }

    #[rstest]
    #[case(SerializationEncoding::Json)]
    #[case(SerializationEncoding::MsgPack)]
    fn test_native_requests_reach_processors_without_stream_registration(
        #[case] encoding: SerializationEncoding,
    ) {
        use nautilus_core::UUID4;
        use nautilus_model::{data::DataType, identifiers::ClientId};

        use crate::messages::data::RequestCustomData;

        let publications = install_egress();
        get_message_bus()
            .borrow_mut()
            .set_external_egress_config(
                Box::new(RecordingEgress {
                    messages: publications.clone(),
                    closed: false,
                }),
                &crate::msgbus::MessageBusConfig {
                    encoding,
                    external_streams: Some(vec!["external.test".into()]),
                    ..Default::default()
                },
            )
            .unwrap();

        let request = RequestCommand::Data(RequestCustomData::new(
            ClientId::from("FEED"),
            DataType::new("DistinctRequest", None, None),
            None,
            None,
            None,
            UUID4::new(),
            71.into(),
            None,
        ));

        let payload_type = BusPayloadType::RequestCommand;
        publish_any("commands.data.FEED".into(), &request);

        let message = publications.borrow()[0].clone();
        let mut received = None;
        process_external_typed_message(&message, &mut |value, mapping| {
            let decoded =
                serde_json::to_value(value.downcast_ref::<RequestCommand>().unwrap()).unwrap();

            received = Some((decoded, mapping.clone()));
            Ok(())
        })
        .unwrap();

        let expected_value = serde_json::to_value(&request).unwrap();

        let mut expected_mapping = expected_value.clone();
        expected_mapping
            .as_object_mut()
            .unwrap()
            .insert("payload_type".into(), payload_type.as_str().into());
        assert_eq!(publications.borrow().len(), 1);
        assert_eq!(message.topic.as_str(), "commands.data.FEED");
        assert_eq!(message.payload_type, payload_type);
        assert_eq!(message.encoding, encoding);
        assert_eq!(received, Some((expected_value, expected_mapping)));
    }

    #[rstest]
    #[case(BusPayloadType::RequestCommand, "data request")]
    #[case(BusPayloadType::DataResponse, "data response")]
    fn test_native_messages_reject_unsupported_encoding_without_dispatch(
        #[case] payload_type: BusPayloadType,
        #[case] name: &str,
        #[values(SerializationEncoding::Sbe)] encoding: SerializationEncoding,
    ) {
        set_message_bus(Rc::new(RefCell::new(MessageBus::default())));
        get_message_bus()
            .borrow_mut()
            .add_streaming_type(payload_type);
        let message = BusMessage::with_str_topic(
            "external.test",
            payload_type,
            Bytes::from_static(b"unsupported"),
            encoding,
        );
        let expected = format!("{name} encoding is not supported: {encoding}");
        let mut calls = 0;
        let error = process_external_typed_message(&message, &mut |_, _| {
            calls += 1;
            Ok(())
        })
        .unwrap_err();

        assert_eq!(error.to_string(), expected);
        assert_eq!(
            republish_external_message(&message)
                .unwrap_err()
                .to_string(),
            expected
        );
        assert_eq!(calls, 0);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_external_source_restored_after_nested_dispatch_and_processor_failure(
        #[case] panic: bool,
    ) {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        use nautilus_core::UUID4;
        use nautilus_model::{data::DataType, identifiers::ClientId};

        use crate::{
            messages::data::RequestCustomData,
            msgbus::{ShareableMessageHandler, subscribe_any},
        };

        set_message_bus(Rc::new(RefCell::new(MessageBus::default())));
        get_message_bus()
            .borrow_mut()
            .add_streaming_type(BusPayloadType::RequestCommand);
        let request = RequestCommand::Data(RequestCustomData::new(
            ClientId::from("FEED"),
            DataType::new("DistinctRequest", None, None),
            None,
            None,
            None,
            UUID4::new(),
            71.into(),
            None,
        ));
        let payload = codec::serialize_payload(
            SerializationEncoding::Json,
            BusPayloadType::RequestCommand,
            &request,
        )
        .unwrap();
        let mut message = BusMessage::with_str_topic(
            "external.outer",
            BusPayloadType::RequestCommand,
            payload,
            SerializationEncoding::Json,
        );

        let source = BusMessageSource {
            publisher_id: "PUBLISHER-017".into(),
            instance_id: UUID4::new(),
            stream: "snapshot".into(),
            sequence: 37,
        };

        message.source = Some(source);
        let mut nested = message.clone();
        nested.topic = "external.nested".into();
        nested.source = None;
        let received = Rc::new(RefCell::new(Vec::new()));
        let shared = received.clone();
        subscribe_any(
            nested.topic.as_str().into(),
            ShareableMessageHandler::from_typed::<RequestCommand, _>(move |_| {
                shared.borrow_mut().push(current_external_source());
            }),
            None,
        );

        let result = catch_unwind(AssertUnwindSafe(|| {
            process_external_typed_message(&message, &mut |_, _| {
                assert_eq!(current_external_source(), Some(source));
                republish_external_message(&nested)?;
                assert_eq!(current_external_source(), Some(source));
                assert_eq!(*received.borrow(), vec![None]);

                assert!(!panic, "processor panicked");

                anyhow::bail!("processor rejected payload")
            })
        }));

        if panic {
            assert_eq!(
                result.unwrap_err().downcast_ref::<&str>(),
                Some(&"processor panicked")
            );
        } else {
            assert_eq!(
                result.unwrap().unwrap_err().to_string(),
                "processor rejected payload"
            );
        }

        assert_eq!(current_external_source(), None);
    }

    #[rstest]
    #[case(false, false)]
    #[case(true, false)]
    #[case(false, true)]
    #[case(true, true)]
    fn test_native_response_processors_precede_streaming_filters(
        #[case] streaming: bool,
        #[case] rejected: bool,
    ) {
        use nautilus_core::UUID4;
        use nautilus_model::{data::DataType, identifiers::ClientId};

        use crate::{
            messages::data::CustomDataResponse,
            msgbus::{self, ShareableMessageHandler},
        };

        set_message_bus(Rc::new(RefCell::new(MessageBus::default())));

        if streaming {
            get_message_bus()
                .borrow_mut()
                .add_streaming_type(BusPayloadType::DataResponse);
        }

        let response = CustomDataResponse::new(
            UUID4::new(),
            ClientId::from("FEED"),
            None,
            DataType::new("DistinctReply", None, None),
            Bytes::from_static(b"native-bytes"),
            Some(67.into()),
            Some(71.into()),
            73.into(),
            None,
        );
        let publications = Rc::new(RefCell::new(Vec::new()));
        get_message_bus()
            .borrow_mut()
            .set_external_egress_config(
                Box::new(RecordingEgress {
                    messages: publications.clone(),
                    closed: false,
                }),
                &crate::msgbus::MessageBusConfig {
                    encoding: SerializationEncoding::MsgPack,
                    external_streams: Some(vec!["external.test".into()]),
                    ..Default::default()
                },
            )
            .unwrap();

        publish_any(
            "responses.data.FEED".into(),
            &DataResponse::Data(response.clone()),
        );
        let message = publications.borrow()[0].clone();
        let published = Rc::new(RefCell::new(0));
        let shared = published.clone();
        msgbus::subscribe_any(
            message.topic.as_str().into(),
            ShareableMessageHandler::from_typed::<DataResponse, _>(move |_| {
                *shared.borrow_mut() += 1;
            }),
            None,
        );

        let mut calls = 0;
        let result = process_external_typed_message(&message, &mut |value, mapping| {
            calls += 1;

            let DataResponse::Data(actual) = value.downcast_ref::<DataResponse>().unwrap() else {
                panic!("Expected native custom response");
            };

            assert_eq!(actual.correlation_id, response.correlation_id);
            assert_eq!(
                actual.data.downcast_ref::<Bytes>(),
                response.data.downcast_ref::<Bytes>()
            );
            assert_eq!(
                mapping,
                &serde_json::json!({
                    "payload_type": "DataResponse",
                    "Data": {
                        "correlation_id": response.correlation_id, "client_id": "FEED",
                        "start": 67, "end": 71, "ts_init": 73, "params": null,
                        "venue": null, "data_type": response.data_type,
                        "data": {"Buffer": b"native-bytes".to_vec()},
                    }
                })
            );

            if rejected {
                anyhow::bail!("processor rejected reply");
            }

            Ok(())
        });

        assert_eq!(calls, 1);
        assert_eq!(publications.borrow().len(), 1);
        assert_eq!(message.topic.as_str(), "responses.data.FEED");
        assert_eq!(message.payload_type, BusPayloadType::DataResponse);
        assert_eq!(message.encoding, SerializationEncoding::MsgPack);
        assert_eq!(
            result.err().map(|e| e.to_string()),
            rejected.then(|| "processor rejected reply".to_string())
        );
        assert_eq!(*published.borrow(), usize::from(streaming && !rejected));
    }

    #[rstest]
    fn test_startup_dispatches_only_registered_pending_data_responses() {
        use nautilus_core::UUID4;
        use nautilus_model::{data::DataType, identifiers::ClientId};

        use crate::{
            messages::data::CustomDataResponse,
            msgbus::{self, ShareableMessageHandler},
        };

        let bus = Rc::new(RefCell::new(MessageBus::default()));
        set_message_bus(bus.clone());
        bus.borrow_mut()
            .add_streaming_type(BusPayloadType::DataResponse);
        let id = UUID4::new();
        let response = DataResponse::Data(CustomDataResponse::new(
            id,
            ClientId::from("FEED"),
            None,
            DataType::new("DistinctReply", None, None),
            Bytes::from_static(b"native-bytes"),
            None,
            None,
            73.into(),
            None,
        ));
        let message = BusMessage::with_str_topic(
            "responses.data.FEED",
            BusPayloadType::DataResponse,
            codec::serialize_payload(
                SerializationEncoding::MsgPack,
                BusPayloadType::DataResponse,
                &response,
            )
            .unwrap(),
            SerializationEncoding::MsgPack,
        );

        let topic_handler = ShareableMessageHandler::from_typed::<DataResponse, _>(|reply| {
            msgbus::send_response(reply.correlation_id(), reply);
        });

        msgbus::subscribe_any(message.topic.as_str().into(), topic_handler, None);
        let received = Rc::new(RefCell::new(Vec::new()));
        let shared = received.clone();
        assert!(!republish_pending_external_response(&message).unwrap());
        msgbus::register_response_handler(
            &id,
            TypedHandler::from(move |envelope: &DataResponseEnvelope| {
                let Some(DataResponse::Data(reply)) = envelope.response() else {
                    panic!("expected custom reply");
                };

                shared.borrow_mut().push((
                    reply.correlation_id,
                    reply.data.downcast_ref::<Bytes>().unwrap().clone(),
                    reply.ts_init,
                ));
            }),
        );

        assert!(republish_pending_external_response(&message).unwrap());
        assert!(!republish_pending_external_response(&message).unwrap());
        assert_eq!(
            *received.borrow(),
            vec![(id, Bytes::from_static(b"native-bytes"), 73.into())]
        );
    }

    #[rstest]
    #[case(SerializationEncoding::Json)]
    #[case(SerializationEncoding::MsgPack)]
    fn test_startup_dispatches_payload_free_completion(#[case] encoding: SerializationEncoding) {
        use crate::msgbus::{self, ShareableMessageHandler};

        let bus = Rc::new(RefCell::new(MessageBus::default()));
        set_message_bus(bus.clone());
        bus.borrow_mut()
            .add_streaming_type(BusPayloadType::DataResponseEnvelope);
        let id = nautilus_core::UUID4::new();
        let envelope = DataResponseEnvelope::terminal(id, Some("Source unavailable".to_owned()));
        let payload =
            codec::serialize_payload(encoding, BusPayloadType::DataResponseEnvelope, &envelope)
                .unwrap();
        let message = BusMessage::with_str_topic(
            "responses.data.FEED",
            BusPayloadType::DataResponseEnvelope,
            payload,
            encoding,
        );
        let received = Rc::new(RefCell::new(Vec::new()));
        let shared = received.clone();
        msgbus::subscribe_any(
            message.topic.as_str().into(),
            ShareableMessageHandler::from_typed::<DataResponseEnvelope, _>(
                msgbus::send_response_envelope,
            ),
            None,
        );
        msgbus::register_response_handler(
            &id,
            TypedHandler::from(move |response: &DataResponseEnvelope| {
                shared.borrow_mut().push(response.clone());
            }),
        );

        assert!(republish_pending_external_response(&message).unwrap());

        let received = received.borrow();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].correlation_id(), &id);
        assert!(received[0].completed());
        assert!(received[0].response().is_none());
        assert_eq!(received[0].sub_correlation_id(), None);
        assert_eq!(received[0].failure_reason(), Some("Source unavailable"));
        assert!(bus.borrow().get_response_handler(&id).is_none());
        assert!(!republish_pending_external_response(&message).unwrap());
    }

    fn install_egress() -> Rc<RefCell<Vec<BusMessage>>> {
        let messages = Rc::new(RefCell::new(Vec::new()));
        let mut bus = MessageBus::default();
        bus.set_external_egress(
            Box::new(RecordingEgress {
                messages: messages.clone(),
                closed: false,
            }),
            SerializationEncoding::Json,
        );

        set_message_bus(Rc::new(RefCell::new(bus)));
        messages
    }

    fn assert_message(message: &BusMessage, topic: &str, payload: &[u8]) {
        assert_eq!(message.topic.as_str(), topic);
        assert_eq!(message.payload_type, BusPayloadType::QuoteTick);
        assert_eq!(message.encoding, SerializationEncoding::Json);
        assert_eq!(message.payload.as_ref(), payload);
    }
}
