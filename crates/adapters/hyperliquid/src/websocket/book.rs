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

//! Typed tracking of venue `l2Book` streams shared by deltas and depth10 uses.
//!
//! One venue `l2Book` subscription per coin serves both order book deltas and
//! depth10 snapshots. This registry records which logical uses are active and
//! the precision options the stream was opened with, so that:
//!
//! - only the first logical use sends a venue subscribe (first-wins options),
//! - releasing one use keeps the stream while another use remains,
//! - reconnect and unsubscribe replay the original subscription shape instead
//!   of a lossy default reconstructed from topic text.

use std::sync::Arc;

use dashmap::{DashMap, mapref::entry::Entry};
use ustr::Ustr;

/// A logical use of a venue `l2Book` stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BookStreamUse {
    Deltas,
    Depth10,
}

/// Venue precision options for an `l2Book` subscription.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct BookStreamOptions {
    pub n_sig_figs: Option<u32>,
    pub mantissa: Option<u32>,
}

/// Outcome of registering a logical use: whether a venue subscribe should be
/// sent, the active stream options (first-wins), and whether the requested
/// options differed from the active stream's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BookStreamRegistration {
    pub subscribe: bool,
    pub options: BookStreamOptions,
    pub options_mismatch: bool,
}

/// Outcome of releasing a logical use: unsubscribe the venue stream with its
/// original options, or retain it because another use remains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BookStreamRelease {
    Unsubscribe(BookStreamOptions),
    Retained,
}

#[derive(Debug)]
struct BookStreamEntry {
    options: BookStreamOptions,
    deltas: bool,
    depth10: bool,
}

/// Registry of active venue `l2Book` streams keyed by coin, shared across
/// client clones.
#[derive(Debug, Clone, Default)]
pub(crate) struct BookStreamRegistry {
    streams: Arc<DashMap<Ustr, BookStreamEntry>>,
}

impl BookStreamRegistry {
    /// Records a logical use of the coin's book stream.
    pub(crate) fn register(
        &self,
        coin: Ustr,
        stream_use: BookStreamUse,
        options: BookStreamOptions,
    ) -> BookStreamRegistration {
        match self.streams.entry(coin) {
            Entry::Occupied(mut occupied) => {
                let entry = occupied.get_mut();
                match stream_use {
                    BookStreamUse::Deltas => entry.deltas = true,
                    BookStreamUse::Depth10 => entry.depth10 = true,
                }
                BookStreamRegistration {
                    subscribe: false,
                    options: entry.options,
                    options_mismatch: options != entry.options,
                }
            }
            Entry::Vacant(vacant) => {
                vacant.insert(BookStreamEntry {
                    options,
                    deltas: stream_use == BookStreamUse::Deltas,
                    depth10: stream_use == BookStreamUse::Depth10,
                });
                BookStreamRegistration {
                    subscribe: true,
                    options,
                    options_mismatch: false,
                }
            }
        }
    }

    /// Releases a logical use of the coin's book stream.
    ///
    /// An untracked coin releases as `Unsubscribe` with default options,
    /// mirroring the pre-registry behavior of always sending the venue
    /// unsubscribe.
    pub(crate) fn release(&self, coin: &Ustr, stream_use: BookStreamUse) -> BookStreamRelease {
        match self.streams.entry(*coin) {
            Entry::Occupied(mut occupied) => {
                let entry = occupied.get_mut();
                match stream_use {
                    BookStreamUse::Deltas => entry.deltas = false,
                    BookStreamUse::Depth10 => entry.depth10 = false,
                }

                if entry.deltas || entry.depth10 {
                    BookStreamRelease::Retained
                } else {
                    let options = entry.options;
                    occupied.remove();
                    BookStreamRelease::Unsubscribe(options)
                }
            }
            Entry::Vacant(_) => BookStreamRelease::Unsubscribe(BookStreamOptions::default()),
        }
    }

    /// Returns the precision options of the coin's active stream, if tracked.
    pub(crate) fn options(&self, coin: &Ustr) -> Option<BookStreamOptions> {
        self.streams.get(coin).map(|entry| entry.options)
    }

    /// Clears all tracked streams.
    ///
    /// Called on connect: a fresh socket has no venue-side subscriptions, so
    /// stale entries must not gate the venue subscribe for re-subscriptions.
    pub(crate) fn clear(&self) {
        self.streams.clear();
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn options(n_sig_figs: Option<u32>, mantissa: Option<u32>) -> BookStreamOptions {
        BookStreamOptions {
            n_sig_figs,
            mantissa,
        }
    }

    #[rstest]
    fn first_register_subscribes_with_requested_options() {
        let registry = BookStreamRegistry::default();
        let coin = Ustr::from("BTC");

        let registration = registry.register(coin, BookStreamUse::Deltas, options(Some(5), None));

        assert!(registration.subscribe);
        assert!(!registration.options_mismatch);
        assert_eq!(registration.options, options(Some(5), None));
        assert_eq!(registry.options(&coin), Some(options(Some(5), None)));
    }

    #[rstest]
    fn second_use_shares_stream_and_reports_option_mismatch() {
        let registry = BookStreamRegistry::default();
        let coin = Ustr::from("BTC");
        registry.register(coin, BookStreamUse::Deltas, options(Some(5), Some(2)));

        let registration = registry.register(coin, BookStreamUse::Depth10, options(None, None));

        assert!(!registration.subscribe);
        assert!(registration.options_mismatch);
        assert_eq!(registration.options, options(Some(5), Some(2)));
    }

    #[rstest]
    fn matching_options_do_not_report_mismatch() {
        let registry = BookStreamRegistry::default();
        let coin = Ustr::from("BTC");
        registry.register(coin, BookStreamUse::Deltas, options(Some(5), None));

        let registration = registry.register(coin, BookStreamUse::Depth10, options(Some(5), None));

        assert!(!registration.subscribe);
        assert!(!registration.options_mismatch);
    }

    #[rstest]
    fn re_registering_same_use_is_idempotent() {
        let registry = BookStreamRegistry::default();
        let coin = Ustr::from("BTC");
        registry.register(coin, BookStreamUse::Deltas, options(None, None));

        let registration = registry.register(coin, BookStreamUse::Deltas, options(None, None));

        assert!(!registration.subscribe);
        assert_eq!(
            registry.release(&coin, BookStreamUse::Deltas),
            BookStreamRelease::Unsubscribe(options(None, None)),
        );
    }

    #[rstest]
    fn releasing_one_use_retains_stream_for_the_other() {
        let registry = BookStreamRegistry::default();
        let coin = Ustr::from("BTC");
        registry.register(coin, BookStreamUse::Deltas, options(Some(5), None));
        registry.register(coin, BookStreamUse::Depth10, options(Some(5), None));

        assert_eq!(
            registry.release(&coin, BookStreamUse::Deltas),
            BookStreamRelease::Retained,
        );
        assert_eq!(registry.options(&coin), Some(options(Some(5), None)));
        assert_eq!(
            registry.release(&coin, BookStreamUse::Depth10),
            BookStreamRelease::Unsubscribe(options(Some(5), None)),
        );
        assert_eq!(registry.options(&coin), None);
    }

    #[rstest]
    fn releasing_use_that_was_never_registered_retains_stream() {
        let registry = BookStreamRegistry::default();
        let coin = Ustr::from("BTC");
        registry.register(coin, BookStreamUse::Deltas, options(None, None));

        assert_eq!(
            registry.release(&coin, BookStreamUse::Depth10),
            BookStreamRelease::Retained,
        );
        assert_eq!(registry.options(&coin), Some(options(None, None)));
    }

    #[rstest]
    fn releasing_untracked_coin_unsubscribes_with_default_options() {
        let registry = BookStreamRegistry::default();
        let coin = Ustr::from("BTC");

        assert_eq!(
            registry.release(&coin, BookStreamUse::Deltas),
            BookStreamRelease::Unsubscribe(BookStreamOptions::default()),
        );
    }

    #[rstest]
    fn clear_removes_all_tracked_streams() {
        let registry = BookStreamRegistry::default();
        registry.register(
            Ustr::from("BTC"),
            BookStreamUse::Deltas,
            options(Some(5), None),
        );
        registry.register(
            Ustr::from("ETH"),
            BookStreamUse::Depth10,
            options(None, None),
        );

        registry.clear();

        assert_eq!(registry.options(&Ustr::from("BTC")), None);
        assert!(
            registry
                .register(
                    Ustr::from("BTC"),
                    BookStreamUse::Deltas,
                    options(None, None)
                )
                .subscribe
        );
    }
}
