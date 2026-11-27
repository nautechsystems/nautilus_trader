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

use std::{
    sync::{Arc, Weak},
    time::Duration,
};

use nautilus_common::messages::DataEvent;
use nautilus_core::{DurationNanos, UnixNanos};
use nautilus_live::task::TaskSpawner;
use nautilus_model::instruments::{Instrument, InstrumentAny};
use parking_lot::Mutex;
use rust_decimal::Decimal;

use super::{
    PolymarketDataClient,
    instruments::{InstrumentUpdateState, cache_instrument_unchecked},
};
use crate::{
    filters::market_closed,
    http::{clob::PolymarketClobPublicClient, parse::rebuild_instrument_with_tick_size},
    providers::extract_condition_id,
    resolve::ResolveContext,
};

const TICK_RECOVERY_GRACE: DurationNanos = DurationNanos::from_mins(10);

#[derive(Debug)]
pub(super) struct TickDisagreement {
    proposed: Decimal,
    next_check: UnixNanos,
    // A canceled request releases its ticket. A WS event or a new proposal
    // removes the entry so that a late HTTP response cannot change the grid.
    request: Weak<()>,
}

pub(super) struct TickRecovery {
    pub(super) clob: PolymarketClobPublicClient,
    pub(super) ctx: ResolveContext,
    pub(super) state: Arc<Mutex<InstrumentUpdateState>>,
    pub(super) tasks: TaskSpawner,
    pub(super) book_timeout: Duration,
}

impl PolymarketDataClient {
    pub(super) fn tick_recovery(&self) -> anyhow::Result<TickRecovery> {
        Ok(TickRecovery {
            clob: self.clob_public_client.clone(),
            ctx: self.resolution_context(),
            state: self.instrument_update_state.clone(),
            tasks: self.tasks.spawner()?,
            book_timeout: Duration::from_secs(self.config.book_snapshot_timeout_secs),
        })
    }
}

impl TickRecovery {
    fn begin_check(&self, gamma: &InstrumentAny) -> Option<Arc<()>> {
        let token = gamma.raw_symbol().inner();
        let id = gamma.id();
        let proposed = gamma.price_increment().as_decimal();
        let mut state = self.state.lock();
        let cache = self.ctx.instruments.load();

        if state.retired || self.ctx.cancellation_token.is_cancelled() {
            return None;
        }

        if cache.get(&id).is_none_or(|cached| {
            cached.price_increment() == gamma.price_increment()
                || market_closed(cached) == Some(true)
        }) || market_closed(gamma) == Some(true)
        {
            state.disagreements.remove(&token);
            return None;
        }

        let now = self.ctx.clock.get_time_ns();
        let next_check = now.saturating_add(TICK_RECOVERY_GRACE);
        let fresh = || TickDisagreement {
            proposed,
            next_check,
            request: Weak::new(),
        };
        let pending = state.disagreements.entry(token).or_insert_with(fresh);

        if pending.proposed != proposed {
            *pending = fresh();
        }

        if now < pending.next_check || pending.request.upgrade().is_some() {
            return None;
        }

        let ticket = Arc::new(());
        pending.next_check = next_check;
        pending.request = Arc::downgrade(&ticket);
        Some(ticket)
    }

    pub(super) async fn observe(&self, gamma: &InstrumentAny) {
        let token = gamma.raw_symbol().inner();
        let id = gamma.id();
        let proposed = gamma.price_increment().as_decimal();
        let Some(ticket) = self.begin_check(gamma) else {
            return;
        };

        let result = self.clob.get_tick_size(token.as_str()).await;
        let mut state = self.state.lock();

        if state.retired || self.ctx.cancellation_token.is_cancelled() {
            return;
        }
        let Some(pending) = state.disagreements.get_mut(&token) else {
            return;
        };

        if !pending.request.ptr_eq(&Arc::downgrade(&ticket)) {
            return;
        }

        let now = self.ctx.clock.get_time_ns();
        pending.next_check = now.saturating_add(TICK_RECOVERY_GRACE);

        match result {
            Ok(reply) if reply.minimum_tick_size == proposed => {}
            Ok(_) => return,
            Err(e) => {
                log::warn!("Could not recover tick size for {id}: {e}");
                return;
            }
        }

        let _owners = self.ctx.apply_mutex.lock();
        let terminal = self.ctx.subscriptions.lock_closed_conditions();
        let current = self.ctx.instruments.load();
        let Some(cached) = current.get(&id) else {
            return;
        };

        if market_closed(cached) == Some(true)
            || extract_condition_id(&id).is_ok_and(|c| {
                terminal.contains(&c) || self.ctx.pending_resolutions.contains_key(&c)
            })
        {
            return;
        }
        let rebuilt =
            match rebuild_instrument_with_tick_size(cached, &proposed.to_string(), now, now) {
                Ok(instrument) => instrument,
                Err(e) => {
                    log::warn!("Failed to recover tick size for {id}: {e}");
                    return;
                }
            };
        state.record_recovered_tick(token);
        cache_instrument_unchecked(
            &self.ctx.instruments,
            &self.ctx.subscriptions.token_meta,
            &rebuilt,
        );

        if let Err(e) = self.ctx.data_sender.send(DataEvent::Data(rebuilt.into())) {
            log::warn!("Failed to publish recovered instrument {id}: {e}");
        }
        self.ctx
            .subscriptions
            .reset_tick_book(id, token.as_str(), self.book_timeout, &self.tasks);
    }
}
