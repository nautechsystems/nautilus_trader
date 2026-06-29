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

use std::sync::{Arc, Mutex as StdMutex};

use nautilus_common::messages::DataEvent;
use nautilus_core::{AtomicMap, time::AtomicTime};
use nautilus_model::{
    data::{Data as NautilusData, InstrumentClose, InstrumentStatus},
    enums::{InstrumentCloseType, MarketStatusAction},
    types::Price,
};

use super::{
    parsing::{
        StrictResolvedMarket, build_resolved_market_from_clob_market, build_strict_resolved_market,
    },
    watchlist::{ResolveWatchEntry, TrackedInstrument},
};
use crate::{
    common::consts::GAMMA_CONDITION_IDS_BATCH_SIZE,
    http::{
        clob::PolymarketClobPublicClient, gamma::PolymarketGammaHttpClient,
        query::GetGammaMarketsParams,
    },
};

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ResolveApplyBatchStats {
    pub(crate) fetched_markets: usize,
    pub(crate) resolved_markets: usize,
    pub(crate) skipped_non_binary_markets: usize,
    pub(crate) clob_fallback_successes: usize,
    pub(crate) emitted_condition_ids: Vec<String>,
    pub(crate) failed_condition_ids: Vec<String>,
    pub(crate) error: Option<String>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResolveBatchErrorMode {
    Continue,
    StopOnFirstError,
}

#[derive(Clone)]
pub(crate) struct ResolveContext {
    pub(crate) clock: &'static AtomicTime,
    pub(crate) data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    pub(crate) watchlist: Arc<AtomicMap<String, ResolveWatchEntry>>,
    pub(crate) apply_mutex: Arc<StdMutex<()>>,
}

pub(crate) async fn fetch_and_apply_resolutions_by_condition_ids(
    gamma_client: &PolymarketGammaHttpClient,
    clob_public_client: &PolymarketClobPublicClient,
    ctx: &ResolveContext,
    condition_ids: &[String],
    error_mode: ResolveBatchErrorMode,
) -> ResolveApplyBatchStats {
    let mut stats = ResolveApplyBatchStats::default();
    let mut unique_condition_ids = condition_ids.to_vec();
    unique_condition_ids.sort();
    unique_condition_ids.dedup();

    for chunk in unique_condition_ids.chunks(GAMMA_CONDITION_IDS_BATCH_SIZE) {
        let mut unresolved_in_chunk: Vec<String> = chunk.to_vec();
        let params = GetGammaMarketsParams {
            condition_ids: Some(chunk.join(",")),
            closed: Some(true),
            ..Default::default()
        };

        match gamma_client.request_markets_by_params(params).await {
            Ok(markets) => {
                stats.fetched_markets += markets.len();
                let mut skipped_in_chunk = 0;
                let resolved_by_condition = markets
                    .into_iter()
                    .filter_map(|market| match build_strict_resolved_market(&market) {
                        Some(resolved) => Some((resolved.condition_id.clone(), resolved)),
                        None => {
                            skipped_in_chunk += 1;
                            None
                        }
                    })
                    .collect::<ahash::AHashMap<String, StrictResolvedMarket>>();
                stats.skipped_non_binary_markets += skipped_in_chunk;

                for condition_id in chunk {
                    let Some(resolved) = resolved_by_condition.get(condition_id) else {
                        continue;
                    };

                    stats.resolved_markets += 1;
                    let emitted = apply_condition_resolution(
                        ctx,
                        &resolved.condition_id,
                        &resolved.winning_asset_id,
                        &resolved.winning_outcome,
                    );

                    if emitted > 0 {
                        stats
                            .emitted_condition_ids
                            .push(resolved.condition_id.clone());
                    }
                }

                unresolved_in_chunk
                    .retain(|condition_id| !resolved_by_condition.contains_key(condition_id));
            }
            Err(e) => {
                let message = format!(
                    "Resolve request failed for {} condition_id(s): {e}",
                    chunk.len()
                );
                log::warn!("{message}");
            }
        }

        for condition_id in unresolved_in_chunk {
            match clob_public_client.get_market(&condition_id).await {
                Ok(market) => {
                    let Some(resolved) = build_resolved_market_from_clob_market(&market) else {
                        continue;
                    };

                    log::debug!(
                        "Resolve fallback via CLOB succeeded for condition_id={}",
                        resolved.condition_id
                    );
                    stats.clob_fallback_successes += 1;
                    stats.resolved_markets += 1;
                    let emitted = apply_condition_resolution(
                        ctx,
                        &resolved.condition_id,
                        &resolved.winning_asset_id,
                        &resolved.winning_outcome,
                    );

                    if emitted > 0 {
                        stats
                            .emitted_condition_ids
                            .push(resolved.condition_id.clone());
                    }
                }
                Err(e) => {
                    let message = format!(
                        "Resolve fallback via CLOB failed for condition_id={condition_id}: {e}"
                    );
                    log::warn!("{message}");
                    if stats.error.is_none() {
                        stats.error = Some(message);
                    }
                    stats.failed_condition_ids.push(condition_id);
                }
            }
        }

        if error_mode == ResolveBatchErrorMode::StopOnFirstError
            && !stats.failed_condition_ids.is_empty()
        {
            break;
        }
    }

    stats.failed_condition_ids.sort();
    stats.failed_condition_ids.dedup();
    stats.emitted_condition_ids.sort();
    stats.emitted_condition_ids.dedup();

    if !unique_condition_ids.is_empty() {
        log::debug!(
            "Polymarket resolve batch requested={} fetched={} resolved={} emitted={} skipped_non_binary={} clob_fallback_successes={} failed={}",
            unique_condition_ids.len(),
            stats.fetched_markets,
            stats.resolved_markets,
            stats.emitted_condition_ids.len(),
            stats.skipped_non_binary_markets,
            stats.clob_fallback_successes,
            stats.failed_condition_ids.len(),
        );
    }

    stats
}

pub(crate) fn merge_resolve_watch_entry(ctx: &ResolveContext, entry: ResolveWatchEntry) {
    let _guard = ctx
        .apply_mutex
        .lock()
        .expect("resolve_apply_mutex poisoned");
    let condition_id = entry.condition_id.clone();
    let incoming_expiration_ns = entry.expiration_ns;
    let incoming_paused = entry.paused;
    let incoming_tracked = entry.tracked;

    ctx.watchlist.rcu(|entries| {
        if let Some(existing) = entries.get_mut(&condition_id) {
            existing.expiration_ns = existing.expiration_ns.max(incoming_expiration_ns);
            existing.paused |= incoming_paused;

            for (token_id, incoming) in &incoming_tracked {
                if let Some(current) = existing.tracked.get_mut(token_id.as_str()) {
                    current
                        .open_position_ids
                        .extend(incoming.open_position_ids.iter().copied());
                } else {
                    existing.tracked.insert(token_id.clone(), incoming.clone());
                }
            }
        } else {
            entries.insert(
                condition_id.clone(),
                ResolveWatchEntry {
                    condition_id: condition_id.clone(),
                    expiration_ns: incoming_expiration_ns,
                    tracked: incoming_tracked.clone(),
                    paused: incoming_paused,
                },
            );
        }
    });
}

pub(crate) fn apply_condition_resolution(
    ctx: &ResolveContext,
    condition_id: &str,
    winning_asset_id: &str,
    winning_outcome: &str,
) -> usize {
    let entry = {
        let _guard = ctx
            .apply_mutex
            .lock()
            .expect("resolve_apply_mutex poisoned");
        let Some(entry) = ctx.watchlist.get_cloned(&condition_id.to_string()) else {
            log::debug!(
                "Ignoring resolution for condition_id={condition_id}: no local watch entry"
            );
            return 0;
        };

        ctx.watchlist.remove(&condition_id.to_string());
        entry
    };

    if entry.tracked.is_empty() {
        return 0;
    }

    let ts_init = ctx.clock.get_time_ns();
    let reason = ustr::Ustr::from(&format!("Winner: {winning_asset_id} ({winning_outcome})"));
    let tracked_instruments: Vec<TrackedInstrument> = entry.tracked.values().cloned().collect();

    for tracked in &tracked_instruments {
        let status = InstrumentStatus::new(
            tracked.instrument_id,
            MarketStatusAction::Close,
            ts_init,
            ts_init,
            Some(reason),
            None,
            Some(false),
            None,
            None,
        );

        if let Err(e) = ctx.data_sender.send(DataEvent::InstrumentStatus(status)) {
            log::error!(
                "Failed to emit instrument status for {}: {e}",
                tracked.instrument_id
            );
            merge_resolve_watch_entry(ctx, entry);
            return 0;
        }

        let close_price = if tracked.token_id == winning_asset_id {
            Price::from_decimal_dp(rust_decimal::Decimal::ONE, tracked.price_precision)
                .expect("valid decimal close price")
        } else {
            Price::from_decimal_dp(rust_decimal::Decimal::ZERO, tracked.price_precision)
                .expect("valid decimal close price")
        };
        let close = InstrumentClose::new(
            tracked.instrument_id,
            close_price,
            InstrumentCloseType::ContractExpired,
            ts_init,
            ts_init,
        );

        if let Err(e) = ctx
            .data_sender
            .send(DataEvent::Data(NautilusData::InstrumentClose(close)))
        {
            log::error!(
                "Failed to emit instrument close for {}: {e}",
                tracked.instrument_id
            );
            merge_resolve_watch_entry(ctx, entry);
            return 0;
        }
    }

    tracked_instruments.len()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex as StdMutex};

    use ahash::AHashSet;
    use nautilus_common::messages::DataEvent;
    use nautilus_core::{AtomicMap, UnixNanos, time::get_atomic_clock_realtime};
    use nautilus_model::identifiers::{InstrumentId, PositionId};
    use rstest::rstest;

    use super::*;

    fn make_resolve_context() -> (
        ResolveContext,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
        let (data_tx, data_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let ctx = ResolveContext {
            clock: get_atomic_clock_realtime(),
            data_sender: data_tx,
            watchlist: Arc::new(AtomicMap::new()),
            apply_mutex: Arc::new(StdMutex::new(())),
        };

        (ctx, data_rx)
    }

    #[rstest]
    fn merge_resolve_watch_entry_unions_existing_token_state() {
        let (ctx, _data_rx) = make_resolve_context();
        let condition_id = "0xCOND-BTC".to_string();
        let token_yes = "0xTOKEN_YES".to_string();
        let token_no = "0xTOKEN_NO".to_string();
        let mut existing_tracked = ahash::AHashMap::new();
        existing_tracked.insert(
            token_yes.clone(),
            TrackedInstrument {
                instrument_id: InstrumentId::from("0xCOND-BTC-0xTOKEN_YES.POLYMARKET"),
                token_id: token_yes.clone(),
                price_precision: 3,
                open_position_ids: AHashSet::from_iter([PositionId::new("P-EXISTING")]),
            },
        );
        ctx.watchlist.insert(
            condition_id.clone(),
            ResolveWatchEntry {
                condition_id: condition_id.clone(),
                expiration_ns: UnixNanos::from(1_000),
                tracked: existing_tracked,
                paused: false,
            },
        );

        let mut incoming_tracked = ahash::AHashMap::new();
        incoming_tracked.insert(
            token_yes.clone(),
            TrackedInstrument {
                instrument_id: InstrumentId::from("0xCOND-BTC-0xTOKEN_YES.POLYMARKET"),
                token_id: token_yes.clone(),
                price_precision: 3,
                open_position_ids: AHashSet::from_iter([PositionId::new("P-INCOMING")]),
            },
        );
        incoming_tracked.insert(
            token_no.clone(),
            TrackedInstrument {
                instrument_id: InstrumentId::from("0xCOND-BTC-0xTOKEN_NO.POLYMARKET"),
                token_id: token_no,
                price_precision: 3,
                open_position_ids: AHashSet::from_iter([PositionId::new("P-NO")]),
            },
        );

        merge_resolve_watch_entry(
            &ctx,
            ResolveWatchEntry {
                condition_id: condition_id.clone(),
                expiration_ns: UnixNanos::from(2_000),
                tracked: incoming_tracked,
                paused: true,
            },
        );

        let watchlist = ctx.watchlist.load();
        let entry = watchlist
            .get(&condition_id)
            .expect("expected merged condition entry");
        assert!(entry.paused);
        assert_eq!(entry.expiration_ns, UnixNanos::from(2_000));
        assert_eq!(entry.tracked.len(), 2);
        let yes = entry
            .tracked
            .get(&token_yes)
            .expect("expected merged yes token");
        assert!(
            yes.open_position_ids
                .contains(&PositionId::new("P-EXISTING"))
        );
        assert!(
            yes.open_position_ids
                .contains(&PositionId::new("P-INCOMING"))
        );
    }
}
