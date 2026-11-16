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

use std::collections::HashMap;

use anyhow::Context;
use nautilus_core::UUID4;
use nautilus_model::{
    enums::OrderSide,
    identifiers::{ClientOrderId, VenueOrderId},
    instruments::BinaryOption,
    types::{AccountBalance, Currency, Money},
};
use rust_decimal::Decimal;

use crate::{
    common::enums::FeeType,
    http::{client::KalshiHttpClient, models::Field, query::EventQuery},
};

#[derive(Debug, Clone)]
pub(super) struct AccountLedger {
    session: UUID4,
    revision: u64,
    total: Option<Decimal>,
    unsettled: bool,
    reservations: HashMap<ReservationId, Decimal>,
}

impl AccountLedger {
    pub(super) fn new() -> Self {
        Self {
            session: UUID4::new(),
            revision: 0,
            total: None,
            unsettled: false,
            reservations: HashMap::new(),
        }
    }

    pub(super) fn total_is_unknown(&self) -> bool {
        self.total.is_none()
    }

    pub(super) fn invalidate(&mut self) {
        self.session = UUID4::new();
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn matches(
        &self,
        total: Decimal,
        reservations: &HashMap<ReservationId, Decimal>,
    ) -> bool {
        !self.unsettled && self.total == Some(total) && self.reservations == *reservations
    }

    pub(super) fn rebase(&mut self) {
        self.invalidate();
        self.total = None;
        self.unsettled = false;
    }

    pub(super) fn begin_refresh(&self) -> RefreshTicket {
        RefreshTicket {
            session: self.session,
            revision: self.revision,
        }
    }

    pub(super) fn check_refresh(&self, ticket: RefreshTicket) -> anyhow::Result<()> {
        anyhow::ensure!(
            ticket.session == self.session && ticket.revision == self.revision,
            "stale Kalshi account refresh"
        );
        Ok(())
    }

    pub(super) fn apply_refresh(
        &mut self,
        ticket: RefreshTicket,
        total: Decimal,
        reservations: HashMap<ReservationId, Decimal>,
    ) -> anyhow::Result<AccountBalance> {
        self.check_refresh(ticket)?;
        anyhow::ensure!(
            !self.unsettled || self.total == Some(total),
            "Kalshi cash does not yet cover collected execution evidence"
        );
        let balance = balance(total, &reservations)?;
        self.total = Some(total);
        self.unsettled = false;
        self.reservations = reservations;
        self.revision = self.revision.wrapping_add(1);
        Ok(balance)
    }

    pub(super) fn reserve(
        &mut self,
        id: ClientOrderId,
        amount: Decimal,
    ) -> anyhow::Result<AccountBalance> {
        anyhow::ensure!(
            !self.reservations.contains_key(&ReservationId::Client(id)),
            "Kalshi command already owns funds"
        );
        let total = self.total.context("Kalshi cash is not initialized")?;
        let mut reservations = self.reservations.clone();
        reservations.insert(ReservationId::Client(id), amount);
        let balance = balance(total, &reservations)?;
        self.reservations = reservations;
        self.revision = self.revision.wrapping_add(1);
        Ok(balance)
    }

    pub(super) fn reserve_amendment(
        &mut self,
        id: ClientOrderId,
        amount: Decimal,
        leaves: Decimal,
        increase: Decimal,
    ) -> anyhow::Result<()> {
        let mut reservations = self.reservations.clone();
        let prior = reservations
            .get(&ReservationId::Client(id))
            .copied()
            .context("Kalshi order reservation is missing")?;

        let maximum = if increase > Decimal::ZERO {
            amount
                .checked_div(leaves)
                .and_then(|unit| unit.checked_mul(increase))
                .and_then(|additional| prior.checked_add(additional))
                .context("Kalshi amendment hold overflow")?
        } else {
            prior
        };

        reservations.insert(ReservationId::Client(id), amount.max(maximum));
        balance(
            self.total.context("Kalshi cash is not initialized")?,
            &reservations,
        )?;
        self.reservations = reservations;
        self.invalidate();
        Ok(())
    }

    pub(super) fn balance(&self) -> anyhow::Result<AccountBalance> {
        balance(
            self.total.context("Kalshi cash is not initialized")?,
            &self.reservations,
        )
    }

    pub(super) fn release(&mut self, client_order_id: ClientOrderId) {
        if self
            .reservations
            .remove(&ReservationId::Client(client_order_id))
            .is_some()
        {
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub(super) fn has_external(&self, venue_order_id: VenueOrderId) -> bool {
        self.reservations
            .contains_key(&ReservationId::Venue(venue_order_id))
    }

    pub(super) fn reservation(&self, id: ClientOrderId) -> Option<Decimal> {
        self.reservations.get(&ReservationId::Client(id)).copied()
    }

    pub(super) fn note_cash_change(&mut self, change: Decimal) -> anyhow::Result<()> {
        if let Some(total) = self.total {
            let next = total.checked_add(change).context("Kalshi cash overflow")?;
            Money::from_decimal_exact(next, Currency::USD())?;
            self.total = Some(next);
            self.unsettled = true;
        }

        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
}

pub(super) async fn hold(
    http: &KalshiHttpClient,
    definition: &BinaryOption,
    side: OrderSide,
    qty: Decimal,
    price: Decimal,
) -> anyhow::Result<Decimal> {
    let event_id = definition
        .event_id
        .context("pending order event identity is missing")?;

    let event = http
        .get_event(
            event_id.as_str(),
            &EventQuery {
                exclude_markets: Some(true),
                ..Default::default()
            },
        )
        .await?
        .event;

    anyhow::ensure!(
        event.event_ticker == event_id.as_str(),
        "fee event identity mismatch"
    );

    let series = http.get_series(&event.series_ticker).await?.series;
    anyhow::ensure!(
        series.ticker == event.series_ticker,
        "fee series identity mismatch"
    );

    let (kind, multiplier) = match (&event.fee_type_override, &event.fee_multiplier_override) {
        (Field::Value(kind), Field::Value(multiplier)) => (kind, multiplier.0),
        (Field::Missing | Field::Null, Field::Missing | Field::Null) => {
            (&series.fee_type, series.fee_multiplier.0)
        }
        _ => anyhow::bail!("incomplete event fee override"),
    };

    anyhow::ensure!(
        matches!(kind, FeeType::Quadratic | FeeType::QuadraticWithMakerFees)
            && multiplier >= Decimal::ZERO,
        "unqualified Kalshi fee model"
    );

    let principal = if side == OrderSide::Buy {
        price
    } else {
        Decimal::ONE - price
    };

    // Each 0.01-contract fill can add at most 0.000001 fee rounding and 0.0001 cash alignment
    let fees = Decimal::new(175, 4)
        .checked_mul(multiplier)
        .and_then(|fee| fee.checked_add(Decimal::new(101, 4)));
    let hold = fees
        .and_then(|fee| fee.checked_add(principal))
        .and_then(|unit| unit.checked_mul(qty))
        .context("Kalshi admission hold overflow")?;
    Ok(hold)
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RefreshTicket {
    session: UUID4,
    revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ReservationId {
    Client(ClientOrderId),
    Venue(VenueOrderId),
}

fn balance(
    total: Decimal,
    reservations: &HashMap<ReservationId, Decimal>,
) -> anyhow::Result<AccountBalance> {
    anyhow::ensure!(total >= Decimal::ZERO, "negative Kalshi balance");
    let mut locked = Decimal::ZERO;

    for amount in reservations.values() {
        anyhow::ensure!(*amount >= Decimal::ZERO, "negative Kalshi admission hold");
        locked = locked
            .checked_add(*amount)
            .context("Kalshi hold overflow")?;
    }

    let free = total
        .checked_sub(locked)
        .context("Kalshi free cash overflow")?;
    anyhow::ensure!(
        free >= Decimal::ZERO,
        "insufficient Kalshi cash for admission holds"
    );
    let currency = Currency::USD();
    let total = Money::from_decimal_exact(total, currency)?;
    let locked = Money::from_decimal_exact(locked, currency)?;
    let free = Money::from_decimal_exact(free, currency)?;
    Ok(AccountBalance::new_checked(total, locked, free)?)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;

    use super::*;

    fn amount(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    fn initialized() -> AccountLedger {
        let mut ledger = AccountLedger::new();
        let ticket = ledger.begin_refresh();
        ledger
            .apply_refresh(ticket, amount("1.0001"), HashMap::new())
            .unwrap();
        ledger
    }

    #[rstest]
    fn pending_admission_preserves_external_holds_and_fences_refresh() {
        let mut ledger = initialized();
        let external = ReservationId::Venue(VenueOrderId::from("external"));
        let ticket = ledger.begin_refresh();
        ledger
            .apply_refresh(
                ticket,
                amount("1.0001"),
                HashMap::from([(external, amount("0.25"))]),
            )
            .unwrap();
        let stale = ledger.begin_refresh();
        let id = ClientOrderId::from("pending");
        let held = ledger.reserve(id, amount("0.600001")).unwrap();
        assert_eq!(held.total.as_decimal(), amount("1.0001"));
        assert_eq!(held.locked.as_decimal(), amount("0.850001"));
        assert_eq!(held.free.as_decimal(), amount("0.150099"));
        assert_eq!(
            ledger.reserve(id, amount("0.01")).unwrap_err().to_string(),
            "Kalshi command already owns funds"
        );
        assert_eq!(
            ledger
                .reserve(ClientOrderId::from("competing"), amount("0.150100"))
                .unwrap_err()
                .to_string(),
            "insufficient Kalshi cash for admission holds"
        );
        assert_eq!(
            ledger
                .apply_refresh(stale, amount("1.0001"), HashMap::new())
                .unwrap_err()
                .to_string(),
            "stale Kalshi account refresh"
        );
        assert_eq!(ledger.balance().unwrap(), held);
        ledger.release(id);
        let balance = ledger.balance().unwrap();
        assert_eq!(balance.locked.as_decimal(), amount("0.25"));
        assert_eq!(balance.free.as_decimal(), amount("0.7501"));
    }

    #[rstest]
    fn concurrent_admission_can_spend_free_cash_only_once() {
        let ledger = std::sync::Arc::new(std::sync::Mutex::new(initialized()));
        let start = std::sync::Arc::new(std::sync::Barrier::new(3));

        let handles = ["concurrent-a", "concurrent-b"].map(|id| {
            let ledger = std::sync::Arc::clone(&ledger);
            let start = std::sync::Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                ledger
                    .lock()
                    .unwrap()
                    .reserve(ClientOrderId::from(id), amount("0.600001"))
            })
        });

        start.wait();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter_map(|result| result.as_ref().err())
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec!["insufficient Kalshi cash for admission holds"]
        );
        let balance = ledger.lock().unwrap().balance().unwrap();
        assert_eq!(balance.total.as_decimal(), amount("1.0001"));
        assert_eq!(balance.locked.as_decimal(), amount("0.600001"));
        assert_eq!(balance.free.as_decimal(), amount("0.400099"));
    }

    #[rstest]
    fn refresh_cannot_release_a_newer_reservation_in_the_same_connection() {
        let mut ledger = initialized();
        let ticket = ledger.begin_refresh();
        let id = ClientOrderId::from("O-001");
        ledger
            .apply_refresh(
                ticket,
                amount("1.0001"),
                HashMap::from([(ReservationId::Client(id), amount("0.123456"))]),
            )
            .unwrap();
        assert_eq!(
            ledger
                .apply_refresh(ticket, amount("1.0001"), HashMap::new())
                .unwrap_err()
                .to_string(),
            "stale Kalshi account refresh"
        );
        assert_eq!(
            ledger.reservations,
            HashMap::from([(ReservationId::Client(id), amount("0.123456"))])
        );
        assert_eq!(ledger.total, Some(amount("1.0001")));
    }

    #[rstest]
    fn reconnect_invalidates_refresh_without_releasing_an_uncertain_hold() {
        let mut ledger = initialized();
        let id = ClientOrderId::from("O-002");
        let initial = ledger.begin_refresh();
        ledger
            .apply_refresh(
                initial,
                amount("1.0001"),
                HashMap::from([(ReservationId::Client(id), amount("0.234567"))]),
            )
            .unwrap();
        let ticket = ledger.begin_refresh();
        ledger.invalidate();
        assert_eq!(
            ledger
                .apply_refresh(ticket, amount("1.0001"), HashMap::new())
                .unwrap_err()
                .to_string(),
            "stale Kalshi account refresh"
        );
        assert_eq!(
            ledger.reservations,
            HashMap::from([(ReservationId::Client(id), amount("0.234567"))])
        );
    }

    #[rstest]
    fn exact_balance_and_fee_cash_survive_without_rounding_or_duplicate_debits() {
        let mut ledger = initialized();
        ledger.note_cash_change(amount("-0.100001")).unwrap();
        let ticket = ledger.begin_refresh();
        assert_eq!(
            ledger
                .apply_refresh(ticket, amount("1.0001"), HashMap::new())
                .unwrap_err()
                .to_string(),
            "Kalshi cash does not yet cover collected execution evidence"
        );
        let id = ClientOrderId::from("O-003");
        let reservations = HashMap::from([(ReservationId::Client(id), amount("0.123456"))]);
        let balance = ledger
            .apply_refresh(ticket, amount("0.900099"), reservations)
            .unwrap();
        assert_eq!(balance.total.as_decimal(), amount("0.900099"));
        assert_eq!(balance.locked.as_decimal(), amount("0.123456"));
        assert_eq!(balance.free.as_decimal(), amount("0.776643"));
        assert_eq!(balance.currency, Currency::USD());
        ledger.release(id);
        assert_eq!(ledger.reservations, HashMap::new());
        assert_eq!(ledger.total, Some(amount("0.900099")));
    }

    #[rstest]
    fn failed_reservation_leaves_the_ledger_unchanged() {
        let mut ledger = initialized();
        let id = ClientOrderId::from("O-004");
        let initial = ledger.begin_refresh();
        ledger
            .apply_refresh(
                initial,
                amount("1.0001"),
                HashMap::from([(ReservationId::Client(id), amount("0.800001"))]),
            )
            .unwrap();
        let revision = ledger.revision;
        assert_eq!(
            ledger
                .apply_refresh(
                    ledger.begin_refresh(),
                    amount("1.0001"),
                    HashMap::from([
                        (ReservationId::Client(id), amount("0.800001")),
                        (
                            ReservationId::Client(ClientOrderId::from("O-005")),
                            amount("0.300001")
                        )
                    ])
                )
                .unwrap_err()
                .to_string(),
            "insufficient Kalshi cash for admission holds"
        );
        assert_eq!(ledger.revision, revision);
        assert_eq!(ledger.total, Some(amount("1.0001")));
        assert_eq!(
            ledger.reservations,
            HashMap::from([(ReservationId::Client(id), amount("0.800001"))])
        );
    }

    #[rstest]
    fn authoritative_non_fill_cash_change_keeps_existing_holds() {
        let mut ledger = initialized();
        let id = ClientOrderId::from("O-006");
        let holds = HashMap::from([(ReservationId::Client(id), amount("0.123456"))]);
        let ticket = ledger.begin_refresh();
        ledger
            .apply_refresh(ticket, amount("1.0001"), holds.clone())
            .unwrap();
        let ticket = ledger.begin_refresh();
        let balance = ledger
            .apply_refresh(ticket, amount("2.0002"), holds.clone())
            .unwrap();
        assert_eq!(balance.total.as_decimal(), amount("2.0002"));
        assert_eq!(balance.locked.as_decimal(), amount("0.123456"));
        assert_eq!(balance.free.as_decimal(), amount("1.876744"));
        assert_eq!(ledger.reservations, holds);
    }

    #[rstest]
    fn client_release_preserves_an_external_reservation_with_the_same_text() {
        let mut ledger = initialized();
        let id = ClientOrderId::from("same-id");
        let external = ReservationId::Venue(VenueOrderId::from("same-id"));
        let reservations = HashMap::from([
            (ReservationId::Client(id), amount("0.123456")),
            (external, amount("0.234567")),
        ]);
        let ticket = ledger.begin_refresh();
        ledger
            .apply_refresh(ticket, amount("1.0001"), reservations)
            .unwrap();
        ledger.release(id);
        let ticket = ledger.begin_refresh();
        let balance = ledger
            .apply_refresh(ticket, amount("1.0001"), ledger.reservations.clone())
            .unwrap();
        assert_eq!(
            ledger.reservations,
            HashMap::from([(external, amount("0.234567"))])
        );
        assert_eq!(balance.total.as_decimal(), amount("1.0001"));
        assert_eq!(balance.locked.as_decimal(), amount("0.234567"));
        assert_eq!(balance.free.as_decimal(), amount("0.765533"));
    }

    #[rstest]
    fn rebase_invalidates_the_old_cash_anchor_without_releasing_holds() {
        let mut ledger = initialized();
        let id = ClientOrderId::from("pending");
        let ticket = ledger.begin_refresh();
        ledger
            .apply_refresh(
                ticket,
                amount("1.0001"),
                HashMap::from([(ReservationId::Client(id), amount("0.25"))]),
            )
            .unwrap();
        let ticket = ledger.begin_refresh();
        ledger.rebase();
        assert!(ledger.total_is_unknown());
        assert_eq!(
            ledger.reservations,
            HashMap::from([(ReservationId::Client(id), amount("0.25"))])
        );
        assert_eq!(
            ledger
                .apply_refresh(ticket, amount("1.0001"), HashMap::new())
                .unwrap_err()
                .to_string(),
            "stale Kalshi account refresh"
        );
        let current = ledger.begin_refresh();
        let balance = ledger
            .apply_refresh(
                current,
                amount("2.0002"),
                HashMap::from([(ReservationId::Client(id), amount("0.25"))]),
            )
            .unwrap();
        assert_eq!(balance.total.as_decimal(), amount("2.0002"));
        assert_eq!(balance.locked.as_decimal(), amount("0.25"));
        assert_eq!(balance.free.as_decimal(), amount("1.7502"));
    }

    #[rstest]
    fn signed_recovery_deltas_require_a_nonnegative_final_cash_balance() {
        let mut ledger = initialized();
        ledger.note_cash_change(amount("-1.0002")).unwrap();
        let ticket = ledger.begin_refresh();
        assert_eq!(
            ledger
                .apply_refresh(ticket, amount("-0.0001"), HashMap::new())
                .unwrap_err()
                .to_string(),
            "negative Kalshi balance"
        );
        ledger.note_cash_change(amount("0.0003")).unwrap();
        let ticket = ledger.begin_refresh();
        let balance = ledger
            .apply_refresh(ticket, amount("0.0002"), HashMap::new())
            .unwrap();
        assert_eq!(balance.total.as_decimal(), amount("0.0002"));
        assert_eq!(balance.free.as_decimal(), amount("0.0002"));
        assert_eq!(balance.locked.as_decimal(), Decimal::ZERO);
    }
}
