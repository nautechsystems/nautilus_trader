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

//! External message bus payload codecs.

use std::any::Any;

use bytes::Bytes;
use nautilus_model::data::{
    Bar, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate, OptionGreeks, OrderBookDeltas,
    OrderBookDepth10, QuoteTick, TradeTick,
};
use serde::de::DeserializeOwned;

use super::super::BusPayloadType;
use crate::enums::SerializationEncoding;

mod json;
mod msgpack;

#[cfg(feature = "capnp")]
mod capnp;
#[cfg(not(feature = "capnp"))]
#[path = "capnp_unavailable.rs"]
mod capnp;

#[cfg(feature = "sbe")]
mod sbe;
#[cfg(not(feature = "sbe"))]
#[path = "sbe_unavailable.rs"]
mod sbe;

#[derive(Debug)]
pub(super) enum PayloadCodecError {
    Dropped(String),
    Failed(String),
}

pub(super) fn serialize_payload<T>(
    encoding: SerializationEncoding,
    payload_type: BusPayloadType,
    message: &T,
) -> Result<Bytes, PayloadCodecError>
where
    T: serde::Serialize + Any,
{
    let type_name = payload_type.as_str();

    match encoding {
        SerializationEncoding::Json => json::serialize(message, type_name),
        SerializationEncoding::MsgPack => msgpack::serialize(message, type_name),
        SerializationEncoding::Capnp => capnp::serialize_payload(payload_type, message),
        SerializationEncoding::Sbe => sbe::serialize_payload(payload_type, message),
    }
}

pub(crate) fn deserialize_json_msgpack_payload<T>(
    payload_type: BusPayloadType,
    encoding: SerializationEncoding,
    payload: &[u8],
) -> anyhow::Result<Option<T>>
where
    T: DeserializeOwned,
{
    if !payload_type.supports(encoding) {
        warn_unsupported_inbound(payload_type, encoding);
        return Ok(None);
    }

    let type_name = payload_type.as_str();
    match encoding {
        SerializationEncoding::Json => deserialize_json_payload(payload, type_name).map(Some),
        SerializationEncoding::MsgPack => deserialize_msgpack_payload(payload, type_name).map(Some),
        SerializationEncoding::Sbe | SerializationEncoding::Capnp => {
            warn_unsupported_inbound(payload_type, encoding);
            Ok(None)
        }
    }
}

macro_rules! define_market_data_deserializer {
    ($fn_name:ident, $payload_type:ident, $ty:ty) => {
        pub(super) fn $fn_name(
            encoding: SerializationEncoding,
            payload: &[u8],
        ) -> anyhow::Result<Option<$ty>> {
            deserialize_market_data_payload(
                BusPayloadType::$payload_type,
                encoding,
                payload,
                sbe::$fn_name,
                capnp::$fn_name,
            )
        }
    };
}

define_market_data_deserializer!(
    deserialize_order_book_deltas,
    OrderBookDeltas,
    OrderBookDeltas
);
define_market_data_deserializer!(
    deserialize_order_book_depth10,
    OrderBookDepth10,
    OrderBookDepth10
);
define_market_data_deserializer!(deserialize_quote, QuoteTick, QuoteTick);
define_market_data_deserializer!(deserialize_trade, TradeTick, TradeTick);
define_market_data_deserializer!(deserialize_bar, Bar, Bar);
define_market_data_deserializer!(deserialize_mark_price, MarkPriceUpdate, MarkPriceUpdate);
define_market_data_deserializer!(deserialize_index_price, IndexPriceUpdate, IndexPriceUpdate);
define_market_data_deserializer!(
    deserialize_funding_rate,
    FundingRateUpdate,
    FundingRateUpdate
);
define_market_data_deserializer!(deserialize_option_greeks, OptionGreeks, OptionGreeks);

fn deserialize_market_data_payload<T>(
    payload_type: BusPayloadType,
    encoding: SerializationEncoding,
    payload: &[u8],
    deserialize_sbe: fn(&[u8]) -> anyhow::Result<T>,
    deserialize_capnp: fn(&[u8]) -> anyhow::Result<T>,
) -> anyhow::Result<Option<T>>
where
    T: DeserializeOwned,
{
    if !payload_type.supports(encoding) {
        warn_unsupported_inbound(payload_type, encoding);
        return Ok(None);
    }

    let type_name = payload_type.as_str();
    match encoding {
        SerializationEncoding::Json => deserialize_json_payload(payload, type_name).map(Some),
        SerializationEncoding::MsgPack => deserialize_msgpack_payload(payload, type_name).map(Some),
        SerializationEncoding::Sbe => deserialize_sbe(payload).map(Some),
        SerializationEncoding::Capnp => deserialize_capnp(payload).map(Some),
    }
}

pub(crate) fn deserialize_json_payload<T>(payload: &[u8], type_name: &str) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    json::deserialize(payload, type_name)
}

pub(crate) fn deserialize_msgpack_payload<T>(payload: &[u8], type_name: &str) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    msgpack::deserialize(payload, type_name)
}

pub(super) fn warn_unsupported_inbound(
    payload_type: BusPayloadType,
    encoding: SerializationEncoding,
) {
    log::warn!(
        "{} inbound republishing is not supported for {}",
        encoding,
        payload_type.as_str()
    );
}

#[cfg(test)]
mod tests {
    #[cfg(any(not(feature = "sbe"), not(feature = "capnp")))]
    use nautilus_model::data::QuoteTick;
    #[cfg(any(feature = "sbe", feature = "capnp"))]
    use nautilus_model::data::stubs::stub_custom_data;
    use rstest::rstest;
    #[cfg(any(feature = "sbe", feature = "capnp"))]
    use ustr::Ustr;

    use super::*;

    #[cfg(feature = "sbe")]
    #[rstest]
    fn unsupported_payload_under_sbe_is_classified_as_dropped() {
        let custom = stub_custom_data(100, 42, None, Some("stub-id".to_string()));

        let error = serialize_payload(
            SerializationEncoding::Sbe,
            BusPayloadType::Custom(Ustr::from("StubCustomData")),
            &custom,
        )
        .expect_err("unsupported SBE payload must be dropped");

        assert!(matches!(error, PayloadCodecError::Dropped(_)));
    }

    #[cfg(not(feature = "sbe"))]
    #[rstest]
    fn sbe_without_feature_is_classified_as_dropped() {
        let quote = QuoteTick::default();

        let error = serialize_payload(
            SerializationEncoding::Sbe,
            BusPayloadType::QuoteTick,
            &quote,
        )
        .expect_err("SBE without feature must be dropped");

        assert!(matches!(error, PayloadCodecError::Dropped(_)));
    }

    #[cfg(feature = "capnp")]
    #[rstest]
    fn unsupported_payload_under_capnp_is_classified_as_dropped() {
        let custom = stub_custom_data(100, 42, None, Some("stub-id".to_string()));

        let error = serialize_payload(
            SerializationEncoding::Capnp,
            BusPayloadType::Custom(Ustr::from("StubCustomData")),
            &custom,
        )
        .expect_err("unsupported Cap'n Proto payload must be dropped");

        assert!(matches!(error, PayloadCodecError::Dropped(_)));
    }

    #[cfg(not(feature = "capnp"))]
    #[rstest]
    fn capnp_without_feature_is_classified_as_dropped() {
        let quote = QuoteTick::default();

        let error = serialize_payload(
            SerializationEncoding::Capnp,
            BusPayloadType::QuoteTick,
            &quote,
        )
        .expect_err("Cap'n Proto without feature must be dropped");

        assert!(matches!(error, PayloadCodecError::Dropped(_)));
    }
}
