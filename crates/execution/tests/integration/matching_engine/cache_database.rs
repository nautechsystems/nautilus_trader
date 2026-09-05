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

use ahash::AHashMap;
use bytes::Bytes;
use nautilus_common::{
    cache::database::{CacheDatabaseAdapter, CacheMap},
    signal::Signal,
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    accounts::AccountAny,
    data::{
        Bar, CustomData, DataType, FundingRateUpdate, InstrumentClose, QuoteTick, TradeTick,
        greeks::{GreeksData, YieldCurveData},
    },
    events::{OrderEventAny, OrderSnapshot, position::snapshot::PositionSnapshot},
    identifiers::{
        AccountId, ActorId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId,
        VenueOrderId,
    },
    instruments::{InstrumentAny, SyntheticInstrument},
    orderbook::OrderBook,
    orders::OrderAny,
    position::Position,
    types::{Currency, Money},
};
use parking_lot::Mutex;
use ustr::Ustr;

#[derive(Debug, Default)]
struct FailNthAddOrderState {
    fail_add_order_on: Option<usize>,
    add_order_calls: usize,
    order_snapshots: Vec<OrderSnapshot>,
    position_snapshots: Vec<PositionSnapshot>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct FailNthAddOrderDatabaseControl {
    state: Arc<Mutex<FailNthAddOrderState>>,
}

impl FailNthAddOrderDatabaseControl {
    pub(super) fn set_fail_add_order_on(&self, call: Option<usize>) {
        let mut state = self.state.lock();
        state.fail_add_order_on = call;
        state.add_order_calls = 0;
    }

    #[allow(dead_code, reason = "used by the sibling exec_engine test module")]
    pub(super) fn order_snapshots(&self) -> Vec<OrderSnapshot> {
        self.state.lock().order_snapshots.clone()
    }

    #[allow(dead_code, reason = "used by the sibling exec_engine test module")]
    pub(super) fn position_snapshots(&self) -> Vec<PositionSnapshot> {
        self.state.lock().position_snapshots.clone()
    }
}

#[derive(Debug)]
pub(super) struct FailNthAddOrderDatabase {
    control: FailNthAddOrderDatabaseControl,
}

impl FailNthAddOrderDatabase {
    pub(super) fn create() -> (Self, FailNthAddOrderDatabaseControl) {
        let control = FailNthAddOrderDatabaseControl::default();
        (
            Self {
                control: control.clone(),
            },
            control,
        )
    }
}

#[async_trait::async_trait]
impl CacheDatabaseAdapter for FailNthAddOrderDatabase {
    fn close(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn load_all(&self) -> anyhow::Result<CacheMap> {
        Ok(CacheMap::default())
    }

    fn load(&self) -> anyhow::Result<AHashMap<String, Bytes>> {
        Ok(AHashMap::new())
    }

    async fn load_currencies(&self) -> anyhow::Result<AHashMap<Ustr, Currency>> {
        Ok(AHashMap::new())
    }

    async fn load_instruments(&self) -> anyhow::Result<AHashMap<InstrumentId, InstrumentAny>> {
        Ok(AHashMap::new())
    }

    async fn load_instrument_closes(
        &self,
    ) -> anyhow::Result<AHashMap<InstrumentId, InstrumentClose>> {
        Ok(AHashMap::new())
    }

    async fn load_synthetics(&self) -> anyhow::Result<AHashMap<InstrumentId, SyntheticInstrument>> {
        Ok(AHashMap::new())
    }

    async fn load_accounts(&self) -> anyhow::Result<AHashMap<AccountId, AccountAny>> {
        Ok(AHashMap::new())
    }

    async fn load_orders(&self) -> anyhow::Result<AHashMap<ClientOrderId, OrderAny>> {
        Ok(AHashMap::new())
    }

    async fn load_positions(&self) -> anyhow::Result<AHashMap<PositionId, Position>> {
        Ok(AHashMap::new())
    }

    fn load_index_order_position(&self) -> anyhow::Result<AHashMap<ClientOrderId, PositionId>> {
        Ok(AHashMap::new())
    }

    fn load_index_order_client(&self) -> anyhow::Result<AHashMap<ClientOrderId, ClientId>> {
        Ok(AHashMap::new())
    }

    async fn load_currency(&self, _code: &Ustr) -> anyhow::Result<Option<Currency>> {
        Ok(None)
    }

    async fn load_instrument(
        &self,
        _instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<InstrumentAny>> {
        Ok(None)
    }

    async fn load_synthetic(
        &self,
        _instrument_id: &InstrumentId,
    ) -> anyhow::Result<Option<SyntheticInstrument>> {
        Ok(None)
    }

    async fn load_account(&self, _account_id: &AccountId) -> anyhow::Result<Option<AccountAny>> {
        Ok(None)
    }

    async fn load_order(
        &self,
        _client_order_id: &ClientOrderId,
    ) -> anyhow::Result<Option<OrderAny>> {
        Ok(None)
    }

    async fn load_position(&self, _position_id: &PositionId) -> anyhow::Result<Option<Position>> {
        Ok(None)
    }

    fn load_actor(&self, _actor_id: &ActorId) -> anyhow::Result<AHashMap<String, Bytes>> {
        Ok(AHashMap::new())
    }

    fn load_strategy(&self, _strategy_id: &StrategyId) -> anyhow::Result<AHashMap<String, Bytes>> {
        Ok(AHashMap::new())
    }

    fn load_signals(&self, _name: &str) -> anyhow::Result<Vec<Signal>> {
        Ok(Vec::new())
    }

    fn load_custom_data(&self, _data_type: &DataType) -> anyhow::Result<Vec<CustomData>> {
        Ok(Vec::new())
    }

    fn load_order_snapshot(
        &self,
        _client_order_id: &ClientOrderId,
    ) -> anyhow::Result<Option<OrderSnapshot>> {
        Ok(None)
    }

    fn load_position_snapshot(
        &self,
        _position_id: &PositionId,
    ) -> anyhow::Result<Option<PositionSnapshot>> {
        Ok(None)
    }

    fn load_quotes(&self, _instrument_id: &InstrumentId) -> anyhow::Result<Vec<QuoteTick>> {
        Ok(Vec::new())
    }

    fn load_trades(&self, _instrument_id: &InstrumentId) -> anyhow::Result<Vec<TradeTick>> {
        Ok(Vec::new())
    }

    fn load_funding_rates(
        &self,
        _instrument_id: &InstrumentId,
    ) -> anyhow::Result<Vec<FundingRateUpdate>> {
        Ok(Vec::new())
    }

    fn load_bars(&self, _instrument_id: &InstrumentId) -> anyhow::Result<Vec<Bar>> {
        Ok(Vec::new())
    }

    fn add(&self, _key: String, _value: Bytes) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_currency(&self, _currency: &Currency) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_instrument(&self, _instrument: &InstrumentAny) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_instrument_close(&self, _close: &InstrumentClose) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_synthetic(&self, _synthetic: &SyntheticInstrument) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_account(&self, _account: &AccountAny) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_order(&self, _order: &OrderAny, _client_id: Option<ClientId>) -> anyhow::Result<()> {
        let mut state = self.control.state.lock();
        state.add_order_calls += 1;
        if state.fail_add_order_on == Some(state.add_order_calls) {
            anyhow::bail!("test add order failure");
        }
        Ok(())
    }

    fn add_order_snapshot(&self, _snapshot: &OrderSnapshot) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_position(&self, _position: &Position) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_position_snapshot(&self, _snapshot: &PositionSnapshot) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_order_book(&self, _order_book: &OrderBook) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_signal(&self, _signal: &Signal) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_custom_data(&self, _data: &CustomData) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_quote(&self, _quote: &QuoteTick) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_trade(&self, _trade: &TradeTick) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_funding_rate(&self, _funding_rate: &FundingRateUpdate) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_bar(&self, _bar: &Bar) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_greeks(&self, _greeks: &GreeksData) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_yield_curve(&self, _yield_curve: &YieldCurveData) -> anyhow::Result<()> {
        Ok(())
    }

    fn delete_actor(&self, _actor_id: &ActorId) -> anyhow::Result<()> {
        Ok(())
    }

    fn delete_strategy(&self, _component_id: &StrategyId) -> anyhow::Result<()> {
        Ok(())
    }

    fn delete_order(&self, _client_order_id: &ClientOrderId) -> anyhow::Result<()> {
        Ok(())
    }

    fn delete_position(&self, _position_id: &PositionId) -> anyhow::Result<()> {
        Ok(())
    }

    fn delete_account_event(&self, _account_id: &AccountId, _event_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn index_venue_order_id(
        &self,
        _client_order_id: ClientOrderId,
        _venue_order_id: VenueOrderId,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn index_order_position(
        &self,
        _client_order_id: ClientOrderId,
        _position_id: PositionId,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn update_actor(
        &self,
        _actor_id: &ActorId,
        _state: &AHashMap<String, Bytes>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn update_strategy(
        &self,
        _strategy_id: &StrategyId,
        _state: &AHashMap<String, Bytes>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn update_account(&self, _account: &AccountAny) -> anyhow::Result<()> {
        Ok(())
    }

    fn update_order(&self, _order_event: &OrderEventAny) -> anyhow::Result<()> {
        Ok(())
    }

    fn update_position(&self, _position: &Position) -> anyhow::Result<()> {
        Ok(())
    }

    fn snapshot_order_state(&self, order: &OrderAny) -> anyhow::Result<()> {
        self.control
            .state
            .lock()
            .order_snapshots
            .push(OrderSnapshot::from(order.clone()));
        Ok(())
    }

    fn snapshot_position_state(
        &self,
        position: &Position,
        ts_snapshot: UnixNanos,
        unrealized_pnl: Option<Money>,
    ) -> anyhow::Result<()> {
        let mut snapshot = PositionSnapshot::from(position, unrealized_pnl);
        snapshot.ts_init = ts_snapshot;

        self.control.state.lock().position_snapshots.push(snapshot);
        Ok(())
    }

    fn heartbeat(&self, _timestamp: UnixNanos) -> anyhow::Result<()> {
        Ok(())
    }
}
