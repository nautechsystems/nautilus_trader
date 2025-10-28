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

//! DeFi-specific data engine functionality.
//!
//! This module provides DeFi processing methods for the `DataEngine`.
//! All code in this module requires the `defi` feature flag.

use std::{any::Any, rc::Rc, sync::Arc};

use nautilus_common::{
    defi,
    messages::defi::{
        DefiRequestCommand, DefiSubscribeCommand, DefiUnsubscribeCommand, RequestPoolSnapshot,
    },
    msgbus::{self, handler::ShareableMessageHandler},
};
use nautilus_core::UUID4;
use nautilus_model::{
    defi::{
        Blockchain, DefiData, PoolProfiler,
        data::{DexPoolData, block::BlockPosition},
    },
    identifiers::{ClientId, InstrumentId},
};

use crate::engine::{DataEngine, pool::PoolUpdater};

/// Extracts the block position tuple from a DexPoolData event.
fn get_event_block_position(event: &DexPoolData) -> (u64, u32, u32) {
    match event {
        DexPoolData::Swap(s) => (s.block, s.transaction_index, s.log_index),
        DexPoolData::LiquidityUpdate(u) => (u.block, u.transaction_index, u.log_index),
        DexPoolData::FeeCollect(c) => (c.block, c.transaction_index, c.log_index),
        DexPoolData::Flash(f) => (f.block, f.transaction_index, f.log_index),
    }
}

/// Converts buffered DefiData events to DexPoolData and sorts by block position.
fn convert_and_sort_buffered_events(buffered_events: Vec<DefiData>) -> Vec<DexPoolData> {
    let mut events: Vec<DexPoolData> = buffered_events
        .into_iter()
        .filter_map(|event| match event {
            DefiData::PoolSwap(swap) => Some(DexPoolData::Swap(swap)),
            DefiData::PoolLiquidityUpdate(update) => Some(DexPoolData::LiquidityUpdate(update)),
            DefiData::PoolFeeCollect(collect) => Some(DexPoolData::FeeCollect(collect)),
            DefiData::PoolFlash(flash) => Some(DexPoolData::Flash(flash)),
            _ => None,
        })
        .collect();

    events.sort_by(|a, b| {
        let pos_a = get_event_block_position(a);
        let pos_b = get_event_block_position(b);
        pos_a.cmp(&pos_b)
    });

    events
}

impl DataEngine {
    /// Returns all blockchains for which blocks subscriptions exist.
    #[must_use]
    pub fn subscribed_blocks(&self) -> Vec<Blockchain> {
        self.collect_subscriptions(|client| &client.subscriptions_blocks)
    }

    /// Returns all instrument IDs for which pool subscriptions exist.
    #[must_use]
    pub fn subscribed_pools(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_pools)
    }

    /// Returns all instrument IDs for which swap subscriptions exist.
    #[must_use]
    pub fn subscribed_pool_swaps(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_pool_swaps)
    }

    /// Returns all instrument IDs for which liquidity update subscriptions exist.
    #[must_use]
    pub fn subscribed_pool_liquidity_updates(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_pool_liquidity_updates)
    }

    /// Returns all instrument IDs for which fee collect subscriptions exist.
    #[must_use]
    pub fn subscribed_pool_fee_collects(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_pool_fee_collects)
    }

    /// Returns all instrument IDs for which flash loan subscriptions exist.
    #[must_use]
    pub fn subscribed_pool_flash(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_pool_flash)
    }

    /// Handles a subscribe command, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription is invalid (e.g., synthetic instrument for book data),
    /// or if the underlying client operation fails.
    pub fn execute_defi_subscribe(&mut self, cmd: &DefiSubscribeCommand) -> anyhow::Result<()> {
        if let Some(client_id) = cmd.client_id()
            && self.external_clients.contains(client_id)
        {
            if self.config.debug {
                log::debug!("Skipping defi subscribe for external client {client_id}: {cmd:?}",);
            }
            return Ok(());
        }

        if let Some(client) = self.get_client(cmd.client_id(), cmd.venue()) {
            log::info!("Forwarding subscription to client {}", client.client_id);
            client.execute_defi_subscribe(cmd);
        } else {
            log::error!(
                "Cannot handle command: no client found for client_id={:?}, venue={:?}",
                cmd.client_id(),
                cmd.venue(),
            );
        }

        match cmd {
            DefiSubscribeCommand::Pool(cmd) => {
                self.setup_pool_updater(&cmd.instrument_id, cmd.client_id.as_ref());
            }
            DefiSubscribeCommand::PoolSwaps(cmd) => {
                self.setup_pool_updater(&cmd.instrument_id, cmd.client_id.as_ref());
            }
            DefiSubscribeCommand::PoolLiquidityUpdates(cmd) => {
                self.setup_pool_updater(&cmd.instrument_id, cmd.client_id.as_ref());
            }
            DefiSubscribeCommand::PoolFeeCollects(cmd) => {
                self.setup_pool_updater(&cmd.instrument_id, cmd.client_id.as_ref());
            }
            DefiSubscribeCommand::PoolFlashEvents(cmd) => {
                self.setup_pool_updater(&cmd.instrument_id, cmd.client_id.as_ref());
            }
            DefiSubscribeCommand::Blocks(_) => {} // No pool setup needed for blocks
        }

        Ok(())
    }

    /// Handles an unsubscribe command, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client operation fails.
    pub fn execute_defi_unsubscribe(&mut self, cmd: &DefiUnsubscribeCommand) -> anyhow::Result<()> {
        if let Some(client_id) = cmd.client_id()
            && self.external_clients.contains(client_id)
        {
            if self.config.debug {
                log::debug!("Skipping defi unsubscribe for external client {client_id}: {cmd:?}",);
            }
            return Ok(());
        }

        if let Some(client) = self.get_client(cmd.client_id(), cmd.venue()) {
            client.execute_defi_unsubscribe(cmd);
        } else {
            log::error!(
                "Cannot handle command: no client found for client_id={:?}, venue={:?}",
                cmd.client_id(),
                cmd.venue(),
            );
        }

        Ok(())
    }

    /// Sends a [`DefiRequestCommand`] to a suitable data client implementation.
    ///
    /// # Errors
    ///
    /// Returns an error if no client is found for the given client ID or venue,
    /// or if the client fails to process the request.
    pub fn execute_defi_request(&mut self, req: &DefiRequestCommand) -> anyhow::Result<()> {
        // Skip requests for external clients
        if let Some(cid) = req.client_id()
            && self.external_clients.contains(cid)
        {
            if self.config.debug {
                log::debug!("Skipping defi data request for external client {cid}: {req:?}");
            }
            return Ok(());
        }

        if let Some(client) = self.get_client(req.client_id(), req.venue()) {
            client.execute_defi_request(req)
        } else {
            anyhow::bail!(
                "Cannot handle request: no client found for {:?} {:?}",
                req.client_id(),
                req.venue()
            );
        }
    }

    /// Processes DeFi-specific data events.
    pub fn process_defi_data(&mut self, data: DefiData) {
        match data {
            DefiData::Block(block) => {
                let topic = defi::switchboard::get_defi_blocks_topic(block.chain());
                msgbus::publish(topic, &block as &dyn Any);
            }
            DefiData::Pool(pool) => {
                if let Err(e) = self.cache.borrow_mut().add_pool(pool.clone()) {
                    log::error!("Failed to add Pool to cache: {e}");
                }

                // Check if pool profiler creation was deferred
                if self.pool_updaters_pending.remove(&pool.instrument_id) {
                    log::info!(
                        "Pool {} now loaded, creating deferred pool profiler",
                        pool.instrument_id
                    );
                    self.setup_pool_updater(&pool.instrument_id, None);
                }

                let topic = defi::switchboard::get_defi_pool_topic(pool.instrument_id);
                msgbus::publish(topic, &pool as &dyn Any);
            }
            DefiData::PoolSnapshot(snapshot) => {
                let instrument_id = snapshot.instrument_id;
                log::info!(
                    "Received pool snapshot for {instrument_id} at block {} with {} positions and {} ticks",
                    snapshot.block_position.number,
                    snapshot.positions.len(),
                    snapshot.ticks.len()
                );

                // Validate we're expecting this snapshot
                if !self.pool_snapshot_pending.contains(&instrument_id) {
                    log::warn!(
                        "Received unexpected pool snapshot for {instrument_id} (not in pending set)"
                    );
                    return;
                }

                // Get pool from cache
                let pool = match self.cache.borrow().pool(&instrument_id) {
                    Some(pool) => Arc::new(pool.clone()),
                    None => {
                        log::error!(
                            "Pool {instrument_id} not found in cache when processing snapshot"
                        );
                        return;
                    }
                };

                // Create profiler and restore from snapshot
                let mut profiler = PoolProfiler::new(pool);
                if let Err(e) = profiler.restore_from_snapshot(snapshot.clone()) {
                    log::error!(
                        "Failed to restore profiler from snapshot for {instrument_id}: {e}"
                    );
                    return;
                }
                log::debug!("Restored pool profiler for {instrument_id} from snapshot");

                // Process buffered events
                let buffered_events = self
                    .pool_event_buffers
                    .remove(&instrument_id)
                    .unwrap_or_default();

                if !buffered_events.is_empty() {
                    log::info!(
                        "Processing {} buffered events for {instrument_id}",
                        buffered_events.len()
                    );

                    let events_to_apply = convert_and_sort_buffered_events(buffered_events);
                    let applied_count = Self::apply_buffered_events_to_profiler(
                        &mut profiler,
                        events_to_apply,
                        &snapshot.block_position,
                        instrument_id,
                    );

                    log::info!(
                        "Applied {applied_count} buffered events to profiler for {instrument_id}"
                    );
                }

                // Add profiler to cache
                if let Err(e) = self.cache.borrow_mut().add_pool_profiler(profiler) {
                    log::error!("Failed to add pool profiler to cache for {instrument_id}: {e}");
                    return;
                }

                // Create updater and subscribe to topics
                self.pool_snapshot_pending.remove(&instrument_id);
                let updater = Rc::new(PoolUpdater::new(&instrument_id, self.cache.clone()));
                let handler = ShareableMessageHandler(updater.clone());

                self.subscribe_pool_updater_topics(instrument_id, handler);
                self.pool_updaters.insert(instrument_id, updater);

                log::info!(
                    "Pool profiler setup completed for {instrument_id}, now processing live events"
                );
            }
            DefiData::PoolSwap(swap) => {
                let instrument_id = swap.instrument_id;
                // Buffer if waiting for snapshot, otherwise publish
                if self.pool_snapshot_pending.contains(&instrument_id) {
                    log::debug!("Buffering swap event for {instrument_id} (waiting for snapshot)");
                    self.pool_event_buffers
                        .entry(instrument_id)
                        .or_default()
                        .push(DefiData::PoolSwap(swap));
                } else {
                    let topic = defi::switchboard::get_defi_pool_swaps_topic(instrument_id);
                    msgbus::publish(topic, &swap as &dyn Any);
                }
            }
            DefiData::PoolLiquidityUpdate(update) => {
                let instrument_id = update.instrument_id;
                // Buffer if waiting for snapshot, otherwise publish
                if self.pool_snapshot_pending.contains(&instrument_id) {
                    log::debug!(
                        "Buffering liquidity update event for {instrument_id} (waiting for snapshot)"
                    );
                    self.pool_event_buffers
                        .entry(instrument_id)
                        .or_default()
                        .push(DefiData::PoolLiquidityUpdate(update));
                } else {
                    let topic = defi::switchboard::get_defi_liquidity_topic(instrument_id);
                    msgbus::publish(topic, &update as &dyn Any);
                }
            }
            DefiData::PoolFeeCollect(collect) => {
                let instrument_id = collect.instrument_id;
                // Buffer if waiting for snapshot, otherwise publish
                if self.pool_snapshot_pending.contains(&instrument_id) {
                    log::debug!(
                        "Buffering fee collect event for {instrument_id} (waiting for snapshot)"
                    );
                    self.pool_event_buffers
                        .entry(instrument_id)
                        .or_default()
                        .push(DefiData::PoolFeeCollect(collect));
                } else {
                    let topic = defi::switchboard::get_defi_collect_topic(instrument_id);
                    msgbus::publish(topic, &collect as &dyn Any);
                }
            }
            DefiData::PoolFlash(flash) => {
                let instrument_id = flash.instrument_id;
                // Buffer if waiting for snapshot, otherwise publish
                if self.pool_snapshot_pending.contains(&instrument_id) {
                    log::debug!("Buffering flash event for {instrument_id} (waiting for snapshot)");
                    self.pool_event_buffers
                        .entry(instrument_id)
                        .or_default()
                        .push(DefiData::PoolFlash(flash));
                } else {
                    let topic = defi::switchboard::get_defi_flash_topic(instrument_id);
                    msgbus::publish(topic, &flash as &dyn Any);
                }
            }
        }
    }

    /// Subscribes a pool updater handler to all relevant pool data topics.
    fn subscribe_pool_updater_topics(
        &self,
        instrument_id: InstrumentId,
        handler: ShareableMessageHandler,
    ) {
        let topics = [
            defi::switchboard::get_defi_pool_swaps_topic(instrument_id),
            defi::switchboard::get_defi_liquidity_topic(instrument_id),
            defi::switchboard::get_defi_collect_topic(instrument_id),
            defi::switchboard::get_defi_flash_topic(instrument_id),
        ];

        for topic in topics {
            if !msgbus::is_subscribed(topic.as_str(), handler.clone()) {
                msgbus::subscribe(topic.into(), handler.clone(), Some(self.msgbus_priority));
            }
        }
    }

    /// Applies buffered events to a pool profiler, filtering to events after the snapshot.
    ///
    /// Returns the count of successfully applied events.
    fn apply_buffered_events_to_profiler(
        profiler: &mut PoolProfiler,
        events: Vec<DexPoolData>,
        snapshot_block: &BlockPosition,
        instrument_id: InstrumentId,
    ) -> usize {
        let mut applied_count = 0;

        for event in events {
            let event_block = get_event_block_position(&event);

            // Only apply events that occurred after the snapshot
            let is_after_snapshot = event_block.0 > snapshot_block.number
                || (event_block.0 == snapshot_block.number
                    && event_block.1 > snapshot_block.transaction_index)
                || (event_block.0 == snapshot_block.number
                    && event_block.1 == snapshot_block.transaction_index
                    && event_block.2 > snapshot_block.log_index);

            if is_after_snapshot {
                if let Err(e) = profiler.process(&event) {
                    log::error!(
                        "Failed to apply buffered event to profiler for {instrument_id}: {e}"
                    );
                } else {
                    applied_count += 1;
                }
            }
        }

        applied_count
    }

    fn setup_pool_updater(&mut self, instrument_id: &InstrumentId, client_id: Option<&ClientId>) {
        // Early return if updater already exists or we are in the middle of setting it up.
        if self.pool_updaters.contains_key(instrument_id)
            || self.pool_updaters_pending.contains(instrument_id)
        {
            log::debug!("Pool updater for {instrument_id} already exists");
            return;
        }

        log::info!("Setting up pool updater for {instrument_id}");

        // Check cache state and ensure profiler exists
        {
            let mut cache = self.cache.borrow_mut();

            if cache.pool_profiler(instrument_id).is_some() {
                // Profiler already exists, proceed to create updater
                log::debug!("Pool profiler already exists for {instrument_id}");
            } else if let Some(pool) = cache.pool(instrument_id) {
                // Pool exists but no profiler, create profiler from pool
                let pool = Arc::new(pool.clone());
                let mut pool_profiler = PoolProfiler::new(pool.clone());

                if let Some(initial_sqrt_price_x96) = pool.initial_sqrt_price_x96 {
                    pool_profiler.initialize(initial_sqrt_price_x96);
                    log::debug!(
                        "Initialized pool profiler for {instrument_id} with sqrt_price {initial_sqrt_price_x96}"
                    );
                } else {
                    log::debug!("Created pool profiler for {instrument_id}");
                }

                if let Err(e) = cache.add_pool_profiler(pool_profiler) {
                    log::error!("Failed to add pool profiler for {instrument_id}: {e}");
                    drop(cache);
                    return;
                }
                drop(cache);
            } else {
                // Neither profiler nor pool exists, request snapshot
                drop(cache);

                let request_id = UUID4::new();
                let ts_init = self.clock.borrow().timestamp_ns();
                let request = RequestPoolSnapshot::new(
                    *instrument_id,
                    client_id.copied(),
                    request_id,
                    ts_init,
                    None,
                );

                if let Err(e) =
                    self.execute_defi_request(&DefiRequestCommand::PoolSnapshot(request))
                {
                    log::warn!("Failed to request pool snapshot for {instrument_id}: {e}");
                } else {
                    log::debug!("Requested pool snapshot for {instrument_id}");
                    self.pool_snapshot_pending.insert(*instrument_id);
                    self.pool_updaters_pending.insert(*instrument_id);
                    self.pool_event_buffers.entry(*instrument_id).or_default();
                }
                return;
            }
        }

        // Profiler exists, create updater and subscribe to topics
        let updater = Rc::new(PoolUpdater::new(instrument_id, self.cache.clone()));
        let handler = ShareableMessageHandler(updater.clone());

        self.subscribe_pool_updater_topics(*instrument_id, handler);
        self.pool_updaters.insert(*instrument_id, updater);

        log::debug!("Created PoolUpdater for instrument ID {instrument_id}");
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use alloy_primitives::{Address, I256, U160, U256};
    use nautilus_model::{
        defi::{
            DefiData, PoolFeeCollect, PoolFlash, PoolLiquidityUpdate, PoolSwap,
            chain::chains,
            data::DexPoolData,
            dex::{AmmType, Dex, DexType},
        },
        identifiers::{InstrumentId, Symbol, Venue},
    };
    use rstest::*;

    use super::*;

    // Test fixtures
    #[fixture]
    fn test_instrument_id() -> InstrumentId {
        InstrumentId::new(Symbol::from("ETH/USDC"), Venue::from("UNISWAPV3"))
    }

    #[fixture]
    fn test_chain() -> Arc<nautilus_model::defi::Chain> {
        Arc::new(chains::ETHEREUM.clone())
    }

    #[fixture]
    fn test_dex(test_chain: Arc<nautilus_model::defi::Chain>) -> Arc<Dex> {
        Arc::new(Dex::new(
            (*test_chain).clone(),
            DexType::UniswapV3,
            "0x1F98431c8aD98523631AE4a59f267346ea31F984",
            12369621,
            AmmType::CLAMM,
            "PoolCreated(address,address,uint24,int24,address)",
            "Swap(address,address,int256,int256,uint160,uint128,int24)",
            "Mint(address,address,int24,int24,uint128,uint256,uint256)",
            "Burn(address,int24,int24,uint128,uint256,uint256)",
            "Collect(address,address,int24,int24,uint128,uint128)",
        ))
    }

    fn create_test_swap(
        test_instrument_id: InstrumentId,
        test_chain: Arc<nautilus_model::defi::Chain>,
        test_dex: Arc<Dex>,
        block: u64,
        tx_index: u32,
        log_index: u32,
    ) -> PoolSwap {
        PoolSwap::new(
            test_chain,
            test_dex,
            test_instrument_id,
            Address::ZERO,
            block,
            format!("0x{:064x}", block),
            tx_index,
            log_index,
            None,
            Address::ZERO,
            Address::ZERO,
            I256::ZERO,
            I256::ZERO,
            U160::ZERO,
            0,
            0,
            None,
            None,
            None,
        )
    }

    fn create_test_liquidity_update(
        test_instrument_id: InstrumentId,
        test_chain: Arc<nautilus_model::defi::Chain>,
        test_dex: Arc<Dex>,
        block: u64,
        tx_index: u32,
        log_index: u32,
    ) -> PoolLiquidityUpdate {
        use nautilus_model::defi::PoolLiquidityUpdateType;

        PoolLiquidityUpdate::new(
            test_chain,
            test_dex,
            test_instrument_id,
            Address::ZERO,
            PoolLiquidityUpdateType::Mint,
            block,
            format!("0x{:064x}", block),
            tx_index,
            log_index,
            None,
            Address::ZERO,
            0,
            U256::ZERO,
            U256::ZERO,
            0,
            0,
            None,
        )
    }

    fn create_test_fee_collect(
        test_instrument_id: InstrumentId,
        test_chain: Arc<nautilus_model::defi::Chain>,
        test_dex: Arc<Dex>,
        block: u64,
        tx_index: u32,
        log_index: u32,
    ) -> PoolFeeCollect {
        PoolFeeCollect::new(
            test_chain,
            test_dex,
            test_instrument_id,
            Address::ZERO,
            block,
            format!("0x{:064x}", block),
            tx_index,
            log_index,
            Address::ZERO,
            0,
            0,
            0,
            0,
            None,
        )
    }

    fn create_test_flash(
        test_instrument_id: InstrumentId,
        test_chain: Arc<nautilus_model::defi::Chain>,
        test_dex: Arc<Dex>,
        block: u64,
        tx_index: u32,
        log_index: u32,
    ) -> PoolFlash {
        PoolFlash::new(
            test_chain,
            test_dex,
            test_instrument_id,
            Address::ZERO,
            block,
            format!("0x{:064x}", block),
            tx_index,
            log_index,
            None,
            Address::ZERO,
            Address::ZERO,
            U256::ZERO,
            U256::ZERO,
            U256::ZERO,
            U256::ZERO,
        )
    }

    #[rstest]
    fn test_get_event_block_position_swap(
        test_instrument_id: InstrumentId,
        test_chain: Arc<nautilus_model::defi::Chain>,
        test_dex: Arc<Dex>,
    ) {
        let swap = create_test_swap(test_instrument_id, test_chain, test_dex, 100, 5, 3);
        let pos = get_event_block_position(&DexPoolData::Swap(swap));
        assert_eq!(pos, (100, 5, 3));
    }

    #[rstest]
    fn test_get_event_block_position_liquidity_update(
        test_instrument_id: InstrumentId,
        test_chain: Arc<nautilus_model::defi::Chain>,
        test_dex: Arc<Dex>,
    ) {
        let update =
            create_test_liquidity_update(test_instrument_id, test_chain, test_dex, 200, 10, 7);
        let pos = get_event_block_position(&DexPoolData::LiquidityUpdate(update));
        assert_eq!(pos, (200, 10, 7));
    }

    #[rstest]
    fn test_get_event_block_position_fee_collect(
        test_instrument_id: InstrumentId,
        test_chain: Arc<nautilus_model::defi::Chain>,
        test_dex: Arc<Dex>,
    ) {
        let collect = create_test_fee_collect(test_instrument_id, test_chain, test_dex, 300, 15, 2);
        let pos = get_event_block_position(&DexPoolData::FeeCollect(collect));
        assert_eq!(pos, (300, 15, 2));
    }

    #[rstest]
    fn test_get_event_block_position_flash(
        test_instrument_id: InstrumentId,
        test_chain: Arc<nautilus_model::defi::Chain>,
        test_dex: Arc<Dex>,
    ) {
        let flash = create_test_flash(test_instrument_id, test_chain, test_dex, 400, 20, 8);
        let pos = get_event_block_position(&DexPoolData::Flash(flash));
        assert_eq!(pos, (400, 20, 8));
    }

    #[rstest]
    fn test_convert_and_sort_empty_events() {
        let events = convert_and_sort_buffered_events(vec![]);
        assert!(events.is_empty());
    }

    #[rstest]
    fn test_convert_and_sort_filters_non_pool_events(
        test_instrument_id: InstrumentId,
        test_chain: Arc<nautilus_model::defi::Chain>,
        test_dex: Arc<Dex>,
    ) {
        let events = vec![
            DefiData::PoolSwap(create_test_swap(
                test_instrument_id,
                test_chain,
                test_dex,
                100,
                0,
                0,
            )),
            // Block events would be filtered out
        ];
        let sorted = convert_and_sort_buffered_events(events);
        assert_eq!(sorted.len(), 1);
    }

    #[rstest]
    fn test_convert_and_sort_single_event(
        test_instrument_id: InstrumentId,
        test_chain: Arc<nautilus_model::defi::Chain>,
        test_dex: Arc<Dex>,
    ) {
        let swap = create_test_swap(test_instrument_id, test_chain, test_dex, 100, 5, 3);
        let events = vec![DefiData::PoolSwap(swap)];
        let sorted = convert_and_sort_buffered_events(events);
        assert_eq!(sorted.len(), 1);
        assert_eq!(get_event_block_position(&sorted[0]), (100, 5, 3));
    }

    #[rstest]
    fn test_convert_and_sort_already_sorted(
        test_instrument_id: InstrumentId,
        test_chain: Arc<nautilus_model::defi::Chain>,
        test_dex: Arc<Dex>,
    ) {
        let events = vec![
            DefiData::PoolSwap(create_test_swap(
                test_instrument_id,
                test_chain.clone(),
                test_dex.clone(),
                100,
                0,
                0,
            )),
            DefiData::PoolSwap(create_test_swap(
                test_instrument_id,
                test_chain.clone(),
                test_dex.clone(),
                100,
                0,
                1,
            )),
            DefiData::PoolSwap(create_test_swap(
                test_instrument_id,
                test_chain,
                test_dex,
                100,
                1,
                0,
            )),
        ];
        let sorted = convert_and_sort_buffered_events(events);
        assert_eq!(sorted.len(), 3);
        assert_eq!(get_event_block_position(&sorted[0]), (100, 0, 0));
        assert_eq!(get_event_block_position(&sorted[1]), (100, 0, 1));
        assert_eq!(get_event_block_position(&sorted[2]), (100, 1, 0));
    }

    #[rstest]
    fn test_convert_and_sort_reverse_order(
        test_instrument_id: InstrumentId,
        test_chain: Arc<nautilus_model::defi::Chain>,
        test_dex: Arc<Dex>,
    ) {
        let events = vec![
            DefiData::PoolSwap(create_test_swap(
                test_instrument_id,
                test_chain.clone(),
                test_dex.clone(),
                100,
                2,
                5,
            )),
            DefiData::PoolSwap(create_test_swap(
                test_instrument_id,
                test_chain.clone(),
                test_dex.clone(),
                100,
                1,
                3,
            )),
            DefiData::PoolSwap(create_test_swap(
                test_instrument_id,
                test_chain,
                test_dex,
                100,
                0,
                1,
            )),
        ];
        let sorted = convert_and_sort_buffered_events(events);
        assert_eq!(sorted.len(), 3);
        assert_eq!(get_event_block_position(&sorted[0]), (100, 0, 1));
        assert_eq!(get_event_block_position(&sorted[1]), (100, 1, 3));
        assert_eq!(get_event_block_position(&sorted[2]), (100, 2, 5));
    }

    #[rstest]
    fn test_convert_and_sort_mixed_blocks(
        test_instrument_id: InstrumentId,
        test_chain: Arc<nautilus_model::defi::Chain>,
        test_dex: Arc<Dex>,
    ) {
        let events = vec![
            DefiData::PoolSwap(create_test_swap(
                test_instrument_id,
                test_chain.clone(),
                test_dex.clone(),
                102,
                0,
                0,
            )),
            DefiData::PoolSwap(create_test_swap(
                test_instrument_id,
                test_chain.clone(),
                test_dex.clone(),
                100,
                5,
                2,
            )),
            DefiData::PoolSwap(create_test_swap(
                test_instrument_id,
                test_chain,
                test_dex,
                101,
                3,
                1,
            )),
        ];
        let sorted = convert_and_sort_buffered_events(events);
        assert_eq!(sorted.len(), 3);
        assert_eq!(get_event_block_position(&sorted[0]), (100, 5, 2));
        assert_eq!(get_event_block_position(&sorted[1]), (101, 3, 1));
        assert_eq!(get_event_block_position(&sorted[2]), (102, 0, 0));
    }

    #[rstest]
    fn test_convert_and_sort_mixed_event_types(
        test_instrument_id: InstrumentId,
        test_chain: Arc<nautilus_model::defi::Chain>,
        test_dex: Arc<Dex>,
    ) {
        let events = vec![
            DefiData::PoolSwap(create_test_swap(
                test_instrument_id,
                test_chain.clone(),
                test_dex.clone(),
                100,
                2,
                0,
            )),
            DefiData::PoolLiquidityUpdate(create_test_liquidity_update(
                test_instrument_id,
                test_chain.clone(),
                test_dex.clone(),
                100,
                0,
                0,
            )),
            DefiData::PoolFeeCollect(create_test_fee_collect(
                test_instrument_id,
                test_chain.clone(),
                test_dex.clone(),
                100,
                1,
                0,
            )),
            DefiData::PoolFlash(create_test_flash(
                test_instrument_id,
                test_chain,
                test_dex,
                100,
                3,
                0,
            )),
        ];
        let sorted = convert_and_sort_buffered_events(events);
        assert_eq!(sorted.len(), 4);
        assert_eq!(get_event_block_position(&sorted[0]), (100, 0, 0));
        assert_eq!(get_event_block_position(&sorted[1]), (100, 1, 0));
        assert_eq!(get_event_block_position(&sorted[2]), (100, 2, 0));
        assert_eq!(get_event_block_position(&sorted[3]), (100, 3, 0));
    }

    #[rstest]
    fn test_convert_and_sort_same_block_and_tx_different_log_index(
        test_instrument_id: InstrumentId,
        test_chain: Arc<nautilus_model::defi::Chain>,
        test_dex: Arc<Dex>,
    ) {
        let events = vec![
            DefiData::PoolSwap(create_test_swap(
                test_instrument_id,
                test_chain.clone(),
                test_dex.clone(),
                100,
                5,
                10,
            )),
            DefiData::PoolSwap(create_test_swap(
                test_instrument_id,
                test_chain.clone(),
                test_dex.clone(),
                100,
                5,
                5,
            )),
            DefiData::PoolSwap(create_test_swap(
                test_instrument_id,
                test_chain,
                test_dex,
                100,
                5,
                1,
            )),
        ];
        let sorted = convert_and_sort_buffered_events(events);
        assert_eq!(sorted.len(), 3);
        assert_eq!(get_event_block_position(&sorted[0]), (100, 5, 1));
        assert_eq!(get_event_block_position(&sorted[1]), (100, 5, 5));
        assert_eq!(get_event_block_position(&sorted[2]), (100, 5, 10));
    }
}
