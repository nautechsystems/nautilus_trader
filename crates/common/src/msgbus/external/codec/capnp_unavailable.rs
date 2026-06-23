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

use super::PayloadCodecError;
use crate::msgbus::BusPayloadType;

macro_rules! define_deserializer {
    ($fn_name:ident, $ty:ty) => {
        pub(crate) fn $fn_name(_payload: &[u8]) -> anyhow::Result<$ty> {
            anyhow::bail!("Cap'n Proto decoding requires the `capnp` feature")
        }
    };
}

define_deserializer!(deserialize_order_book_deltas, OrderBookDeltas);
define_deserializer!(deserialize_order_book_depth10, OrderBookDepth10);
define_deserializer!(deserialize_quote, QuoteTick);
define_deserializer!(deserialize_trade, TradeTick);
define_deserializer!(deserialize_bar, Bar);
define_deserializer!(deserialize_mark_price, MarkPriceUpdate);
define_deserializer!(deserialize_index_price, IndexPriceUpdate);
define_deserializer!(deserialize_funding_rate, FundingRateUpdate);

pub(super) fn serialize_payload(
    payload_type: BusPayloadType,
    _message: &dyn Any,
) -> Result<Bytes, PayloadCodecError> {
    let type_name = payload_type.as_str();
    Err(PayloadCodecError::Dropped(format!(
        "Cap'n Proto serialization for {type_name} requires the `capnp` feature"
    )))
}
