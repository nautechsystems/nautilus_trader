// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::sync::LazyLock;

use alloy::primitives::Address;
use hypersync_client::simple_types::Log;
use nautilus_model::defi::{
    chain::chains,
    dex::{AmmType, Dex, DexType},
};

use crate::{
    events::pool_created::PoolCreatedEvent,
    exchanges::extended::DexExtended,
    hypersync::helpers::{
        extract_address_from_topic, extract_block_number, validate_event_signature_hash,
    },
};

const POOL_CREATED_EVENT_SIGNATURE_HASH: &str =
    "91ccaa7a278130b65168c3a0c8d3bcae84cf5e43704342bd3ec0b59e59c036db";

/// Camelot V3 DEX on Arbitrum.
pub static CAMELOT_V3: LazyLock<DexExtended> = LazyLock::new(|| {
    let mut dex = DexExtended::new(Dex::new(
        chains::ARBITRUM.clone(),
        DexType::CamelotV3,
        "0x1a3c9B1d2F0529D97f2afC5136Cc23e58f1FD35B",
        102286676,
        AmmType::CLAMM,
        "Pool(address,address,address)",
        "",
        "",
        "",
        "",
    ));
    dex.set_pool_created_event_parsing(parse_camelot_v3_pool_created_event);
    dex
});

fn parse_camelot_v3_pool_created_event(log: Log) -> anyhow::Result<PoolCreatedEvent> {
    validate_event_signature_hash("Pool", POOL_CREATED_EVENT_SIGNATURE_HASH, &log)?;

    let block_number = extract_block_number(&log)?;
    let token = extract_address_from_topic(&log, 1, "token0")?;
    let token1 = extract_address_from_topic(&log, 2, "token1")?;

    if let Some(data) = log.data {
        let data_bytes = data.as_ref();

        // Extract pool address (only 32 bytes)
        let pool_address = Address::from_slice(&data_bytes[12..32]);

        Ok(PoolCreatedEvent::new(
            block_number,
            token,
            token1,
            pool_address,
            None,
            None,
        ))
    } else {
        anyhow::bail!("Missing data in the pool created event log")
    }
}
