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

//! Stateful cache database test double.

use std::sync::{Arc, Mutex};

use ahash::AHashMap;
use bytes::Bytes;
use indexmap::IndexMap;
use nautilus_common::{
    cache::database::{CacheDatabaseAdapter, CacheMap},
    signal::Signal,
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    accounts::AccountAny,
    data::{
        Bar, CustomData, DataType, FundingRateUpdate, QuoteTick, TradeTick,
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
use ustr::Ustr;

#[expect(
    clippy::struct_excessive_bools,
    reason = "independent switches cover actor and strategy load and update failures"
)]
#[derive(Debug, Default)]
struct TestCacheDatabaseState {
    actors: AHashMap<ActorId, AHashMap<String, Bytes>>,
    strategies: AHashMap<StrategyId, AHashMap<String, Bytes>>,
    events: Vec<String>,
    fail_load_actor: bool,
    fail_load_strategy: bool,
    fail_update_actor: bool,
    fail_update_strategy: bool,
}

/// Shared control and observation handle for [`TestCacheDatabase`].
#[derive(Clone, Debug, Default)]
pub struct TestCacheDatabaseControl {
    state: Arc<Mutex<TestCacheDatabaseState>>,
}

#[allow(
    clippy::missing_panics_doc,
    reason = "mutex poisoning is not expected in lifecycle tests"
)]
impl TestCacheDatabaseControl {
    /// Creates an adapter and its shared control handle.
    #[must_use]
    pub fn create() -> (TestCacheDatabase, Self) {
        let control = Self::default();
        (
            TestCacheDatabase {
                control: control.clone(),
            },
            control,
        )
    }

    /// Records an event in the shared lifecycle log.
    pub fn record(&self, event: impl Into<String>) {
        self.state.lock().unwrap().events.push(event.into());
    }

    /// Returns the recorded lifecycle events.
    #[must_use]
    pub fn events(&self) -> Vec<String> {
        self.state.lock().unwrap().events.clone()
    }

    /// Seeds actor state for a later load.
    pub fn set_actor_state(&self, actor_id: ActorId, state: &IndexMap<String, Vec<u8>>) {
        self.state
            .lock()
            .unwrap()
            .actors
            .insert(actor_id, encode_state(state));
    }

    /// Seeds strategy state for a later load.
    pub fn set_strategy_state(&self, strategy_id: StrategyId, state: &IndexMap<String, Vec<u8>>) {
        self.state
            .lock()
            .unwrap()
            .strategies
            .insert(strategy_id, encode_state(state));
    }

    /// Returns persisted actor state.
    #[must_use]
    pub fn actor_state(&self, actor_id: &ActorId) -> Option<IndexMap<String, Vec<u8>>> {
        self.state
            .lock()
            .unwrap()
            .actors
            .get(actor_id)
            .cloned()
            .map(decode_state)
    }

    /// Returns persisted strategy state.
    #[must_use]
    pub fn strategy_state(&self, strategy_id: &StrategyId) -> Option<IndexMap<String, Vec<u8>>> {
        self.state
            .lock()
            .unwrap()
            .strategies
            .get(strategy_id)
            .cloned()
            .map(decode_state)
    }

    /// Configures actor loads to fail.
    pub fn set_fail_load_actor(&self, fail: bool) {
        self.state.lock().unwrap().fail_load_actor = fail;
    }

    /// Configures strategy loads to fail.
    pub fn set_fail_load_strategy(&self, fail: bool) {
        self.state.lock().unwrap().fail_load_strategy = fail;
    }

    /// Configures actor updates to fail.
    pub fn set_fail_update_actor(&self, fail: bool) {
        self.state.lock().unwrap().fail_update_actor = fail;
    }

    /// Configures strategy updates to fail.
    pub fn set_fail_update_strategy(&self, fail: bool) {
        self.state.lock().unwrap().fail_update_strategy = fail;
    }
}

/// Stateful cache database adapter for lifecycle tests.
#[derive(Debug)]
pub struct TestCacheDatabase {
    control: TestCacheDatabaseControl,
}

#[async_trait::async_trait]
impl CacheDatabaseAdapter for TestCacheDatabase {
    fn close(&mut self) -> anyhow::Result<()> {
        self.control.record("database.close");
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

    fn load_actor(&self, actor_id: &ActorId) -> anyhow::Result<AHashMap<String, Bytes>> {
        self.control.record(format!("actor.load:{actor_id}"));
        let state = self.control.state.lock().unwrap();
        if state.fail_load_actor {
            anyhow::bail!("test actor load failure");
        }
        Ok(state.actors.get(actor_id).cloned().unwrap_or_default())
    }

    fn load_strategy(&self, strategy_id: &StrategyId) -> anyhow::Result<AHashMap<String, Bytes>> {
        self.control.record(format!("strategy.load:{strategy_id}"));
        let state = self.control.state.lock().unwrap();
        if state.fail_load_strategy {
            anyhow::bail!("test strategy load failure");
        }
        Ok(state
            .strategies
            .get(strategy_id)
            .cloned()
            .unwrap_or_default())
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

    fn add_synthetic(&self, _synthetic: &SyntheticInstrument) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_account(&self, _account: &AccountAny) -> anyhow::Result<()> {
        Ok(())
    }

    fn add_order(&self, _order: &OrderAny, _client_id: Option<ClientId>) -> anyhow::Result<()> {
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
        actor_id: &ActorId,
        actor_state: &AHashMap<String, Bytes>,
    ) -> anyhow::Result<()> {
        self.control.record(format!("actor.update:{actor_id}"));
        let mut state = self.control.state.lock().unwrap();
        if state.fail_update_actor {
            anyhow::bail!("test actor update failure");
        }
        state.actors.insert(*actor_id, actor_state.clone());
        Ok(())
    }

    fn update_strategy(
        &self,
        strategy_id: &StrategyId,
        strategy_state: &AHashMap<String, Bytes>,
    ) -> anyhow::Result<()> {
        self.control
            .record(format!("strategy.update:{strategy_id}"));
        let mut state = self.control.state.lock().unwrap();
        if state.fail_update_strategy {
            anyhow::bail!("test strategy update failure");
        }
        state
            .strategies
            .insert(*strategy_id, strategy_state.clone());
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

    fn snapshot_order_state(&self, _order: &OrderAny) -> anyhow::Result<()> {
        Ok(())
    }

    fn snapshot_position_state(
        &self,
        _position: &Position,
        _ts_snapshot: UnixNanos,
        _unrealized_pnl: Option<Money>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn heartbeat(&self, _timestamp: UnixNanos) -> anyhow::Result<()> {
        Ok(())
    }
}

fn decode_state(state: AHashMap<String, Bytes>) -> IndexMap<String, Vec<u8>> {
    state
        .into_iter()
        .map(|(key, value)| (key, value.to_vec()))
        .collect()
}

fn encode_state(state: &IndexMap<String, Vec<u8>>) -> AHashMap<String, Bytes> {
    state
        .iter()
        .map(|(key, value)| (key.clone(), Bytes::copy_from_slice(value)))
        .collect()
}
