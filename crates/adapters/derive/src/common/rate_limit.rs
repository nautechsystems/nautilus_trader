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

//! Fixed-window rate limiting for the Derive adapter.
//!
//! Derive refills every request allowance in discrete five-second windows, not
//! one token at a time. A Trader can spend a full burst of `tps * 5` matching
//! requests inside one window. The next request must then wait for the window
//! boundary; nothing refills before it.
//!
//! The buckets and their allowances:
//!
//! - Matching writes draw on two independent allowances: account-wide
//!   matching and per-instrument matching.
//! - `private/cancel_all` and unscoped `private/cancel_by_label` have custom
//!   quotas.
//! - REST non-matching requests use a flat per-IP allowance; authenticated
//!   WebSocket non-matching requests use a separate one.
//!
//! See <https://docs.derive.xyz/reference/rate-limits>.
//!
//! `FixedWindowLimiter` keeps one packed atomic word per bucket key holding
//! the window index and the count consumed from it, so a check-and-consume is
//! a single compare-and-swap. Windows align to limiter creation because the
//! venue's own window phase cannot be observed from the client. A wait is
//! therefore at most one full window, and the long-run average rate stays at
//! the venue allowance. A burst can still straddle a venue window boundary;
//! the venue then rejects the request outright. That rejection is definitive
//! (surfaced as an `OrderRejected`), not ambiguous.
//!
//! The limiter is generic over the `nautilus_network` clocks: tests drive it
//! deterministically with `FakeRelativeClock`, production uses
//! [`MonotonicClock`].

use std::{
    num::NonZeroU32,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use dashmap::DashMap;
#[cfg(test)]
use nautilus_network::ratelimiter::clock::FakeRelativeClock;
use nautilus_network::ratelimiter::clock::{Clock, MonotonicClock, Reference};
use ustr::Ustr;

/// Rate-limit bucket key for matching-engine requests (order create/cancel/replace).
pub const DERIVE_MATCHING_RATE_KEY: &str = "derive:matching";

/// Rate-limit bucket key for non-matching requests (reads, subscriptions, login).
pub const DERIVE_NON_MATCHING_RATE_KEY: &str = "derive:non-matching";

/// Rate-limit bucket key for `private/cancel_all` requests.
pub const DERIVE_CANCEL_ALL_RATE_KEY: &str = "derive:cancel-all";

/// Rate-limit bucket key for unscoped `private/cancel_by_label` requests.
pub const DERIVE_CANCEL_BY_LABEL_RATE_KEY: &str = "derive:cancel-by-label";

/// Prefix of the per-instrument matching bucket keys (`derive:matching:instrument:<name>`).
const DERIVE_PER_INSTRUMENT_RATE_KEY_PREFIX: &str = "derive:matching:instrument:";

/// Default matching-engine allowance for a Trader-tier account, in requests
/// per second. Market Maker accounts negotiate higher limits via
/// [`crate::config::DeriveExecutionClientConfig`]'s
/// `max_matching_requests_per_second` field.
pub const DERIVE_DEFAULT_MATCHING_TPS: u32 = 1;

/// Default per-instrument matching allowance for a Trader-tier account, in
/// requests per second. Market Maker accounts negotiate higher limits via
/// [`crate::config::DeriveExecutionClientConfig`]'s
/// `max_per_instrument_matching_requests_per_second` field. The account-wide
/// override never inflates this bucket.
pub const DERIVE_DEFAULT_PER_INSTRUMENT_MATCHING_TPS: u32 = 1;

/// Flat REST non-matching allowance per IP (requests per second).
pub const DERIVE_NON_MATCHING_TPS: u32 = 10;

/// Default authenticated WebSocket non-matching allowance for a Trader account.
pub const DERIVE_WEBSOCKET_NON_MATCHING_TPS: u32 = 5;

/// Custom allowance for `private/cancel_all` (requests per second).
pub const DERIVE_CANCEL_ALL_TPS: u32 = 1;

/// Custom allowance for unscoped `private/cancel_by_label` (requests per second).
pub const DERIVE_CANCEL_BY_LABEL_TPS: u32 = 10;

/// Fixed-window length: Derive refills every allowance discretely at window
/// boundaries spaced five seconds apart.
pub const DERIVE_RATE_WINDOW_SECS: u64 = 5;

/// Burst multiplier: each window admits five seconds' worth of requests.
pub const DERIVE_RATE_BURST_MULTIPLIER: u32 = 5;

const RATE_WINDOW_NANOS: u64 = DERIVE_RATE_WINDOW_SECS * 1_000_000_000;

/// Venue rate classification of an RPC method.
///
/// Methods outside the venue's matching and custom lists are non-matching.
///
/// `private/trigger_order` and `private/cancel_trigger_order` are missing
/// from the venue's matching list, but this adapter still paces them as
/// matching writes. They reach the matching engine, so the stricter
/// classification stays on the safe side of the documented contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RateClass {
    NonMatching,
    Matching,
    CancelAll,
    CancelByLabel,
}

/// A single venue rate bucket a request draws from.
///
/// [`RateBucket::PerInstrument`] is keyed per instrument name so each
/// instrument's matching allowance is enforced independently of the
/// account-wide [`RateBucket::Matching`] allowance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RateBucket<'a> {
    NonMatching,
    Matching,
    PerInstrument(&'a Ustr),
    CancelAll,
    CancelByLabel,
}

/// Returns the venue rate classification of an RPC method used by this adapter.
#[must_use]
pub(crate) fn rate_class_for_method(method: &str) -> RateClass {
    match method.trim_start_matches('/') {
        "private/order"
        | "private/trigger_order"
        | "private/replace"
        | "private/cancel"
        | "private/cancel_trigger_order" => RateClass::Matching,
        "private/cancel_all" => RateClass::CancelAll,
        "private/cancel_by_label" => RateClass::CancelByLabel,
        _ => RateClass::NonMatching,
    }
}

/// Per-window allowance for each Derive rate bucket.
///
/// Each allowance is the bucket's documented requests-per-second rate times
/// [`DERIVE_RATE_BURST_MULTIPLIER`], refilled discretely at each window
/// boundary.
///
/// One limiter instance serves one transport, so a single `non_matching`
/// allowance applies: the flat per-IP REST limit or the authenticated
/// WebSocket limit, selected by the [`FixedWindowLimits::rest`] and
/// [`FixedWindowLimits::websocket`] constructors.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FixedWindowLimits {
    /// Transport non-matching allowance per window (REST per-IP or WebSocket).
    pub(crate) non_matching: NonZeroU32,
    /// Account-wide matching allowance per window.
    pub(crate) matching: NonZeroU32,
    /// Per-instrument matching allowance per window.
    pub(crate) per_instrument_matching: NonZeroU32,
    /// `private/cancel_all` allowance per window.
    pub(crate) cancel_all: NonZeroU32,
    /// Unscoped `private/cancel_by_label` allowance per window.
    pub(crate) cancel_by_label: NonZeroU32,
}

impl FixedWindowLimits {
    /// Builds the REST limits: flat per-IP non-matching, with the configured
    /// matching allowances (`None` or zero applies the Trader-tier defaults).
    #[must_use]
    pub(crate) fn rest(
        matching_tps: Option<u32>,
        per_instrument_matching_tps: Option<u32>,
    ) -> Self {
        Self {
            non_matching: window_limit(DERIVE_NON_MATCHING_TPS),
            matching: window_limit(resolve_tps(matching_tps, DERIVE_DEFAULT_MATCHING_TPS)),
            per_instrument_matching: window_limit(resolve_tps(
                per_instrument_matching_tps,
                DERIVE_DEFAULT_PER_INSTRUMENT_MATCHING_TPS,
            )),
            cancel_all: window_limit(DERIVE_CANCEL_ALL_TPS),
            cancel_by_label: window_limit(DERIVE_CANCEL_BY_LABEL_TPS),
        }
    }

    /// Builds the authenticated WebSocket limits: Trader non-matching, with
    /// the configured matching allowances (`None` or zero applies the
    /// Trader-tier defaults).
    #[must_use]
    pub(crate) fn websocket(
        matching_tps: Option<u32>,
        per_instrument_matching_tps: Option<u32>,
    ) -> Self {
        Self {
            non_matching: window_limit(DERIVE_WEBSOCKET_NON_MATCHING_TPS),
            ..Self::rest(matching_tps, per_instrument_matching_tps)
        }
    }

    /// Returns the window limit for a bucket.
    #[must_use]
    pub(crate) fn limit_for(&self, bucket: RateBucket<'_>) -> NonZeroU32 {
        match bucket {
            RateBucket::NonMatching => self.non_matching,
            RateBucket::Matching => self.matching,
            RateBucket::PerInstrument(_) => self.per_instrument_matching,
            RateBucket::CancelAll => self.cancel_all,
            RateBucket::CancelByLabel => self.cancel_by_label,
        }
    }
}

/// Fixed-window rate limiter for Derive request pacing.
///
/// Each bucket key maps to one packed atomic word holding the window index and
/// the count consumed from that window. A successful check consumes one cell
/// of the current window; a denied check reports the wait until the next
/// window boundary, when the whole allowance refills at once.
pub(crate) struct FixedWindowLimiter<C: Clock> {
    limits: FixedWindowLimits,
    cells: DashMap<Ustr, AtomicU64>,
    clock: C,
    start: C::Instant,
}

impl<C: Clock> FixedWindowLimiter<C> {
    /// Creates a limiter whose windows are aligned to this call.
    pub(crate) fn new(limits: FixedWindowLimits, clock: C) -> Self {
        let start = clock.now();
        Self {
            limits,
            cells: DashMap::new(),
            clock,
            start,
        }
    }

    /// Attempts to consume one cell of the bucket's current window.
    ///
    /// # Errors
    ///
    /// Returns the wait until the next window boundary when the current
    /// window's allowance is exhausted.
    #[cfg(test)]
    pub(crate) fn check_bucket(&self, bucket: RateBucket<'_>) -> Result<(), Duration> {
        loop {
            let elapsed = self.elapsed_nanos();
            let window = window_index(elapsed);
            let limit = self.limits.limit_for(bucket).get();
            let key = bucket_key(bucket);
            let cell = self.cells.entry(key).or_default();
            match consume_cell_fixed_window(cell.value(), limit, window) {
                CellOutcome::Consumed => return Ok(()),
                CellOutcome::Exhausted => {
                    let window_end_nanos = (u64::from(window) + 1) * RATE_WINDOW_NANOS;
                    return Err(Duration::from_nanos(
                        window_end_nanos.saturating_sub(elapsed),
                    ));
                }
                // The window rolled over mid-attempt; retry on the fresh
                // window rather than writing a stale index back.
                CellOutcome::Advanced => {}
            }
        }
    }

    /// Waits until every bucket admits the request in one single window,
    /// consuming one cell from each, and returns that window's index.
    ///
    /// All buckets share the venue's five-second window grid, so a denied
    /// attempt sleeps to the same boundary whichever bucket denied. Cells
    /// consumed earlier in a failed attempt are rolled back, so a departure
    /// never mixes cells from two different windows.
    pub(crate) async fn await_buckets_ready(&self, buckets: &[RateBucket<'_>]) -> u32 {
        loop {
            let elapsed = self.elapsed_nanos();
            let window = window_index(elapsed);
            let mut acquired: Vec<Ustr> = Vec::with_capacity(buckets.len());
            let mut denial = None;

            for bucket in buckets {
                let limit = self.limits.limit_for(*bucket).get();
                let key = bucket_key(*bucket);
                let cell = self.cells.entry(key).or_default();
                match consume_cell_fixed_window(cell.value(), limit, window) {
                    CellOutcome::Consumed => acquired.push(key),
                    CellOutcome::Exhausted => {
                        let window_end_nanos = (u64::from(window) + 1) * RATE_WINDOW_NANOS;
                        denial = Some(Duration::from_nanos(
                            window_end_nanos.saturating_sub(elapsed),
                        ));
                        break;
                    }
                    // The window rolled over mid-attempt; discard this attempt
                    // (rolling back partial consumption) and retry fresh.
                    CellOutcome::Advanced => break,
                }
            }

            match denial {
                None if acquired.len() == buckets.len() => return window,
                None => {
                    self.rollback_window(acquired, window);
                }
                Some(wait) => {
                    self.rollback_window(acquired, window);
                    self.clock.sleep(wait).await;
                }
            }
        }
    }

    /// Waits for the buckets a request class draws from, honouring the venue's
    /// per-instrument matching allowance when the request carries an
    /// instrument.
    pub(crate) async fn await_class_ready(
        &self,
        class: RateClass,
        instrument_name: Option<&Ustr>,
    ) -> u32 {
        match class {
            RateClass::Matching if instrument_name.is_some() => {
                let instrument = instrument_name.expect("checked above");
                self.await_buckets_ready(&[
                    RateBucket::Matching,
                    RateBucket::PerInstrument(instrument),
                ])
                .await
            }
            RateClass::Matching => self.await_buckets_ready(&[RateBucket::Matching]).await,
            RateClass::NonMatching => self.await_buckets_ready(&[RateBucket::NonMatching]).await,
            RateClass::CancelAll => self.await_buckets_ready(&[RateBucket::CancelAll]).await,
            RateClass::CancelByLabel => {
                self.await_buckets_ready(&[RateBucket::CancelByLabel]).await
            }
        }
    }

    /// Returns one consumed cell per key, but only while its cell still holds
    /// `window`. Once a cell moved to a later window, the stale consumption
    /// was already superseded and there is nothing to undo.
    fn rollback_window(&self, keys: Vec<Ustr>, window: u32) {
        for key in keys {
            if let Some(cell) = self.cells.get(&key) {
                rollback_cell_fixed_window(cell.value(), window);
            }
        }
    }

    /// Re-paces a reserved request when the window it reserved in has ended.
    ///
    /// A matching reservation consumes its cells before the caller signs the
    /// request. If the signing work crosses a window boundary, the departure
    /// lands in a later window whose ledger never recorded it. Re-acquiring in
    /// the current window keeps every departure's cells in its own window's
    /// ledger; it is a no-op unless the window rolled.
    ///
    /// Callers pass the window index returned by the acquisition, so the
    /// stamp can never disagree with the cells actually consumed.
    pub(crate) async fn ensure_window_current(
        &self,
        class: RateClass,
        instrument_name: Option<&Ustr>,
        reserved_window: u32,
    ) {
        if window_index(self.elapsed_nanos()) != reserved_window {
            self.await_class_ready(class, instrument_name).await;
        }
    }

    fn elapsed_nanos(&self) -> u64 {
        self.clock.now().duration_since(self.start).as_u64()
    }
}

#[cfg(test)]
impl FixedWindowLimiter<FakeRelativeClock> {
    /// Advances the fake clock by the specified duration (tests only).
    pub(crate) fn advance_clock(&self, by: Duration) {
        self.clock.advance(by);
    }
}

impl<C: Clock> std::fmt::Debug for FixedWindowLimiter<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(FixedWindowLimiter)).finish()
    }
}

/// Type alias for the production limiter used by both Derive transports.
pub(crate) type DeriveRateLimiter = FixedWindowLimiter<MonotonicClock>;

fn bucket_key(bucket: RateBucket<'_>) -> Ustr {
    match bucket {
        RateBucket::NonMatching => Ustr::from(DERIVE_NON_MATCHING_RATE_KEY),
        RateBucket::Matching => Ustr::from(DERIVE_MATCHING_RATE_KEY),
        RateBucket::PerInstrument(instrument_name) => Ustr::from(
            format!(
                "{DERIVE_PER_INSTRUMENT_RATE_KEY_PREFIX}{}",
                instrument_name.as_str(),
            )
            .as_str(),
        ),
        RateBucket::CancelAll => Ustr::from(DERIVE_CANCEL_ALL_RATE_KEY),
        RateBucket::CancelByLabel => Ustr::from(DERIVE_CANCEL_BY_LABEL_RATE_KEY),
    }
}

fn resolve_tps(configured: Option<u32>, default_tps: u32) -> u32 {
    configured.filter(|&v| v > 0).unwrap_or(default_tps)
}

fn window_limit(tps: u32) -> NonZeroU32 {
    NonZeroU32::new(tps.saturating_mul(DERIVE_RATE_BURST_MULTIPLIER))
        .expect("window limit must be non-zero")
}

fn window_index(elapsed_nanos: u64) -> u32 {
    u32::try_from(elapsed_nanos / RATE_WINDOW_NANOS).expect("window index fits u32")
}

/// Packs `(window index, consumed)` into one atomic word; the window index in
/// the high half so the default zero value reads as a stale window.
fn pack(window: u32, consumed: u32) -> u64 {
    (u64::from(window) << 32) | u64::from(consumed)
}

fn unpack(packed: u64) -> (u32, u32) {
    (
        u32::try_from(packed >> 32).expect("window index fits u32"),
        packed as u32,
    )
}

/// Outcome of a one-cell consumption attempt against a fixed `window`.
enum CellOutcome {
    Consumed,
    Exhausted,
    /// The cell already holds a later window than the attempt targeted.
    Advanced,
}

/// Checks and consumes one cell of `window` under `limit` in a CAS loop.
///
/// A cell observed in a later window than `window` yields [`CellOutcome::Advanced`]
/// instead of writing the stale index back, so a window cell never regresses.
fn consume_cell_fixed_window(cell: &AtomicU64, limit: u32, window: u32) -> CellOutcome {
    let mut prev = cell.load(Ordering::Acquire);
    loop {
        let (prev_window, prev_consumed) = unpack(prev);
        if prev_window > window {
            return CellOutcome::Advanced;
        }
        let next = if prev_window < window {
            pack(window, 1)
        } else if prev_consumed < limit {
            pack(window, prev_consumed + 1)
        } else {
            return CellOutcome::Exhausted;
        };

        match cell.compare_exchange_weak(prev, next, Ordering::Release, Ordering::Relaxed) {
            Ok(_) => return CellOutcome::Consumed,
            Err(contended) => prev = contended,
        }
    }
}

/// Returns one consumed cell when it still holds `window`; a cell that moved
/// to a later window has already superseded the stale consumption.
fn rollback_cell_fixed_window(cell: &AtomicU64, window: u32) {
    let mut prev = cell.load(Ordering::Acquire);
    loop {
        let (prev_window, prev_consumed) = unpack(prev);
        if prev_window != window || prev_consumed == 0 {
            return;
        }
        let next = pack(prev_window, prev_consumed - 1);
        match cell.compare_exchange_weak(prev, next, Ordering::Release, Ordering::Relaxed) {
            Ok(_) => return,
            Err(contended) => prev = contended,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn instrument(name: &str) -> Ustr {
        Ustr::from(name)
    }

    fn trader_limits() -> FixedWindowLimits {
        FixedWindowLimits::websocket(None, None)
    }

    fn limiter() -> FixedWindowLimiter<FakeRelativeClock> {
        FixedWindowLimiter::new(trader_limits(), FakeRelativeClock::default())
    }

    #[rstest]
    fn test_rest_limits_match_documented_trader_contract() {
        let limits = FixedWindowLimits::rest(None, None);
        assert_eq!(limits.non_matching.get(), 50); // 10 TPS * 5x burst
        assert_eq!(limits.matching.get(), 5); // 1 TPS * 5x burst
        assert_eq!(limits.per_instrument_matching.get(), 5);
        assert_eq!(limits.cancel_all.get(), 5); // 1 TPS * 5x burst
        assert_eq!(limits.cancel_by_label.get(), 50); // 10 TPS * 5x burst
    }

    #[rstest]
    fn test_websocket_limits_match_documented_trader_contract() {
        let limits = FixedWindowLimits::websocket(None, None);
        assert_eq!(limits.non_matching.get(), 25); // 5 TPS * 5x burst
        assert_eq!(limits.matching.get(), 5);
        assert_eq!(limits.per_instrument_matching.get(), 5);
    }

    #[rstest]
    fn test_matching_overrides_do_not_leak_into_per_instrument_allowance() {
        let limits = FixedWindowLimits::websocket(Some(500), None);
        assert_eq!(limits.matching.get(), 2_500);
        assert_eq!(limits.per_instrument_matching.get(), 5);

        let limits = FixedWindowLimits::websocket(None, Some(10));
        assert_eq!(limits.matching.get(), 5);
        assert_eq!(limits.per_instrument_matching.get(), 50);
    }

    #[rstest]
    fn test_matching_overrides_treat_zero_as_unset() {
        let limits = FixedWindowLimits::websocket(Some(0), Some(0));
        assert_eq!(limits.matching.get(), 5);
        assert_eq!(limits.per_instrument_matching.get(), 5);
    }

    #[rstest]
    #[case("private/order", RateClass::Matching)]
    #[case("/private/order", RateClass::Matching)]
    #[case("private/trigger_order", RateClass::Matching)]
    #[case("private/replace", RateClass::Matching)]
    #[case("private/cancel", RateClass::Matching)]
    #[case("private/cancel_trigger_order", RateClass::Matching)]
    #[case("private/cancel_all", RateClass::CancelAll)]
    #[case("private/cancel_by_label", RateClass::CancelByLabel)]
    #[case("private/get_subaccount", RateClass::NonMatching)]
    #[case("private/get_open_orders", RateClass::NonMatching)]
    #[case("public/get_instruments", RateClass::NonMatching)]
    #[case("public/login", RateClass::NonMatching)]
    #[case("subscribe", RateClass::NonMatching)]
    fn test_rate_class_for_method(#[case] method: &str, #[case] expected: RateClass) {
        assert_eq!(rate_class_for_method(method), expected);
    }

    #[rstest]
    fn test_full_matching_burst_denies_sixth_request_until_window_reset() {
        let limiter = limiter();

        for _ in 0..5 {
            assert!(
                limiter.check_bucket(RateBucket::Matching).is_ok(),
                "Trader matching burst is five requests",
            );
        }
        assert!(
            limiter.check_bucket(RateBucket::Matching).is_err(),
            "sixth matching request must wait for the window reset",
        );
    }

    #[rstest]
    fn test_allowance_refills_discretely_at_window_boundary() {
        let limiter = limiter();
        for _ in 0..5 {
            limiter.check_bucket(RateBucket::Matching).expect("burst");
        }

        limiter.advance_clock(Duration::from_millis(4_999));
        assert!(
            limiter.check_bucket(RateBucket::Matching).is_err(),
            "window has not rolled: nothing refills before the boundary",
        );

        limiter.advance_clock(Duration::from_millis(1));

        for sequence in 0..5 {
            assert!(
                limiter.check_bucket(RateBucket::Matching).is_ok(),
                "full allowance must refill at the boundary, request {sequence}",
            );
        }
        assert!(
            limiter.check_bucket(RateBucket::Matching).is_err(),
            "only one window's allowance refills",
        );
    }

    #[rstest]
    fn test_window_reset_does_not_refill_one_token_at_a_time() {
        // After a burst, sub-window advances that would refill a GCRA cell
        // must not admit anything; only a full five-second crossing does.
        let limiter = limiter();
        for _ in 0..5 {
            limiter.check_bucket(RateBucket::Matching).expect("burst");
        }

        for _ in 0..4 {
            limiter.advance_clock(Duration::from_secs(1));
            assert!(
                limiter.check_bucket(RateBucket::Matching).is_err(),
                "sustained-rate refill must not apply inside a window",
            );
        }

        limiter.advance_clock(Duration::from_secs(1));
        assert!(
            limiter.check_bucket(RateBucket::Matching).is_ok(),
            "full refill lands exactly at the five-second boundary",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_await_buckets_ready_waits_for_window_reset_and_consumes() {
        let limiter = limiter();
        for _ in 0..5 {
            limiter.check_bucket(RateBucket::Matching).expect("burst");
        }

        // The fake clock's sleep advances time, so this await completes
        // deterministically at the window boundary.
        limiter.await_buckets_ready(&[RateBucket::Matching]).await;

        // The fresh window holds five cells and the await consumed exactly
        // one of them, leaving four.
        for _ in 0..4 {
            limiter
                .check_bucket(RateBucket::Matching)
                .expect("fresh window minus the awaited cell");
        }
        assert!(
            limiter.check_bucket(RateBucket::Matching).is_err(),
            "await must consume from the fresh window",
        );
    }

    #[rstest]
    fn test_per_instrument_buckets_are_independent() {
        let limiter = FixedWindowLimiter::new(
            FixedWindowLimits::websocket(Some(10), None),
            FakeRelativeClock::default(),
        );

        for _ in 0..5 {
            limiter
                .check_bucket(RateBucket::PerInstrument(&instrument("ETH-PERP")))
                .expect("ETH-PERP burst");
        }
        assert!(
            limiter
                .check_bucket(RateBucket::PerInstrument(&instrument("ETH-PERP")))
                .is_err(),
            "ETH-PERP allowance is exhausted",
        );
        assert!(
            limiter
                .check_bucket(RateBucket::PerInstrument(&instrument("BTC-PERP")))
                .is_ok(),
            "BTC-PERP has an independent allowance",
        );
        assert!(
            limiter.check_bucket(RateBucket::Matching).is_ok(),
            "account-wide matching still has headroom (10 TPS)",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_global_matching_bucket_enforced_alongside_per_instrument() {
        let clock = FakeRelativeClock::default();
        let limiter =
            FixedWindowLimiter::new(FixedWindowLimits::websocket(None, Some(10)), clock.clone());

        // Five ETH-PERP writes drain the account-wide Trader allowance while
        // barely touching ETH-PERP's own 10 TPS allowance.
        for _ in 0..5 {
            limiter
                .await_class_ready(RateClass::Matching, Some(&instrument("ETH-PERP")))
                .await;
        }

        // A BTC-PERP write has per-instrument headroom but cannot depart in
        // this window: the global bucket is exhausted, so the await must
        // advance the clock to the window boundary.
        limiter
            .await_class_ready(RateClass::Matching, Some(&instrument("BTC-PERP")))
            .await;
        assert_eq!(
            clock.now().as_u64(),
            RATE_WINDOW_NANOS,
            "BTC-PERP write must wait for the global window reset",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_matching_write_consumes_global_and_per_instrument_buckets() {
        let limiter = FixedWindowLimiter::new(
            FixedWindowLimits::websocket(Some(2), None),
            FakeRelativeClock::default(),
        );

        // Five ETH-PERP writes exhaust ETH-PERP's Trader allowance (5 per
        // window) but only consume five of the global override's 10 cells.
        for _ in 0..5 {
            limiter
                .await_class_ready(RateClass::Matching, Some(&instrument("ETH-PERP")))
                .await;
        }

        assert!(
            limiter
                .check_bucket(RateBucket::PerInstrument(&instrument("ETH-PERP")))
                .is_err(),
            "each write consumes the instrument bucket",
        );
        assert!(
            limiter.check_bucket(RateBucket::Matching).is_ok(),
            "five of the global override's ten window cells remain",
        );
        assert!(
            limiter
                .check_bucket(RateBucket::PerInstrument(&instrument("BTC-PERP")))
                .is_ok(),
            "other instruments are unaffected",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_multi_bucket_wait_consumes_both_buckets_from_one_window() {
        let clock = FakeRelativeClock::default();
        let limiter =
            FixedWindowLimiter::new(FixedWindowLimits::websocket(Some(10), None), clock.clone());

        // Five ETH-PERP writes exhaust ETH-PERP's Trader allowance while the
        // global override (10 TPS) still has window-0 headroom.
        for _ in 0..5 {
            limiter
                .await_class_ready(RateClass::Matching, Some(&instrument("ETH-PERP")))
                .await;
        }

        // The sixth write must not burn the global cell in window 0 and then
        // depart from window 1: both cells come from the boundary window.
        limiter
            .await_class_ready(RateClass::Matching, Some(&instrument("ETH-PERP")))
            .await;
        assert_eq!(
            clock.now().as_u64(),
            RATE_WINDOW_NANOS,
            "the denied write must wait for the window boundary",
        );

        // Window 1 holds one consumed global cell of 50, so exactly 49 more
        // admissions remain. The cross-window bug consumed the global cell in
        // window 0 and would leave 50.
        let mut remaining = 0;
        while limiter.check_bucket(RateBucket::Matching).is_ok() {
            remaining += 1;
        }
        assert_eq!(remaining, 49, "global cell must come from window 1");

        // The instrument bucket also consumed its window-1 cell.
        for _ in 0..4 {
            limiter
                .check_bucket(RateBucket::PerInstrument(&instrument("ETH-PERP")))
                .expect("window 1 holds one consumed cell of five");
        }
        assert!(
            limiter
                .check_bucket(RateBucket::PerInstrument(&instrument("ETH-PERP")))
                .is_err(),
            "the awaited write consumed the fifth ETH-PERP cell of window 1",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_ensure_window_current_reacquires_only_after_rollover() {
        let clock = FakeRelativeClock::default();
        let limiter =
            FixedWindowLimiter::new(FixedWindowLimits::websocket(None, None), clock.clone());

        // Reserve one ETH-PERP write and record its window.
        let reserved_window = limiter
            .await_class_ready(RateClass::Matching, Some(&instrument("ETH-PERP")))
            .await;
        assert_eq!(reserved_window, 0);

        // Same window: the refresh must not consume another cell pair.
        limiter
            .ensure_window_current(
                RateClass::Matching,
                Some(&instrument("ETH-PERP")),
                reserved_window,
            )
            .await;
        assert!(
            limiter.check_bucket(RateBucket::Matching).is_ok(),
            "same-window refresh consumes nothing",
        );
        limiter
            .check_bucket(RateBucket::PerInstrument(&instrument("ETH-PERP")))
            .expect("same-window refresh consumes nothing");

        // Window rolled past the reservation: the refresh must consume a
        // fresh pair in the new window, leaving 4 of 5 cells in each bucket.
        clock.advance(Duration::from_secs(5));
        limiter
            .ensure_window_current(
                RateClass::Matching,
                Some(&instrument("ETH-PERP")),
                reserved_window,
            )
            .await;

        for _ in 0..4 {
            limiter
                .check_bucket(RateBucket::Matching)
                .expect("window 1 global has 4 cells left of 5");
        }
        assert!(
            limiter.check_bucket(RateBucket::Matching).is_err(),
            "rolled-window refresh consumed a window-1 global cell",
        );

        for _ in 0..4 {
            limiter
                .check_bucket(RateBucket::PerInstrument(&instrument("ETH-PERP")))
                .expect("window 1 instrument has 4 cells left of 5");
        }
        assert!(
            limiter
                .check_bucket(RateBucket::PerInstrument(&instrument("ETH-PERP")))
                .is_err(),
            "rolled-window refresh consumed a window-1 instrument cell",
        );
    }

    #[rstest]
    fn test_custom_cancel_all_quota_is_one_tps_burst() {
        let limiter = limiter();
        for _ in 0..5 {
            limiter.check_bucket(RateBucket::CancelAll).expect("burst");
        }
        assert!(
            limiter.check_bucket(RateBucket::CancelAll).is_err(),
            "custom cancel_all allowance is 5 per window",
        );
        limiter.advance_clock(Duration::from_secs(5));
        assert!(limiter.check_bucket(RateBucket::CancelAll).is_ok());
    }

    #[rstest]
    fn test_custom_unscoped_cancel_by_label_quota_is_ten_tps_burst() {
        let limiter = limiter();
        for _ in 0..50 {
            limiter
                .check_bucket(RateBucket::CancelByLabel)
                .expect("burst");
        }
        assert!(
            limiter.check_bucket(RateBucket::CancelByLabel).is_err(),
            "unscoped cancel_by_label allowance is 50 per window",
        );
        limiter.advance_clock(Duration::from_secs(5));
        assert!(limiter.check_bucket(RateBucket::CancelByLabel).is_ok());
    }

    #[rstest]
    fn test_rest_non_matching_quota_is_fifty_per_window() {
        let limiter = FixedWindowLimiter::new(
            FixedWindowLimits::rest(None, None),
            FakeRelativeClock::default(),
        );

        for _ in 0..50 {
            limiter
                .check_bucket(RateBucket::NonMatching)
                .expect("burst");
        }
        assert!(
            limiter.check_bucket(RateBucket::NonMatching).is_err(),
            "REST non-matching allowance is 50 per window",
        );
    }

    #[rstest]
    fn test_websocket_non_matching_quota_is_twenty_five_per_window() {
        let limiter = limiter();
        for _ in 0..25 {
            limiter
                .check_bucket(RateBucket::NonMatching)
                .expect("burst");
        }
        assert!(
            limiter.check_bucket(RateBucket::NonMatching).is_err(),
            "WebSocket non-matching allowance is 25 per window",
        );
    }

    #[rstest]
    fn test_window_limit_and_index_arithmetic() {
        assert_eq!(window_limit(1).get(), 5);
        assert_eq!(window_index(0), 0);
        assert_eq!(window_index(RATE_WINDOW_NANOS - 1), 0);
        assert_eq!(window_index(RATE_WINDOW_NANOS), 1);
        assert_eq!(unpack(pack(7, 3)), (7, 3));
        assert_eq!(unpack(0), (0, 0));
    }
}
