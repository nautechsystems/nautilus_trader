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

//! Message handler that maintains the `Pool` state stored in the global [`Cache`].
//!
//! The handler is functionally equivalent to `BookUpdater` but for DeFi liquidity
//! pools. Whenever a [`PoolSwap`] or [`PoolLiquidityUpdate`] is published on the
//! message bus the handler looks up the corresponding `Pool` instance in the
//! cache and applies the change in-place (for now we only update the `ts_init`
//! timestamp so that consumers can tell the pool has been touched).

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{cache::Cache, msgbus::handler::MessageHandler};
use nautilus_model::{
    defi::{PoolFeeCollect, PoolFlash, PoolLiquidityUpdate, PoolLiquidityUpdateType, PoolSwap},
    identifiers::InstrumentId,
};
use ustr::Ustr;

/// Handles [`PoolSwap`]s and [`PoolLiquidityUpdate`]s for a single AMM pool.
#[derive(Debug)]
pub struct PoolUpdater {
    id: Ustr,
    instrument_id: InstrumentId,
    cache: Rc<RefCell<Cache>>,
}

impl PoolUpdater {
    /// Creates a new [`PoolUpdater`] bound to the given `instrument_id` and `cache`.
    #[must_use]
    pub fn new(instrument_id: &InstrumentId, cache: Rc<RefCell<Cache>>) -> Self {
        Self {
            id: Ustr::from(&format!("{}-{}", stringify!(PoolUpdater), instrument_id)),
            instrument_id: *instrument_id,
            cache,
        }
    }

    fn handle_pool_swap(&self, swap: &PoolSwap) {
        if let Some(pool_profiler) = self
            .cache
            .borrow_mut()
            .pool_profiler_mut(&self.instrument_id)
            && let Err(e) = pool_profiler.process_swap(swap)
        {
            log::error!("Failed to process pool swap: {e}");
        }
    }

    fn handle_pool_liquidity_update(&self, update: &PoolLiquidityUpdate) {
        if let Some(pool_profiler) = self
            .cache
            .borrow_mut()
            .pool_profiler_mut(&self.instrument_id)
            && let Err(e) = match update.kind {
                PoolLiquidityUpdateType::Mint => pool_profiler.process_mint(update),
                PoolLiquidityUpdateType::Burn => pool_profiler.process_burn(update),
                _ => panic!("Liquidity update operation {} not implemented", update.kind),
            }
        {
            log::error!("Failed to process pool liquidity update: {e}");
        }
    }

    fn handle_pool_fee_collect(&self, event: &PoolFeeCollect) {
        if let Some(pool_profiler) = self
            .cache
            .borrow_mut()
            .pool_profiler_mut(&self.instrument_id)
            && let Err(e) = pool_profiler.process_collect(event)
        {
            log::error!("Failed to process pool fee collect: {e}");
        }
    }

    fn handle_pool_flash(&self, event: &PoolFlash) {
        if let Some(pool_profiler) = self
            .cache
            .borrow_mut()
            .pool_profiler_mut(&self.instrument_id)
            && let Err(e) = pool_profiler.process_flash(event)
        {
            log::error!("Failed to process pool flash: {e}");
        }
    }
}

impl MessageHandler for PoolUpdater {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &dyn Any) {
        if let Some(swap) = message.downcast_ref::<PoolSwap>() {
            self.handle_pool_swap(swap);
            return;
        }

        if let Some(update) = message.downcast_ref::<PoolLiquidityUpdate>() {
            self.handle_pool_liquidity_update(update);
            return;
        }

        if let Some(update) = message.downcast_ref::<PoolFeeCollect>() {
            self.handle_pool_fee_collect(update);
            return;
        }

        if let Some(flash) = message.downcast_ref::<PoolFlash>() {
            self.handle_pool_flash(flash);
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
