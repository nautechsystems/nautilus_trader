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
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, OnceLock, Weak},
};

use nautilus_common::{cache::Cache, live::sender::EventSender, messages::DataEvent};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{Data, InstrumentClose},
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    orders::{Order, OrderAny},
    reports::{FillReport, OrderStatusReport},
    types::Quantity,
};
use rust_decimal::Decimal;

static SCOPES: OnceLock<Mutex<Vec<Scope>>> = OnceLock::new();

struct Scope {
    id: UUID4,
    sender: EventSender<DataEvent>,
    barrier: Weak<Mutex<SettlementBarrier>>,
}

#[derive(Debug)]
pub(super) struct Registration(UUID4);

impl Registration {
    pub(super) fn new(
        sender: EventSender<DataEvent>,
        barrier: &Arc<Mutex<SettlementBarrier>>,
    ) -> Self {
        let id = UUID4::new();
        let mut scopes = SCOPES
            .get_or_init(Default::default)
            .lock()
            .expect("settlement registry lock");
        scopes.retain(|scope| !scope.sender.is_closed());
        scopes.push(Scope {
            id,
            sender,
            barrier: Arc::downgrade(barrier),
        });

        Self(id)
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        let scopes = SCOPES
            .get_or_init(Default::default)
            .lock()
            .expect("settlement registry lock");

        if let Some(barrier) = scopes
            .iter()
            .find(|scope| scope.id == self.0)
            .and_then(|scope| scope.barrier.upgrade())
        {
            barrier.lock().expect("settlement barrier lock").block();
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct SettlementBarrier {
    session: String,
    observed: bool,
    blocked: bool,
    tracked: HashSet<InstrumentId>,
    receipts: HashMap<ClientOrderId, Receipt>,
    reports: HashMap<TradeId, (InstrumentId, VenueOrderId)>,
    seals: HashMap<InstrumentId, Seal>,
}

#[derive(Debug)]
struct Receipt {
    start: UnixNanos,
    instrument_id: InstrumentId,
    trades: HashSet<TradeId>,
    filled: Quantity,
    terminal: bool,
}

#[derive(Debug)]
struct Seal {
    value: Decimal,
    timestamp: UnixNanos,
    exposure: Option<Decimal>,
    prepared: bool,
    received: bool,
    released: bool,
}

impl SettlementBarrier {
    pub(super) fn invalidate(&mut self, session: &str) {
        self.session = session.to_owned();
        self.observed = false;

        for seal in self.seals.values_mut() {
            seal.prepared = false;
            seal.received = false;
        }
    }

    pub(super) fn track(&mut self, id: InstrumentId) {
        self.tracked.insert(id);
    }

    pub(super) fn receipt(&mut self, order: &OrderAny) {
        self.track(order.instrument_id());
        self.receipts.insert(
            order.client_order_id(),
            Receipt {
                start: order.ts_init(),
                instrument_id: order.instrument_id(),
                trades: order.trade_ids().into_iter().copied().collect(),
                filled: order.filled_qty(),
                terminal: order.is_closed(),
            },
        );
    }

    pub(super) fn recovery_start(&self, start: UnixNanos, prior: UnixNanos) -> UnixNanos {
        let start = if self.reports.is_empty() {
            start
        } else {
            start.min(prior)
        };

        self.receipts
            .values()
            .fold(start, |start, receipt| start.min(receipt.start))
    }

    pub(super) fn sealed(&self, id: InstrumentId) -> bool {
        self.seals
            .get(&id)
            .is_some_and(|seal| seal.exposure.is_some() || seal.released)
    }

    pub(super) fn block(&mut self) {
        self.blocked = true;
        self.observed = false;
    }

    pub(super) fn is_observed(&self) -> bool {
        self.observed
    }

    pub(super) fn is_blocked(&self) -> bool {
        self.blocked
    }

    pub(super) fn requested(&self) -> Vec<(InstrumentId, Decimal, UnixNanos)> {
        self.seals
            .iter()
            .filter(|(id, seal)| self.tracked.contains(id) && !seal.released)
            .map(|(id, seal)| (*id, seal.value, seal.timestamp))
            .collect()
    }

    pub(super) fn check_signature(
        &mut self,
        id: InstrumentId,
        value: Decimal,
        timestamp: UnixNanos,
    ) -> anyhow::Result<()> {
        if self
            .seals
            .get(&id)
            .is_some_and(|seal| seal.value != value || seal.timestamp != timestamp)
        {
            self.block();
            anyhow::bail!("settlement signature changed");
        }

        Ok(())
    }

    pub(super) fn prepare(
        &mut self,
        id: InstrumentId,
        value: Decimal,
        timestamp: UnixNanos,
        exposure: Decimal,
    ) -> anyhow::Result<()> {
        self.check_signature(id, value, timestamp)?;

        let seal = self.seals.entry(id).or_insert(Seal {
            value,
            timestamp,
            exposure: None,
            prepared: false,
            received: false,
            released: false,
        });

        if seal.exposure.is_none() {
            seal.exposure = Some(exposure);
        }

        seal.prepared = true;
        Ok(())
    }

    pub(super) fn observe(&mut self, cache: &Cache, account: AccountId) {
        for id in &self.tracked {
            if let Some(close) = cache.instrument_close(id) {
                self.seals.entry(*id).or_insert(Seal {
                    value: close.close_price.as_decimal(),
                    timestamp: close.ts_event,
                    exposure: Some(Decimal::ZERO),
                    prepared: false,
                    received: false,
                    released: true,
                });
            }
        }

        self.observed = cache
            .account(&account)
            .and_then(|account| account.last_event())
            .and_then(|event| event.info)
            .is_some_and(|info| {
                info.get("kalshi_session")
                    .and_then(serde_json::Value::as_str)
                    == Some(self.session.as_str())
            });

        self.receipts.retain(|id, receipt| {
            !cache.order_ref(id).is_some_and(|order| {
                order.instrument_id() == receipt.instrument_id
                    && order.filled_qty() >= receipt.filled
                    && (!receipt.terminal || order.is_closed())
                    && receipt
                        .trades
                        .iter()
                        .all(|trade| order.trade_ids().contains(&trade))
            })
        });

        self.reports.retain(|trade, (id, venue)| {
            !cache
                .client_order_id(venue)
                .and_then(|client| cache.order_ref(client))
                .is_some_and(|order| {
                    order.instrument_id() == *id && order.trade_ids().contains(&trade)
                })
        });

        for (id, seal) in &mut self.seals {
            let net: Decimal = cache
                .positions_open(None, Some(id), None, Some(&account), None)
                .iter()
                .map(|position| position.signed_decimal_qty())
                .sum();
            seal.received = !self
                .receipts
                .values()
                .any(|receipt| receipt.instrument_id == *id)
                && !self
                    .reports
                    .values()
                    .any(|(instrument, _)| instrument == id)
                && seal.exposure == Some(net);
            if let Some(close) = cache.instrument_close(id) {
                if close.close_price.as_decimal() != seal.value || close.ts_event != seal.timestamp
                {
                    self.blocked = true;
                }

                seal.received = net.is_zero()
                    && !self
                        .receipts
                        .values()
                        .any(|receipt| receipt.instrument_id == *id);
                seal.received &= !self
                    .reports
                    .values()
                    .any(|(instrument, _)| instrument == id);
                seal.released = true;
            }
        }
    }

    pub(super) fn admit_fill(&mut self, fill: &FillReport) {
        self.track(fill.instrument_id);
        self.reports
            .insert(fill.trade_id, (fill.instrument_id, fill.venue_order_id));
    }

    pub(super) fn admit_order(&mut self, report: &OrderStatusReport) {
        self.track(report.instrument_id);
        if let Some(id) = report.client_order_id {
            let receipt = self.receipts.entry(id).or_insert(Receipt {
                start: report.ts_accepted,
                instrument_id: report.instrument_id,
                trades: HashSet::new(),
                filled: report.filled_qty,
                terminal: false,
            });

            receipt.filled = receipt.filled.max(report.filled_qty);
        }
    }

    fn request(&mut self, close: InstrumentClose) -> anyhow::Result<bool> {
        let seal = self.seals.entry(close.instrument_id).or_insert(Seal {
            value: close.close_price.as_decimal(),
            timestamp: close.ts_event,
            exposure: None,
            prepared: false,
            received: false,
            released: false,
        });

        anyhow::ensure!(
            seal.value == close.close_price.as_decimal() && seal.timestamp == close.ts_event,
            "final close contradicts execution settlement"
        );
        Ok(!self.blocked
            && self.observed
            && (!self.tracked.contains(&close.instrument_id)
                || (seal.prepared || seal.released) && seal.received))
    }
}

pub(crate) fn publish_close(
    sender: &EventSender<DataEvent>,
    close: InstrumentClose,
) -> anyhow::Result<bool> {
    // Keep registration and epoch changes outside the publication boundary.
    let scopes = SCOPES
        .get_or_init(Default::default)
        .lock()
        .expect("settlement registry lock");
    let matching: Vec<_> = scopes
        .iter()
        .filter(|scope| scope.sender.same_channel(sender))
        .collect();
    let barriers: Option<Vec<_>> = matching
        .iter()
        .map(|scope| scope.barrier.upgrade())
        .collect();

    let Some(barriers) = barriers else {
        return Ok(false);
    };

    let mut guards: Vec<_> = barriers
        .iter()
        .map(|barrier| barrier.lock().expect("settlement barrier lock"))
        .collect();
    let mut allowed = true;
    for state in &mut guards {
        allowed &= state.request(close)?;
    }

    if !allowed {
        return Ok(false);
    }

    sender
        .send(DataEvent::Data(Data::InstrumentClose(close)))
        .map_err(|_| anyhow::anyhow!("close event receiver closed"))?;

    for state in &mut guards {
        state
            .seals
            .get_mut(&close.instrument_id)
            .expect("requested close")
            .released = true;
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use nautilus_model::{enums::InstrumentCloseType, types::Price};
    use rstest::rstest;

    use super::*;

    fn close() -> InstrumentClose {
        InstrumentClose::new(
            "DEMO.KALSHI".into(),
            Price::from("0.420000"),
            InstrumentCloseType::ContractExpired,
            UnixNanos::from(123),
            UnixNanos::from(456),
        )
    }

    #[rstest]
    fn all_scopes_on_the_data_channel_must_authorize_a_close() {
        let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
        let sender: EventSender<_> = sender.into();
        let first = Arc::new(Mutex::new(SettlementBarrier::default()));
        let second = Arc::new(Mutex::new(SettlementBarrier::default()));
        let first_registration = Registration::new(sender.clone(), &first);
        let second_registration = Registration::new(sender.clone(), &second);
        first.lock().unwrap().observed = true;
        assert!(!publish_close(&sender, close()).unwrap());
        assert!(output.try_recv().is_err());
        second.lock().unwrap().observed = true;
        assert!(publish_close(&sender, close()).unwrap());
        assert!(
            matches!(output.try_recv().unwrap(), DataEvent::Data(Data::InstrumentClose(actual)) if actual == close())
        );
        second.lock().unwrap().invalidate("new session");
        assert!(!publish_close(&sender, close()).unwrap());
        drop(second_registration);
        assert!(!publish_close(&sender, close()).unwrap());
        drop(second);
        assert!(!publish_close(&sender, close()).unwrap());
        drop(first_registration);
        assert!(output.try_recv().is_err());
    }

    #[rstest]
    fn a_data_only_channel_does_not_inherit_another_channels_barrier() {
        let (first, _first_output) = tokio::sync::mpsc::unbounded_channel();
        let first: EventSender<_> = first.into();
        let barrier = Arc::new(Mutex::new(SettlementBarrier::default()));
        let _registration = Registration::new(first, &barrier);
        let (second, mut output) = tokio::sync::mpsc::unbounded_channel();
        let second: EventSender<_> = second.into();
        assert!(publish_close(&second, close()).unwrap());
        assert!(
            matches!(output.try_recv().unwrap(), DataEvent::Data(Data::InstrumentClose(actual)) if actual == close())
        );
        assert!(output.try_recv().is_err());
    }

    #[rstest]
    fn a_failed_close_send_does_not_seal_untracked_execution() {
        let (sender, output) = tokio::sync::mpsc::unbounded_channel();
        let sender: EventSender<_> = sender.into();
        let barrier = Arc::new(Mutex::new(SettlementBarrier::default()));
        let _registration = Registration::new(sender.clone(), &barrier);
        barrier.lock().unwrap().observed = true;
        drop(output);
        assert_eq!(
            publish_close(&sender, close()).unwrap_err().to_string(),
            "close event receiver closed"
        );
        assert!(!barrier.lock().unwrap().sealed(close().instrument_id));
    }

    #[rstest]
    fn pending_closes_remain_requested_across_invalidations() {
        let mut barrier = SettlementBarrier::default();
        let close = close();
        barrier.track(close.instrument_id);
        assert!(!barrier.request(close).unwrap());
        let expected = vec![(
            close.instrument_id,
            close.close_price.as_decimal(),
            close.ts_event,
        )];
        assert_eq!(barrier.requested(), expected);
        barrier.invalidate("next session");
        assert_eq!(barrier.requested(), expected);
    }

    #[rstest]
    fn cached_close_replay_uses_current_receipts_without_metadata_revalidation() {
        let mut cache = Cache::default();
        let close = close();
        cache.add_instrument_close(close).unwrap();
        let mut barrier = SettlementBarrier::default();
        barrier.track(close.instrument_id);
        barrier.invalidate("reconnected");
        barrier.observe(&cache, "KALSHI-001".into());
        barrier.observed = true;
        assert!(barrier.request(close).unwrap());
        assert_eq!(barrier.requested(), Vec::new());
        assert!(barrier.sealed(close.instrument_id));
        barrier.receipts.insert(
            "pending".into(),
            Receipt {
                start: UnixNanos::from(1),
                instrument_id: close.instrument_id,
                trades: HashSet::new(),
                filled: Quantity::from("0.01"),
                terminal: false,
            },
        );

        barrier.observe(&cache, "KALSHI-001".into());
        barrier.observed = true;
        assert!(!barrier.request(close).unwrap());
    }
}
