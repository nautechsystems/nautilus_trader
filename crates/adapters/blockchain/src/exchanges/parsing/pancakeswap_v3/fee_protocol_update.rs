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

use alloy::{dyn_abi::SolType, primitives::Address, sol};
use nautilus_model::defi::{PoolIdentifier, SharedDex, rpc::RpcLog};
use ustr::Ustr;

use crate::{
    events::fee_protocol_update::FeeProtocolUpdateEvent,
    hypersync::{
        HypersyncLog,
        helpers::{
            extract_block_number, extract_log_index, extract_transaction_hash,
            extract_transaction_index, validate_event_signature_hash,
        },
    },
    rpc::helpers as rpc_helpers,
};

const FEE_PROTOCOL_UPDATE_EVENT_SIGNATURE_HASH: &str =
    "b3159fed3ddfba67bae294599eafe2d0ec98c08bb38e0e5fb87d33154b6e05aa";

sol! {
    struct SetFeeProtocolEventData {
        uint32 fee_protocol0_old;
        uint32 fee_protocol1_old;
        uint32 fee_protocol0_new;
        uint32 fee_protocol1_new;
    }
}

/// Parses a PancakeSwap V3 `SetFeeProtocol` event from a HyperSync log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
///
/// # Panics
///
/// Panics if the contract address is not set in the log.
pub fn parse_fee_protocol_update_event_hypersync(
    dex: SharedDex,
    log: &HypersyncLog,
) -> anyhow::Result<FeeProtocolUpdateEvent> {
    validate_event_signature_hash(
        "SetFeeProtocolEvent",
        FEE_PROTOCOL_UPDATE_EVENT_SIGNATURE_HASH,
        log,
    )?;

    if let Some(data) = &log.data {
        let data_bytes = data.as_ref();

        if data_bytes.len() < 4 * 32 {
            anyhow::bail!("SetFeeProtocol event data is too short");
        }

        let decoded = match <SetFeeProtocolEventData as SolType>::abi_decode(data_bytes) {
            Ok(decoded) => decoded,
            Err(e) => anyhow::bail!("Failed to decode SetFeeProtocol event data: {e}"),
        };

        let pool_address = Address::from_slice(
            log.address
                .clone()
                .expect("Contract address should be set in logs")
                .as_ref(),
        );
        let pool_identifier = PoolIdentifier::Address(Ustr::from(&pool_address.to_string()));

        Ok(FeeProtocolUpdateEvent::new(
            dex,
            pool_identifier,
            extract_block_number(log)?,
            extract_transaction_hash(log)?,
            extract_transaction_index(log)?,
            extract_log_index(log)?,
            decoded.fee_protocol0_new,
            decoded.fee_protocol1_new,
        ))
    } else {
        anyhow::bail!("Missing data in SetFeeProtocol event log");
    }
}

/// Parses a PancakeSwap V3 `SetFeeProtocol` event from an RPC log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
pub fn parse_fee_protocol_update_event_rpc(
    dex: SharedDex,
    log: &RpcLog,
) -> anyhow::Result<FeeProtocolUpdateEvent> {
    rpc_helpers::validate_event_signature(
        log,
        FEE_PROTOCOL_UPDATE_EVENT_SIGNATURE_HASH,
        "SetFeeProtocol",
    )?;

    let data_bytes = rpc_helpers::extract_data_bytes(log)?;

    if data_bytes.len() < 4 * 32 {
        anyhow::bail!("SetFeeProtocol event data is too short");
    }

    let decoded = match <SetFeeProtocolEventData as SolType>::abi_decode(&data_bytes) {
        Ok(decoded) => decoded,
        Err(e) => anyhow::bail!("Failed to decode SetFeeProtocol event data: {e}"),
    };

    let pool_address = rpc_helpers::extract_address(log)?;
    let pool_identifier = PoolIdentifier::Address(Ustr::from(&pool_address.to_string()));
    Ok(FeeProtocolUpdateEvent::new(
        dex,
        pool_identifier,
        rpc_helpers::extract_block_number(log)?,
        rpc_helpers::extract_transaction_hash(log)?,
        rpc_helpers::extract_transaction_index(log)?,
        rpc_helpers::extract_log_index(log)?,
        decoded.fee_protocol0_new,
        decoded.fee_protocol1_new,
    ))
}

#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;
    use crate::exchanges::bsc;

    const HYPERSYNC_LOG: &str =
        include_str!("../../../../test_data/pancakeswap_v3_set_fee_protocol_hypersync.json");
    const RPC_LOG: &str =
        include_str!("../../../../test_data/pancakeswap_v3_set_fee_protocol_rpc.json");

    #[fixture]
    fn hypersync_log() -> HypersyncLog {
        serde_json::from_str(HYPERSYNC_LOG).expect("Failed to deserialize HyperSync log")
    }

    #[fixture]
    fn rpc_log() -> RpcLog {
        serde_json::from_str(RPC_LOG).expect("Failed to deserialize RPC log")
    }

    #[rstest]
    fn test_parse_fee_protocol_update_event_hypersync(hypersync_log: HypersyncLog) {
        let dex = bsc::PANCAKESWAP_V3.dex.clone();
        let event = parse_fee_protocol_update_event_hypersync(dex, &hypersync_log).unwrap();

        assert_eq!(
            event.pool_identifier.to_string(),
            "0x172fcD41E0913e95784454622d1c3724f546f849"
        );
        assert_eq!(event.fee_protocol0_new, 1_000);
        assert_eq!(event.fee_protocol1_new, 4_000);
        assert_eq!(event.block_number, 105_475_426);
        assert_eq!(event.transaction_index, 5);
        assert_eq!(event.log_index, 17);
    }

    #[rstest]
    fn test_parse_fee_protocol_update_event_rpc(rpc_log: RpcLog) {
        let dex = bsc::PANCAKESWAP_V3.dex.clone();
        let event = parse_fee_protocol_update_event_rpc(dex, &rpc_log).unwrap();

        assert_eq!(
            event.pool_identifier.to_string(),
            "0x172fcD41E0913e95784454622d1c3724f546f849"
        );
        assert_eq!(event.fee_protocol0_new, 1_000);
        assert_eq!(event.fee_protocol1_new, 4_000);
        assert_eq!(event.block_number, 105_475_426);
    }

    #[rstest]
    fn test_hypersync_rpc_match(hypersync_log: HypersyncLog, rpc_log: RpcLog) {
        let dex = bsc::PANCAKESWAP_V3.dex.clone();
        let event_hypersync =
            parse_fee_protocol_update_event_hypersync(dex.clone(), &hypersync_log).unwrap();
        let event_rpc = parse_fee_protocol_update_event_rpc(dex, &rpc_log).unwrap();

        assert_eq!(event_hypersync.pool_identifier, event_rpc.pool_identifier);
        assert_eq!(
            event_hypersync.fee_protocol0_new,
            event_rpc.fee_protocol0_new
        );
        assert_eq!(
            event_hypersync.fee_protocol1_new,
            event_rpc.fee_protocol1_new
        );
        assert_eq!(event_hypersync.block_number, event_rpc.block_number);
        assert_eq!(event_hypersync.transaction_hash, event_rpc.transaction_hash);
        assert_eq!(
            event_hypersync.transaction_index,
            event_rpc.transaction_index
        );
        assert_eq!(event_hypersync.log_index, event_rpc.log_index);
    }
}
