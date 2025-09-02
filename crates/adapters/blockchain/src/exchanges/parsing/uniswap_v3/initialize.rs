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

use alloy::{dyn_abi::SolType, primitives::Address, sol};
use hypersync_client::simple_types::Log;
use nautilus_model::defi::SharedDex;

use crate::{
    events::initialize::InitializeEvent, hypersync::helpers::validate_event_signature_hash,
};

const INITIALIZE_EVENT_SIGNATURE_HASH: &str =
    "98636036cb66a9c19a37435efc1e90142190214e8abeb821bdba3f2990dd4c95";

// Define sol macro for easier parsing of Initialize event data
// It contains 2 parameters:
// sqrtPriceX96 (uint160), tick (int24)
sol! {
    struct InitializeEventData {
        uint160 sqrt_price_x96;
        int24 tick;
    }
}

/// Parses an initialize event from a Uniswap V3 log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
///
/// # Panics
///
/// Panics if the contract address is not set in the log.
pub fn parse_initialize_event(dex: SharedDex, log: Log) -> anyhow::Result<InitializeEvent> {
    validate_event_signature_hash("InitializeEvent", INITIALIZE_EVENT_SIGNATURE_HASH, &log)?;

    if let Some(data) = &log.data {
        let data_bytes = data.as_ref();

        // Validate if data contains 2 parameters of 32 bytes each (sqrtPriceX96 and tick)
        if data_bytes.len() < 2 * 32 {
            anyhow::bail!("Initialize event data is too short");
        }

        // Decode the data using the InitializeEventData struct
        let decoded = match <InitializeEventData as SolType>::abi_decode(data_bytes) {
            Ok(decoded) => decoded,
            Err(e) => anyhow::bail!("Failed to decode initialize event data: {e}"),
        };

        let pool_address = Address::from_slice(
            log.address
                .clone()
                .expect("Contract address should be set in logs")
                .as_ref(),
        );

        Ok(InitializeEvent::new(
            dex,
            pool_address,
            decoded.sqrt_price_x96,
            i32::try_from(decoded.tick)?,
        ))
    } else {
        Err(anyhow::anyhow!("Missing data in initialize event log"))
    }
}
