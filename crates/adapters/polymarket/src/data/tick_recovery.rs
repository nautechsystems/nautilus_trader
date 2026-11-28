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

use ahash::AHashMap;
use futures_util::stream::{self, StreamExt};
use nautilus_common::messages::DataEvent;
use nautilus_core::{DurationNanos, UnixNanos};
use nautilus_live::task::TaskSpawner;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use parking_lot::Mutex;
use rust_decimal::Decimal;
use serde::{Deserialize, de::IgnoredAny};
use ustr::Ustr;

use super::{
    PolymarketDataClient,
    instruments::{InstrumentUpdateState, cache_instrument_unchecked, definition_superseded},
};
use crate::{
    filters::market_closed,
    http::{clob::PolymarketClobPublicClient, parse::rebuild_instrument_with_tick_size},
    providers::extract_condition_id,
    resolve::ResolveContext,
};

const TICK_RECOVERY_GRACE: DurationNanos = DurationNanos::from_mins(10);
const TICK_RECOVERY_CONCURRENCY: usize = 8;

#[derive(Debug)]
pub(super) struct TickDisagreement {
    proposed: Decimal,
    next_check: UnixNanos,
    // A canceled request releases its ticket. A WS event or a new proposal
    // removes the entry so that a late HTTP response cannot change the grid.
    request: Weak<()>,
}

#[derive(Debug)]
pub(super) struct TickCheck {
    id: InstrumentId,
    token: Ustr,
    proposed: Decimal,
    ticket: Arc<()>,
}

#[derive(Deserialize)]
struct GammaTick {
    #[serde(rename = "orderPriceMinTickSize")]
    tick: Option<IgnoredAny>,
}

#[derive(Clone)]
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
    pub(super) fn observe_batch<'a>(
        &self,
        snapshot: &AHashMap<InstrumentId, InstrumentAny>,
        instruments: impl IntoIterator<Item = &'a InstrumentAny>,
    ) -> Vec<TickCheck> {
        let mut latest = AHashMap::<_, &InstrumentAny>::new();
        for gamma in instruments {
            let current = latest.entry(gamma.raw_symbol().inner()).or_insert(gamma);
            if gamma.ts_init() >= current.ts_init() {
                *current = gamma;
            }
        }

        #[expect(
            clippy::needless_collect,
            reason = "Parse Gamma rows before taking shared locks"
        )]
        let observations: Vec<_> = latest
            .into_values()
            .map(|gamma| {
                let reported = gamma
                    .info()
                    .and_then(|info| info.get_str("gamma_market"))
                    .and_then(|raw| serde_json::from_str::<GammaTick>(raw).ok())
                    .is_some_and(|raw| raw.tick.is_some());
                (gamma, reported)
            })
            .collect();

        let mut state = self.state.lock();
        if state.retired || self.ctx.cancellation_token.is_cancelled() {
            return Vec::new();
        }

        let cache = self.ctx.instruments.load();
        let terminal = self.ctx.subscriptions.lock_closed_conditions();
        observations
            .into_iter()
            .filter_map(|(gamma, reported)| {
                let id = gamma.id();
                if definition_superseded(snapshot, &cache, id)
                    || extract_condition_id(&id).is_ok_and(|id| terminal.contains(&id))
                {
                    return None;
                }

                self.begin_check(gamma, cache.get(&id), reported, &mut state)
            })
            .collect()
    }

    pub(super) fn schedule(&self, checks: Vec<TickCheck>) {
        if checks.is_empty() {
            return;
        }

        let recovery = self.clone();

        let result = self.tasks.spawn(async move {
            stream::iter(checks)
                .for_each_concurrent(TICK_RECOVERY_CONCURRENCY, |check| recovery.check(check))
                .await;
        });

        if let Err(e) = result {
            log::debug!("Skipping Polymarket tick checks after shutdown began: {e}");
        }
    }

    fn begin_check(
        &self,
        gamma: &InstrumentAny,
        cached: Option<&InstrumentAny>,
        reported: bool,
        state: &mut InstrumentUpdateState,
    ) -> Option<TickCheck> {
        let token = gamma.raw_symbol().inner();
        let id = gamma.id();

        let Some(cached) = cached else {
            state.disagreements.remove(&token);
            return None;
        };

        if market_closed(cached) == Some(true) || market_closed(gamma) == Some(true) {
            state.disagreements.remove(&token);
            return None;
        }

        if !reported {
            return None;
        }

        if cached.price_increment() == gamma.price_increment() {
            state.disagreements.remove(&token);
            return None;
        }

        let proposed = gamma.price_increment().as_decimal();
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
        pending.request = Arc::downgrade(&ticket);
        Some(TickCheck {
            id,
            token,
            proposed,
            ticket,
        })
    }

    pub(super) async fn check(&self, check: TickCheck) {
        let TickCheck {
            id,
            token,
            proposed,
            ticket,
        } = check;
        let request = Arc::downgrade(&ticket);

        {
            let mut state = self.state.lock();
            if state.retired || self.ctx.cancellation_token.is_cancelled() {
                return;
            }

            let Some(pending) = state.disagreements.get_mut(&token) else {
                return;
            };

            if !pending.request.ptr_eq(&request) {
                return;
            }

            pending.next_check = self
                .ctx
                .clock
                .get_time_ns()
                .saturating_add(TICK_RECOVERY_GRACE);
        }

        let result = self.clob.get_tick_size(token.as_str()).await;
        let mut state = self.state.lock();

        if state.retired || self.ctx.cancellation_token.is_cancelled() {
            return;
        }
        let Some(pending) = state.disagreements.get_mut(&token) else {
            return;
        };

        if !pending.request.ptr_eq(&request) {
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

        let integrity = self.ctx.subscriptions.book_integrity.lock();
        state.record_recovered_tick(token);
        cache_instrument_unchecked(
            &self.ctx.instruments,
            &self.ctx.subscriptions.token_meta,
            &rebuilt,
        );

        if let Err(e) = self.ctx.data_sender.send(DataEvent::Data(rebuilt.into())) {
            log::warn!("Failed to publish recovered instrument {id}: {e}");
        }

        self.ctx.subscriptions.reset_tick_book(
            id,
            token.as_str(),
            proposed,
            self.book_timeout,
            &self.tasks,
            integrity,
        );
    }
}
