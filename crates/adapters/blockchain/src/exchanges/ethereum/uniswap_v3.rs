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

use alloy::primitives::{Address, U256};
use hypersync_client::simple_types::Log;
use nautilus_model::defi::{
    chain::chains,
    dex::{AmmType, Dex},
};

use crate::{events::pool_created::PoolCreated, exchanges::extended::DexExtended};

/// Uniswap V3 DEX on Ethereum.
pub static UNISWAP_V3: LazyLock<DexExtended> = LazyLock::new(|| {
    let mut dex = DexExtended::new(Dex::new(
        chains::ETHEREUM.clone(),
        "Uniswap V3",
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        AmmType::CLAMM,
        "PoolCreated(address,address,uint24,int24,address)",
    ));
    dex.set_pool_created_event_parsing(parse_pool_created_event);
    dex
});

fn parse_pool_created_event(log: Log) -> anyhow::Result<PoolCreated> {
    let block_number = log
        .block_number
        .expect("Block number should be set in logs");
    let token = if let Some(topic) = log.topics.get(1).and_then(|t| t.as_ref()) {
        // Address is stored in the last 20 bytes of the 32-byte topic
        Address::from_slice(&topic.as_ref()[12..32])
    } else {
        anyhow::bail!("Missing token0 address in topic1");
    };

    let token1 = if let Some(topic) = log.topics.get(2).and_then(|t| t.as_ref()) {
        Address::from_slice(&topic.as_ref()[12..32])
    } else {
        anyhow::bail!("Missing token1 address in topic2");
    };

    let fee = if let Some(topic) = log.topics.get(3).and_then(|t| t.as_ref()) {
        U256::from_be_slice(topic.as_ref()).as_limbs()[0] as u32
    } else {
        anyhow::bail!("Missing fee in topic3");
    };

    if let Some(data) = log.data {
        // Data contains: [tick_spacing (32 bytes), pool_address (32 bytes)]
        let data_bytes = data.as_ref();

        // Extract tick_spacing (first 32 bytes)
        let tick_spacing_bytes: [u8; 32] = data_bytes[0..32].try_into()?;
        let tick_spacing = u32::from_be_bytes(tick_spacing_bytes[28..32].try_into()?);

        // Extract pool_address (next 32 bytes)
        let pool_address_bytes: [u8; 32] = data_bytes[32..64].try_into()?;
        let pool_address = Address::from_slice(&pool_address_bytes[12..32]);

        Ok(PoolCreated::new(
            block_number.into(),
            token,
            token1,
            fee,
            tick_spacing,
            pool_address,
        ))
    } else {
        Err(anyhow::anyhow!("Missing data in log"))
    }
}
