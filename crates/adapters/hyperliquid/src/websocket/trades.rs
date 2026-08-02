// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Typed tracking of venue `trades` streams shared by generic and rich trade data.

use std::sync::Arc;

use dashmap::{DashMap, mapref::entry::Entry};
use ustr::Ustr;

/// A logical consumer of a venue `trades` stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TradeStreamUse {
    Ticks,
    PublicTrades,
}

/// Active logical consumers for one coin's venue `trades` stream.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TradeStreamUses {
    pub ticks: bool,
    pub public_trades: bool,
}

impl TradeStreamUses {
    pub(crate) fn is_empty(self) -> bool {
        !self.ticks && !self.public_trades
    }

    fn set(&mut self, stream_use: TradeStreamUse, active: bool) {
        match stream_use {
            TradeStreamUse::Ticks => self.ticks = active,
            TradeStreamUse::PublicTrades => self.public_trades = active,
        }
    }
}

/// Outcome of registering a logical use of a venue `trades` stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TradeStreamRegistration {
    pub subscribe: bool,
    pub uses: TradeStreamUses,
}

/// Outcome of releasing a logical use of a venue `trades` stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TradeStreamRelease {
    pub unsubscribe: bool,
    pub uses: TradeStreamUses,
}

/// Registry of active venue `trades` streams keyed by coin, shared across client clones.
#[derive(Debug, Clone, Default)]
pub(crate) struct TradeStreamRegistry {
    streams: Arc<DashMap<Ustr, TradeStreamUses>>,
}

impl TradeStreamRegistry {
    /// Records a logical use of the coin's `trades` stream.
    pub(crate) fn register(
        &self,
        coin: Ustr,
        stream_use: TradeStreamUse,
    ) -> TradeStreamRegistration {
        match self.streams.entry(coin) {
            Entry::Occupied(mut occupied) => {
                let uses = occupied.get_mut();
                uses.set(stream_use, true);
                TradeStreamRegistration {
                    subscribe: false,
                    uses: *uses,
                }
            }
            Entry::Vacant(vacant) => {
                let mut uses = TradeStreamUses::default();
                uses.set(stream_use, true);
                vacant.insert(uses);
                TradeStreamRegistration {
                    subscribe: true,
                    uses,
                }
            }
        }
    }

    /// Releases a logical use of the coin's `trades` stream.
    pub(crate) fn release(&self, coin: &Ustr, stream_use: TradeStreamUse) -> TradeStreamRelease {
        match self.streams.entry(*coin) {
            Entry::Occupied(mut occupied) => {
                let uses = occupied.get_mut();
                uses.set(stream_use, false);
                let uses = *uses;
                if uses.is_empty() {
                    occupied.remove();
                }
                TradeStreamRelease {
                    unsubscribe: uses.is_empty(),
                    uses,
                }
            }
            Entry::Vacant(_) => TradeStreamRelease {
                unsubscribe: false,
                uses: TradeStreamUses::default(),
            },
        }
    }

    /// Returns all active logical consumers, for initializing a fresh handler.
    pub(crate) fn snapshot(&self) -> Vec<(Ustr, TradeStreamUses)> {
        self.streams
            .iter()
            .map(|entry| (*entry.key(), *entry.value()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn logical_uses_share_one_venue_subscription() {
        let registry = TradeStreamRegistry::default();
        let coin = Ustr::from("BTC");

        assert!(registry.register(coin, TradeStreamUse::Ticks).subscribe);
        let registration = registry.register(coin, TradeStreamUse::PublicTrades);
        assert!(!registration.subscribe);
        assert_eq!(
            registration.uses,
            TradeStreamUses {
                ticks: true,
                public_trades: true,
            }
        );
    }

    #[rstest]
    fn duplicate_logical_use_is_idempotent() {
        let registry = TradeStreamRegistry::default();
        let coin = Ustr::from("BTC");

        assert!(
            registry
                .register(coin, TradeStreamUse::PublicTrades)
                .subscribe
        );
        assert!(
            !registry
                .register(coin, TradeStreamUse::PublicTrades)
                .subscribe
        );
        assert!(
            registry
                .release(&coin, TradeStreamUse::PublicTrades)
                .unsubscribe
        );
    }

    #[rstest]
    fn releasing_one_use_retains_the_other() {
        let registry = TradeStreamRegistry::default();
        let coin = Ustr::from("BTC");
        registry.register(coin, TradeStreamUse::Ticks);
        registry.register(coin, TradeStreamUse::PublicTrades);

        let release = registry.release(&coin, TradeStreamUse::Ticks);
        assert!(!release.unsubscribe);
        assert!(release.uses.public_trades);

        let release = registry.release(&coin, TradeStreamUse::PublicTrades);
        assert!(release.unsubscribe);
        assert!(release.uses.is_empty());
    }
}
