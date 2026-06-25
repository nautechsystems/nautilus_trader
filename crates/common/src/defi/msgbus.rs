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

//! DeFi-specific external message bus republishing.

use alloy_primitives::U256;
use nautilus_model::defi::{Block, Blockchain};
use ustr::Ustr;

use crate::{
    enums::SerializationEncoding,
    msgbus::{
        BusPayloadType,
        external::{codec::deserialize_json_msgpack_payload, handle_json_msgpack},
        mstr::{MStr, Topic},
        publish_defi_block, publish_defi_collect, publish_defi_flash, publish_defi_liquidity,
        publish_defi_pool,
    },
};

pub(crate) fn republish_external_message(
    topic: MStr<Topic>,
    payload_type: BusPayloadType,
    encoding: SerializationEncoding,
    payload: &[u8],
) -> anyhow::Result<()> {
    match payload_type {
        BusPayloadType::Block => handle_block(topic, payload_type, encoding, payload),
        BusPayloadType::Pool => {
            handle_json_msgpack(topic, payload_type, encoding, payload, publish_defi_pool)
        }
        BusPayloadType::PoolLiquidityUpdate => handle_json_msgpack(
            topic,
            payload_type,
            encoding,
            payload,
            publish_defi_liquidity,
        ),
        BusPayloadType::PoolFeeCollect => {
            handle_json_msgpack(topic, payload_type, encoding, payload, publish_defi_collect)
        }
        BusPayloadType::PoolFlash => {
            handle_json_msgpack(topic, payload_type, encoding, payload, publish_defi_flash)
        }
        _ => Ok(()),
    }
}

fn handle_block(
    topic: MStr<Topic>,
    payload_type: BusPayloadType,
    encoding: SerializationEncoding,
    payload: &[u8],
) -> anyhow::Result<()> {
    let Some(block) = decode_block_payload(payload_type, encoding, payload)? else {
        return Ok(());
    };

    publish_defi_block(topic, &block);
    Ok(())
}

fn decode_block_payload(
    payload_type: BusPayloadType,
    encoding: SerializationEncoding,
    payload: &[u8],
) -> anyhow::Result<Option<Block>> {
    deserialize_json_msgpack_payload::<ExternalBlockPayload>(payload_type, encoding, payload)
        .map(|payload| payload.map(Block::from))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExternalBlockPayload {
    #[serde(default)]
    chain: Option<Blockchain>,
    hash: String,
    number: u64,
    parent_hash: String,
    miner: Ustr,
    gas_limit: u64,
    gas_used: u64,
    #[serde(default)]
    base_fee_per_gas: Option<U256>,
    #[serde(default)]
    blob_gas_used: Option<U256>,
    #[serde(default)]
    excess_blob_gas: Option<U256>,
    #[serde(default)]
    l1_gas_price: Option<U256>,
    #[serde(default)]
    l1_gas_used: Option<u64>,
    #[serde(default)]
    l1_fee_scalar: Option<u64>,
    timestamp: nautilus_core::UnixNanos,
}

impl From<ExternalBlockPayload> for Block {
    fn from(value: ExternalBlockPayload) -> Self {
        Self {
            chain: value.chain,
            hash: value.hash,
            number: value.number,
            parent_hash: value.parent_hash,
            miner: value.miner,
            gas_limit: value.gas_limit,
            gas_used: value.gas_used,
            base_fee_per_gas: value.base_fee_per_gas,
            blob_gas_used: value.blob_gas_used,
            excess_blob_gas: value.excess_blob_gas,
            l1_gas_price: value.l1_gas_price,
            l1_gas_used: value.l1_gas_used,
            l1_fee_scalar: value.l1_fee_scalar,
            timestamp: value.timestamp,
        }
    }
}
