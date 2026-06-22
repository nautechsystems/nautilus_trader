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

use std::fmt::Debug;

use bytes::Bytes;
use nautilus_core::UUID4;
use nautilus_model::identifiers::TraderId;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{
    config::{ConfigError, ConfigErrorCollector, ConfigResult},
    enums::SerializationEncoding,
};

/// Configuration for `MessageBus` instances.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.common")
)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
pub struct MessageBusConfig {
    /// The encoding for backing operations, controls the type of serializer used.
    #[builder(default = SerializationEncoding::Json)]
    pub encoding: SerializationEncoding,
    /// The encoding for market data payloads supported by the external bus binary codecs.
    pub encoding_market_data: Option<SerializationEncoding>,
    /// The encoding for built-in account, portfolio, order, and position payloads.
    pub encoding_builtin: Option<SerializationEncoding>,
    /// If timestamps should be persisted as ISO 8601 strings.
    /// If `false`, then timestamps will be persisted as UNIX nanoseconds.
    #[builder(default)]
    pub timestamps_as_iso8601: bool,
    /// The buffer interval (milliseconds) between pipelined/batched transactions.
    /// The recommended range if using buffered pipelining is [10, 1000] milliseconds,
    /// with a good compromise being 100 milliseconds.
    pub buffer_interval_ms: Option<u32>,
    /// The lookback window in minutes for automatic stream trimming.
    /// The actual window may extend up to one minute beyond the specified value since streams are trimmed at most once every minute.
    /// This feature requires Redis version 6.2 or higher; otherwise, it will result in a command syntax error.
    pub autotrim_mins: Option<u32>,
    /// If a 'trader-' prefix is used for stream names.
    #[builder(default = true)]
    pub use_trader_prefix: bool,
    /// If the trader's ID is used for stream names.
    #[builder(default = true)]
    pub use_trader_id: bool,
    /// If the trader's instance ID is used for stream names. Default is `false`.
    #[builder(default)]
    pub use_instance_id: bool,
    /// The prefix for externally published stream names. Must have a `backing` config.
    #[builder(default = "stream".to_string())]
    pub streams_prefix: String,
    /// If `true`, messages will be written to separate streams per topic.
    /// If `false`, all messages will be written to the same stream.
    #[builder(default = true)]
    pub stream_per_topic: bool,
    /// The external stream keys the message bus will listen to for publishing deserialized message payloads internally.
    pub external_streams: Option<Vec<String>>,
    /// A list of serializable types **not** to publish externally.
    pub types_filter: Option<Vec<String>>,
    /// The heartbeat interval (seconds).
    pub heartbeat_interval_secs: Option<u16>,
}

impl Default for MessageBusConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl MessageBusConfig {
    /// Validates external message bus encoding policy.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] when the default encoding cannot carry custom payloads, or when
    /// a category override selects an encoding unsupported by any payload type in that category.
    pub fn validate(&self) -> ConfigResult<()> {
        let mut errors = ConfigErrorCollector::new();

        if !super::BusPayloadType::Custom(Ustr::from("Custom")).supports(self.encoding) {
            errors.push(ConfigError::unsupported_value(
                "MessageBusConfig.encoding",
                format!(
                    "{} does not support custom or unmapped payloads",
                    self.encoding
                ),
            ));
        }

        if let Some(encoding) = self.encoding_market_data {
            validate_category_encoding(
                &mut errors,
                "MessageBusConfig.encoding_market_data",
                super::BusPayloadCategory::MarketData,
                encoding,
            );
        }

        if let Some(encoding) = self.encoding_builtin {
            validate_category_encoding(
                &mut errors,
                "MessageBusConfig.encoding_builtin",
                super::BusPayloadCategory::BuiltIn,
                encoding,
            );
        }

        errors.into_result()
    }
}

fn validate_category_encoding(
    errors: &mut ConfigErrorCollector,
    field: &'static str,
    category: super::BusPayloadCategory,
    encoding: SerializationEncoding,
) {
    let unsupported = super::BusPayloadType::PUBLISHED_TYPES
        .iter()
        .copied()
        .filter(|payload_type| payload_type.category() == category)
        .filter(|payload_type| !payload_type.supports(encoding))
        .map(|payload_type| payload_type.as_str().to_string())
        .collect::<Vec<_>>();

    if unsupported.is_empty() {
        return;
    }

    errors.push(ConfigError::unsupported_value(
        field,
        format!(
            "{} is not supported by {}",
            encoding,
            unsupported.join(", ")
        ),
    ));
}

/// External publisher for serialized message bus publications.
///
/// The core bus passes each outbound message as a [`BusMessage`](super::BusMessage) carrying the
/// `topic`, `payload_type`, and serialized `payload`. Implementations must not block the publishing
/// thread. If the underlying publisher is full, drop the message in the implementation rather than
/// applying back-pressure to the node.
pub trait MessageBusPublisher {
    fn is_closed(&self) -> bool;
    fn publish(&self, message: super::BusMessage);
    fn close(&mut self);
}

/// External subscriber for serialized message bus publications.
///
/// The core bus consumes each inbound [`BusMessage`](super::BusMessage) as a transport-neutral
/// `topic` and serialized `payload`. The receiver can be taken only once so subscribers can hand
/// ownership of the inbound stream to the live bridge without exposing their backing transport.
#[cfg(feature = "live")]
pub trait MessageBusSubscriber {
    fn is_closed(&self) -> bool;

    /// # Errors
    ///
    /// Returns an error if the receiver has already been taken or is unavailable.
    fn take_receiver(&mut self) -> anyhow::Result<tokio::sync::mpsc::Receiver<super::BusMessage>>;

    fn close(&mut self);
}

/// Factory for constructing external message bus backings at runtime.
///
/// Implementations own the concrete backing configuration and return the transport-neutral
/// [`MessageBusBacking`] surface used by the core bus.
pub trait MessageBusBackingFactory: Debug + Send + Sync {
    /// Creates a message bus backing for the given bus runtime.
    ///
    /// # Errors
    ///
    /// Returns an error if backing construction or connection setup fails.
    fn create(
        &self,
        trader_id: TraderId,
        instance_id: UUID4,
        config: MessageBusConfig,
    ) -> anyhow::Result<Box<dyn MessageBusBacking>>;
}

/// A generic message bus backing facade.
///
/// Implementations own the concrete backing technology and expose transport-neutral publisher and
/// subscriber surfaces through separate traits.
pub trait MessageBusBacking {
    fn is_closed(&self) -> bool;
    fn publish(&self, topic: Ustr, payload: Bytes);
    fn close(&mut self);
}

#[cfg(test)]
mod tests {
    use rstest::*;
    use serde_json::json;

    use super::*;
    #[cfg(feature = "live")]
    use crate::msgbus::{
        BusMessage, BusPayloadType, MessageBusSubscriber as ReexportedMessageBusSubscriber,
    };

    #[cfg(feature = "live")]
    struct CapturingSubscriber {
        rx: Option<tokio::sync::mpsc::Receiver<BusMessage>>,
        closed: bool,
    }

    #[cfg(feature = "live")]
    impl ReexportedMessageBusSubscriber for CapturingSubscriber {
        fn is_closed(&self) -> bool {
            self.closed
        }

        fn take_receiver(&mut self) -> anyhow::Result<tokio::sync::mpsc::Receiver<BusMessage>> {
            self.rx
                .take()
                .ok_or_else(|| anyhow::anyhow!("Stream receiver already taken"))
        }

        fn close(&mut self) {
            self.closed = true;
        }
    }

    #[cfg(feature = "live")]
    #[rstest]
    fn test_message_bus_subscriber_reexport_accepts_bus_messages() {
        let (tx, rx) = tokio::sync::mpsc::channel::<BusMessage>(1);
        let mut subscriber = CapturingSubscriber {
            rx: Some(rx),
            closed: false,
        };
        let message = BusMessage::with_str_topic(
            "events/data",
            SerializationEncoding::Json,
            BusPayloadType::QuoteTick,
            Bytes::from_static(b"payload"),
        );

        tx.try_send(message.clone()).unwrap();
        let mut stream_rx = ReexportedMessageBusSubscriber::take_receiver(&mut subscriber).unwrap();
        let received = stream_rx.try_recv().unwrap();

        assert_eq!(received.topic, message.topic);
        assert_eq!(received.payload, message.payload);
        assert!(ReexportedMessageBusSubscriber::take_receiver(&mut subscriber).is_err());

        ReexportedMessageBusSubscriber::close(&mut subscriber);
        assert!(ReexportedMessageBusSubscriber::is_closed(&subscriber));
    }

    #[rstest]
    fn test_default_message_bus_config() {
        let config = MessageBusConfig::default();
        assert_eq!(config.encoding, SerializationEncoding::Json);
        assert_eq!(config.encoding_market_data, None);
        assert_eq!(config.encoding_builtin, None);
        assert!(!config.timestamps_as_iso8601);
        assert_eq!(config.buffer_interval_ms, None);
        assert_eq!(config.autotrim_mins, None);
        assert!(config.use_trader_prefix);
        assert!(config.use_trader_id);
        assert!(!config.use_instance_id);
        assert_eq!(config.streams_prefix, "stream");
        assert!(config.stream_per_topic);
        assert_eq!(config.external_streams, None);
        assert_eq!(config.types_filter, None);
    }

    #[rstest]
    fn test_deserialize_message_bus_config() {
        let config_json = json!({
            "encoding": "json",
            "encoding_market_data": "sbe",
            "encoding_builtin": "msgpack",
            "timestamps_as_iso8601": true,
            "buffer_interval_ms": 100,
            "autotrim_mins": 60,
            "use_trader_prefix": false,
            "use_trader_id": false,
            "use_instance_id": true,
            "streams_prefix": "data_streams",
            "stream_per_topic": false,
            "external_streams": ["stream1", "stream2"],
            "types_filter": ["type1", "type2"]
        });
        let config: MessageBusConfig = serde_json::from_value(config_json).unwrap();
        assert_eq!(config.encoding, SerializationEncoding::Json);
        assert_eq!(
            config.encoding_market_data,
            Some(SerializationEncoding::Sbe)
        );
        assert_eq!(
            config.encoding_builtin,
            Some(SerializationEncoding::MsgPack)
        );
        assert!(config.timestamps_as_iso8601);
        assert_eq!(config.buffer_interval_ms, Some(100));
        assert_eq!(config.autotrim_mins, Some(60));
        assert!(!config.use_trader_prefix);
        assert!(!config.use_trader_id);
        assert!(config.use_instance_id);
        assert_eq!(config.streams_prefix, "data_streams");
        assert!(!config.stream_per_topic);
        assert_eq!(
            config.external_streams,
            Some(vec!["stream1".to_string(), "stream2".to_string()])
        );
        assert_eq!(
            config.types_filter,
            Some(vec!["type1".to_string(), "type2".to_string()])
        );
    }

    #[rstest]
    fn test_deserialize_message_bus_config_rejects_backing_field() {
        let config_json = json!({
            "backing": {},
        });

        let error = serde_json::from_value::<MessageBusConfig>(config_json).unwrap_err();
        assert!(error.to_string().contains("unknown field `backing`"));
    }

    #[rstest]
    #[case("sbe", SerializationEncoding::Sbe)]
    #[case("capnp", SerializationEncoding::Capnp)]
    fn test_deserialize_message_bus_config_with_schema_encoding(
        #[case] encoding_name: &str,
        #[case] expected: SerializationEncoding,
    ) {
        let config_json = json!({
            "encoding": encoding_name,
        });

        let config: MessageBusConfig = serde_json::from_value(config_json).unwrap();
        assert_eq!(config.encoding, expected);
    }

    #[rstest]
    fn message_bus_config_validate_accepts_default() {
        let config = MessageBusConfig::default();

        assert!(config.validate().is_ok());
    }

    #[rstest]
    #[case(SerializationEncoding::Json)]
    #[case(SerializationEncoding::MsgPack)]
    fn message_bus_config_validate_accepts_custom_safe_default(
        #[case] encoding: SerializationEncoding,
    ) {
        let config = MessageBusConfig {
            encoding,
            ..Default::default()
        };

        assert!(config.validate().is_ok());
    }

    #[rstest]
    #[case(SerializationEncoding::Sbe)]
    #[case(SerializationEncoding::Capnp)]
    fn message_bus_config_validate_rejects_schema_default(#[case] encoding: SerializationEncoding) {
        let config = MessageBusConfig {
            encoding,
            ..Default::default()
        };

        let error = config.validate().unwrap_err();
        assert!(matches!(
            error,
            ConfigError::UnsupportedValue { field, .. }
                if field == "MessageBusConfig.encoding"
        ));
    }

    #[cfg(any(feature = "sbe", feature = "capnp"))]
    #[rstest]
    #[cfg_attr(feature = "sbe", case(SerializationEncoding::Sbe))]
    #[cfg_attr(feature = "capnp", case(SerializationEncoding::Capnp))]
    fn message_bus_config_validate_accepts_market_data_override(
        #[case] encoding: SerializationEncoding,
    ) {
        let config = MessageBusConfig {
            encoding_market_data: Some(encoding),
            ..Default::default()
        };

        assert!(config.validate().is_ok());
    }

    #[cfg(not(feature = "sbe"))]
    #[rstest]
    fn message_bus_config_validate_rejects_market_data_sbe_without_feature() {
        let config = MessageBusConfig {
            encoding_market_data: Some(SerializationEncoding::Sbe),
            ..Default::default()
        };

        let error = config.validate().unwrap_err();
        assert!(matches!(
            error,
            ConfigError::UnsupportedValue { field, .. }
                if field == "MessageBusConfig.encoding_market_data"
        ));
    }

    #[cfg(not(feature = "capnp"))]
    #[rstest]
    fn message_bus_config_validate_rejects_market_data_capnp_without_feature() {
        let config = MessageBusConfig {
            encoding_market_data: Some(SerializationEncoding::Capnp),
            ..Default::default()
        };

        let error = config.validate().unwrap_err();
        assert!(matches!(
            error,
            ConfigError::UnsupportedValue { field, .. }
                if field == "MessageBusConfig.encoding_market_data"
        ));
    }

    #[rstest]
    #[case(SerializationEncoding::Sbe)]
    #[case(SerializationEncoding::Capnp)]
    fn message_bus_config_validate_rejects_builtin_schema_override(
        #[case] encoding: SerializationEncoding,
    ) {
        let config = MessageBusConfig {
            encoding_builtin: Some(encoding),
            ..Default::default()
        };

        let error = config.validate().unwrap_err();
        assert!(matches!(
            error,
            ConfigError::UnsupportedValue { field, .. }
                if field == "MessageBusConfig.encoding_builtin"
        ));
    }
}
