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
    BusMessage, BusPayloadType, HAS_EXTERNAL_EGRESS, SUPPRESS_EXTERNAL_DEPTH,
    SuppressExternalGuard,
    api::{
        publish_account_state, publish_any, publish_bar, publish_deltas, publish_depth10,
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
        data::{SubscribeCommand, UnsubscribeCommand},
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

    let bus_rc = get_message_bus();
    let bus = bus_rc.borrow();
    let Some(external_egress) = bus
        .external_egress()
        .filter(|external_egress| !external_egress.is_closed())
    else {
        return;
    };

    if payload_type.is_typed_message() && !bus.has_external_streams() {
        return;
    }

    if bus.types_filter().contains(&payload_type) {
        return;
    }

    let encoding = bus.encoding_for(payload_type);

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

    // Build after drop checks to avoid allocating discarded external messages
    external_egress.publish(BusMessage::new(*topic, payload_type, payload, encoding));
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
/// type/encoding pairs are skipped with a warning.
pub fn republish_external_message(message: &BusMessage) -> anyhow::Result<()> {
    let topic =
        MStr::<Topic>::topic_from_ustr(message.topic).context("invalid external message topic")?;

    if !is_registered_streaming_type(message) {
        return Ok(());
    }

    let _guard = SuppressExternalGuard::new();

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
            handle_json_msgpack(
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
        BusPayloadType::OrderBookDepth10 => handle_market_data(
            topic,
            message.encoding,
            &message.payload,
            codec::deserialize_order_book_depth10,
            publish_depth10,
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
            handle_json_msgpack(
                topic,
                message.payload_type,
                message.encoding,
                &message.payload,
                publish_account_state,
            )?;
        }
        BusPayloadType::OrderEvent => {
            handle_json_msgpack(
                topic,
                message.payload_type,
                message.encoding,
                &message.payload,
                publish_order_event,
            )?;
        }
        BusPayloadType::PositionEvent => {
            handle_json_msgpack(
                topic,
                message.payload_type,
                message.encoding,
                &message.payload,
                publish_position_event,
            )?;
        }
        BusPayloadType::PortfolioSnapshot => {
            handle_json_msgpack(
                topic,
                message.payload_type,
                message.encoding,
                &message.payload,
                publish_portfolio_snapshot,
            )?;
        }
        BusPayloadType::SubscribeCommand => {
            handle_json_msgpack_any::<SubscribeCommand>(topic, message)?;
        }
        BusPayloadType::UnsubscribeCommand => {
            handle_json_msgpack_any::<UnsubscribeCommand>(topic, message)?;
        }
        BusPayloadType::TradingCommand => {
            handle_json_msgpack_any::<TradingCommand>(topic, message)?;
        }
        BusPayloadType::GenerateExecutionMassStatus => {
            handle_json_msgpack_any::<GenerateExecutionMassStatus>(topic, message)?;
        }
        BusPayloadType::OrderStatusReport => {
            handle_json_msgpack_any::<OrderStatusReport>(topic, message)?;
        }
        BusPayloadType::FillReport => {
            handle_json_msgpack_any::<FillReport>(topic, message)?;
        }
        BusPayloadType::PositionStatusReport => {
            handle_json_msgpack_any::<PositionStatusReport>(topic, message)?;
        }
        BusPayloadType::ExecutionMassStatus => {
            handle_json_msgpack_any::<ExecutionMassStatus>(topic, message)?;
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
/// JSON or MessagePack payloads identified by [`BusPayloadType::is_typed_message`] reach
/// `processor` regardless of internal streaming registration. Internal republishing still requires
/// registration. Unsupported type/encoding pairs are skipped with a warning. The processor mapping
/// includes a `payload_type` field containing the [`BusPayloadType`] name. External egress is
/// suppressed for the duration of processing, including processor callbacks. Other payloads bypass
/// the processor and follow normal inbound republishing. A processor error stops processing and
/// skips internal republishing.
///
/// # Errors
///
/// Returns an error if:
/// - The topic is invalid.
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

    match message.payload_type {
        BusPayloadType::SubscribeCommand => {
            process_typed_payload::<SubscribeCommand>(topic, message, processor)
        }
        BusPayloadType::UnsubscribeCommand => {
            process_typed_payload::<UnsubscribeCommand>(topic, message, processor)
        }
        BusPayloadType::TradingCommand => {
            process_typed_payload::<TradingCommand>(topic, message, processor)
        }
        BusPayloadType::GenerateExecutionMassStatus => {
            process_typed_payload::<GenerateExecutionMassStatus>(topic, message, processor)
        }
        BusPayloadType::OrderStatusReport => {
            process_typed_payload::<OrderStatusReport>(topic, message, processor)
        }
        BusPayloadType::FillReport => {
            process_typed_payload::<FillReport>(topic, message, processor)
        }
        BusPayloadType::PositionStatusReport => {
            process_typed_payload::<PositionStatusReport>(topic, message, processor)
        }
        BusPayloadType::ExecutionMassStatus => {
            process_typed_payload::<ExecutionMassStatus>(topic, message, processor)
        }
        _ => republish_external_message(message),
    }
}

fn handle_json_msgpack_any<T>(topic: MStr<Topic>, message: &BusMessage) -> anyhow::Result<()>
where
    T: DeserializeOwned + Any,
{
    handle_json_msgpack(
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
    let Some(value) = codec::deserialize_json_msgpack_payload::<T>(
        message.payload_type,
        message.encoding,
        &message.payload,
    )?
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

pub(crate) fn handle_json_msgpack<T>(
    topic: MStr<Topic>,
    payload_type: BusPayloadType,
    encoding: SerializationEncoding,
    payload: &[u8],
    publish: impl FnOnce(MStr<Topic>, &T),
) -> anyhow::Result<()>
where
    T: DeserializeOwned,
{
    let Some(value) = codec::deserialize_json_msgpack_payload(payload_type, encoding, payload)?
    else {
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
        SerializationEncoding::Sbe | SerializationEncoding::Capnp => {
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
