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

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{ClientOrderId, TradeId},
    types::{AccountBalance, Money},
};
use rust_decimal::Decimal;

/// Reconciled BUY fills older than this before the anchor are treated as already drained.
/// Live fills are not subject to the cutoff.
const RECONCILE_HORIZON_NS: u64 = 15_000_000_000;

/// Open BUY reservations and own BUY fill spend not yet reflected in the venue collateral total.
#[derive(Debug, Default)]
pub(super) struct CollateralLocks {
    pub reservations: AHashMap<ClientOrderId, Money>,
    unsettled: UnsettledBuySpend,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct RefreshTicket {
    epoch: u64,
    session: u64,
}

#[derive(Debug)]
pub(super) enum AppliedBalance {
    Fresh(AccountBalance),
    Stale,
}

#[derive(Debug, Default)]
struct UnsettledBuySpend {
    anchor: Option<Money>,
    anchor_ts: UnixNanos,
    open: AHashMap<TradeId, Money>,
    absorbed: AHashMap<TradeId, Money>,
    voided: AHashMap<TradeId, Money>,
    released: AHashSet<TradeId>,
    applied_epoch: u64,
    next_epoch: u64,
    session: u64,
}

impl CollateralLocks {
    pub(super) fn begin_refresh(&mut self) -> RefreshTicket {
        self.unsettled.next_epoch = self.unsettled.next_epoch.saturating_add(1);

        RefreshTicket {
            epoch: self.unsettled.next_epoch,
            session: self.unsettled.session,
        }
    }

    /// Drops reservations and unsettled spend and invalidates in-flight refresh tickets.
    pub(super) fn clear_session(&mut self) {
        let session = self.unsettled.session.saturating_add(1);
        self.reservations.clear();

        self.unsettled = UnsettledBuySpend {
            session,
            ..UnsettledBuySpend::default()
        };
    }

    /// Invalidates in-flight refresh tickets without dropping reservations or unsettled spend.
    pub(super) fn discard_inflight_refreshes(&mut self) {
        self.unsettled.session = self.unsettled.session.saturating_add(1);
    }

    pub(super) fn note_buy_spend(
        &mut self,
        trade_id: TradeId,
        spend: Money,
        ts_event: UnixNanos,
        reconciliation: bool,
    ) {
        if self.unsettled.released.contains(&trade_id)
            || (self.unsettled.absorbed.contains_key(&trade_id)
                && !self.unsettled.open.contains_key(&trade_id))
        {
            return;
        }

        if reconciliation && !self.reconciled_fill_is_recent(ts_event) {
            return;
        }

        self.unsettled.open.insert(trade_id, spend);
    }

    pub(super) fn note_voided_spend(&mut self, trade_id: TradeId, voided: Money) {
        if self.unsettled.released.contains(&trade_id) || voided.as_decimal().is_zero() {
            return;
        }

        let previous = self.unsettled.voided.get(&trade_id).copied();

        let Some(increase) = void_increase(previous, voided) else {
            return;
        };

        let Some(open_spend) = self.unsettled.open.get(&trade_id).copied() else {
            return;
        };

        self.unsettled.voided.insert(trade_id, voided);

        let applied = if increase.as_decimal() > open_spend.as_decimal() {
            open_spend
        } else {
            increase
        };

        let remainder = open_spend.as_decimal() - applied.as_decimal();
        if remainder.is_zero() {
            self.unsettled.open.remove(&trade_id);
        } else {
            match Money::from_decimal(remainder, open_spend.currency) {
                Ok(remainder) => {
                    self.unsettled.open.insert(trade_id, remainder);
                }
                Err(e) => {
                    log::error!("Cannot retain Polymarket unsettled remainder for {trade_id}: {e}");
                    self.unsettled.open.remove(&trade_id);
                    self.add_absorbed(trade_id, open_spend);
                    return;
                }
            }
        }

        self.add_absorbed(trade_id, applied);
    }

    pub(super) fn apply_refresh(
        &mut self,
        ticket: RefreshTicket,
        total: Money,
        now: UnixNanos,
    ) -> anyhow::Result<AppliedBalance> {
        if ticket.session != self.unsettled.session || ticket.epoch <= self.unsettled.applied_epoch
        {
            return Ok(AppliedBalance::Stale);
        }

        let pending = self.pending_drain(total)?;
        let open_spend = self.open_spend(total)?;
        let reserved = self.reservation_sum(total)?;
        let decrease = self.unsettled.anchor.map_or(Decimal::ZERO, |anchor| {
            (anchor.as_decimal() - total.as_decimal()).max(Decimal::ZERO)
        });

        let covered = self.unsettled.anchor.is_some()
            && !pending.as_decimal().is_zero()
            && decrease >= pending.as_decimal();
        let void_only = open_spend.as_decimal().is_zero() && !pending.as_decimal().is_zero();
        let release = covered || void_only;
        let open_after = if release || self.unsettled.anchor.is_none() {
            if release {
                Money::zero(total.currency)
            } else {
                open_spend
            }
        } else {
            uncovered_open_spend(open_spend, pending, decrease)?
        };

        let locked = reserved
            .checked_add(open_after)
            .context("invalid Polymarket unsettled buy spend")?;

        self.unsettled.applied_epoch = ticket.epoch;

        if release {
            self.unsettled.release_all();
            self.set_anchor(total, now);
        } else if self.unsettled.anchor.is_none() || pending.as_decimal().is_zero() {
            self.set_anchor(total, now);
        }

        let balance = AccountBalance::from_total_and_locked(
            total.as_decimal(),
            locked.as_decimal(),
            total.currency,
        )?;
        Ok(AppliedBalance::Fresh(balance))
    }

    fn reconciled_fill_is_recent(&self, ts_event: UnixNanos) -> bool {
        if self.unsettled.anchor.is_none() || ts_event.is_zero() {
            return false;
        }

        ts_event.as_u64().saturating_add(RECONCILE_HORIZON_NS) >= self.unsettled.anchor_ts.as_u64()
    }

    fn set_anchor(&mut self, total: Money, now: UnixNanos) {
        self.unsettled.anchor = Some(total);
        self.unsettled.anchor_ts = now;
    }

    fn add_absorbed(&mut self, trade_id: TradeId, amount: Money) {
        let Some(existing) = self.unsettled.absorbed.get(&trade_id).copied() else {
            self.unsettled.absorbed.insert(trade_id, amount);
            return;
        };

        match existing.checked_add(amount) {
            Some(sum) => {
                self.unsettled.absorbed.insert(trade_id, sum);
            }
            None => log::error!(
                "Cannot absorb Polymarket voided spend for {trade_id}: addition overflow"
            ),
        }
    }

    fn pending_drain(&self, total: Money) -> anyhow::Result<Money> {
        let open = self.open_spend(total)?;
        let absorbed = sum_money(self.unsettled.absorbed.values().copied(), total)?;
        open.checked_add(absorbed)
            .context("invalid Polymarket unsettled buy spend")
    }

    fn open_spend(&self, total: Money) -> anyhow::Result<Money> {
        sum_money(self.unsettled.open.values().copied(), total)
    }

    fn reservation_sum(&self, total: Money) -> anyhow::Result<Money> {
        self.reservations
            .values()
            .copied()
            .try_fold(Money::zero(total.currency), |sum, amount| {
                sum.checked_add(amount)
                    .context("invalid Polymarket order reservation total")
            })
    }
}

impl UnsettledBuySpend {
    fn release_all(&mut self) {
        self.released.extend(self.open.keys().copied());
        self.released.extend(self.absorbed.keys().copied());
        self.open.clear();
        self.absorbed.clear();
        self.voided.clear();
    }
}

fn uncovered_open_spend(
    open_spend: Money,
    pending: Money,
    decrease: Decimal,
) -> anyhow::Result<Money> {
    let shortfall = (pending.as_decimal() - decrease).max(Decimal::ZERO);
    let locked = open_spend.as_decimal().min(shortfall);
    if locked.is_zero() {
        return Ok(Money::zero(open_spend.currency));
    }

    Money::from_decimal(locked, open_spend.currency)
        .context("invalid Polymarket unsettled buy spend")
}

fn void_increase(previous: Option<Money>, voided: Money) -> Option<Money> {
    let Some(previous) = previous else {
        return Some(voided);
    };

    if previous.currency != voided.currency {
        log::error!(
            "Cannot apply Polymarket void revision: currency {} does not match {}",
            voided.currency,
            previous.currency
        );
        return None;
    }

    let increase = voided.as_decimal() - previous.as_decimal();
    if increase <= Decimal::ZERO {
        return None;
    }

    match Money::from_decimal(increase, voided.currency) {
        Ok(increase) => Some(increase),
        Err(e) => {
            log::error!("Cannot apply Polymarket void revision: {e}");
            None
        }
    }
}

fn sum_money(mut amounts: impl Iterator<Item = Money>, total: Money) -> anyhow::Result<Money> {
    amounts.try_fold(Money::zero(total.currency), |sum, amount| {
        sum.checked_add(amount)
            .context("invalid Polymarket unsettled buy spend")
    })
}

#[cfg(test)]
mod tests {
    use nautilus_model::types::{Currency, money::MONEY_RAW_MAX};
    use rstest::rstest;

    use super::*;

    fn pusd(amount: &str) -> Money {
        Money::from(format!("{amount} pUSD").as_str())
    }

    fn apply(locks: &mut CollateralLocks, total: &str, now: u64) -> AccountBalance {
        let ticket = locks.begin_refresh();
        match locks
            .apply_refresh(ticket, pusd(total), UnixNanos::from(now))
            .unwrap()
        {
            AppliedBalance::Fresh(balance) => balance,
            AppliedBalance::Stale => panic!("refresh was stale"),
        }
    }

    #[rstest]
    fn buy_fill_reduces_free_until_drained_refresh() {
        let mut locks = CollateralLocks::default();
        let baseline = apply(&mut locks, "100", 1);
        locks.note_buy_spend(
            TradeId::from("BUY-1"),
            pusd("5.125"),
            UnixNanos::from(2),
            false,
        );

        let still_included = apply(&mut locks, "100", 3);
        let drained = apply(&mut locks, "94.875", 4);

        assert_eq!(
            baseline,
            AccountBalance::new(pusd("100"), pusd("0"), pusd("100"))
        );
        assert_eq!(
            still_included,
            AccountBalance::new(pusd("100"), pusd("5.125"), pusd("94.875"))
        );
        assert_eq!(
            drained,
            AccountBalance::new(pusd("94.875"), pusd("0"), pusd("94.875"))
        );
    }

    #[rstest]
    fn reservations_and_fill_spend_lock_together() {
        let mut locks = CollateralLocks::default();
        apply(&mut locks, "100", 1);
        locks
            .reservations
            .insert(ClientOrderId::from("BUY-OPEN"), pusd("3"));
        locks.note_buy_spend(
            TradeId::from("BUY-1"),
            pusd("2.10"),
            UnixNanos::from(2),
            false,
        );

        let balance = apply(&mut locks, "100", 3);

        assert_eq!(
            balance,
            AccountBalance::new(pusd("100"), pusd("5.10"), pusd("94.90"))
        );
    }

    #[rstest]
    fn cumulative_void_revisions_remove_only_the_increase() {
        let mut locks = CollateralLocks::default();
        apply(&mut locks, "100", 1);
        locks.note_buy_spend(
            TradeId::from("REVISED"),
            pusd("10"),
            UnixNanos::from(2),
            false,
        );
        locks.note_voided_spend(TradeId::from("REVISED"), pusd("2"));
        locks.note_voided_spend(TradeId::from("REVISED"), pusd("3"));
        locks.note_voided_spend(TradeId::from("REVISED"), pusd("3"));
        locks.note_voided_spend(TradeId::from("REVISED"), pusd("1"));

        let after_voids = apply(&mut locks, "100", 3);
        let short_drop = apply(&mut locks, "93", 4);
        let drained = apply(&mut locks, "90", 5);

        assert_eq!(
            after_voids,
            AccountBalance::new(pusd("100"), pusd("7"), pusd("93"))
        );
        assert_eq!(
            short_drop,
            AccountBalance::new(pusd("93"), pusd("3"), pusd("90"))
        );
        assert_eq!(
            drained,
            AccountBalance::new(pusd("90"), pusd("0"), pusd("90"))
        );
    }

    #[rstest]
    fn earlier_drain_does_not_release_a_later_fill() {
        let mut locks = CollateralLocks::default();
        apply(&mut locks, "100", 1_000);
        locks.note_buy_spend(TradeId::from("A"), pusd("5"), UnixNanos::from(1_001), false);
        apply(&mut locks, "95", 1_010);
        locks.note_buy_spend(TradeId::from("B"), pusd("3"), UnixNanos::from(1_005), false);

        assert_eq!(
            apply(&mut locks, "95", 1_011),
            AccountBalance::new(pusd("95"), pusd("3"), pusd("92"))
        );
    }

    #[rstest]
    fn void_of_an_unlocked_fill_does_not_raise_the_threshold() {
        let mut locks = CollateralLocks::default();
        apply(&mut locks, "100", 1);
        locks.note_voided_spend(TradeId::from("VOIDED"), pusd("2"));
        locks.note_voided_spend(TradeId::from("VOIDED"), pusd("3"));
        locks.note_buy_spend(TradeId::from("OPEN"), pusd("10"), UnixNanos::from(2), false);

        assert_eq!(
            apply(&mut locks, "90", 3),
            AccountBalance::new(pusd("90"), pusd("0"), pusd("90"))
        );
    }

    #[rstest]
    fn drained_fill_is_not_locked_while_another_fill_is_open() {
        let mut locks = CollateralLocks::default();
        apply(&mut locks, "100", 1);
        locks.note_buy_spend(TradeId::from("A"), pusd("5"), UnixNanos::from(2), false);
        locks.note_buy_spend(TradeId::from("B"), pusd("3"), UnixNanos::from(3), false);

        let partial = apply(&mut locks, "95", 4);
        let both = apply(&mut locks, "92", 5);

        assert_eq!(
            partial,
            AccountBalance::new(pusd("95"), pusd("3"), pusd("92"))
        );
        assert_eq!(both, AccountBalance::new(pusd("92"), pusd("0"), pusd("92")));
    }

    #[rstest]
    fn failed_buy_void_does_not_pin_later_spend() {
        let mut locks = CollateralLocks::default();
        apply(&mut locks, "100", 1);
        locks.note_buy_spend(
            TradeId::from("FAILED"),
            pusd("4"),
            UnixNanos::from(2),
            false,
        );
        locks.note_voided_spend(TradeId::from("FAILED"), pusd("4"));
        apply(&mut locks, "100", 3);
        locks.note_buy_spend(TradeId::from("LATER"), pusd("3"), UnixNanos::from(4), false);

        let held = apply(&mut locks, "100", 5);
        let drained = apply(&mut locks, "97", 6);

        assert_eq!(
            held,
            AccountBalance::new(pusd("100"), pusd("3"), pusd("97"))
        );
        assert_eq!(
            drained,
            AccountBalance::new(pusd("97"), pusd("0"), pusd("97"))
        );
    }

    #[rstest]
    fn discarded_refresh_keeps_unsettled_spend() {
        let mut locks = CollateralLocks::default();
        apply(&mut locks, "100", 1);
        locks.note_buy_spend(TradeId::from("BUY-1"), pusd("5"), UnixNanos::from(2), false);
        let stale = locks.begin_refresh();
        locks.discard_inflight_refreshes();

        let discarded = locks
            .apply_refresh(stale, pusd("90"), UnixNanos::from(3))
            .unwrap();

        assert!(matches!(discarded, AppliedBalance::Stale));
        assert_eq!(
            apply(&mut locks, "100", 4),
            AccountBalance::new(pusd("100"), pusd("5"), pusd("95"))
        );
    }

    #[rstest]
    fn voided_spend_stays_in_the_release_threshold() {
        let mut locks = CollateralLocks::default();
        apply(&mut locks, "100", 1);
        locks.note_buy_spend(
            TradeId::from("VOIDED"),
            pusd("4"),
            UnixNanos::from(2),
            false,
        );
        locks.note_buy_spend(TradeId::from("OPEN"), pusd("10"), UnixNanos::from(3), false);
        locks.note_voided_spend(TradeId::from("VOIDED"), pusd("4"));

        let after_void_drop = apply(&mut locks, "96", 4);
        let after_both = apply(&mut locks, "86", 5);

        assert_eq!(
            after_void_drop,
            AccountBalance::new(pusd("96"), pusd("10"), pusd("86"))
        );
        assert_eq!(
            after_both,
            AccountBalance::new(pusd("86"), pusd("0"), pusd("86"))
        );
    }

    #[rstest]
    fn stale_refresh_does_not_release_or_move_the_anchor() {
        let mut locks = CollateralLocks::default();
        apply(&mut locks, "100", 1);
        locks.note_buy_spend(TradeId::from("BUY-1"), pusd("5"), UnixNanos::from(2), false);
        let stale = locks.begin_refresh();
        let current = locks.begin_refresh();
        let applied = locks
            .apply_refresh(current, pusd("100"), UnixNanos::from(3))
            .unwrap();
        let discarded = locks
            .apply_refresh(stale, pusd("90"), UnixNanos::from(4))
            .unwrap();

        assert!(matches!(applied, AppliedBalance::Fresh(_)));
        assert!(matches!(discarded, AppliedBalance::Stale));
        assert_eq!(
            apply(&mut locks, "100", 5),
            AccountBalance::new(pusd("100"), pusd("5"), pusd("95"))
        );
    }

    #[rstest]
    fn cleared_session_discards_an_in_flight_refresh() {
        let mut locks = CollateralLocks::default();
        let ticket = locks.begin_refresh();
        locks.clear_session();
        locks.note_buy_spend(TradeId::from("BUY-1"), pusd("5"), UnixNanos::from(2), false);

        let discarded = locks
            .apply_refresh(ticket, pusd("100"), UnixNanos::from(3))
            .unwrap();

        assert!(matches!(discarded, AppliedBalance::Stale));
        assert_eq!(
            apply(&mut locks, "100", 4),
            AccountBalance::new(pusd("100"), pusd("5"), pusd("95"))
        );
    }

    #[rstest]
    fn old_reconciliation_fill_is_not_locked() {
        let mut locks = CollateralLocks::default();
        apply(&mut locks, "100", 20_000_000_000);
        locks.note_buy_spend(TradeId::from("OLD"), pusd("5"), UnixNanos::from(1), true);
        locks.note_buy_spend(
            TradeId::from("RECENT"),
            pusd("2"),
            UnixNanos::from(20_000_000_000 - 1_000_000_000),
            true,
        );

        assert_eq!(
            apply(&mut locks, "100", 21_000_000_000),
            AccountBalance::new(pusd("100"), pusd("2"), pusd("98"))
        );
    }

    #[rstest]
    #[case::sufficient("10", "7", "3")]
    #[case::clamped("5", "5", "0")]
    #[case::empty_balance("0", "0", "0")]
    fn reservations_reduce_free(#[case] total: &str, #[case] locked: &str, #[case] free: &str) {
        let mut locks = CollateralLocks::default();
        locks
            .reservations
            .insert(ClientOrderId::from("BUY-1"), pusd("3"));
        locks
            .reservations
            .insert(ClientOrderId::from("BUY-2"), pusd("4"));

        assert_eq!(
            apply(&mut locks, total, 1),
            AccountBalance::new(pusd(total), pusd(locked), pusd(free))
        );
    }

    #[rstest]
    fn reservation_overflow_is_rejected() {
        let mut locks = CollateralLocks::default();
        let amount = Money::from_raw(MONEY_RAW_MAX, Currency::pUSD());
        locks
            .reservations
            .insert(ClientOrderId::from("BUY-1"), amount);
        locks
            .reservations
            .insert(ClientOrderId::from("BUY-2"), amount);
        let ticket = locks.begin_refresh();

        let error = locks
            .apply_refresh(ticket, pusd("100"), UnixNanos::from(1))
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "invalid Polymarket order reservation total"
        );
    }

    #[rstest]
    fn spend_overflow_leaves_the_cohort_unchanged() {
        let mut locks = CollateralLocks::default();
        apply(&mut locks, "100", 1);
        let amount = Money::from_raw(MONEY_RAW_MAX, Currency::pUSD());
        locks.note_buy_spend(TradeId::from("BUY-1"), amount, UnixNanos::from(2), false);
        locks.note_buy_spend(TradeId::from("BUY-2"), amount, UnixNanos::from(3), false);
        let ticket = locks.begin_refresh();

        let error = locks
            .apply_refresh(ticket, pusd("100"), UnixNanos::from(4))
            .unwrap_err();

        assert_eq!(error.to_string(), "invalid Polymarket unsettled buy spend");
        assert!(locks.unsettled.open.contains_key(&TradeId::from("BUY-1")));
        assert!(locks.unsettled.open.contains_key(&TradeId::from("BUY-2")));
    }
}
