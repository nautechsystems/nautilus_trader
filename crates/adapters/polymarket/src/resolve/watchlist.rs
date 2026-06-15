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

use std::sync::Arc;

use ahash::AHashSet;
use nautilus_core::{AtomicMap, UnixNanos};
use nautilus_model::{
    events::PositionEvent,
    identifiers::{InstrumentId, PositionId},
    instruments::{Instrument, InstrumentAny},
};

use crate::{common::consts::POLYMARKET_VENUE, providers::extract_condition_id};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TrackedInstrument {
    pub(crate) instrument_id: InstrumentId,
    pub(crate) token_id: String,
    pub(crate) price_precision: u8,
    pub(crate) open_position_ids: AHashSet<PositionId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolveWatchEntry {
    pub(crate) condition_id: String,
    pub(crate) expiration_ns: UnixNanos,
    pub(crate) tracked: ahash::AHashMap<String, TrackedInstrument>,
    pub(crate) paused: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolveWatchSelectionMode {
    AutoPoll,
    ManualFallback,
    ManualAllEligible,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ResolveWatchSelection {
    pub(crate) condition_ids: Vec<String>,
    pub(crate) skipped_not_expired: usize,
    pub(crate) timed_out_watchlist: usize,
    pub(crate) paused_watchlist: usize,
    pub(crate) min_ready_in_secs: Option<u64>,
    pub(crate) pause_condition_ids: Vec<String>,
}

pub(crate) fn instrument_market_context(
    instrument: &InstrumentAny,
) -> (Option<String>, Option<String>, Option<String>) {
    match instrument {
        InstrumentAny::BinaryOption(binary) => {
            let slug = binary
                .info
                .as_ref()
                .and_then(|info| info.get_str("market_slug"))
                .map(ToString::to_string);
            let market_id = binary
                .info
                .as_ref()
                .and_then(|info| info.get_str("market_id"))
                .map(ToString::to_string);
            let condition_id = binary
                .info
                .as_ref()
                .and_then(|info| info.get_str("condition_id"))
                .map(ToString::to_string);
            (slug, market_id, condition_id)
        }
        _ => (None, None, None),
    }
}

fn binary_option_context(
    instrument: &InstrumentAny,
) -> Option<(String, String, UnixNanos, TrackedInstrument)> {
    if !matches!(instrument, InstrumentAny::BinaryOption(_)) {
        return None;
    }

    let expiration_ns = instrument.expiration_ns()?;
    let (_, _, condition_id) = instrument_market_context(instrument);
    let condition_id = condition_id.or_else(|| extract_condition_id(&instrument.id()).ok())?;
    let token_id = instrument.raw_symbol().as_str().to_string();
    let tracked = TrackedInstrument {
        instrument_id: instrument.id(),
        token_id: token_id.clone(),
        price_precision: instrument.price_precision(),
        open_position_ids: AHashSet::new(),
    };

    Some((condition_id, token_id, expiration_ns, tracked))
}

pub(crate) fn upsert_resolve_watch_entry_from_instrument(
    watchlist: &Arc<AtomicMap<String, ResolveWatchEntry>>,
    instrument: &InstrumentAny,
    position_id: PositionId,
) {
    let Some((condition_id, token_id, expiration_ns, tracked)) = binary_option_context(instrument)
    else {
        return;
    };

    watchlist.rcu(|entries| {
        let entry = entries
            .entry(condition_id.clone())
            .or_insert_with(|| ResolveWatchEntry {
                condition_id: condition_id.clone(),
                expiration_ns,
                tracked: ahash::AHashMap::new(),
                paused: false,
            });
        entry.expiration_ns = expiration_ns;
        entry
            .tracked
            .entry(token_id.clone())
            .and_modify(|existing| {
                existing.open_position_ids.insert(position_id);
            })
            .or_insert_with(|| {
                let mut seeded = tracked.clone();
                seeded.open_position_ids.insert(position_id);
                seeded
            });
    });
}

fn remove_resolve_watch_instrument(
    watchlist: &Arc<AtomicMap<String, ResolveWatchEntry>>,
    instrument: &InstrumentAny,
    position_id: PositionId,
) {
    let Some((condition_id, token_id, _expiration_ns, _tracked)) =
        binary_option_context(instrument)
    else {
        return;
    };

    watchlist.rcu(|entries| {
        let remove_entry = match entries.get_mut(&condition_id) {
            Some(entry) => {
                let remove_token = match entry.tracked.get_mut(&token_id) {
                    Some(tracked) => {
                        tracked.open_position_ids.remove(&position_id);
                        tracked.open_position_ids.is_empty()
                    }
                    None => false,
                };

                if remove_token {
                    entry.tracked.remove(&token_id);
                }
                entry.tracked.is_empty()
            }
            None => false,
        };

        if remove_entry {
            entries.remove(&condition_id);
        }
    });
}

pub(crate) fn update_resolve_watchlist_from_position_event(
    watchlist: &Arc<AtomicMap<String, ResolveWatchEntry>>,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    event: &PositionEvent,
) {
    let instrument_id = event.instrument_id();
    if instrument_id.venue != *POLYMARKET_VENUE {
        return;
    }

    let loaded = instruments.load();
    let Some(instrument) = loaded.get(&instrument_id) else {
        return;
    };

    let position_id = match event {
        PositionEvent::PositionOpened(position) => position.position_id,
        PositionEvent::PositionChanged(position) => position.position_id,
        PositionEvent::PositionClosed(position) => position.position_id,
        PositionEvent::PositionAdjusted(position) => position.position_id,
    };

    match event {
        PositionEvent::PositionClosed(_) => {
            remove_resolve_watch_instrument(watchlist, instrument, position_id);
        }
        PositionEvent::PositionOpened(_)
        | PositionEvent::PositionChanged(_)
        | PositionEvent::PositionAdjusted(_) => {
            upsert_resolve_watch_entry_from_instrument(watchlist, instrument, position_id);
        }
    }
}

pub(crate) fn collect_resolve_watch_selection(
    watchlist: &ahash::AHashMap<String, ResolveWatchEntry>,
    now_ns: UnixNanos,
    grace_secs: u64,
    max_wait_secs: u64,
    mode: ResolveWatchSelectionMode,
) -> ResolveWatchSelection {
    let mut selection = ResolveWatchSelection::default();
    let grace_ns = grace_secs.saturating_mul(1_000_000_000);
    let max_wait_ns = max_wait_secs.saturating_mul(1_000_000_000);

    for (condition_id, entry) in watchlist {
        if entry.tracked.is_empty() {
            continue;
        }

        let ready_at_ns = entry.expiration_ns.as_u64().saturating_add(grace_ns);
        if now_ns.as_u64() < ready_at_ns {
            selection.skipped_not_expired += 1;
            let ready_in_secs = (ready_at_ns - now_ns.as_u64()) / 1_000_000_000;
            selection.min_ready_in_secs = Some(
                selection
                    .min_ready_in_secs
                    .map_or(ready_in_secs, |current| current.min(ready_in_secs)),
            );
            continue;
        }

        let timed_out = now_ns.as_u64() >= entry.expiration_ns.as_u64().saturating_add(max_wait_ns);

        if timed_out {
            selection.timed_out_watchlist += 1;
            if entry.paused {
                selection.paused_watchlist += 1;
            } else {
                selection.pause_condition_ids.push(condition_id.clone());
            }

            if mode == ResolveWatchSelectionMode::AutoPoll {
                continue;
            }
        } else if entry.paused {
            selection.paused_watchlist += 1;

            if mode == ResolveWatchSelectionMode::AutoPoll {
                continue;
            }
        } else if mode == ResolveWatchSelectionMode::ManualFallback {
            continue;
        }

        selection.condition_ids.push(condition_id.clone());
    }

    selection
}

pub(crate) fn pause_resolve_watch_entries(
    watchlist: &Arc<AtomicMap<String, ResolveWatchEntry>>,
    condition_ids: &[String],
) {
    if condition_ids.is_empty() {
        return;
    }

    watchlist.rcu(|entries| {
        for condition_id in condition_ids {
            if let Some(entry) = entries.get_mut(condition_id) {
                entry.paused = true;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use ahash::AHashSet;
    use nautilus_core::{Params, UUID4};
    use nautilus_model::{
        enums::{AssetClass, OrderSide, PositionSide},
        events::{PositionClosed, PositionEvent, PositionOpened},
        identifiers::{AccountId, ClientOrderId, PositionId, StrategyId, Symbol, TraderId},
        instruments::BinaryOption,
        types::{Currency, Money, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    fn stub_instrument(
        raw_symbol: &str,
        price_increment: Price,
        size_increment: Quantity,
    ) -> InstrumentAny {
        let price_precision = price_increment.precision;
        let size_precision = size_increment.precision;
        InstrumentAny::BinaryOption(BinaryOption::new(
            InstrumentId::from(format!("{raw_symbol}.POLYMARKET").as_str()),
            Symbol::new(raw_symbol),
            AssetClass::Alternative,
            Currency::pUSD(),
            UnixNanos::default(),
            UnixNanos::from(u64::MAX),
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    #[derive(Clone, Copy, Default)]
    struct SeedInstrumentContext<'a> {
        market_slug: Option<&'a str>,
        market_id: Option<&'a str>,
        condition_id: Option<&'a str>,
        expiration_ns: Option<UnixNanos>,
    }

    fn seed_instrument_with_context(
        instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        raw_symbol: &str,
        price_increment: Price,
        size_increment: Quantity,
        seed_ctx: SeedInstrumentContext<'_>,
    ) -> InstrumentAny {
        let mut inst = stub_instrument(raw_symbol, price_increment, size_increment);
        if let InstrumentAny::BinaryOption(ref mut binary) = inst {
            if let Some(expiration_ns) = seed_ctx.expiration_ns {
                binary.expiration_ns = expiration_ns;
            }

            let mut info = Params::new();
            info.insert(
                "token_id".to_string(),
                serde_json::Value::String(raw_symbol.to_string()),
            );

            if let Some(market_slug) = seed_ctx.market_slug {
                info.insert(
                    "market_slug".to_string(),
                    serde_json::Value::String(market_slug.to_string()),
                );
            }

            if let Some(market_id) = seed_ctx.market_id {
                info.insert(
                    "market_id".to_string(),
                    serde_json::Value::String(market_id.to_string()),
                );
            }

            if let Some(condition_id) = seed_ctx.condition_id {
                info.insert(
                    "condition_id".to_string(),
                    serde_json::Value::String(condition_id.to_string()),
                );
            }

            binary.info = Some(info);
        }

        instruments.insert(inst.id(), inst.clone());
        inst
    }

    fn stub_position_opened_event_with_position_id(
        instrument_id: InstrumentId,
        position_id: &str,
    ) -> PositionEvent {
        PositionEvent::PositionOpened(PositionOpened {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("STRATEGY-001"),
            instrument_id,
            position_id: PositionId::new(position_id),
            account_id: AccountId::from("ACCOUNT-001"),
            opening_order_id: ClientOrderId::from("ENTRY-1"),
            entry: OrderSide::Buy,
            side: PositionSide::Long,
            signed_qty: 1.0,
            quantity: Quantity::from("1"),
            last_qty: Quantity::from("1"),
            last_px: Price::from("0.75"),
            currency: Currency::pUSD(),
            avg_px_open: 0.75,
            event_id: UUID4::new(),
            ts_event: UnixNanos::from(1),
            ts_init: UnixNanos::from(1),
        })
    }

    fn stub_position_opened_event(instrument_id: InstrumentId) -> PositionEvent {
        stub_position_opened_event_with_position_id(instrument_id, "P-1")
    }

    fn stub_position_closed_event_with_position_id(
        instrument_id: InstrumentId,
        position_id: &str,
    ) -> PositionEvent {
        PositionEvent::PositionClosed(PositionClosed {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("STRATEGY-001"),
            instrument_id,
            position_id: PositionId::new(position_id),
            account_id: AccountId::from("ACCOUNT-001"),
            opening_order_id: ClientOrderId::from("ENTRY-1"),
            closing_order_id: Some(ClientOrderId::from("EXIT-1")),
            entry: OrderSide::Buy,
            side: PositionSide::Flat,
            signed_qty: 0.0,
            quantity: Quantity::from("0"),
            peak_quantity: Quantity::from("1"),
            last_qty: Quantity::from("1"),
            last_px: Price::from("1.0"),
            currency: Currency::pUSD(),
            avg_px_open: 0.75,
            avg_px_close: Some(1.0),
            realized_return: 0.3333333333,
            realized_pnl: Some(Money::new(0.25, Currency::pUSD())),
            unrealized_pnl: Money::new(0.0, Currency::pUSD()),
            duration: 1u64,
            event_id: UUID4::new(),
            ts_opened: UnixNanos::from(1),
            ts_closed: Some(UnixNanos::from(2)),
            ts_event: UnixNanos::from(2),
            ts_init: UnixNanos::from(2),
        })
    }

    fn stub_position_closed_event(instrument_id: InstrumentId) -> PositionEvent {
        stub_position_closed_event_with_position_id(instrument_id, "P-1")
    }

    #[rstest]
    fn position_events_build_condition_level_watch_entries() {
        let watchlist: Arc<AtomicMap<String, ResolveWatchEntry>> = Arc::new(AtomicMap::new());
        let instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>> = Arc::new(AtomicMap::new());
        let expiration_ns = UnixNanos::from(1_000_000_000);
        let yes = seed_instrument_with_context(
            &instruments,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                market_slug: Some("btc-updown-5m"),
                market_id: Some("1778973900"),
                condition_id: Some("0xCOND-BTC"),
                expiration_ns: Some(expiration_ns),
            },
        );
        let no = seed_instrument_with_context(
            &instruments,
            "0xTOKEN_NO",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                market_slug: Some("btc-updown-5m"),
                market_id: Some("1778973900"),
                condition_id: Some("0xCOND-BTC"),
                expiration_ns: Some(expiration_ns),
            },
        );

        update_resolve_watchlist_from_position_event(
            &watchlist,
            &instruments,
            &stub_position_opened_event(yes.id()),
        );
        update_resolve_watchlist_from_position_event(
            &watchlist,
            &instruments,
            &stub_position_opened_event(no.id()),
        );

        let entries = watchlist.load();
        let entry = entries
            .get("0xCOND-BTC")
            .expect("expected watch entry for condition");
        assert_eq!(entry.tracked.len(), 2);
        assert_eq!(
            entry
                .tracked
                .get("0xTOKEN_YES")
                .expect("expected yes tracked")
                .open_position_ids
                .len(),
            1
        );
        assert_eq!(
            entry
                .tracked
                .get("0xTOKEN_NO")
                .expect("expected no tracked")
                .open_position_ids
                .len(),
            1
        );
        assert!(!entry.paused);
        drop(entries);

        update_resolve_watchlist_from_position_event(
            &watchlist,
            &instruments,
            &stub_position_closed_event(yes.id()),
        );
        let entries = watchlist.load();
        let entry = entries
            .get("0xCOND-BTC")
            .expect("expected remaining condition entry");
        assert_eq!(entry.tracked.len(), 1);
        drop(entries);

        update_resolve_watchlist_from_position_event(
            &watchlist,
            &instruments,
            &stub_position_closed_event(no.id()),
        );
        assert!(!watchlist.contains_key(&"0xCOND-BTC".to_string()));
    }

    #[rstest]
    fn position_events_keep_token_watched_until_last_position_id_closes() {
        let watchlist: Arc<AtomicMap<String, ResolveWatchEntry>> = Arc::new(AtomicMap::new());
        let instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>> = Arc::new(AtomicMap::new());
        let expiration_ns = UnixNanos::from(1_000_000_000);
        let yes = seed_instrument_with_context(
            &instruments,
            "0xTOKEN_YES",
            Price::from("0.001"),
            Quantity::from("0.01"),
            SeedInstrumentContext {
                market_slug: Some("btc-updown-5m"),
                market_id: Some("1778973900"),
                condition_id: Some("0xCOND-BTC"),
                expiration_ns: Some(expiration_ns),
            },
        );

        update_resolve_watchlist_from_position_event(
            &watchlist,
            &instruments,
            &stub_position_opened_event_with_position_id(yes.id(), "P-1"),
        );
        update_resolve_watchlist_from_position_event(
            &watchlist,
            &instruments,
            &stub_position_opened_event_with_position_id(yes.id(), "P-2"),
        );

        let entries = watchlist.load();
        let entry = entries
            .get("0xCOND-BTC")
            .expect("expected watch entry for condition");
        let yes_tracked = entry
            .tracked
            .get("0xTOKEN_YES")
            .expect("expected tracked yes token");
        assert_eq!(yes_tracked.open_position_ids.len(), 2);
        drop(entries);

        update_resolve_watchlist_from_position_event(
            &watchlist,
            &instruments,
            &stub_position_closed_event_with_position_id(yes.id(), "P-1"),
        );

        let entries = watchlist.load();
        let entry = entries
            .get("0xCOND-BTC")
            .expect("expected condition still watched");
        let yes_tracked = entry
            .tracked
            .get("0xTOKEN_YES")
            .expect("expected tracked yes token");
        assert_eq!(yes_tracked.open_position_ids.len(), 1);
        assert!(
            yes_tracked
                .open_position_ids
                .contains(&PositionId::new("P-2"))
        );
        drop(entries);

        update_resolve_watchlist_from_position_event(
            &watchlist,
            &instruments,
            &stub_position_closed_event_with_position_id(yes.id(), "P-2"),
        );

        assert!(!watchlist.contains_key(&"0xCOND-BTC".to_string()));
    }

    #[rstest]
    fn resolve_watch_selection_deduplicates_shared_condition_ids_and_pauses_timed_out_entries() {
        let now_ns = UnixNanos::from(2_000_000_000_000);
        let mut watchlist = ahash::AHashMap::new();

        let mut tracked = ahash::AHashMap::new();
        tracked.insert(
            "0xYES".to_string(),
            TrackedInstrument {
                instrument_id: InstrumentId::from("0xCOND-A-0xYES.POLYMARKET"),
                token_id: "0xYES".to_string(),
                price_precision: 3,
                open_position_ids: AHashSet::new(),
            },
        );
        tracked.insert(
            "0xNO".to_string(),
            TrackedInstrument {
                instrument_id: InstrumentId::from("0xCOND-A-0xNO.POLYMARKET"),
                token_id: "0xNO".to_string(),
                price_precision: 3,
                open_position_ids: AHashSet::new(),
            },
        );
        watchlist.insert(
            "0xCOND-A".to_string(),
            ResolveWatchEntry {
                condition_id: "0xCOND-A".to_string(),
                expiration_ns: UnixNanos::from(1_000_000_000_000),
                tracked,
                paused: false,
            },
        );

        let selection = collect_resolve_watch_selection(
            &watchlist,
            now_ns,
            10,
            1800,
            ResolveWatchSelectionMode::AutoPoll,
        );
        assert_eq!(selection.condition_ids, vec!["0xCOND-A".to_string()]);

        let timed_out_now = UnixNanos::from(1_000_000_000_000 + (1900_u64 * 1_000_000_000));
        let selection = collect_resolve_watch_selection(
            &watchlist,
            timed_out_now,
            10,
            1800,
            ResolveWatchSelectionMode::AutoPoll,
        );
        assert!(selection.condition_ids.is_empty());
        assert_eq!(selection.pause_condition_ids, vec!["0xCOND-A".to_string()]);
    }

    #[rstest]
    fn resolve_watch_selection_manual_fallback_only_includes_paused_or_timed_out_entries() {
        let mut watchlist = ahash::AHashMap::new();
        watchlist.insert(
            "0xCOND-PAUSED".to_string(),
            ResolveWatchEntry {
                condition_id: "0xCOND-PAUSED".to_string(),
                expiration_ns: UnixNanos::from(1_000_000_000_000),
                tracked: ahash::AHashMap::from_iter([(
                    "0xYES".to_string(),
                    TrackedInstrument {
                        instrument_id: InstrumentId::from("0xCOND-PAUSED-0xYES.POLYMARKET"),
                        token_id: "0xYES".to_string(),
                        price_precision: 3,
                        open_position_ids: AHashSet::new(),
                    },
                )]),
                paused: true,
            },
        );
        watchlist.insert(
            "0xCOND-ACTIVE".to_string(),
            ResolveWatchEntry {
                condition_id: "0xCOND-ACTIVE".to_string(),
                expiration_ns: UnixNanos::from(1_000_000_000_000),
                tracked: ahash::AHashMap::from_iter([(
                    "0xYES".to_string(),
                    TrackedInstrument {
                        instrument_id: InstrumentId::from("0xCOND-ACTIVE-0xYES.POLYMARKET"),
                        token_id: "0xYES".to_string(),
                        price_precision: 3,
                        open_position_ids: AHashSet::new(),
                    },
                )]),
                paused: false,
            },
        );

        let selection = collect_resolve_watch_selection(
            &watchlist,
            UnixNanos::from(1_100_000_000_000),
            10,
            1800,
            ResolveWatchSelectionMode::ManualFallback,
        );
        assert_eq!(selection.condition_ids, vec!["0xCOND-PAUSED".to_string()]);
    }

    #[rstest]
    fn resolve_watch_selection_manual_all_eligible_includes_expired_unpaused_entries() {
        let mut watchlist = ahash::AHashMap::new();
        watchlist.insert(
            "0xCOND-ACTIVE".to_string(),
            ResolveWatchEntry {
                condition_id: "0xCOND-ACTIVE".to_string(),
                expiration_ns: UnixNanos::from(1_000_000_000_000),
                tracked: ahash::AHashMap::from_iter([(
                    "0xYES".to_string(),
                    TrackedInstrument {
                        instrument_id: InstrumentId::from("0xCOND-ACTIVE-0xYES.POLYMARKET"),
                        token_id: "0xYES".to_string(),
                        price_precision: 3,
                        open_position_ids: AHashSet::new(),
                    },
                )]),
                paused: false,
            },
        );

        let selection = collect_resolve_watch_selection(
            &watchlist,
            UnixNanos::from(1_100_000_000_000),
            10,
            1800,
            ResolveWatchSelectionMode::ManualAllEligible,
        );
        assert_eq!(selection.condition_ids, vec!["0xCOND-ACTIVE".to_string()]);
    }
}
