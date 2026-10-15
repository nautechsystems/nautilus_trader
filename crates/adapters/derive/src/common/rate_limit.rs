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

//! Derive v3 request budgets and per-wallet/instrument order pacing.
//!
//! Request budgets reset in five-second fixed windows. Instrument buckets
//! refill continuously and hold reserved capacity until a signed request is
//! dispatched. HTTP and WebSocket clients on the same API host share wallet
//! state across session keys and subaccounts. Public requests share a separate
//! process-local per-host budget, conservatively assuming one source IP.
//!
//! Published defaults are a reference snapshot. Read deployed request budgets
//! with `public/getRateLimits`; it does not report instrument allowances.
//! Client windows cannot observe the venue's phase, and transport queues can
//! delay departure, so definitive venue throttling remains possible.
//!
//! See <https://docs.derive.xyz/rate-limits>.

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::Duration,
};

use nautilus_network::{
    http::Url,
    ratelimiter::clock::{Clock, MonotonicClock, Reference},
};
use parking_lot::Mutex;
use ustr::Ustr;

/// Identifier for the matching request budget.
pub const DERIVE_MATCHING_RATE_KEY: &str = "derive:matching";
/// Identifier for the non-matching request budget.
pub const DERIVE_NON_MATCHING_RATE_KEY: &str = "derive:non-matching";
/// Identifier for the `private/cancel_all` request budget.
pub const DERIVE_CANCEL_ALL_RATE_KEY: &str = "derive:cancel-all";
/// Identifier for the `private/cancel_by_label` request budget.
pub const DERIVE_CANCEL_BY_LABEL_RATE_KEY: &str = "derive:cancel-by-label";
/// Reference wallet matching requests per second.
pub const DERIVE_DEFAULT_MATCHING_TPS: u32 = 1;
/// Reference per-instrument refill rate for perpetuals and spot.
pub const DERIVE_DEFAULT_PER_INSTRUMENT_MATCHING_TPS: u32 = 1;
/// Reference public request allowance per IP, in requests per second.
pub const DERIVE_NON_MATCHING_TPS: u32 = 5;
/// Reference authenticated non-matching requests per second.
pub const DERIVE_WEBSOCKET_NON_MATCHING_TPS: u32 = DERIVE_NON_MATCHING_TPS;
/// Reference `private/cancel_all` requests per second.
pub const DERIVE_CANCEL_ALL_TPS: u32 = 1;
/// Adapter `private/cancel_by_label` endpoint allowance, retaining the
/// integrator reference's 10 TPS for unscoped calls.
pub const DERIVE_CANCEL_BY_LABEL_TPS: u32 = 10;
/// Request-budget fixed-window duration in seconds.
pub const DERIVE_RATE_WINDOW_SECS: u64 = 5;
/// Instrument capacity multiplier and requests per fixed window per TPS.
pub const DERIVE_RATE_BURST_MULTIPLIER: u32 = 5;

const DERIVE_DEFAULT_OPTION_MATCHING_TPS: u32 = 1;
const TOKEN_UNITS: u64 = 1_000_000_000;
const RATE_WINDOW_NANOS: u64 = DERIVE_RATE_WINDOW_SECS * TOKEN_UNITS;

// Strong references preserve budget debt through client recreation and reconnects
static RATE_STATES: OnceLock<Mutex<HashMap<RateScope, Arc<RateState<MonotonicClock>>>>> =
    OnceLock::new();

#[derive(Debug)]
pub(crate) struct RequestLimiter<C: Clock> {
    state: Arc<RateState<C>>,
    public: Arc<RateState<C>>,
}

impl<C: Clock> RequestLimiter<C> {
    pub(crate) fn new(limits: RateLimits, clock: C) -> Self {
        let state = Arc::new(RateState::new(limits, clock));

        Self {
            public: Arc::clone(&state),
            state,
        }
    }

    pub(crate) fn try_class_ready(
        &self,
        class: RateClass,
        instrument: Option<&Ustr>,
    ) -> Result<u64, Duration> {
        self.state.try_request(class, instrument, false)
    }

    pub(crate) async fn await_class_ready(
        &self,
        class: RateClass,
        instrument: Option<&Ustr>,
    ) -> u64 {
        loop {
            match self.try_class_ready(class, instrument) {
                Ok(window) => return window,
                Err(wait) => self.state.clock.sleep(wait).await,
            }
        }
    }

    pub(crate) async fn await_public_ready(&self) {
        loop {
            match self.public.try_request(RateClass::NonMatching, None, false) {
                Ok(_) => return,
                Err(wait) => self.public.clock.sleep(wait).await,
            }
        }
    }

    pub(crate) async fn reserve_matching_ready(&self, instrument: &Ustr) -> RateReservation<C> {
        loop {
            match self
                .state
                .try_request(RateClass::Matching, Some(instrument), true)
            {
                Ok(window) => {
                    return RateReservation {
                        state: Arc::clone(&self.state),
                        instrument: *instrument,
                        window,
                        committed: false,
                    };
                }
                Err(wait) => self.state.clock.sleep(wait).await,
            }
        }
    }
}

impl RequestLimiter<MonotonicClock> {
    pub(crate) fn shared(
        url: &str,
        wallet: Option<&str>,
        matching: Option<u32>,
        instrument: Option<u32>,
    ) -> Self {
        let authority = Url::parse(url).map_or_else(
            |_| url.to_ascii_lowercase(),
            |url| {
                format!(
                    "{}:{}",
                    url.host_str().unwrap_or_default(),
                    url.port_or_known_default().unwrap_or_default()
                )
            },
        );

        let public_scope = RateScope {
            authority,
            wallet: None,
        };

        let mut states = RATE_STATES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock();
        let public = Arc::clone(
            states
                .entry(public_scope.clone())
                .or_insert_with(|| Self::new(RateLimits::new(None, None), MonotonicClock).state),
        );

        let state = match wallet {
            Some(wallet) => {
                let scope = RateScope {
                    wallet: Some(wallet.to_ascii_lowercase()),
                    ..public_scope
                };

                Arc::clone(states.entry(scope).or_insert_with(|| {
                    Self::new(RateLimits::new(None, None), MonotonicClock).state
                }))
            }
            None => Arc::clone(&public),
        };

        state.configure(matching, instrument);
        Self { state, public }
    }
}

pub(crate) type DeriveRateLimiter = RequestLimiter<MonotonicClock>;

#[derive(Debug)]
pub(crate) struct RateReservation<C: Clock> {
    state: Arc<RateState<C>>,
    instrument: Ustr,
    window: u64,
    committed: bool,
}

impl<C: Clock> RateReservation<C> {
    // Commit never sleeps after the caller has generated its nonce and signature
    pub(crate) fn commit(mut self) -> Result<(), Duration> {
        let mut ledger = self.state.ledger.lock();
        let now = self.state.elapsed_nanos();
        let window = now / RATE_WINDOW_NANOS;
        if window != self.window {
            ledger.check_requests(RateClass::Matching, window, now)?;
            ledger.consume_requests(RateClass::Matching, window);
        }

        let tps = ledger.limits.instrument_tps(&self.instrument);
        let bucket = ledger
            .instruments
            .get_mut(&self.instrument)
            .expect("reserved instrument exists");
        bucket.refill(now, tps);
        bucket.pending -= 1;
        self.committed = true;
        Ok(())
    }
}

impl<C: Clock> Drop for RateReservation<C> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }

        let mut ledger = self.state.ledger.lock();
        let now = self.state.elapsed_nanos();
        let tps = ledger.limits.instrument_tps(&self.instrument);
        let bucket = ledger
            .instruments
            .get_mut(&self.instrument)
            .expect("reserved instrument exists");
        bucket.refill(now, tps);
        bucket.pending -= 1;
        bucket.tokens = bucket
            .tokens
            .saturating_add(TOKEN_UNITS)
            .min(bucket.capacity(tps));

        for key in request_buckets(RateClass::Matching) {
            let cell = &mut ledger.requests[*key as usize];
            if cell.window == self.window {
                cell.consumed -= 1;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RateClass {
    NonMatching,
    Matching,
    CancelAll,
    CancelByLabel,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RateLimits {
    matching: u32,
    instrument: u32,
    option: u32,
}

impl RateLimits {
    pub(crate) fn new(matching: Option<u32>, instrument: Option<u32>) -> Self {
        Self {
            matching: resolve_tps(matching, DERIVE_DEFAULT_MATCHING_TPS),
            instrument: resolve_tps(instrument, DERIVE_DEFAULT_PER_INSTRUMENT_MATCHING_TPS),
            option: resolve_tps(instrument, DERIVE_DEFAULT_OPTION_MATCHING_TPS),
        }
    }

    fn instrument_tps(&self, instrument: &Ustr) -> u32 {
        match instrument.as_str().split('-').nth(3) {
            Some("C" | "P") => self.option,
            _ => self.instrument,
        }
    }

    fn request_limit(&self, bucket: RequestBucket) -> u32 {
        let tps = match bucket {
            RequestBucket::NonMatching => DERIVE_WEBSOCKET_NON_MATCHING_TPS,
            RequestBucket::Matching => self.matching,
            RequestBucket::CancelAll => DERIVE_CANCEL_ALL_TPS,
            RequestBucket::CancelByLabel => DERIVE_CANCEL_BY_LABEL_TPS,
        };

        window_limit(tps)
    }
}

#[derive(Debug)]
struct RateState<C: Clock> {
    clock: C,
    start: C::Instant,
    ledger: Mutex<RateLedger>,
}

impl<C: Clock> RateState<C> {
    fn new(limits: RateLimits, clock: C) -> Self {
        let start = clock.now();

        Self {
            clock,
            start,
            ledger: Mutex::new(RateLedger {
                limits,
                matching_configured: None,
                instrument_configured: None,
                requests: std::array::from_fn(|_| RequestCell::default()),
                instruments: HashMap::new(),
            }),
        }
    }

    fn configure(&self, matching: Option<u32>, instrument: Option<u32>) {
        let mut ledger = self.ledger.lock();
        if let Some(tps) = matching.filter(|&v| v > 0) {
            match ledger.matching_configured {
                None => {
                    ledger.limits.matching = tps;
                    ledger.matching_configured = Some(tps);
                }
                Some(existing) if existing != tps => {
                    log::warn!(
                        "Conflicting Derive wallet matching limits; retaining {existing} TPS"
                    );
                }
                _ => {}
            }
        }

        if let Some(tps) = instrument.filter(|&v| v > 0) {
            match ledger.instrument_configured {
                None => {
                    let now = self.elapsed_nanos();
                    let old = ledger.limits;
                    for (name, bucket) in &mut ledger.instruments {
                        let old_tps = old.instrument_tps(name);
                        bucket.refill(now, old_tps);
                        let extra = window_limit(tps).saturating_sub(window_limit(old_tps));
                        bucket.tokens = bucket
                            .tokens
                            .saturating_add(u64::from(extra) * TOKEN_UNITS)
                            .min(bucket.capacity(tps));
                    }

                    ledger.limits.instrument = tps;
                    ledger.limits.option = tps;
                    ledger.instrument_configured = Some(tps);
                }
                Some(existing) if existing != tps => {
                    log::warn!("Conflicting Derive instrument limits; retaining {existing} TPS");
                }
                _ => {}
            }
        }
    }

    fn try_request(
        &self,
        class: RateClass,
        instrument: Option<&Ustr>,
        reserve: bool,
    ) -> Result<u64, Duration> {
        let mut ledger = self.ledger.lock();
        let now = self.elapsed_nanos();
        let window = now / RATE_WINDOW_NANOS;
        ledger.check_requests(class, window, now)?;
        if matches!(class, RateClass::Matching | RateClass::CancelByLabel)
            && let Some(instrument) = instrument
        {
            let tps = ledger.limits.instrument_tps(instrument);

            let bucket =
                ledger
                    .instruments
                    .entry(*instrument)
                    .or_insert_with(|| InstrumentBucket {
                        tokens: u64::from(window_limit(tps)) * TOKEN_UNITS,
                        updated: now,
                        pending: 0,
                    });

            bucket.refill(now, tps);
            if bucket.tokens < TOKEN_UNITS {
                let deficit = TOKEN_UNITS - bucket.tokens;
                return Err(Duration::from_nanos(deficit.div_ceil(u64::from(tps))));
            }

            bucket.tokens -= TOKEN_UNITS;
            if reserve {
                bucket.pending += 1;
            }
        }

        ledger.consume_requests(class, window);
        Ok(window)
    }

    fn elapsed_nanos(&self) -> u64 {
        self.clock.now().duration_since(self.start).as_u64()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RateScope {
    authority: String,
    wallet: Option<String>,
}

#[derive(Debug)]
struct RateLedger {
    limits: RateLimits,
    matching_configured: Option<u32>,
    instrument_configured: Option<u32>,
    requests: [RequestCell; 4],
    instruments: HashMap<Ustr, InstrumentBucket>,
}

impl RateLedger {
    fn check_requests(&self, class: RateClass, window: u64, now: u64) -> Result<(), Duration> {
        for key in request_buckets(class) {
            let cell = self.requests[*key as usize];
            if cell.window == window && cell.consumed >= self.limits.request_limit(*key) {
                return Err(Duration::from_nanos(
                    RATE_WINDOW_NANOS - now % RATE_WINDOW_NANOS,
                ));
            }
        }

        Ok(())
    }

    fn consume_requests(&mut self, class: RateClass, window: u64) {
        for key in request_buckets(class) {
            let cell = &mut self.requests[*key as usize];
            if cell.window != window {
                *cell = RequestCell {
                    window,
                    consumed: 0,
                };
            }

            cell.consumed += 1;
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct RequestCell {
    window: u64,
    consumed: u32,
}

#[derive(Debug)]
struct InstrumentBucket {
    tokens: u64,
    updated: u64,
    pending: u32,
}

impl InstrumentBucket {
    fn refill(&mut self, now: u64, tps: u32) {
        let added = u128::from(now.saturating_sub(self.updated)) * u128::from(tps);
        self.tokens =
            u64::try_from((u128::from(self.tokens) + added).min(u128::from(self.capacity(tps))))
                .expect("instrument credit fits u64");
        self.updated = now;
    }

    fn capacity(&self, tps: u32) -> u64 {
        u64::from(window_limit(tps).saturating_sub(self.pending)) * TOKEN_UNITS
    }
}

pub(crate) fn rate_class_for_method(method: &str) -> RateClass {
    // The legacy trigger caller remains outside the order-lifecycle migration
    match method.trim_start_matches('/') {
        "private/order"
        | "private/replace"
        | "private/cancel"
        | "private/cancel_by_instrument"
        | "private/cancel_by_nonce" => RateClass::Matching,
        "private/cancel_all" => RateClass::CancelAll,
        "private/cancel_by_label" => RateClass::CancelByLabel,
        _ => RateClass::NonMatching,
    }
}

#[derive(Debug, Clone, Copy)]
enum RequestBucket {
    NonMatching,
    Matching,
    CancelAll,
    CancelByLabel,
}

fn request_buckets(class: RateClass) -> &'static [RequestBucket] {
    match class {
        RateClass::NonMatching => &[RequestBucket::NonMatching],
        RateClass::Matching => &[RequestBucket::Matching],
        RateClass::CancelAll => &[RequestBucket::CancelAll],
        RateClass::CancelByLabel => &[RequestBucket::Matching, RequestBucket::CancelByLabel],
    }
}

fn resolve_tps(configured: Option<u32>, default_tps: u32) -> u32 {
    configured.filter(|&v| v > 0).unwrap_or(default_tps)
}

fn window_limit(tps: u32) -> u32 {
    tps.saturating_mul(DERIVE_RATE_BURST_MULTIPLIER)
}

#[cfg(test)]
mod tests {
    use std::{sync::Barrier, thread};

    use nautilus_network::ratelimiter::clock::FakeRelativeClock;
    use rstest::rstest;

    use super::*;

    fn limiter(
        matching: Option<u32>,
        instrument: Option<u32>,
    ) -> RequestLimiter<FakeRelativeClock> {
        RequestLimiter::new(
            RateLimits::new(matching, instrument),
            FakeRelativeClock::default(),
        )
    }

    #[rstest]
    #[case("private/order", RateClass::Matching)]
    #[case("/private/replace", RateClass::Matching)]
    #[case("private/cancel", RateClass::Matching)]
    #[case("private/cancel_by_instrument", RateClass::Matching)]
    #[case("private/cancel_by_nonce", RateClass::Matching)]
    #[case("private/cancel_trigger_order", RateClass::NonMatching)]
    #[case("private/get_trigger_orders", RateClass::NonMatching)]
    #[case("private/get_open_orders", RateClass::NonMatching)]
    #[case("private/get_subaccount", RateClass::NonMatching)]
    #[case("private/cancel_all", RateClass::CancelAll)]
    #[case("private/cancel_by_label", RateClass::CancelByLabel)]
    #[case("public/login", RateClass::NonMatching)]
    #[case("subscribe", RateClass::NonMatching)]
    fn method_classification(#[case] method: &str, #[case] expected: RateClass) {
        assert_eq!(rate_class_for_method(method), expected);
    }

    #[rstest]
    fn defaults_and_independent_overrides() {
        let limits = RateLimits::new(None, None);
        assert_eq!(limits.matching, 1);
        assert_eq!(limits.instrument, 1);
        assert_eq!(limits.option, 1);
        assert_eq!(limits.request_limit(RequestBucket::NonMatching), 25);
        assert_eq!(limits.request_limit(RequestBucket::CancelAll), 5);
        assert_eq!(limits.request_limit(RequestBucket::CancelByLabel), 50);
        let wallet = RateLimits::new(Some(500), None);
        assert_eq!(wallet.matching, 500);
        assert_eq!(wallet.instrument, 1);
        assert_eq!(wallet.option, 1);
        let instrument = RateLimits::new(None, Some(10));
        assert_eq!(instrument.matching, 1);
        assert_eq!(instrument.instrument, 10);
        assert_eq!(instrument.option, 10);
        let zero = RateLimits::new(Some(0), Some(0));
        assert_eq!(zero.matching, limits.matching);
        assert_eq!(zero.instrument, limits.instrument);
        assert_eq!(zero.option, limits.option);
    }

    #[rstest]
    fn request_windows_reset_only_at_boundary() {
        let limiter = limiter(None, None);

        for _ in 0..5 {
            assert_eq!(limiter.try_class_ready(RateClass::Matching, None), Ok(0));
        }

        limiter.state.clock.advance(Duration::from_millis(4_999));
        assert_eq!(
            limiter.try_class_ready(RateClass::Matching, None),
            Err(Duration::from_millis(1))
        );
        limiter.state.clock.advance(Duration::from_millis(1));

        for _ in 0..5 {
            assert_eq!(limiter.try_class_ready(RateClass::Matching, None), Ok(1));
        }

        assert_eq!(
            limiter.try_class_ready(RateClass::Matching, None),
            Err(Duration::from_secs(5))
        );
    }

    #[rstest]
    fn instrument_refills_continuously_with_fractional_credit() {
        let limiter = limiter(Some(100), None);
        let name = Ustr::from("ETH-PERP");

        for _ in 0..5 {
            assert_eq!(
                limiter.try_class_ready(RateClass::Matching, Some(&name)),
                Ok(0)
            );
        }

        limiter.state.clock.advance(Duration::from_millis(999));
        assert_eq!(
            limiter.try_class_ready(RateClass::Matching, Some(&name)),
            Err(Duration::from_millis(1))
        );
        limiter.state.clock.advance(Duration::from_millis(1));
        assert_eq!(
            limiter.try_class_ready(RateClass::Matching, Some(&name)),
            Ok(0)
        );
        assert_eq!(
            limiter.try_class_ready(RateClass::Matching, Some(&name)),
            Err(Duration::from_secs(1))
        );
        limiter.state.clock.advance(Duration::from_secs(10));

        for _ in 0..5 {
            assert_eq!(
                limiter.try_class_ready(RateClass::Matching, Some(&name)),
                Ok(2)
            );
        }

        assert_eq!(
            limiter.try_class_ready(RateClass::Matching, Some(&name)),
            Err(Duration::from_secs(1))
        );
    }

    #[rstest]
    fn instrument_has_no_window_boundary_double_burst() {
        let limiter = limiter(Some(100), None);
        let name = Ustr::from("ETH-PERP");
        limiter.state.clock.advance(Duration::from_millis(4_999));

        for _ in 0..5 {
            assert_eq!(
                limiter.try_class_ready(RateClass::Matching, Some(&name)),
                Ok(0)
            );
        }

        limiter.state.clock.advance(Duration::from_millis(1));
        assert_eq!(
            limiter.try_class_ready(RateClass::Matching, Some(&name)),
            Err(Duration::from_millis(999))
        );
    }

    #[rstest]
    fn instrument_denial_does_not_consume_wallet_points() {
        let limiter = limiter(Some(2), None);
        let eth = Ustr::from("ETH-PERP");
        let btc = Ustr::from("BTC-PERP");

        for _ in 0..5 {
            assert_eq!(
                limiter.try_class_ready(RateClass::Matching, Some(&eth)),
                Ok(0)
            );
        }

        for _ in 0..10 {
            assert_eq!(
                limiter.try_class_ready(RateClass::Matching, Some(&eth)),
                Err(Duration::from_secs(1))
            );
        }

        for _ in 0..5 {
            assert_eq!(
                limiter.try_class_ready(RateClass::Matching, Some(&btc)),
                Ok(0)
            );
        }

        assert_eq!(
            limiter.state.ledger.lock().requests[RequestBucket::Matching as usize].consumed,
            10
        );
    }

    #[rstest]
    fn wallet_denial_does_not_consume_instrument_credit() {
        let limiter = limiter(None, Some(2));
        let name = Ustr::from("ETH-PERP");

        for _ in 0..5 {
            assert_eq!(
                limiter.try_class_ready(RateClass::Matching, Some(&name)),
                Ok(0)
            );
        }

        assert_eq!(
            limiter.try_class_ready(RateClass::Matching, Some(&name)),
            Err(Duration::from_secs(5))
        );
        assert_eq!(
            limiter.state.ledger.lock().instruments[&name].tokens,
            5 * TOKEN_UNITS
        );
    }

    #[rstest]
    fn asset_types_and_instruments_keep_independent_credit() {
        let limiter = RequestLimiter::new(
            RateLimits {
                matching: 100,
                instrument: 1,
                option: 2,
            },
            FakeRelativeClock::default(),
        );

        for (name, burst) in [
            ("ETH-PERP", 5),
            ("ETH-USDC", 5),
            ("ETH-20261030-3000-C", 10),
        ] {
            let name = Ustr::from(name);

            for _ in 0..burst {
                assert_eq!(
                    limiter.try_class_ready(RateClass::Matching, Some(&name)),
                    Ok(0)
                );
            }

            assert_eq!(
                limiter.try_class_ready(RateClass::Matching, Some(&name)),
                Err(Duration::from_millis(5_000 / burst))
            );
        }
    }

    #[rstest]
    fn cancellation_classes_charge_only_required_budgets() {
        let limiter = limiter(Some(100), None);
        let name = Ustr::from("ETH-PERP");
        assert_eq!(
            limiter.try_class_ready(RateClass::CancelByLabel, None),
            Ok(0)
        );
        assert_eq!(
            limiter.try_class_ready(RateClass::CancelByLabel, Some(&name)),
            Ok(0)
        );
        assert_eq!(
            limiter.try_class_ready(RateClass::CancelAll, Some(&name)),
            Ok(0)
        );
        assert_eq!(
            limiter.try_class_ready(RateClass::NonMatching, Some(&name)),
            Ok(0)
        );
        let ledger = limiter.state.ledger.lock();
        assert_eq!(
            ledger.requests[RequestBucket::Matching as usize].consumed,
            2
        );
        assert_eq!(
            ledger.requests[RequestBucket::CancelByLabel as usize].consumed,
            2
        );
        assert_eq!(
            ledger.requests[RequestBucket::CancelAll as usize].consumed,
            1
        );
        assert_eq!(
            ledger.requests[RequestBucket::NonMatching as usize].consumed,
            1
        );
        assert_eq!(ledger.instruments[&name].tokens, 4 * TOKEN_UNITS);
    }

    #[rstest]
    fn label_endpoint_denial_does_not_consume_matching_points() {
        let limiter = limiter(Some(100), None);

        for _ in 0..50 {
            assert_eq!(
                limiter.try_class_ready(RateClass::CancelByLabel, None),
                Ok(0)
            );
        }

        assert_eq!(
            limiter.try_class_ready(RateClass::CancelByLabel, None),
            Err(Duration::from_secs(5))
        );
        assert_eq!(
            limiter.state.ledger.lock().requests[RequestBucket::Matching as usize].consumed,
            50
        );
        assert_eq!(limiter.try_class_ready(RateClass::Matching, None), Ok(0));
    }

    #[tokio::test]
    async fn held_reservations_cannot_accumulate_a_second_burst() {
        let limiter = limiter(Some(100), None);
        let name = Ustr::from("ETH-PERP");
        let mut held = Vec::new();
        for _ in 0..5 {
            held.push(limiter.reserve_matching_ready(&name).await);
        }

        limiter.state.clock.advance(Duration::from_secs(5));

        for reservation in held {
            assert_eq!(reservation.commit(), Ok(()));
        }

        assert_eq!(
            limiter.try_class_ready(RateClass::Matching, Some(&name)),
            Err(Duration::from_secs(1))
        );
        let ledger = limiter.state.ledger.lock();
        assert_eq!(
            ledger.requests[RequestBucket::Matching as usize].consumed,
            5
        );
        assert_eq!(ledger.instruments[&name].pending, 0);
        assert_eq!(ledger.instruments[&name].tokens, 0);
    }

    #[tokio::test]
    async fn abandoned_reservation_refunds_each_budget_once() {
        let limiter = limiter(None, None);
        let name = Ustr::from("ETH-PERP");
        let reservation = limiter.reserve_matching_ready(&name).await;
        drop(reservation);
        let ledger = limiter.state.ledger.lock();
        assert_eq!(
            ledger.requests[RequestBucket::Matching as usize].consumed,
            0
        );
        assert_eq!(ledger.instruments[&name].pending, 0);
        assert_eq!(ledger.instruments[&name].tokens, 5 * TOKEN_UNITS);
    }

    #[tokio::test]
    async fn rolled_reservation_never_waits_after_signing() {
        let limiter = limiter(None, None);
        let name = Ustr::from("ETH-PERP");
        let reservation = limiter.reserve_matching_ready(&name).await;
        limiter.state.clock.advance(Duration::from_secs(5));

        for _ in 0..5 {
            assert_eq!(limiter.try_class_ready(RateClass::Matching, None), Ok(1));
        }

        assert_eq!(reservation.commit(), Err(Duration::from_secs(5)));
        assert_eq!(limiter.state.elapsed_nanos(), RATE_WINDOW_NANOS);
        let ledger = limiter.state.ledger.lock();
        assert_eq!(
            ledger.requests[RequestBucket::Matching as usize].consumed,
            5
        );
        assert_eq!(ledger.instruments[&name].pending, 0);
        assert_eq!(ledger.instruments[&name].tokens, 5 * TOKEN_UNITS);
    }

    #[tokio::test]
    async fn reservation_commit_does_not_charge_twice() {
        let limiter = limiter(None, None);
        let name = Ustr::from("ETH-PERP");
        let reservation = limiter.reserve_matching_ready(&name).await;
        assert_eq!(reservation.commit(), Ok(()));
        let ledger = limiter.state.ledger.lock();
        assert_eq!(
            ledger.requests[RequestBucket::Matching as usize].consumed,
            1
        );
        assert_eq!(ledger.instruments[&name].pending, 0);
        assert_eq!(ledger.instruments[&name].tokens, 4 * TOKEN_UNITS);
    }

    #[rstest]
    fn first_explicit_override_preserves_debt_and_conflicts_do_not_reset_it() {
        let limiter = limiter(None, None);
        let name = Ustr::from("ETH-PERP");
        assert_eq!(
            limiter.try_class_ready(RateClass::Matching, Some(&name)),
            Ok(0)
        );
        limiter.state.configure(Some(2), Some(10));
        limiter.state.configure(None, None);
        limiter.state.configure(Some(100), Some(100));
        let ledger = limiter.state.ledger.lock();
        assert_eq!(ledger.limits.matching, 2);
        assert_eq!(ledger.limits.instrument, 10);
        assert_eq!(ledger.limits.option, 10);
        assert_eq!(
            ledger.requests[RequestBucket::Matching as usize].consumed,
            1
        );
        assert_eq!(ledger.instruments[&name].tokens, 49 * TOKEN_UNITS);
    }

    #[rstest]
    fn registry_shares_transports_session_keys_and_subaccounts_but_not_wallets() {
        let http =
            DeriveRateLimiter::shared("https://pacing.example/v3", Some("0xAbC"), None, None);
        let ws =
            DeriveRateLimiter::shared("wss://pacing.example:443/v3/ws", Some("0xabc"), None, None);
        let other =
            DeriveRateLimiter::shared("https://pacing.example/v3", Some("0xdef"), None, None);
        let testnet = DeriveRateLimiter::shared(
            "https://testnet.pacing.example/v3",
            Some("0xabc"),
            None,
            None,
        );
        assert!(Arc::ptr_eq(&http.state, &ws.state));
        assert!(Arc::ptr_eq(&http.public, &ws.public));
        assert!(Arc::ptr_eq(&http.public, &other.public));
        assert!(!Arc::ptr_eq(&http.state, &other.state));
        assert!(!Arc::ptr_eq(&http.state, &testnet.state));
        assert!(!Arc::ptr_eq(&http.state, &http.public));
    }

    #[tokio::test]
    async fn shared_handles_consume_the_same_fixed_window() {
        let limiter = limiter(None, None);

        let second = RequestLimiter {
            state: Arc::clone(&limiter.state),
            public: Arc::clone(&limiter.public),
        };

        for _ in 0..5 {
            limiter.await_class_ready(RateClass::Matching, None).await;
        }

        assert_eq!(second.await_class_ready(RateClass::Matching, None).await, 1);
        assert_eq!(limiter.state.elapsed_nanos(), RATE_WINDOW_NANOS);
        let independent =
            RequestLimiter::new(RateLimits::new(None, None), limiter.state.clock.clone());
        assert_eq!(
            independent.try_class_ready(RateClass::Matching, None),
            Ok(0)
        );
    }

    #[rstest]
    #[expect(
        clippy::needless_collect,
        reason = "all threads must start before any can pass the barrier"
    )]
    fn concurrent_admission_never_overspends_or_loses_wallet_points() {
        let limiter = Arc::new(limiter(Some(10), None));
        let barrier = Arc::new(Barrier::new(8));

        let threads = (0..8)
            .map(|_| {
                let limiter = Arc::clone(&limiter);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    limiter.try_class_ready(RateClass::Matching, Some(&Ustr::from("ETH-PERP")))
                })
            })
            .collect::<Vec<_>>();

        let outcomes = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            outcomes.iter().filter(|result| **result == Ok(0)).count(),
            5
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|result| **result == Err(Duration::from_secs(1)))
                .count(),
            3
        );
        let ledger = limiter.state.ledger.lock();
        assert_eq!(
            ledger.requests[RequestBucket::Matching as usize].consumed,
            5
        );
        assert_eq!(ledger.instruments[&Ustr::from("ETH-PERP")].tokens, 0);
    }

    #[tokio::test]
    async fn delayed_abandonment_refunds_only_the_held_credit() {
        let limiter = limiter(Some(100), None);
        let name = Ustr::from("ETH-PERP");
        let mut held = Vec::new();
        for _ in 0..5 {
            held.push(limiter.reserve_matching_ready(&name).await);
        }

        limiter.state.clock.advance(Duration::from_secs(5));
        drop(held.pop());
        assert_eq!(
            limiter.try_class_ready(RateClass::Matching, Some(&name)),
            Ok(1)
        );
        assert_eq!(
            limiter.try_class_ready(RateClass::Matching, Some(&name)),
            Err(Duration::from_secs(1))
        );
        drop(held);

        for _ in 0..4 {
            assert_eq!(
                limiter.try_class_ready(RateClass::Matching, Some(&name)),
                Ok(1)
            );
        }

        assert_eq!(
            limiter.try_class_ready(RateClass::Matching, Some(&name)),
            Err(Duration::from_secs(1))
        );
    }

    #[rstest]
    fn depleted_matching_budget_keeps_reads_and_cancel_all_independent() {
        let limiter = limiter(None, None);

        for _ in 0..5 {
            assert_eq!(limiter.try_class_ready(RateClass::Matching, None), Ok(0));
        }

        assert_eq!(
            limiter.try_class_ready(RateClass::CancelByLabel, None),
            Err(Duration::from_secs(5))
        );

        for (class, allowance) in [(RateClass::NonMatching, 25), (RateClass::CancelAll, 5)] {
            for _ in 0..allowance {
                assert_eq!(limiter.try_class_ready(class, None), Ok(0));
            }

            assert_eq!(
                limiter.try_class_ready(class, None),
                Err(Duration::from_secs(5))
            );
        }

        let ledger = limiter.state.ledger.lock();
        assert_eq!(
            ledger.requests[RequestBucket::Matching as usize].consumed,
            5
        );
        assert_eq!(
            ledger.requests[RequestBucket::CancelByLabel as usize].consumed,
            0
        );
    }
}
