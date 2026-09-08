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

//! DeFi-specific data client functionality.
//!
//! This module provides DeFi subscription and request methods
//! for the `DataClientAdapter`. All code in this module requires the `defi` feature flag.

use nautilus_common::{
    clients::log_command_error,
    messages::defi::{
        DefiRequestCommand, DefiSubscribeCommand, DefiUnsubscribeCommand, RequestPoolSnapshot,
        SubscribeBlocks, SubscribePool, SubscribePoolFeeCollects, SubscribePoolFlashEvents,
        SubscribePoolLiquidityUpdates, SubscribePoolSwaps, UnsubscribeBlocks, UnsubscribePool,
        UnsubscribePoolFeeCollects, UnsubscribePoolFlashEvents, UnsubscribePoolLiquidityUpdates,
        UnsubscribePoolSwaps,
    },
};

use crate::{
    client::DataClientAdapter,
    subscription::{DefiSubscriptionKey, SubscriptionRelease},
};

impl DataClientAdapter {
    #[inline]
    pub fn execute_defi_subscribe(&mut self, cmd: DefiSubscribeCommand) {
        self.execute_defi_subscribe_with_retained(cmd, false);
    }

    pub(crate) fn execute_defi_subscribe_with_retained(
        &mut self,
        cmd: DefiSubscribeCommand,
        retain_on_failure: bool,
    ) {
        let key = match &cmd {
            DefiSubscribeCommand::Blocks(command) => DefiSubscriptionKey::Blocks(command.chain),
            DefiSubscribeCommand::Pool(command) => DefiSubscriptionKey::Pool(command.instrument_id),
            DefiSubscribeCommand::PoolSwaps(command) => {
                DefiSubscriptionKey::PoolSwaps(command.instrument_id)
            }
            DefiSubscribeCommand::PoolLiquidityUpdates(command) => {
                DefiSubscriptionKey::PoolLiquidityUpdates(command.instrument_id)
            }
            DefiSubscribeCommand::PoolFeeCollects(command) => {
                DefiSubscriptionKey::PoolFeeCollects(command.instrument_id)
            }
            DefiSubscribeCommand::PoolFlashEvents(command) => {
                DefiSubscriptionKey::PoolFlashEvents(command.instrument_id)
            }
        };
        let active = match &key {
            DefiSubscriptionKey::Blocks(chain) => self.subscriptions_blocks.contains(chain),
            DefiSubscriptionKey::Pool(id) => self.subscriptions_pools.contains(id),
            DefiSubscriptionKey::PoolSwaps(id) => self.subscriptions_pool_swaps.contains(id),
            DefiSubscriptionKey::PoolLiquidityUpdates(id) => {
                self.subscriptions_pool_liquidity_updates.contains(id)
            }
            DefiSubscriptionKey::PoolFeeCollects(id) => {
                self.subscriptions_pool_fee_collects.contains(id)
            }
            DefiSubscriptionKey::PoolFlashEvents(id) => self.subscriptions_pool_flash.contains(id),
        };

        if active {
            self.subscriptions_active_defi
                .retain(key, cmd.command_id(), cmd);
            return;
        }

        let retained = cmd.clone();
        let cmd_debug = format!("{cmd:?}");
        let result = match cmd {
            DefiSubscribeCommand::Blocks(cmd) => self.subscribe_blocks(cmd),
            DefiSubscribeCommand::Pool(cmd) => self.subscribe_pool(cmd),
            DefiSubscribeCommand::PoolSwaps(cmd) => self.subscribe_pool_swaps(cmd),
            DefiSubscribeCommand::PoolLiquidityUpdates(cmd) => {
                self.subscribe_pool_liquidity_updates(cmd)
            }
            DefiSubscribeCommand::PoolFeeCollects(cmd) => self.subscribe_pool_fee_collects(cmd),
            DefiSubscribeCommand::PoolFlashEvents(cmd) => self.subscribe_pool_flash_events(cmd),
        };

        if let Err(e) = result {
            if retain_on_failure {
                self.subscriptions_active_defi
                    .retain(key, retained.command_id(), retained);
            }

            log_command_error(&cmd_debug, &e);
            return;
        }

        if let Some(subscription) = self.subscriptions_active_defi.get_mut(&key) {
            subscription.command = retained.clone();
        }
        self.subscriptions_active_defi
            .retain(key, retained.command_id(), retained);
    }

    #[inline]
    pub fn execute_defi_unsubscribe(&mut self, cmd: &DefiUnsubscribeCommand) {
        let key = match cmd {
            DefiUnsubscribeCommand::Blocks(command) => DefiSubscriptionKey::Blocks(command.chain),
            DefiUnsubscribeCommand::Pool(command) => {
                DefiSubscriptionKey::Pool(command.instrument_id)
            }
            DefiUnsubscribeCommand::PoolSwaps(command) => {
                DefiSubscriptionKey::PoolSwaps(command.instrument_id)
            }
            DefiUnsubscribeCommand::PoolLiquidityUpdates(command) => {
                DefiSubscriptionKey::PoolLiquidityUpdates(command.instrument_id)
            }
            DefiUnsubscribeCommand::PoolFeeCollects(command) => {
                DefiSubscriptionKey::PoolFeeCollects(command.instrument_id)
            }
            DefiUnsubscribeCommand::PoolFlashEvents(command) => {
                DefiSubscriptionKey::PoolFlashEvents(command.instrument_id)
            }
        };
        let command = match self.subscriptions_active_defi.release(&key) {
            SubscriptionRelease::Retained => return,
            SubscriptionRelease::Final(subscribe) => {
                subscribe.into_unsubscribe(cmd.command_id(), cmd.ts_init())
            }
            SubscriptionRelease::Untracked => cmd.clone(),
        };

        if let Err(e) = match &command {
            DefiUnsubscribeCommand::Blocks(cmd) => self.unsubscribe_blocks(cmd),
            DefiUnsubscribeCommand::Pool(cmd) => self.unsubscribe_pool(cmd),
            DefiUnsubscribeCommand::PoolSwaps(cmd) => self.unsubscribe_pool_swaps(cmd),
            DefiUnsubscribeCommand::PoolLiquidityUpdates(cmd) => {
                self.unsubscribe_pool_liquidity_updates(cmd)
            }
            DefiUnsubscribeCommand::PoolFeeCollects(cmd) => self.unsubscribe_pool_fee_collects(cmd),
            DefiUnsubscribeCommand::PoolFlashEvents(cmd) => self.unsubscribe_pool_flash_events(cmd),
        } {
            log_command_error(&command, &e);
        } else {
            self.subscriptions_active_defi.remove(&key);
        }
    }

    /// Executes a DeFi data request command by dispatching to the appropriate handler.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client request fails.
    #[inline]
    pub fn execute_defi_request(&self, cmd: DefiRequestCommand) -> anyhow::Result<()> {
        match cmd {
            DefiRequestCommand::PoolSnapshot(cmd) => self.request_pool_snapshot(cmd),
        }
    }

    /// Subscribes to block events for the specified blockchain.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_blocks(&mut self, cmd: SubscribeBlocks) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_blocks,
            cmd.chain,
            "blocks",
            |client| client.subscribe_blocks(cmd),
        )
    }

    /// Unsubscribes from block events for the specified blockchain.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_blocks(&mut self, cmd: &UnsubscribeBlocks) -> anyhow::Result<()> {
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_blocks,
            &cmd.chain,
            "blocks",
            |client| client.unsubscribe_blocks(cmd),
        )
    }

    /// Subscribes to pool definition updates for the specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_pool(&mut self, cmd: SubscribePool) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_pools,
            cmd.instrument_id,
            "pool",
            |client| client.subscribe_pool(cmd),
        )
    }

    /// Subscribes to pool swap events for the specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_pool_swaps(&mut self, cmd: SubscribePoolSwaps) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_pool_swaps,
            cmd.instrument_id,
            "pool swaps",
            |client| client.subscribe_pool_swaps(cmd),
        )
    }

    /// Subscribes to pool liquidity update events for the specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_pool_liquidity_updates(
        &mut self,
        cmd: SubscribePoolLiquidityUpdates,
    ) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_pool_liquidity_updates,
            cmd.instrument_id,
            "pool liquidity updates",
            |client| client.subscribe_pool_liquidity_updates(cmd),
        )
    }

    /// Subscribes to pool fee collect events for the specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_pool_fee_collects(&mut self, cmd: SubscribePoolFeeCollects) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_pool_fee_collects,
            cmd.instrument_id,
            "pool fee collects",
            |client| client.subscribe_pool_fee_collects(cmd),
        )
    }

    /// Subscribes to pool flash loan events for the specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_pool_flash_events(&mut self, cmd: SubscribePoolFlashEvents) -> anyhow::Result<()> {
        Self::execute_tracked_subscribe(
            self.client.as_mut(),
            &mut self.subscriptions_pool_flash,
            cmd.instrument_id,
            "pool flash events",
            |client| client.subscribe_pool_flash_events(cmd),
        )
    }

    /// Unsubscribes from pool definition updates for the specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_pool(&mut self, cmd: &UnsubscribePool) -> anyhow::Result<()> {
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_pools,
            &cmd.instrument_id,
            "pool",
            |client| client.unsubscribe_pool(cmd),
        )
    }

    /// Unsubscribes from swap events for the specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_pool_swaps(&mut self, cmd: &UnsubscribePoolSwaps) -> anyhow::Result<()> {
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_pool_swaps,
            &cmd.instrument_id,
            "pool swaps",
            |client| client.unsubscribe_pool_swaps(cmd),
        )
    }

    /// Unsubscribes from pool liquidity update events for the specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_pool_liquidity_updates(
        &mut self,
        cmd: &UnsubscribePoolLiquidityUpdates,
    ) -> anyhow::Result<()> {
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_pool_liquidity_updates,
            &cmd.instrument_id,
            "pool liquidity updates",
            |client| client.unsubscribe_pool_liquidity_updates(cmd),
        )
    }

    /// Unsubscribes from pool fee collect events for the specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_pool_fee_collects(
        &mut self,
        cmd: &UnsubscribePoolFeeCollects,
    ) -> anyhow::Result<()> {
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_pool_fee_collects,
            &cmd.instrument_id,
            "pool fee collects",
            |client| client.unsubscribe_pool_fee_collects(cmd),
        )
    }

    /// Unsubscribes from pool flash loan events for the specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_pool_flash_events(
        &mut self,
        cmd: &UnsubscribePoolFlashEvents,
    ) -> anyhow::Result<()> {
        Self::execute_tracked_unsubscribe(
            self.client.as_mut(),
            &mut self.subscriptions_pool_flash,
            &cmd.instrument_id,
            "pool flash events",
            |client| client.unsubscribe_pool_flash_events(cmd),
        )
    }

    /// Sends a pool snapshot request for a given AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to process the pool snapshot request.
    pub fn request_pool_snapshot(&self, req: RequestPoolSnapshot) -> anyhow::Result<()> {
        self.client.request_pool_snapshot(req)
    }
}
