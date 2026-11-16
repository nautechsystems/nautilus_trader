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

use std::sync::LazyLock;

use alloy::primitives::Address;
use nautilus_model::defi::{
    PoolIdentifier,
    chain::chains,
    dex::{AmmType, Dex, DexType},
};
use ustr::Ustr;

use crate::{
    events::pool_created::PoolCreatedEvent,
    exchanges::extended::DexExtended,
    hypersync::{
        HypersyncLog,
        log::{extract_address_from_topic, extract_block_number, validate_event_signature_hash},
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
    dex.set_pool_created_event_hypersync_parsing(parse_camelot_v3_pool_created_event_hypersync);
    dex
});

fn parse_camelot_v3_pool_created_event_hypersync(
    log: HypersyncLog,
) -> anyhow::Result<PoolCreatedEvent> {
    validate_event_signature_hash("Pool", POOL_CREATED_EVENT_SIGNATURE_HASH, &log)?;

    let block_number = extract_block_number(&log)?;
    let token = extract_address_from_topic(&log, 1, "token0")?;
    let token1 = extract_address_from_topic(&log, 2, "token1")?;

    if let Some(data) = log.data {
        let data_bytes = data.as_ref();
        anyhow::ensure!(
            data_bytes.len() >= 32,
            "Pool created event data too short: expected at least 32 bytes, was {}",
            data_bytes.len()
        );

        // Pool address is right-aligned in the first 32-byte word
        let pool_address = Address::from_slice(&data_bytes[12..32]);
        let pool_identifier = PoolIdentifier::Address(Ustr::from(&pool_address.to_string()));
        Ok(PoolCreatedEvent::new(
            block_number,
            token,
            token1,
            pool_address,
            pool_identifier,
            None,
            None,
        ))
    } else {
        anyhow::bail!("Missing data in the pool created event log")
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    fn camelot_log(data: &str) -> HypersyncLog {
        let log_json = json!({
            "block_number": "0x1",
            "address": "0x1a3c9b1d2f0529d97f2afc5136cc23e58f1fd35b",
            "data": data,
            "topics": [
                "0x91ccaa7a278130b65168c3a0c8d3bcae84cf5e43704342bd3ec0b59e59c036db",
                "0x00000000000000000000000082af49447d8a07e3bd95bd0d56f35241523fbab1",
                "0x000000000000000000000000af88d065e77c8cc2239327c5edb3a432268e5831"
            ]
        });
        serde_json::from_value(log_json).expect("Failed to deserialize HyperSync log")
    }

    #[rstest]
    fn test_parse_pool_created_hypersync_rejects_short_data() {
        let valid_log =
            camelot_log("0x000000000000000000000000f64dfe17c8b87f012fcf50fbda1d62bfa148366a");
        let short_log =
            camelot_log("0x000000000000000000000000f64dfe17c8b87f012fcf50fbda1d62bfa14836");

        let error = parse_camelot_v3_pool_created_event_hypersync(short_log).unwrap_err();
        let event = parse_camelot_v3_pool_created_event_hypersync(valid_log).unwrap();

        assert_eq!(
            error.to_string(),
            "Pool created event data too short: expected at least 32 bytes, was 31"
        );
        assert_eq!(event.block_number, 1);
        assert_eq!(
            event.pool_identifier.to_string().to_lowercase(),
            "0xf64dfe17c8b87f012fcf50fbda1d62bfa148366a"
        );
    }
}
