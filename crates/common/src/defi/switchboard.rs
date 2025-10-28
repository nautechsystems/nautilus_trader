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

//! DeFi-specific switchboard functionality.

use ahash::AHashMap;
use nautilus_model::{defi::Blockchain, identifiers::InstrumentId};

use crate::msgbus::{
    core::{MStr, Topic},
    get_message_bus,
    switchboard::MessagingSwitchboard,
};

/// DeFi-specific switchboard state.
#[derive(Clone, Debug, Default)]
pub(crate) struct DefiSwitchboard {
    pub(crate) block_topics: AHashMap<Blockchain, MStr<Topic>>,
    pub(crate) pool_topics: AHashMap<InstrumentId, MStr<Topic>>,
    pub(crate) pool_swap_topics: AHashMap<InstrumentId, MStr<Topic>>,
    pub(crate) pool_liquidity_topics: AHashMap<InstrumentId, MStr<Topic>>,
    pub(crate) pool_collect_topics: AHashMap<InstrumentId, MStr<Topic>>,
    pub(crate) pool_flash_topics: AHashMap<InstrumentId, MStr<Topic>>,
}

#[must_use]
pub fn get_defi_blocks_topic(chain: Blockchain) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_defi_blocks_topic(chain)
}

#[must_use]
pub fn get_defi_pool_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_defi_pool_topic(instrument_id)
}

#[must_use]
pub fn get_defi_pool_swaps_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_defi_pool_swaps_topic(instrument_id)
}

#[must_use]
pub fn get_defi_liquidity_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_defi_pool_liquidity_topic(instrument_id)
}

#[must_use]
pub fn get_defi_collect_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_defi_pool_collect_topic(instrument_id)
}

#[must_use]
pub fn get_defi_flash_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_defi_pool_flash_topic(instrument_id)
}

impl MessagingSwitchboard {
    #[must_use]
    pub fn get_defi_blocks_topic(&mut self, chain: Blockchain) -> MStr<Topic> {
        *self
            .defi
            .block_topics
            .entry(chain)
            .or_insert_with(|| format!("data.defi.blocks.{chain}").into())
    }

    #[must_use]
    pub fn get_defi_pool_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .defi
            .pool_topics
            .entry(instrument_id)
            .or_insert_with(|| format!("data.defi.pool.{instrument_id}").into())
    }

    #[must_use]
    pub fn get_defi_pool_swaps_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .defi
            .pool_swap_topics
            .entry(instrument_id)
            .or_insert_with(|| format!("data.defi.pool_swaps.{instrument_id}").into())
    }

    #[must_use]
    pub fn get_defi_pool_liquidity_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .defi
            .pool_liquidity_topics
            .entry(instrument_id)
            .or_insert_with(|| format!("data.defi.pool_liquidity.{instrument_id}").into())
    }

    #[must_use]
    pub fn get_defi_pool_collect_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .defi
            .pool_collect_topics
            .entry(instrument_id)
            .or_insert_with(|| format!("data.defi.pool_collect.{instrument_id}").into())
    }

    #[must_use]
    pub fn get_defi_pool_flash_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .defi
            .pool_flash_topics
            .entry(instrument_id)
            .or_insert_with(|| format!("data.defi.pool_flash.{instrument_id}").into())
    }
}
