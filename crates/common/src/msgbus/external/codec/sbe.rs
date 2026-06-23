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

use bytes::Bytes;
use nautilus_model::data::{
    Bar, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate, OrderBookDeltas, OrderBookDepth10,
    QuoteTick, TradeTick,
};
use nautilus_serialization::sbe::{FromSbe, ToSbe};

use super::PayloadCodecError;
use crate::msgbus::BusPayloadType;

fn deserialize_payload<T>(payload: &[u8], type_name: &str) -> anyhow::Result<T>
where
    T: FromSbe,
{
    T::from_sbe(payload).map_err(|e| anyhow::anyhow!("failed to decode SBE {type_name}: {e}"))
}

macro_rules! define_deserializer {
    ($fn_name:ident, $ty:ty, $type_name:literal) => {
        pub(crate) fn $fn_name(payload: &[u8]) -> anyhow::Result<$ty> {
            deserialize_payload::<$ty>(payload, $type_name)
        }
    };
}

define_deserializer!(
    deserialize_order_book_deltas,
    OrderBookDeltas,
    "OrderBookDeltas"
);
define_deserializer!(
    deserialize_order_book_depth10,
    OrderBookDepth10,
    "OrderBookDepth10"
);
define_deserializer!(deserialize_quote, QuoteTick, "QuoteTick");
define_deserializer!(deserialize_trade, TradeTick, "TradeTick");
define_deserializer!(deserialize_bar, Bar, "Bar");
define_deserializer!(deserialize_mark_price, MarkPriceUpdate, "MarkPriceUpdate");
define_deserializer!(
    deserialize_index_price,
    IndexPriceUpdate,
    "IndexPriceUpdate"
);
define_deserializer!(
    deserialize_funding_rate,
    FundingRateUpdate,
    "FundingRateUpdate"
);

pub(super) fn serialize_payload(
    payload_type: BusPayloadType,
    message: &dyn Any,
) -> Result<Bytes, PayloadCodecError> {
    let type_name = payload_type.as_str();
    match payload_type {
        BusPayloadType::OrderBookDeltas => {
            serialize_payload_as::<OrderBookDeltas>(type_name, message)
        }
        BusPayloadType::OrderBookDepth10 => {
            serialize_payload_as::<OrderBookDepth10>(type_name, message)
        }
        BusPayloadType::QuoteTick => serialize_payload_as::<QuoteTick>(type_name, message),
        BusPayloadType::TradeTick => serialize_payload_as::<TradeTick>(type_name, message),
        BusPayloadType::Bar => serialize_payload_as::<Bar>(type_name, message),
        BusPayloadType::MarkPriceUpdate => {
            serialize_payload_as::<MarkPriceUpdate>(type_name, message)
        }
        BusPayloadType::IndexPriceUpdate => {
            serialize_payload_as::<IndexPriceUpdate>(type_name, message)
        }
        BusPayloadType::FundingRateUpdate => {
            serialize_payload_as::<FundingRateUpdate>(type_name, message)
        }
        _ => Err(PayloadCodecError::Dropped(format!(
            "SBE serialization is not supported for {type_name}"
        ))),
    }
}

fn serialize_payload_as<T>(type_name: &str, message: &dyn Any) -> Result<Bytes, PayloadCodecError>
where
    T: Any + ToSbe,
{
    let Some(value) = message.downcast_ref::<T>() else {
        return Err(PayloadCodecError::Failed(format!(
            "SBE payload type mismatch for {type_name}"
        )));
    };

    value.to_sbe().map(Bytes::from).map_err(|e| {
        PayloadCodecError::Failed(format!("SBE serialization failed for {type_name}: {e}"))
    })
}
