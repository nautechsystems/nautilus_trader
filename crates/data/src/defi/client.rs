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

//! DeFi-specific data client functionality.
//!
//! This module provides DeFi subscription and request helper methods
//! for the `DataClientAdapter`. All code in this module requires the `defi` feature flag.

use std::fmt::{Debug, Display};

use nautilus_common::messages::defi::{
    DefiRequestCommand, DefiSubscribeCommand, DefiUnsubscribeCommand, RequestPoolSnapshot,
    SubscribeBlocks, SubscribePool, SubscribePoolFeeCollects, SubscribePoolFlashEvents,
    SubscribePoolLiquidityUpdates, SubscribePoolSwaps, UnsubscribeBlocks, UnsubscribePool,
    UnsubscribePoolFeeCollects, UnsubscribePoolFlashEvents, UnsubscribePoolLiquidityUpdates,
    UnsubscribePoolSwaps,
};

use crate::client::DataClientAdapter;

impl DataClientAdapter {
    #[inline]
    pub fn execute_defi_subscribe(&mut self, cmd: &DefiSubscribeCommand) {
        if let Err(e) = match cmd {
            DefiSubscribeCommand::Blocks(cmd) => self.subscribe_blocks(cmd),
            DefiSubscribeCommand::Pool(cmd) => self.subscribe_pool(cmd),
            DefiSubscribeCommand::PoolSwaps(cmd) => self.subscribe_pool_swaps(cmd),
            DefiSubscribeCommand::PoolLiquidityUpdates(cmd) => {
                self.subscribe_pool_liquidity_updates(cmd)
            }
            DefiSubscribeCommand::PoolFeeCollects(cmd) => self.subscribe_pool_fee_collects(cmd),
            DefiSubscribeCommand::PoolFlashEvents(cmd) => self.subscribe_pool_flash_events(cmd),
        } {
            log_command_error(&cmd, &e);
        }
    }

    #[inline]
    pub fn execute_defi_unsubscribe(&mut self, cmd: &DefiUnsubscribeCommand) {
        if let Err(e) = match cmd {
            DefiUnsubscribeCommand::Blocks(cmd) => self.unsubscribe_blocks(cmd),
            DefiUnsubscribeCommand::Pool(cmd) => self.unsubscribe_pool(cmd),
            DefiUnsubscribeCommand::PoolSwaps(cmd) => self.unsubscribe_pool_swaps(cmd),
            DefiUnsubscribeCommand::PoolLiquidityUpdates(cmd) => {
                self.unsubscribe_pool_liquidity_updates(cmd)
            }
            DefiUnsubscribeCommand::PoolFeeCollects(cmd) => self.unsubscribe_pool_fee_collects(cmd),
            DefiUnsubscribeCommand::PoolFlashEvents(cmd) => self.unsubscribe_pool_flash_events(cmd),
        } {
            log_command_error(&cmd, &e);
        }
    }

    /// Executes a DeFi data request command by dispatching to the appropriate handler.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client request fails.
    #[inline]
    pub fn execute_defi_request(&self, cmd: &DefiRequestCommand) -> anyhow::Result<()> {
        match cmd {
            DefiRequestCommand::PoolSnapshot(cmd) => self.request_pool_snapshot(cmd),
        }
    }

    /// Subscribes to block events for the specified blockchain.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_blocks(&mut self, cmd: &SubscribeBlocks) -> anyhow::Result<()> {
        if !self.subscriptions_blocks.contains(&cmd.chain) {
            self.subscriptions_blocks.insert(cmd.chain);
            self.client.subscribe_blocks(cmd)?;
        }
        Ok(())
    }

    /// Unsubscribes from block events for the specified blockchain.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_blocks(&mut self, cmd: &UnsubscribeBlocks) -> anyhow::Result<()> {
        if self.subscriptions_blocks.contains(&cmd.chain) {
            self.subscriptions_blocks.remove(&cmd.chain);
            self.client.unsubscribe_blocks(cmd)?;
        }
        Ok(())
    }

    /// Subscribes to pool definition updates for the specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_pool(&mut self, cmd: &SubscribePool) -> anyhow::Result<()> {
        if !self.subscriptions_pools.contains(&cmd.instrument_id) {
            self.subscriptions_pools.insert(cmd.instrument_id);
            self.client.subscribe_pool(cmd)?;
        }
        Ok(())
    }

    /// Subscribes to pool swap events for the specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_pool_swaps(&mut self, cmd: &SubscribePoolSwaps) -> anyhow::Result<()> {
        if !self.subscriptions_pool_swaps.contains(&cmd.instrument_id) {
            self.subscriptions_pool_swaps.insert(cmd.instrument_id);
            self.client.subscribe_pool_swaps(cmd)?;
        }
        Ok(())
    }

    /// Subscribes to pool liquidity update events for the specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_pool_liquidity_updates(
        &mut self,
        cmd: &SubscribePoolLiquidityUpdates,
    ) -> anyhow::Result<()> {
        if !self
            .subscriptions_pool_liquidity_updates
            .contains(&cmd.instrument_id)
        {
            self.subscriptions_pool_liquidity_updates
                .insert(cmd.instrument_id);
            self.client.subscribe_pool_liquidity_updates(cmd)?;
        }
        Ok(())
    }

    /// Subscribes to pool fee collect events for the specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_pool_fee_collects(
        &mut self,
        cmd: &SubscribePoolFeeCollects,
    ) -> anyhow::Result<()> {
        if !self
            .subscriptions_pool_fee_collects
            .contains(&cmd.instrument_id)
        {
            self.subscriptions_pool_fee_collects
                .insert(cmd.instrument_id);
            self.client.subscribe_pool_fee_collects(cmd)?;
        }
        Ok(())
    }

    /// Subscribes to pool flash loan events for the specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client subscribe operation fails.
    fn subscribe_pool_flash_events(
        &mut self,
        cmd: &SubscribePoolFlashEvents,
    ) -> anyhow::Result<()> {
        if !self.subscriptions_pool_flash.contains(&cmd.instrument_id) {
            self.subscriptions_pool_flash.insert(cmd.instrument_id);
            self.client.subscribe_pool_flash_events(cmd)?;
        }
        Ok(())
    }

    /// Unsubscribes from pool definition updates for the specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_pool(&mut self, cmd: &UnsubscribePool) -> anyhow::Result<()> {
        if self.subscriptions_pools.contains(&cmd.instrument_id) {
            self.subscriptions_pools.remove(&cmd.instrument_id);
            self.client.unsubscribe_pool(cmd)?;
        }
        Ok(())
    }

    /// Unsubscribes from swap events for the specified AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client unsubscribe operation fails.
    fn unsubscribe_pool_swaps(&mut self, cmd: &UnsubscribePoolSwaps) -> anyhow::Result<()> {
        if self.subscriptions_pool_swaps.contains(&cmd.instrument_id) {
            self.subscriptions_pool_swaps.remove(&cmd.instrument_id);
            self.client.unsubscribe_pool_swaps(cmd)?;
        }
        Ok(())
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
        if self
            .subscriptions_pool_liquidity_updates
            .contains(&cmd.instrument_id)
        {
            self.subscriptions_pool_liquidity_updates
                .remove(&cmd.instrument_id);
            self.client.unsubscribe_pool_liquidity_updates(cmd)?;
        }
        Ok(())
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
        if self
            .subscriptions_pool_fee_collects
            .contains(&cmd.instrument_id)
        {
            self.subscriptions_pool_fee_collects
                .remove(&cmd.instrument_id);
            self.client.unsubscribe_pool_fee_collects(cmd)?;
        }
        Ok(())
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
        if self.subscriptions_pool_flash.contains(&cmd.instrument_id) {
            self.subscriptions_pool_flash.remove(&cmd.instrument_id);
            self.client.unsubscribe_pool_flash_events(cmd)?;
        }
        Ok(())
    }

    /// Sends a pool snapshot request for a given AMM pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to process the pool snapshot request.
    pub fn request_pool_snapshot(&self, req: &RequestPoolSnapshot) -> anyhow::Result<()> {
        self.client.request_pool_snapshot(req)
    }
}

#[inline(always)]
fn log_command_error<C: Debug, E: Display>(cmd: &C, e: &E) {
    log::error!("Error on {cmd:?}: {e}");
}
