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

//! Unified account-state reconciler for Lighter.
//!
//! Lighter publishes the account-state inputs across two WS streams:
//!
//! - `account_all_assets` — per-asset spot `balance`, spot `locked_balance`,
//!   and perp `margin_balance` (collateral pledged to the perp side).
//! - `user_stats`         — perp-account rollup: `collateral`,
//!   `available_balance`, `margin_usage`.
//!
//! Both share the same `account_id` and both legitimately update
//! `AccountState`. Emitting one per stream produces flip-flopping balances
//! (see the design report for the full failure mode). This reconciler
//! holds the latest snapshot of each input and emits **one merged
//! `AccountState`** per update from either source.
//!
//! Output contract:
//! - `AccountType::Margin` (Lighter is a unified margin venue — see
//!   `MarginAccount` superset-of-`CashAccount` notes in the design report).
//! - `base_currency = Some(USDC)` — Lighter is USDC-collateralized.
//! - `balances`: one `AccountBalance` per asset reported by
//!   `account_all_assets`, with `total = balance + margin_balance` and
//!   `locked = locked_balance + margin_balance` (perp collateral is
//!   unspendable as spot until withdrawn).
//! - `margins`:  one cross-margin `MarginBalance(currency=USDC, instrument_id=None)`
//!   with `initial = collateral − available_balance`.
//!
//! Emission gate: the reconciler refuses to emit until BOTH streams have
//! delivered at least one frame. This prevents emitting half-formed state
//! during startup that would then immediately flip when the other stream
//! lands.

use std::sync::Mutex;

use ahash::AHashMap;
use nautilus_core::UnixNanos;
use nautilus_model::{events::AccountState, identifiers::AccountId};
use ustr::Ustr;

use super::{
    messages::{LighterAsset, LighterUserStats},
    parse::{
        account_balance_from_lighter_asset, build_unified_account_state,
        margin_balance_from_user_stats,
    },
};

/// In-handler snapshot store for the two account-state input streams.
///
/// `assets` is keyed by the venue's asset-id string (matches the wire's
/// outer map key in `account_all_assets`). Updates upsert per-key so a
/// delta-style frame that touches only USDC doesn't wipe out a previously
/// known ETH entry. Lighter sends full snapshots in practice today, but
/// the upsert semantics keep us correct if that ever changes.
#[derive(Debug, Default)]
pub(crate) struct LighterAccountStateReconciler {
    inner: Mutex<ReconcilerInner>,
}

#[derive(Debug, Default)]
struct ReconcilerInner {
    assets: AHashMap<Ustr, LighterAsset>,
    user_stats: Option<LighterUserStats>,
    assets_seen: bool,
    user_stats_seen: bool,
}

impl LighterAccountStateReconciler {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Clear all cached state. Use before re-subscribing on a new WS
    /// session so the next emission reflects only fresh frames.
    pub(crate) fn reset(&self) {
        let mut inner = self.inner.lock().expect("reconciler mutex poisoned");
        inner.assets.clear();
        inner.user_stats = None;
        inner.assets_seen = false;
        inner.user_stats_seen = false;
    }

    /// Update the asset snapshot from an `account_all_assets` frame.
    ///
    /// Upsert per-key: each entry in `assets` overwrites any prior entry
    /// with the same key; keys absent from this frame are retained.
    /// Returns `true` once both streams have delivered, signalling the
    /// caller to call [`Self::build_state`].
    pub(crate) fn update_assets(&self, assets: &AHashMap<Ustr, LighterAsset>) -> bool {
        let mut inner = self.inner.lock().expect("reconciler mutex poisoned");
        for (key, asset) in assets {
            inner.assets.insert(*key, asset.clone());
        }
        inner.assets_seen = true;
        inner.both_seen()
    }

    /// Update the perp-rollup snapshot from a `user_stats` frame.
    /// Returns `true` once both streams have delivered.
    pub(crate) fn update_user_stats(&self, stats: &LighterUserStats) -> bool {
        let mut inner = self.inner.lock().expect("reconciler mutex poisoned");
        inner.user_stats = Some(stats.clone());
        inner.user_stats_seen = true;
        inner.both_seen()
    }

    /// Build the unified [`AccountState`] from the current snapshots.
    ///
    /// Returns `None` when either stream has not yet delivered. Returns
    /// `Some(Err)` if any per-asset balance or the margin balance fails
    /// to construct (negative numbers, currency mismatch, etc).
    pub(crate) fn build_state(
        &self,
        account_id: AccountId,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Option<anyhow::Result<AccountState>> {
        let inner = self.inner.lock().expect("reconciler mutex poisoned");
        if !inner.both_seen() {
            return None;
        }

        let mut balances = Vec::with_capacity(inner.assets.len());
        for asset in inner.assets.values() {
            match account_balance_from_lighter_asset(asset) {
                Ok(balance) => balances.push(balance),
                Err(e) => return Some(Err(e)),
            }
        }

        let margin = match inner
            .user_stats
            .as_ref()
            .map(margin_balance_from_user_stats)
        {
            Some(Ok(m)) => Some(m),
            Some(Err(e)) => return Some(Err(e)),
            None => None,
        };

        Some(Ok(build_unified_account_state(
            balances, margin, account_id, ts_event, ts_init,
        )))
    }
}

impl ReconcilerInner {
    fn both_seen(&self) -> bool {
        self.assets_seen && self.user_stats_seen
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_model::{
        enums::AccountType,
        identifiers::AccountId,
        types::{Currency, Money},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;
    use crate::websocket::messages::LighterUserStats;

    fn account_id() -> AccountId {
        AccountId::from("LIGHTER-728422")
    }

    fn asset_map(asset: LighterAsset) -> AHashMap<Ustr, LighterAsset> {
        let mut map = AHashMap::new();
        map.insert(Ustr::from(&asset.asset_id.to_string()), asset);
        map
    }

    fn usdc_asset(spot: &str, locked: &str, perp: &str) -> LighterAsset {
        LighterAsset {
            symbol: Ustr::from("USDC"),
            asset_id: 3,
            balance: Decimal::from_str(spot).unwrap(),
            locked_balance: Decimal::from_str(locked).unwrap(),
            margin_balance: Decimal::from_str(perp).unwrap(),
            margin_mode: Ustr::from("disabled"),
        }
    }

    fn user_stats(collateral: &str, available: &str, margin_usage: &str) -> LighterUserStats {
        LighterUserStats {
            account_trading_mode: 0,
            available_balance: Decimal::from_str(available).unwrap(),
            buying_power: Decimal::ZERO,
            collateral: Decimal::from_str(collateral).unwrap(),
            leverage: Decimal::ZERO,
            margin_usage: Decimal::from_str(margin_usage).unwrap(),
            portfolio_value: Decimal::from_str(collateral).unwrap(),
            cross_stats: None,
            total_stats: None,
        }
    }

    #[rstest]
    fn reconciler_refuses_emit_until_both_streams_seen() {
        let r = LighterAccountStateReconciler::new();
        assert!(
            r.build_state(account_id(), UnixNanos::default(), UnixNanos::default())
                .is_none()
        );

        // Only assets so far: still no emit.
        r.update_assets(&asset_map(usdc_asset("10.0", "0.0", "40.0")));
        assert!(
            r.build_state(account_id(), UnixNanos::default(), UnixNanos::default())
                .is_none()
        );

        // user_stats lands → emit unblocked.
        r.update_user_stats(&user_stats("40.0", "40.0", "0.0"));
        let state = r
            .build_state(account_id(), UnixNanos::default(), UnixNanos::default())
            .expect("ready")
            .expect("ok");
        assert_eq!(state.account_type, AccountType::Margin);
    }

    #[rstest]
    fn reconciler_emits_unchanged_when_only_one_stream_updates() {
        // Stream-ordering invariance: assets-first-then-stats and
        // stats-first-then-assets must both produce the same merged state.
        let r1 = LighterAccountStateReconciler::new();
        r1.update_assets(&asset_map(usdc_asset("10.0", "0.0", "40.0")));
        r1.update_user_stats(&user_stats("40.0", "40.0", "0.0"));
        let s1 = r1
            .build_state(account_id(), UnixNanos::default(), UnixNanos::default())
            .expect("ready")
            .expect("ok");

        let r2 = LighterAccountStateReconciler::new();
        r2.update_user_stats(&user_stats("40.0", "40.0", "0.0"));
        r2.update_assets(&asset_map(usdc_asset("10.0", "0.0", "40.0")));
        let s2 = r2
            .build_state(account_id(), UnixNanos::default(), UnixNanos::default())
            .expect("ready")
            .expect("ok");

        assert_eq!(s1.balances, s2.balances);
        assert_eq!(s1.margins, s2.margins);
        assert_eq!(s1.account_type, s2.account_type);
        assert_eq!(s1.base_currency, s2.base_currency);
    }

    #[rstest]
    fn reconciler_merges_spot_and_perp_into_one_balance() {
        // 10 spot + 40 perp, no positions, no resting spot orders.
        // Unified-margin shape: both legs are deployable equity, so
        // total=50, locked=0, free=50. MarginBalance carries the
        // (zero) margin-in-use breakdown.
        let r = LighterAccountStateReconciler::new();
        r.update_assets(&asset_map(usdc_asset("10.0", "0.0", "40.0")));
        r.update_user_stats(&user_stats("40.0", "40.0", "0.0"));

        let state = r
            .build_state(account_id(), UnixNanos::default(), UnixNanos::default())
            .expect("ready")
            .expect("ok");

        let usdc = Currency::get_or_create_crypto("USDC");
        assert_eq!(state.base_currency, Some(usdc));
        assert_eq!(state.balances.len(), 1);
        let bal = &state.balances[0];
        assert_eq!(bal.currency, usdc);
        assert_eq!(bal.total, Money::from("50.000000 USDC"));
        assert_eq!(bal.locked, Money::from("0 USDC"));
        assert_eq!(bal.free, Money::from("50.000000 USDC"));

        assert_eq!(state.margins.len(), 1);
        let margin = &state.margins[0];
        assert_eq!(margin.currency, usdc);
        assert_eq!(margin.initial, Money::from("0 USDC"));
        assert!(margin.instrument_id.is_none());
    }

    #[rstest]
    fn reconciler_tracks_position_open_via_user_stats_drift() {
        // After opening a perp position: collateral unchanged at 40,
        // available_balance drops to 35 (5 USDC margin in use).
        // AccountBalance is unchanged — margin-in-use lives on
        // MarginBalance, not in `locked`. Maintenance stays 0 because
        // Lighter doesn't publish maintenance on `user_stats`.
        let r = LighterAccountStateReconciler::new();
        r.update_assets(&asset_map(usdc_asset("10.0", "0.0", "40.0")));
        r.update_user_stats(&user_stats("40.0", "35.0", "12.50"));

        let state = r
            .build_state(account_id(), UnixNanos::default(), UnixNanos::default())
            .expect("ready")
            .expect("ok");

        let bal = &state.balances[0];
        assert_eq!(bal.total, Money::from("50.000000 USDC"));
        assert_eq!(bal.locked, Money::from("0 USDC"));
        assert_eq!(bal.free, Money::from("50.000000 USDC"));

        let margin = &state.margins[0];
        assert_eq!(margin.initial, Money::from("5.000000 USDC"));
        assert_eq!(margin.maintenance, Money::from("0 USDC"));
    }

    #[rstest]
    fn reconciler_upserts_assets_preserves_unrelated_entries() {
        // ETH spot reported once; later USDC-only frame must not evict ETH.
        let mut first_frame = AHashMap::new();
        first_frame.insert(Ustr::from("3"), usdc_asset("10.0", "0.0", "40.0"));
        first_frame.insert(
            Ustr::from("1"),
            LighterAsset {
                symbol: Ustr::from("ETH"),
                asset_id: 1,
                balance: Decimal::from_str("2.5").unwrap(),
                locked_balance: Decimal::ZERO,
                margin_balance: Decimal::ZERO,
                margin_mode: Ustr::default(),
            },
        );
        let r = LighterAccountStateReconciler::new();
        r.update_assets(&first_frame);
        r.update_user_stats(&user_stats("40.0", "40.0", "0.0"));

        let mut second_frame = AHashMap::new();
        second_frame.insert(Ustr::from("3"), usdc_asset("12.0", "0.0", "40.0"));
        r.update_assets(&second_frame);

        let state = r
            .build_state(account_id(), UnixNanos::default(), UnixNanos::default())
            .expect("ready")
            .expect("ok");

        assert_eq!(
            state.balances.len(),
            2,
            "ETH must survive the USDC-only update"
        );
    }

    #[rstest]
    fn reconciler_reset_clears_snapshots() {
        let r = LighterAccountStateReconciler::new();
        r.update_assets(&asset_map(usdc_asset("10.0", "0.0", "40.0")));
        r.update_user_stats(&user_stats("40.0", "40.0", "0.0"));
        assert!(
            r.build_state(account_id(), UnixNanos::default(), UnixNanos::default())
                .is_some()
        );

        r.reset();
        assert!(
            r.build_state(account_id(), UnixNanos::default(), UnixNanos::default())
                .is_none()
        );
    }
}
