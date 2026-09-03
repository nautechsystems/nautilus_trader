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

use std::fmt::Display;

use bytes::Bytes;
use serde::{Deserialize, Serialize, de::Error as _};
use ustr::Ustr;

use super::switchboard::CLOSE_TOPIC;
use crate::enums::SerializationEncoding;

/// External message bus payload category used to select category-level encodings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BusPayloadCategory {
    MarketData,
    BuiltIn,
    Other,
}

/// The payload type carried by a [`BusMessage`].
///
/// The fixed variants cover every type the bus publishes externally; [`BusPayloadType::Custom`]
/// carries the user-defined type name for arbitrary custom data. Untagged variants serialize as
/// flat name strings such as `"QuoteTick"`. Typed message variants serialize as
/// `{"typed":"<name>"}` so they remain distinct from custom payloads with the same name. Redis
/// carries the discriminator as `payload_kind=typed` next to its flat `type` field. Without it,
/// [`BusPayloadType::from_name`] resolves a typed name as [`BusPayloadType::Custom`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BusPayloadType {
    /// User-defined custom data, identified by its type name.
    Custom(Ustr),
    Instrument,
    OrderBookDeltas,
    OrderBookDepth10,
    QuoteTick,
    TradeTick,
    Bar,
    MarkPriceUpdate,
    IndexPriceUpdate,
    FundingRateUpdate,
    OptionGreeks,
    AccountState,
    OrderEvent,
    PositionEvent,
    PortfolioSnapshot,
    SubscribeCommand,
    UnsubscribeCommand,
    TradingCommand,
    GenerateExecutionMassStatus,
    OrderStatusReport,
    FillReport,
    PositionStatusReport,
    ExecutionMassStatus,
    #[cfg(feature = "defi")]
    Block,
    #[cfg(feature = "defi")]
    Pool,
    #[cfg(feature = "defi")]
    PoolLiquidityUpdate,
    #[cfg(feature = "defi")]
    PoolFeeCollect,
    #[cfg(feature = "defi")]
    PoolFlash,
}

impl BusPayloadType {
    pub(crate) const PUBLISHED_TYPES: &'static [Self] = &[
        Self::Instrument,
        Self::OrderBookDeltas,
        Self::OrderBookDepth10,
        Self::QuoteTick,
        Self::TradeTick,
        Self::Bar,
        Self::MarkPriceUpdate,
        Self::IndexPriceUpdate,
        Self::FundingRateUpdate,
        Self::OptionGreeks,
        Self::AccountState,
        Self::OrderEvent,
        Self::PositionEvent,
        Self::PortfolioSnapshot,
        Self::SubscribeCommand,
        Self::UnsubscribeCommand,
        Self::TradingCommand,
        Self::GenerateExecutionMassStatus,
        Self::OrderStatusReport,
        Self::FillReport,
        Self::PositionStatusReport,
        Self::ExecutionMassStatus,
        #[cfg(feature = "defi")]
        Self::Block,
        #[cfg(feature = "defi")]
        Self::Pool,
        #[cfg(feature = "defi")]
        Self::PoolLiquidityUpdate,
        #[cfg(feature = "defi")]
        Self::PoolFeeCollect,
        #[cfg(feature = "defi")]
        Self::PoolFlash,
    ];

    /// Returns the canonical type name for this payload type.
    ///
    /// The name alone does not identify variants for which [`Self::is_typed_message`] returns
    /// `true`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Custom(type_name) => type_name.as_str(),
            Self::Instrument => "InstrumentAny",
            Self::OrderBookDeltas => "OrderBookDeltas",
            Self::OrderBookDepth10 => "OrderBookDepth10",
            Self::QuoteTick => "QuoteTick",
            Self::TradeTick => "TradeTick",
            Self::Bar => "Bar",
            Self::MarkPriceUpdate => "MarkPriceUpdate",
            Self::IndexPriceUpdate => "IndexPriceUpdate",
            Self::FundingRateUpdate => "FundingRateUpdate",
            Self::OptionGreeks => "OptionGreeks",
            Self::AccountState => "AccountState",
            Self::OrderEvent => "OrderEventAny",
            Self::PositionEvent => "PositionEvent",
            Self::PortfolioSnapshot => "PortfolioSnapshot",
            Self::SubscribeCommand => "SubscribeCommand",
            Self::UnsubscribeCommand => "UnsubscribeCommand",
            Self::TradingCommand => "TradingCommand",
            Self::GenerateExecutionMassStatus => "GenerateExecutionMassStatus",
            Self::OrderStatusReport => "OrderStatusReport",
            Self::FillReport => "FillReport",
            Self::PositionStatusReport => "PositionStatusReport",
            Self::ExecutionMassStatus => "ExecutionMassStatus",
            #[cfg(feature = "defi")]
            Self::Block => "Block",
            #[cfg(feature = "defi")]
            Self::Pool => "Pool",
            #[cfg(feature = "defi")]
            Self::PoolLiquidityUpdate => "PoolLiquidityUpdate",
            #[cfg(feature = "defi")]
            Self::PoolFeeCollect => "PoolFeeCollect",
            #[cfg(feature = "defi")]
            Self::PoolFlash => "PoolFlash",
        }
    }

    /// Resolves an untagged canonical type name to a [`BusPayloadType`].
    ///
    /// Typed message and unknown names resolve to [`BusPayloadType::Custom`]. Use
    /// [`BusPayloadType::from_typed_name`] when a separate discriminator identifies a typed
    /// payload.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name {
            "InstrumentAny" => Self::Instrument,
            "OrderBookDeltas" => Self::OrderBookDeltas,
            "OrderBookDepth10" => Self::OrderBookDepth10,
            "QuoteTick" => Self::QuoteTick,
            "TradeTick" => Self::TradeTick,
            "Bar" => Self::Bar,
            "MarkPriceUpdate" => Self::MarkPriceUpdate,
            "IndexPriceUpdate" => Self::IndexPriceUpdate,
            "FundingRateUpdate" => Self::FundingRateUpdate,
            "OptionGreeks" => Self::OptionGreeks,
            "AccountState" => Self::AccountState,
            "OrderEventAny" => Self::OrderEvent,
            "PositionEvent" => Self::PositionEvent,
            "PortfolioSnapshot" => Self::PortfolioSnapshot,
            #[cfg(feature = "defi")]
            "Block" => Self::Block,
            #[cfg(feature = "defi")]
            "Pool" => Self::Pool,
            #[cfg(feature = "defi")]
            "PoolLiquidityUpdate" => Self::PoolLiquidityUpdate,
            #[cfg(feature = "defi")]
            "PoolFeeCollect" => Self::PoolFeeCollect,
            #[cfg(feature = "defi")]
            "PoolFlash" => Self::PoolFlash,
            other => Self::Custom(Ustr::from(other)),
        }
    }

    /// Resolves a discriminated typed name to its fixed payload type.
    ///
    /// Returns `None` when the name does not identify a typed message variant.
    #[must_use]
    pub fn from_typed_name(name: &str) -> Option<Self> {
        match name {
            "SubscribeCommand" => Some(Self::SubscribeCommand),
            "UnsubscribeCommand" => Some(Self::UnsubscribeCommand),
            "TradingCommand" => Some(Self::TradingCommand),
            "GenerateExecutionMassStatus" => Some(Self::GenerateExecutionMassStatus),
            "OrderStatusReport" => Some(Self::OrderStatusReport),
            "FillReport" => Some(Self::FillReport),
            "PositionStatusReport" => Some(Self::PositionStatusReport),
            "ExecutionMassStatus" => Some(Self::ExecutionMassStatus),
            _ => None,
        }
    }

    /// Returns whether this payload type uses the typed discriminator.
    #[must_use]
    pub const fn is_typed_message(self) -> bool {
        matches!(
            self,
            Self::SubscribeCommand
                | Self::UnsubscribeCommand
                | Self::TradingCommand
                | Self::GenerateExecutionMassStatus
                | Self::OrderStatusReport
                | Self::FillReport
                | Self::PositionStatusReport
                | Self::ExecutionMassStatus
        )
    }

    /// Returns the encoding policy category for this payload type.
    #[must_use]
    pub fn category(&self) -> BusPayloadCategory {
        if self.has_bus_binary_schema() {
            BusPayloadCategory::MarketData
        } else {
            match self {
                Self::AccountState
                | Self::OrderEvent
                | Self::PositionEvent
                | Self::PortfolioSnapshot => BusPayloadCategory::BuiltIn,
                _ => BusPayloadCategory::Other,
            }
        }
    }

    /// Returns whether this payload type supports the given bus serialization encoding.
    #[must_use]
    pub fn supports(&self, encoding: SerializationEncoding) -> bool {
        match encoding {
            SerializationEncoding::Json | SerializationEncoding::MsgPack => true,
            SerializationEncoding::Sbe => cfg!(feature = "sbe") && self.has_bus_binary_schema(),
            SerializationEncoding::Capnp => cfg!(feature = "capnp") && self.has_bus_binary_schema(),
        }
    }

    fn has_bus_binary_schema(&self) -> bool {
        matches!(
            self,
            Self::OrderBookDeltas
                | Self::OrderBookDepth10
                | Self::QuoteTick
                | Self::TradeTick
                | Self::Bar
                | Self::MarkPriceUpdate
                | Self::IndexPriceUpdate
                | Self::FundingRateUpdate
                | Self::OptionGreeks
        )
    }
}

impl Display for BusPayloadType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for BusPayloadType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.is_typed_message() {
            TypedPayloadTypeRef {
                typed: self.as_str(),
            }
            .serialize(serializer)
        } else {
            serializer.serialize_str(self.as_str())
        }
    }
}

impl<'de> Deserialize<'de> for BusPayloadType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match BusPayloadTypeRepr::deserialize(deserializer)? {
            BusPayloadTypeRepr::Name(name) => Ok(Self::from_name(&name)),
            BusPayloadTypeRepr::Typed(value) => Self::from_typed_name(&value.typed)
                .ok_or_else(|| D::Error::custom(format!("unknown typed payload: {}", value.typed))),
        }
    }
}

#[derive(Serialize)]
struct TypedPayloadTypeRef<'a> {
    typed: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypedPayloadType {
    typed: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BusPayloadTypeRepr {
    Name(String),
    Typed(TypedPayloadType),
}

/// Represents a bus message including a topic and serialized payload.
///
/// Control messages (such as `CLOSE`) that carry no typed payload use an empty
/// [`BusPayloadType::Custom`].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.common", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.common")
)]
pub struct BusMessage {
    /// The topic to publish the message on.
    pub topic: Ustr,
    /// The payload type, carried out-of-band so the receiver can dispatch the serialized payload
    /// without parsing the topic or inspecting the bytes.
    pub payload_type: BusPayloadType,
    /// The serialized payload for the message.
    pub payload: Bytes,
    /// The encoding the `payload` is serialized with, so the receiver can decode it without
    /// relying on its own configuration (mirrors a wire `content-type`).
    pub encoding: SerializationEncoding,
}

impl BusMessage {
    /// Creates a new [`BusMessage`] instance.
    pub fn new(
        topic: Ustr,
        payload_type: BusPayloadType,
        payload: Bytes,
        encoding: SerializationEncoding,
    ) -> Self {
        debug_assert!(!topic.is_empty());
        Self {
            topic,
            payload_type,
            payload,
            encoding,
        }
    }

    /// Creates a new [`BusMessage`] instance with a string-like topic.
    ///
    /// This is a convenience constructor that converts any string-like type
    /// (implementing `AsRef<str>`) into the required `Ustr` type.
    pub fn with_str_topic<T: AsRef<str>>(
        topic: T,
        payload_type: BusPayloadType,
        payload: Bytes,
        encoding: SerializationEncoding,
    ) -> Self {
        Self::new(Ustr::from(topic.as_ref()), payload_type, payload, encoding)
    }

    /// Creates a new [`BusMessage`] instance with the `CLOSE` topic and empty payload.
    pub fn new_close() -> Self {
        Self::with_str_topic(
            CLOSE_TOPIC,
            BusPayloadType::Custom(Ustr::default()),
            Bytes::new(),
            SerializationEncoding::default(),
        )
    }
}

impl Display for BusMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {} {} {}",
            self.topic,
            self.payload_type.as_str(),
            String::from_utf8_lossy(&self.payload),
            self.encoding
        )
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("test/topic", "payload data")]
    #[case("events/trading", "Another payload")]
    fn test_with_str_topic_str(#[case] topic: &str, #[case] payload_str: &str) {
        let payload = Bytes::from(payload_str.to_string());

        let message = BusMessage::with_str_topic(
            topic,
            BusPayloadType::QuoteTick,
            payload.clone(),
            SerializationEncoding::Json,
        );

        assert_eq!(message.topic.as_str(), topic);
        assert_eq!(message.payload_type, BusPayloadType::QuoteTick);
        assert_eq!(message.encoding, SerializationEncoding::Json);
        assert_eq!(message.payload, payload);
    }

    #[rstest]
    fn test_with_str_topic_string() {
        let topic_string = String::from("orders/new");
        let payload = Bytes::from("order payload data");

        let message = BusMessage::with_str_topic(
            topic_string.clone(),
            BusPayloadType::OrderEvent,
            payload.clone(),
            SerializationEncoding::MsgPack,
        );

        assert_eq!(message.topic.as_str(), topic_string);
        assert_eq!(message.payload_type, BusPayloadType::OrderEvent);
        assert_eq!(message.encoding, SerializationEncoding::MsgPack);
        assert_eq!(message.payload, payload);
    }

    #[rstest]
    fn test_new_close() {
        let message = BusMessage::new_close();

        assert_eq!(message.topic.as_str(), "CLOSE");
        assert!(message.payload.is_empty());
    }

    #[rstest]
    #[case(BusPayloadType::QuoteTick, BusPayloadCategory::MarketData)]
    #[case(BusPayloadType::OrderBookDeltas, BusPayloadCategory::MarketData)]
    #[case(BusPayloadType::AccountState, BusPayloadCategory::BuiltIn)]
    #[case(BusPayloadType::OrderEvent, BusPayloadCategory::BuiltIn)]
    #[case(BusPayloadType::Instrument, BusPayloadCategory::Other)]
    #[case(BusPayloadType::OptionGreeks, BusPayloadCategory::MarketData)]
    #[case(
        BusPayloadType::Custom(Ustr::from("CustomPayload")),
        BusPayloadCategory::Other
    )]
    fn bus_payload_type_category(
        #[case] payload_type: BusPayloadType,
        #[case] expected: BusPayloadCategory,
    ) {
        assert_eq!(payload_type.category(), expected);
    }

    #[rstest]
    #[case(BusPayloadType::QuoteTick, SerializationEncoding::Json, true)]
    #[case(BusPayloadType::QuoteTick, SerializationEncoding::MsgPack, true)]
    #[cfg_attr(
        feature = "sbe",
        case(BusPayloadType::QuoteTick, SerializationEncoding::Sbe, true)
    )]
    #[cfg_attr(
        not(feature = "sbe"),
        case(BusPayloadType::QuoteTick, SerializationEncoding::Sbe, false)
    )]
    #[cfg_attr(
        feature = "capnp",
        case(BusPayloadType::QuoteTick, SerializationEncoding::Capnp, true)
    )]
    #[cfg_attr(
        not(feature = "capnp"),
        case(BusPayloadType::QuoteTick, SerializationEncoding::Capnp, false)
    )]
    #[case(BusPayloadType::AccountState, SerializationEncoding::Json, true)]
    #[case(BusPayloadType::AccountState, SerializationEncoding::Capnp, false)]
    #[case(BusPayloadType::Instrument, SerializationEncoding::Sbe, false)]
    #[cfg_attr(
        feature = "sbe",
        case(BusPayloadType::OptionGreeks, SerializationEncoding::Sbe, true)
    )]
    #[cfg_attr(
        not(feature = "sbe"),
        case(BusPayloadType::OptionGreeks, SerializationEncoding::Sbe, false)
    )]
    #[cfg_attr(
        feature = "capnp",
        case(BusPayloadType::OptionGreeks, SerializationEncoding::Capnp, true)
    )]
    #[cfg_attr(
        not(feature = "capnp"),
        case(BusPayloadType::OptionGreeks, SerializationEncoding::Capnp, false)
    )]
    #[case(
        BusPayloadType::Custom(Ustr::from("CustomPayload")),
        SerializationEncoding::MsgPack,
        true
    )]
    #[case(
        BusPayloadType::Custom(Ustr::from("CustomPayload")),
        SerializationEncoding::Sbe,
        false
    )]
    fn bus_payload_type_supports_encoding(
        #[case] payload_type: BusPayloadType,
        #[case] encoding: SerializationEncoding,
        #[case] expected: bool,
    ) {
        assert_eq!(payload_type.supports(encoding), expected);
    }

    #[rstest]
    fn typed_payload_types_use_json_msgpack_policy() {
        let payload_types = [
            (BusPayloadType::SubscribeCommand, "SubscribeCommand"),
            (BusPayloadType::UnsubscribeCommand, "UnsubscribeCommand"),
            (BusPayloadType::TradingCommand, "TradingCommand"),
            (
                BusPayloadType::GenerateExecutionMassStatus,
                "GenerateExecutionMassStatus",
            ),
            (BusPayloadType::OrderStatusReport, "OrderStatusReport"),
            (BusPayloadType::FillReport, "FillReport"),
            (BusPayloadType::PositionStatusReport, "PositionStatusReport"),
            (BusPayloadType::ExecutionMassStatus, "ExecutionMassStatus"),
        ];

        for (payload_type, name) in payload_types {
            let custom_payload_type = BusPayloadType::Custom(Ustr::from(name));
            let typed_json =
                serde_json::to_value(payload_type).expect("typed payload type must serialize");
            let custom_json = serde_json::to_value(custom_payload_type)
                .expect("custom payload type must serialize");

            assert_eq!(payload_type.as_str(), name);
            assert_eq!(BusPayloadType::from_name(name), custom_payload_type);
            assert_eq!(BusPayloadType::from_typed_name(name), Some(payload_type));
            assert_eq!(typed_json, serde_json::json!({ "typed": name }));
            assert_eq!(custom_json, serde_json::json!(name));
            assert_eq!(
                serde_json::from_value::<BusPayloadType>(typed_json)
                    .expect("typed payload type must deserialize"),
                payload_type
            );
            assert_eq!(
                serde_json::from_value::<BusPayloadType>(custom_json)
                    .expect("custom payload type must deserialize"),
                custom_payload_type
            );
            assert!(payload_type.is_typed_message());
            assert!(!custom_payload_type.is_typed_message());
            assert_eq!(payload_type.category(), BusPayloadCategory::Other);
            assert!(payload_type.supports(SerializationEncoding::Json));
            assert!(payload_type.supports(SerializationEncoding::MsgPack));
            assert!(!payload_type.supports(SerializationEncoding::Sbe));
            assert!(!payload_type.supports(SerializationEncoding::Capnp));
        }
    }
}
