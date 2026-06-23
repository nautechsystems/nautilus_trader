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

use std::any::Any;

use anyhow::Context;
use bytes::Bytes;
use nautilus_model::data::{
    Bar, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate, OrderBookDeltas, OrderBookDepth10,
    QuoteTick, TradeTick,
};
use nautilus_serialization::{
    capnp::{FromCapnp, ToCapnp},
    market_capnp,
};

use super::PayloadCodecError;
use crate::msgbus::BusPayloadType;

macro_rules! deserialize_payload_as {
    ($payload:expr, $type_name:expr, $ty:ty, $root:ty) => {{
        let reader = ::capnp::serialize::read_message(
            &mut &$payload[..],
            ::capnp::message::ReaderOptions::new(),
        )
        .context("failed to read Cap'n Proto message")?;
        let root = reader
            .get_root::<$root>()
            .with_context(|| format!("Cap'n Proto payload has no {} root", $type_name))?;
        <$ty>::from_capnp(root)
            .map_err(|e| anyhow::anyhow!("failed to decode Cap'n Proto {}: {}", $type_name, e))
    }};
}

macro_rules! define_deserializer {
    ($fn_name:ident, $ty:ty, $type_name:literal, $root:ty) => {
        pub(crate) fn $fn_name(payload: &[u8]) -> anyhow::Result<$ty> {
            deserialize_payload_as!(payload, $type_name, $ty, $root)
        }
    };
}

define_deserializer!(
    deserialize_order_book_deltas,
    OrderBookDeltas,
    "OrderBookDeltas",
    market_capnp::order_book_deltas::Reader
);
define_deserializer!(
    deserialize_order_book_depth10,
    OrderBookDepth10,
    "OrderBookDepth10",
    market_capnp::order_book_depth10::Reader
);
define_deserializer!(
    deserialize_quote,
    QuoteTick,
    "QuoteTick",
    market_capnp::quote_tick::Reader
);
define_deserializer!(
    deserialize_trade,
    TradeTick,
    "TradeTick",
    market_capnp::trade_tick::Reader
);
define_deserializer!(deserialize_bar, Bar, "Bar", market_capnp::bar::Reader);
define_deserializer!(
    deserialize_mark_price,
    MarkPriceUpdate,
    "MarkPriceUpdate",
    market_capnp::mark_price_update::Reader
);
define_deserializer!(
    deserialize_index_price,
    IndexPriceUpdate,
    "IndexPriceUpdate",
    market_capnp::index_price_update::Reader
);
define_deserializer!(
    deserialize_funding_rate,
    FundingRateUpdate,
    "FundingRateUpdate",
    market_capnp::funding_rate_update::Reader
);

macro_rules! serialize_payload_as {
    ($message:expr, $type_name:expr, $ty:ty, $root:ty) => {{
        let Some(value) = $message.downcast_ref::<$ty>() else {
            return Err(PayloadCodecError::Failed(format!(
                "Cap'n Proto payload type mismatch for {}",
                $type_name
            )));
        };

        let mut capnp_message = ::capnp::message::Builder::new_default();
        let builder = capnp_message.init_root::<$root>();
        value.to_capnp(builder);

        let mut bytes = Vec::new();
        ::capnp::serialize::write_message(&mut bytes, &capnp_message).map_err(|e| {
            PayloadCodecError::Failed(format!(
                "Cap'n Proto serialization failed for {}: {}",
                $type_name, e
            ))
        })?;
        Ok(Bytes::from(bytes))
    }};
}

pub(super) fn serialize_payload(
    payload_type: BusPayloadType,
    message: &dyn Any,
) -> Result<Bytes, PayloadCodecError> {
    let type_name = payload_type.as_str();
    match payload_type {
        BusPayloadType::OrderBookDeltas => serialize_payload_as!(
            message,
            type_name,
            OrderBookDeltas,
            market_capnp::order_book_deltas::Builder
        ),
        BusPayloadType::OrderBookDepth10 => serialize_payload_as!(
            message,
            type_name,
            OrderBookDepth10,
            market_capnp::order_book_depth10::Builder
        ),
        BusPayloadType::QuoteTick => serialize_payload_as!(
            message,
            type_name,
            QuoteTick,
            market_capnp::quote_tick::Builder
        ),
        BusPayloadType::TradeTick => serialize_payload_as!(
            message,
            type_name,
            TradeTick,
            market_capnp::trade_tick::Builder
        ),
        BusPayloadType::Bar => {
            serialize_payload_as!(message, type_name, Bar, market_capnp::bar::Builder)
        }
        BusPayloadType::MarkPriceUpdate => serialize_payload_as!(
            message,
            type_name,
            MarkPriceUpdate,
            market_capnp::mark_price_update::Builder
        ),
        BusPayloadType::IndexPriceUpdate => serialize_payload_as!(
            message,
            type_name,
            IndexPriceUpdate,
            market_capnp::index_price_update::Builder
        ),
        BusPayloadType::FundingRateUpdate => serialize_payload_as!(
            message,
            type_name,
            FundingRateUpdate,
            market_capnp::funding_rate_update::Builder
        ),
        _ => Err(PayloadCodecError::Dropped(format!(
            "Cap'n Proto serialization is not supported for {type_name}"
        ))),
    }
}
