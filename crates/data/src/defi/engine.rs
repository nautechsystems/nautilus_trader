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
    defi::{Blockchain, DefiData, PoolProfiler, data::DexPoolData},
    identifiers::{ClientId, InstrumentId},
};

use crate::engine::{DataEngine, pool::PoolUpdater};

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
            log::info!("Forwarding subscription to client {:?}", cmd.client_id());
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
                self.setup_pool_updater(&cmd.instrument_id, cmd.client_id.as_ref())
            }
            DefiSubscribeCommand::PoolSwaps(cmd) => {
                self.setup_pool_updater(&cmd.instrument_id, cmd.client_id.as_ref())
            }
            DefiSubscribeCommand::PoolLiquidityUpdates(cmd) => {
                self.setup_pool_updater(&cmd.instrument_id, cmd.client_id.as_ref())
            }
            DefiSubscribeCommand::PoolFeeCollects(cmd) => {
                self.setup_pool_updater(&cmd.instrument_id, cmd.client_id.as_ref())
            }
            DefiSubscribeCommand::PoolFlashEvents(cmd) => {
                self.setup_pool_updater(&cmd.instrument_id, cmd.client_id.as_ref())
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

                // Check if we're waiting for this snapshot
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
                let mut profiler = PoolProfiler::new(pool.clone());
                if let Err(e) = profiler.restore_from_snapshot(snapshot.clone()) {
                    log::error!(
                        "Failed to restore profiler from snapshot for {instrument_id}: {e}"
                    );
                    return;
                }

                log::debug!("Restored pool profiler for {instrument_id} from snapshot");

                // Get buffered events for this pool and sort by block position
                let buffered_events = self
                    .pool_event_buffers
                    .remove(&instrument_id)
                    .unwrap_or_default();

                if !buffered_events.is_empty() {
                    log::info!(
                        "Processing {} buffered events for {instrument_id}",
                        buffered_events.len()
                    );

                    // Convert to DexPoolData and sort by block position
                    let mut events_to_apply: Vec<DexPoolData> = Vec::new();

                    for event in buffered_events {
                        match event {
                            DefiData::PoolSwap(swap) => {
                                events_to_apply.push(DexPoolData::Swap(swap))
                            }
                            DefiData::PoolLiquidityUpdate(update) => {
                                events_to_apply.push(DexPoolData::LiquidityUpdate(update))
                            }
                            DefiData::PoolFeeCollect(collect) => {
                                events_to_apply.push(DexPoolData::FeeCollect(collect))
                            }
                            DefiData::PoolFlash(flash) => {
                                events_to_apply.push(DexPoolData::Flash(flash))
                            }
                            _ => {} // Skip non-pool events
                        }
                    }

                    // Sort events by block position
                    events_to_apply.sort_by(|a, b| {
                        let pos_a = match a {
                            DexPoolData::Swap(s) => (s.block, s.transaction_index, s.log_index),
                            DexPoolData::LiquidityUpdate(u) => {
                                (u.block, u.transaction_index, u.log_index)
                            }
                            DexPoolData::FeeCollect(c) => {
                                (c.block, c.transaction_index, c.log_index)
                            }
                            DexPoolData::Flash(f) => (f.block, f.transaction_index, f.log_index),
                        };
                        let pos_b = match b {
                            DexPoolData::Swap(s) => (s.block, s.transaction_index, s.log_index),
                            DexPoolData::LiquidityUpdate(u) => {
                                (u.block, u.transaction_index, u.log_index)
                            }
                            DexPoolData::FeeCollect(c) => {
                                (c.block, c.transaction_index, c.log_index)
                            }
                            DexPoolData::Flash(f) => (f.block, f.transaction_index, f.log_index),
                        };
                        pos_a.cmp(&pos_b)
                    });

                    // Apply events that occurred after the snapshot
                    let snapshot_block = &snapshot.block_position;
                    let mut applied_count = 0;

                    for event in events_to_apply {
                        let event_block = match &event {
                            DexPoolData::Swap(s) => (s.block, s.transaction_index, s.log_index),
                            DexPoolData::LiquidityUpdate(u) => {
                                (u.block, u.transaction_index, u.log_index)
                            }
                            DexPoolData::FeeCollect(c) => {
                                (c.block, c.transaction_index, c.log_index)
                            }
                            DexPoolData::Flash(f) => (f.block, f.transaction_index, f.log_index),
                        };

                        // Only apply events that occurred after the snapshot
                        if event_block.0 > snapshot_block.number
                            || (event_block.0 == snapshot_block.number
                                && event_block.1 > snapshot_block.transaction_index)
                            || (event_block.0 == snapshot_block.number
                                && event_block.1 == snapshot_block.transaction_index
                                && event_block.2 > snapshot_block.log_index)
                        {
                            if let Err(e) = profiler.process(&event) {
                                log::error!(
                                    "Failed to apply buffered event to profiler for {instrument_id}: {e}"
                                );
                            } else {
                                applied_count += 1;
                            }
                        }
                    }

                    log::info!(
                        "Applied {applied_count} buffered events to profiler for {instrument_id}"
                    );
                }

                // Add profiler to cache
                if let Err(e) = self.cache.borrow_mut().add_pool_profiler(profiler) {
                    log::error!("Failed to add pool profiler to cache for {instrument_id}: {e}");
                    return;
                }

                // Remove from pending and create updater
                self.pool_snapshot_pending.remove(&instrument_id);

                let updater = Rc::new(PoolUpdater::new(&instrument_id, self.cache.clone()));
                let handler = ShareableMessageHandler(updater.clone());

                // Subscribe to all required pool data topics
                let swap_topic = defi::switchboard::get_defi_pool_swaps_topic(instrument_id);
                if !msgbus::is_subscribed(swap_topic.as_str(), handler.clone()) {
                    msgbus::subscribe(
                        swap_topic.into(),
                        handler.clone(),
                        Some(self.msgbus_priority),
                    );
                }

                let liquidity_topic = defi::switchboard::get_defi_liquidity_topic(instrument_id);
                if !msgbus::is_subscribed(liquidity_topic.as_str(), handler.clone()) {
                    msgbus::subscribe(
                        liquidity_topic.into(),
                        handler.clone(),
                        Some(self.msgbus_priority),
                    );
                }

                let collect_topic = defi::switchboard::get_defi_collect_topic(instrument_id);
                if !msgbus::is_subscribed(collect_topic.as_str(), handler.clone()) {
                    msgbus::subscribe(
                        collect_topic.into(),
                        handler.clone(),
                        Some(self.msgbus_priority),
                    );
                }

                let flash_topic = defi::switchboard::get_defi_flash_topic(instrument_id);
                if !msgbus::is_subscribed(flash_topic.as_str(), handler.clone()) {
                    msgbus::subscribe(flash_topic.into(), handler, Some(self.msgbus_priority));
                }

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

    fn setup_pool_updater(&mut self, instrument_id: &InstrumentId, client_id: Option<&ClientId>) {
        // Early return if updater already exists
        if self.pool_updaters.contains_key(instrument_id) {
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
                    return;
                }
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
                    self.pool_updaters_pending.insert(*instrument_id);
                } else {
                    log::debug!("Requested pool snapshot for {instrument_id}");
                    self.pool_snapshot_pending.insert(*instrument_id);
                    self.pool_event_buffers.entry(*instrument_id).or_default();
                }
                return;
            }
        }

        // Profiler exists, create updater and subscribe to topics
        let updater = Rc::new(PoolUpdater::new(instrument_id, self.cache.clone()));
        let handler = ShareableMessageHandler(updater.clone());

        let swap_topic = defi::switchboard::get_defi_pool_swaps_topic(*instrument_id);
        if !msgbus::is_subscribed(swap_topic.as_str(), handler.clone()) {
            msgbus::subscribe(
                swap_topic.into(),
                handler.clone(),
                Some(self.msgbus_priority),
            );
        }

        let liquidity_topic = defi::switchboard::get_defi_liquidity_topic(*instrument_id);
        if !msgbus::is_subscribed(liquidity_topic.as_str(), handler.clone()) {
            msgbus::subscribe(
                liquidity_topic.into(),
                handler.clone(),
                Some(self.msgbus_priority),
            );
        }

        let collect_topic = defi::switchboard::get_defi_collect_topic(*instrument_id);
        if !msgbus::is_subscribed(collect_topic.as_str(), handler.clone()) {
            msgbus::subscribe(
                collect_topic.into(),
                handler.clone(),
                Some(self.msgbus_priority),
            );
        }

        let flash_topic = defi::switchboard::get_defi_flash_topic(*instrument_id);
        if !msgbus::is_subscribed(flash_topic.as_str(), handler.clone()) {
            msgbus::subscribe(flash_topic.into(), handler, Some(self.msgbus_priority));
        }

        self.pool_updaters.insert(*instrument_id, updater);
        log::debug!("Created PoolUpdater for instrument ID {instrument_id}");
    }
}
