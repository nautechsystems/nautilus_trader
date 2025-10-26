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

//! DeFi-specific actor functionality.
//!
//! This module provides DeFi subscription and unsubscription helper methods
//! for the `DataActorCore`. All code in this module requires the `defi` feature flag.

use indexmap::IndexMap;
use nautilus_core::UUID4;
use nautilus_model::{
    defi::Blockchain,
    identifiers::{ClientId, InstrumentId},
};

use crate::{
    actor::DataActorCore,
    defi::{
        DefiSubscribeCommand, DefiUnsubscribeCommand, SubscribeBlocks, SubscribePool,
        SubscribePoolFeeCollects, SubscribePoolFlashEvents, SubscribePoolLiquidityUpdates,
        SubscribePoolSwaps, UnsubscribeBlocks, UnsubscribePool, UnsubscribePoolFeeCollects,
        UnsubscribePoolFlashEvents, UnsubscribePoolLiquidityUpdates, UnsubscribePoolSwaps,
        switchboard::{
            get_defi_blocks_topic, get_defi_collect_topic, get_defi_flash_topic,
            get_defi_liquidity_topic, get_defi_pool_swaps_topic, get_defi_pool_topic,
        },
    },
    messages::data::DataCommand,
    msgbus::{MStr, Topic, handler::ShareableMessageHandler},
};

impl DataActorCore {
    /// Helper method for registering block subscriptions from the trait.
    pub fn subscribe_blocks(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
        chain: Blockchain,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        self.add_subscription(topic, handler);

        let command = DefiSubscribeCommand::Blocks(SubscribeBlocks {
            chain,
            client_id,
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::DefiSubscribe(command));
    }

    /// Helper method for registering pool subscriptions from the trait.
    pub fn subscribe_pool(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        self.add_subscription(topic, handler);

        let command = DefiSubscribeCommand::Pool(SubscribePool {
            instrument_id,
            client_id,
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::DefiSubscribe(command));
    }

    /// Helper method for registering pool swap subscriptions from the trait.
    pub fn subscribe_pool_swaps(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        self.add_subscription(topic, handler);

        let command = DefiSubscribeCommand::PoolSwaps(SubscribePoolSwaps {
            instrument_id,
            client_id,
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::DefiSubscribe(command));
    }

    /// Helper method for registering pool liquidity update subscriptions from the trait.
    pub fn subscribe_pool_liquidity_updates(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        self.add_subscription(topic, handler);

        let command = DefiSubscribeCommand::PoolLiquidityUpdates(SubscribePoolLiquidityUpdates {
            instrument_id,
            client_id,
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::DefiSubscribe(command));
    }

    /// Helper method for registering pool fee collect subscriptions from the trait.
    pub fn subscribe_pool_fee_collects(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        self.add_subscription(topic, handler);

        let command = DefiSubscribeCommand::PoolFeeCollects(SubscribePoolFeeCollects {
            instrument_id,
            client_id,
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::DefiSubscribe(command));
    }

    /// Helper method for registering pool flash event subscriptions from the trait.
    pub fn subscribe_pool_flash_events(
        &mut self,
        topic: MStr<Topic>,
        handler: ShareableMessageHandler,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        self.add_subscription(topic, handler);

        let command = DefiSubscribeCommand::PoolFlashEvents(SubscribePoolFlashEvents {
            instrument_id,
            client_id,
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::DefiSubscribe(command));
    }

    /// Helper method for unsubscribing from blocks.
    pub fn unsubscribe_blocks(
        &mut self,
        chain: Blockchain,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_defi_blocks_topic(chain);
        self.remove_subscription(topic);

        let command = DefiUnsubscribeCommand::Blocks(UnsubscribeBlocks {
            chain,
            client_id,
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::DefiUnsubscribe(command));
    }

    /// Helper method for unsubscribing from pool definition updates.
    pub fn unsubscribe_pool(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_defi_pool_topic(instrument_id);
        self.remove_subscription(topic);

        let command = DefiUnsubscribeCommand::Pool(UnsubscribePool {
            instrument_id,
            client_id,
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::DefiUnsubscribe(command));
    }

    /// Helper method for unsubscribing from pool swaps.
    pub fn unsubscribe_pool_swaps(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_defi_pool_swaps_topic(instrument_id);
        self.remove_subscription(topic);

        let command = DefiUnsubscribeCommand::PoolSwaps(UnsubscribePoolSwaps {
            instrument_id,
            client_id,
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::DefiUnsubscribe(command));
    }

    /// Helper method for unsubscribing from pool liquidity updates.
    pub fn unsubscribe_pool_liquidity_updates(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_defi_liquidity_topic(instrument_id);
        self.remove_subscription(topic);

        let command =
            DefiUnsubscribeCommand::PoolLiquidityUpdates(UnsubscribePoolLiquidityUpdates {
                instrument_id,
                client_id,
                command_id: UUID4::new(),
                ts_init: self.timestamp_ns(),
                params,
            });

        self.send_data_cmd(DataCommand::DefiUnsubscribe(command));
    }

    /// Helper method for unsubscribing from pool fee collects.
    pub fn unsubscribe_pool_fee_collects(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_defi_collect_topic(instrument_id);
        self.remove_subscription(topic);

        let command = DefiUnsubscribeCommand::PoolFeeCollects(UnsubscribePoolFeeCollects {
            instrument_id,
            client_id,
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::DefiUnsubscribe(command));
    }

    /// Helper method for unsubscribing from pool flash events.
    pub fn unsubscribe_pool_flash_events(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) {
        self.check_registered();

        let topic = get_defi_flash_topic(instrument_id);
        self.remove_subscription(topic);

        let command = DefiUnsubscribeCommand::PoolFlashEvents(UnsubscribePoolFlashEvents {
            instrument_id,
            client_id,
            command_id: UUID4::new(),
            ts_init: self.timestamp_ns(),
            params,
        });

        self.send_data_cmd(DataCommand::DefiUnsubscribe(command));
    }
}
