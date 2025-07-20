// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Python bindings for the [`Cache`] component.

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    data::{Bar, BarType, QuoteTick, TradeTick},
    enums::{OmsType, OrderSide, PositionSide},
    identifiers::{ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, Venue},
    position::Position,
    python::{
        instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
        orders::{order_any_to_pyobject, pyobject_to_order_any},
    },
    types::Currency,
};
use pyo3::prelude::*;

use crate::{
    cache::{Cache, CacheConfig},
    enums::SerializationEncoding,
};

#[pymethods]
impl CacheConfig {
    #[new]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        encoding: Option<SerializationEncoding>,
        timestamps_as_iso8601: Option<bool>,
        buffer_interval_ms: Option<usize>,
        use_trader_prefix: Option<bool>,
        use_instance_id: Option<bool>,
        flush_on_start: Option<bool>,
        drop_instruments_on_reset: Option<bool>,
        tick_capacity: Option<usize>,
        bar_capacity: Option<usize>,
        save_market_data: Option<bool>,
    ) -> Self {
        Self::new(
            None, // database is None since we can't expose it to Python yet
            encoding.unwrap_or(SerializationEncoding::MsgPack),
            timestamps_as_iso8601.unwrap_or(false),
            buffer_interval_ms,
            use_trader_prefix.unwrap_or(true),
            use_instance_id.unwrap_or(false),
            flush_on_start.unwrap_or(false),
            drop_instruments_on_reset.unwrap_or(true),
            tick_capacity.unwrap_or(10_000),
            bar_capacity.unwrap_or(10_000),
            save_market_data.unwrap_or(false),
        )
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn encoding(&self) -> SerializationEncoding {
        self.encoding
    }

    #[getter]
    fn timestamps_as_iso8601(&self) -> bool {
        self.timestamps_as_iso8601
    }

    #[getter]
    fn buffer_interval_ms(&self) -> Option<usize> {
        self.buffer_interval_ms
    }

    #[getter]
    fn use_trader_prefix(&self) -> bool {
        self.use_trader_prefix
    }

    #[getter]
    fn use_instance_id(&self) -> bool {
        self.use_instance_id
    }

    #[getter]
    fn flush_on_start(&self) -> bool {
        self.flush_on_start
    }

    #[getter]
    fn drop_instruments_on_reset(&self) -> bool {
        self.drop_instruments_on_reset
    }

    #[getter]
    fn tick_capacity(&self) -> usize {
        self.tick_capacity
    }

    #[getter]
    fn bar_capacity(&self) -> usize {
        self.bar_capacity
    }

    #[getter]
    fn save_market_data(&self) -> bool {
        self.save_market_data
    }
}

#[pymethods]
impl Cache {
    #[new]
    fn py_new(config: Option<CacheConfig>) -> Self {
        Self::new(config, None)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }

    #[pyo3(name = "dispose")]
    fn py_dispose(&mut self) {
        self.dispose();
    }

    #[pyo3(name = "add_currency")]
    fn py_add_currency(&mut self, currency: Currency) -> PyResult<()> {
        self.add_currency(currency).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "add_instrument")]
    fn py_add_instrument(&mut self, py: Python, instrument: PyObject) -> PyResult<()> {
        let instrument_any = pyobject_to_instrument_any(py, instrument)?;
        self.add_instrument(instrument_any).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "instrument")]
    fn py_instrument(&self, py: Python, instrument_id: InstrumentId) -> PyResult<Option<PyObject>> {
        match self.instrument(&instrument_id) {
            Some(instrument) => Ok(Some(instrument_any_to_pyobject(py, instrument.clone())?)),
            None => Ok(None),
        }
    }

    #[pyo3(name = "instrument_ids")]
    fn py_instrument_ids(&self, venue: Option<Venue>) -> Vec<InstrumentId> {
        self.instrument_ids(venue.as_ref())
            .into_iter()
            .cloned()
            .collect()
    }

    #[pyo3(name = "instruments")]
    fn py_instruments(&self, py: Python, venue: Option<Venue>) -> PyResult<Vec<PyObject>> {
        let mut py_instruments = Vec::new();

        match venue {
            Some(venue) => {
                let instruments = self.instruments(&venue, None);
                for instrument in instruments {
                    py_instruments.push(instrument_any_to_pyobject(py, (*instrument).clone())?);
                }
            }
            None => {
                // Get all instruments by iterating through instrument_ids and getting each instrument
                let instrument_ids = self.instrument_ids(None);
                for instrument_id in instrument_ids {
                    if let Some(instrument) = self.instrument(instrument_id) {
                        py_instruments.push(instrument_any_to_pyobject(py, instrument.clone())?);
                    }
                }
            }
        }

        Ok(py_instruments)
    }

    #[pyo3(name = "add_order")]
    fn py_add_order(
        &mut self,
        py: Python,
        order: PyObject,
        position_id: Option<PositionId>,
        client_id: Option<ClientId>,
        replace_existing: Option<bool>,
    ) -> PyResult<()> {
        let order_any = pyobject_to_order_any(py, order)?;
        self.add_order(
            order_any,
            position_id,
            client_id,
            replace_existing.unwrap_or(false),
        )
        .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "order")]
    fn py_order(&self, py: Python, client_order_id: ClientOrderId) -> PyResult<Option<PyObject>> {
        match self.order(&client_order_id) {
            Some(order) => Ok(Some(order_any_to_pyobject(py, order.clone())?)),
            None => Ok(None),
        }
    }

    #[pyo3(name = "order_exists")]
    fn py_order_exists(&self, client_order_id: ClientOrderId) -> bool {
        self.order_exists(&client_order_id)
    }

    #[pyo3(name = "is_order_open")]
    fn py_is_order_open(&self, client_order_id: ClientOrderId) -> bool {
        self.is_order_open(&client_order_id)
    }

    #[pyo3(name = "is_order_closed")]
    fn py_is_order_closed(&self, client_order_id: ClientOrderId) -> bool {
        self.is_order_closed(&client_order_id)
    }

    #[pyo3(name = "orders_open_count")]
    fn py_orders_open_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.orders_open_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            side,
        )
    }

    #[pyo3(name = "orders_closed_count")]
    fn py_orders_closed_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.orders_closed_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            side,
        )
    }

    #[pyo3(name = "orders_total_count")]
    fn py_orders_total_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        side: Option<OrderSide>,
    ) -> usize {
        self.orders_total_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            side,
        )
    }

    #[pyo3(name = "add_position")]
    fn py_add_position(
        &mut self,
        py: Python,
        position: PyObject,
        oms_type: OmsType,
    ) -> PyResult<()> {
        let position_obj = position.extract::<Position>(py)?;
        self.add_position(position_obj, oms_type)
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "position")]
    fn py_position(&self, py: Python, position_id: PositionId) -> PyResult<Option<PyObject>> {
        match self.position(&position_id) {
            Some(position) => Ok(Some(position.clone().into_pyobject(py)?.into())),
            None => Ok(None),
        }
    }

    #[pyo3(name = "position_exists")]
    fn py_position_exists(&self, position_id: PositionId) -> bool {
        self.position_exists(&position_id)
    }

    #[pyo3(name = "is_position_open")]
    fn py_is_position_open(&self, position_id: PositionId) -> bool {
        self.is_position_open(&position_id)
    }

    #[pyo3(name = "is_position_closed")]
    fn py_is_position_closed(&self, position_id: PositionId) -> bool {
        self.is_position_closed(&position_id)
    }

    #[pyo3(name = "positions_open_count")]
    fn py_positions_open_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        side: Option<PositionSide>,
    ) -> usize {
        self.positions_open_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            side,
        )
    }

    #[pyo3(name = "positions_closed_count")]
    fn py_positions_closed_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        side: Option<PositionSide>,
    ) -> usize {
        self.positions_closed_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            side,
        )
    }

    #[pyo3(name = "positions_total_count")]
    fn py_positions_total_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        side: Option<PositionSide>,
    ) -> usize {
        self.positions_total_count(
            venue.as_ref(),
            instrument_id.as_ref(),
            strategy_id.as_ref(),
            side,
        )
    }

    #[pyo3(name = "add_quote")]
    fn py_add_quote(&mut self, quote: QuoteTick) -> PyResult<()> {
        self.add_quote(quote).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "add_trade")]
    fn py_add_trade(&mut self, trade: TradeTick) -> PyResult<()> {
        self.add_trade(trade).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "add_bar")]
    fn py_add_bar(&mut self, bar: Bar) -> PyResult<()> {
        self.add_bar(bar).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "quote")]
    fn py_quote(&self, instrument_id: InstrumentId) -> Option<QuoteTick> {
        self.quote(&instrument_id).cloned()
    }

    #[pyo3(name = "trade")]
    fn py_trade(&self, instrument_id: InstrumentId) -> Option<TradeTick> {
        self.trade(&instrument_id).cloned()
    }

    #[pyo3(name = "bar")]
    fn py_bar(&self, bar_type: BarType) -> Option<Bar> {
        self.bar(&bar_type).cloned()
    }

    #[pyo3(name = "quotes")]
    fn py_quotes(&self, instrument_id: InstrumentId) -> Option<Vec<QuoteTick>> {
        self.quotes(&instrument_id).map(|deque| deque.to_vec())
    }

    #[pyo3(name = "trades")]
    fn py_trades(&self, instrument_id: InstrumentId) -> Option<Vec<TradeTick>> {
        self.trades(&instrument_id).map(|deque| deque.to_vec())
    }

    #[pyo3(name = "bars")]
    fn py_bars(&self, bar_type: BarType) -> Option<Vec<Bar>> {
        self.bars(&bar_type).map(|deque| deque.to_vec())
    }

    #[pyo3(name = "has_quote_ticks")]
    fn py_has_quote_ticks(&self, instrument_id: InstrumentId) -> bool {
        self.has_quote_ticks(&instrument_id)
    }

    #[pyo3(name = "has_trade_ticks")]
    fn py_has_trade_ticks(&self, instrument_id: InstrumentId) -> bool {
        self.has_trade_ticks(&instrument_id)
    }

    #[pyo3(name = "has_bars")]
    fn py_has_bars(&self, bar_type: BarType) -> bool {
        self.has_bars(&bar_type)
    }

    #[pyo3(name = "quote_count")]
    fn py_quote_count(&self, instrument_id: InstrumentId) -> usize {
        self.quote_count(&instrument_id)
    }

    #[pyo3(name = "trade_count")]
    fn py_trade_count(&self, instrument_id: InstrumentId) -> usize {
        self.trade_count(&instrument_id)
    }

    #[pyo3(name = "bar_count")]
    fn py_bar_count(&self, bar_type: BarType) -> usize {
        self.bar_count(&bar_type)
    }
}
