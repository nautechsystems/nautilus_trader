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

//! Connects custom Python adapters to the Rust live node through PyO3.
//!
//! Implements the Rust data and execution client traits by forwarding operations to Python
//! clients, and manages factory construction, event-loop binding, and client lifetimes. Adapters
//! receive a read-only cache view and submit typed output to the live runner for processing
//! inside the synchronous core boundary.

pub mod requests;
pub mod responses;

use std::{
    any::Any,
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, ThreadId},
};

use async_trait::async_trait;
use nautilus_common::{
    cache::{Cache, CacheView},
    clients::{
        DEFAULT_POSITION_RECONCILIATION_TOLERANCE, DataClient, ExecutionClient,
        generate_mass_status,
    },
    clock::Clock,
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory, OrderEventFactory},
    live::runner::{get_data_event_sender, get_exec_event_sender},
    messages::{
        DataEvent, ExecutionEvent, ExecutionReport,
        data::{
            RequestBars, RequestBookDeltas, RequestBookDepth, RequestBookSnapshot,
            RequestCustomData, RequestFundingRates, RequestInstrument, RequestInstruments,
            RequestOptionChainReferencePrice, RequestQuotes, RequestTrades, SubscribeBars,
            SubscribeBookDeltas, SubscribeBookDepth10, SubscribeCustomData, SubscribeFundingRates,
            SubscribeIndexPrices, SubscribeInstrument, SubscribeInstrumentClose,
            SubscribeInstrumentStatus, SubscribeInstruments, SubscribeMarkPrices,
            SubscribeOptionGreeks, SubscribeQuotes, SubscribeTrades, UnsubscribeBars,
            UnsubscribeBookDeltas, UnsubscribeBookDepth10, UnsubscribeCustomData,
            UnsubscribeFundingRates, UnsubscribeIndexPrices, UnsubscribeInstrument,
            UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus, UnsubscribeInstruments,
            UnsubscribeMarkPrices, UnsubscribeOptionGreeks, UnsubscribeQuotes, UnsubscribeTrades,
        },
        execution::{
            BatchCancelOrders, BatchModifyOrders, CancelAllOrders, CancelOrder,
            GenerateFillReports, GenerateOrderStatusReport, GenerateOrderStatusReports,
            GeneratePositionStatusReports, ModifyOrder, QueryAccount, QueryOrder, SubmitOrder,
            SubmitOrderList,
        },
    },
    python::clock::PyClock,
};
use nautilus_core::{
    Params, UnixNanos,
    python::{params::pydict_to_params, to_pyruntime_err, to_pytype_err},
};
use nautilus_model::{
    accounts::AccountAny,
    data::{
        Bar, CustomData, Data, FundingRateUpdate, IndexPriceUpdate, InstrumentClose,
        InstrumentStatus, MarkPriceUpdate, OptionGreeks, OrderBookDelta, OrderBookDeltas,
        OrderBookDepth10, QuoteTick, TradeTick,
    },
    enums::{AccountType, LiquiditySide, OmsType, OrderSide, PositionSide},
    events::{
        AccountState, OrderAccepted, OrderAcceptedBatch, OrderCanceled, OrderCanceledBatch,
        OrderSubmitted, OrderSubmittedBatch,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, OrderListId, PositionId, StrategyId,
        TradeId, TraderId, Venue, VenueOrderId,
    },
    instruments::InstrumentAny,
    orderbook::OrderBook,
    orders::OrderList,
    position::Position,
    python::{
        account::account_any_to_pyobject,
        events::order::pyobject_to_order_event,
        instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
        orders::{order_any_to_pyobject, pyobject_to_order_any},
    },
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use parking_lot::Mutex;
use pyo3::{prelude::*, types::PyDict};
use rust_decimal::Decimal;

use self::{
    requests::{
        PyRequestBars, PyRequestBookDeltas, PyRequestBookDepth, PyRequestBookSnapshot,
        PyRequestCustomData, PyRequestFundingRates, PyRequestInstrument, PyRequestInstruments,
        PyRequestOptionChainReferencePrice, PyRequestQuotes, PyRequestTrades,
    },
    responses::extract_response,
};
use super::runtime::PythonOperation;

static CACHE_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static CLIENT_CACHES: RefCell<HashMap<u64, CacheView>> = RefCell::new(HashMap::new());
}

/// Read-only access to the owning node's cache.
#[pyclass(name = "ClientCache", module = "nautilus_trader.live", frozen)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")]
#[derive(Debug)]
pub struct PyClientCache {
    id: u64,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyClientCache {
    #[pyo3(name = "instrument")]
    fn py_instrument(
        &self,
        py: Python<'_>,
        instrument_id: InstrumentId,
    ) -> PyResult<Option<Py<PyAny>>> {
        let view = self.view()?;
        let instrument = view
            .try_borrow()
            .map_err(to_pyruntime_err)?
            .instrument(&instrument_id)
            .cloned();
        instrument
            .map(|instrument| instrument_any_to_pyobject(py, instrument))
            .transpose()
    }

    #[pyo3(name = "quote", signature = (instrument_id, index=0))]
    fn py_quote(&self, instrument_id: InstrumentId, index: usize) -> PyResult<Option<QuoteTick>> {
        let view = self.view()?;
        Ok(view
            .try_borrow()
            .map_err(to_pyruntime_err)?
            .quote_at_index(&instrument_id, index)
            .copied())
    }
    #[pyo3(name = "get")]
    fn py_get(&self, key: &str) -> PyResult<Option<Vec<u8>>> {
        self.read(|cache| {
            cache
                .get(key)
                .map(|value| value.map(|bytes| bytes.to_vec()))
        })?
        .map_err(to_pyruntime_err)
    }

    #[pyo3(name = "instruments", signature = (venue=None))]
    fn py_instruments(&self, py: Python<'_>, venue: Option<Venue>) -> PyResult<Vec<Py<PyAny>>> {
        let instruments = self.read(|cache| {
            cache
                .instrument_ids(venue.as_ref())
                .into_iter()
                .filter_map(|id| cache.instrument(id).cloned())
                .collect::<Vec<_>>()
        })?;

        instruments
            .into_iter()
            .map(|instrument| instrument_any_to_pyobject(py, instrument))
            .collect()
    }

    #[pyo3(name = "instrument_ids", signature = (venue=None))]
    fn py_instrument_ids(&self, venue: Option<Venue>) -> PyResult<Vec<InstrumentId>> {
        self.read(|cache| {
            cache
                .instrument_ids(venue.as_ref())
                .into_iter()
                .copied()
                .collect()
        })
    }

    #[pyo3(name = "account")]
    fn py_account(&self, py: Python<'_>, account_id: AccountId) -> PyResult<Option<Py<PyAny>>> {
        self.read(|cache| cache.account_owned(&account_id))?
            .map(|account| account_any_to_pyobject(py, account))
            .transpose()
    }

    #[pyo3(name = "order")]
    fn py_order(
        &self,
        py: Python<'_>,
        client_order_id: ClientOrderId,
    ) -> PyResult<Option<Py<PyAny>>> {
        self.read(|cache| cache.order_owned(&client_order_id))?
            .map(|order| order_any_to_pyobject(py, order))
            .transpose()
    }

    #[pyo3(name = "client_order_id")]
    fn py_client_order_id(&self, venue_order_id: VenueOrderId) -> PyResult<Option<ClientOrderId>> {
        self.read(|cache| cache.client_order_id(&venue_order_id).copied())
    }

    #[pyo3(name = "venue_order_id")]
    fn py_venue_order_id(&self, client_order_id: ClientOrderId) -> PyResult<Option<VenueOrderId>> {
        self.read(|cache| cache.venue_order_id(&client_order_id).copied())
    }

    #[pyo3(name = "position_id")]
    fn py_position_id(&self, client_order_id: ClientOrderId) -> PyResult<Option<PositionId>> {
        self.read(|cache| cache.position_id(&client_order_id).copied())
    }

    #[pyo3(name = "strategy_id_for_order")]
    fn py_strategy_id_for_order(
        &self,
        client_order_id: ClientOrderId,
    ) -> PyResult<Option<StrategyId>> {
        self.read(|cache| cache.strategy_id_for_order(&client_order_id).copied())
    }

    #[pyo3(name = "order_list")]
    fn py_order_list(&self, order_list_id: OrderListId) -> PyResult<Option<OrderList>> {
        self.read(|cache| cache.order_list(&order_list_id).cloned())
    }

    #[pyo3(name = "order_book")]
    fn py_order_book(&self, instrument_id: InstrumentId) -> PyResult<Option<OrderBook>> {
        self.read(|cache| cache.order_book(&instrument_id).cloned())
    }

    #[pyo3(name = "position")]
    fn py_position(&self, position_id: PositionId) -> PyResult<Option<Position>> {
        self.read(|cache| cache.position_owned(&position_id))
    }

    #[pyo3(name = "orders", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_orders(
        &self,
        py: Python<'_>,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let orders = self.read(|cache| {
            cache
                .orders(
                    venue.as_ref(),
                    instrument_id.as_ref(),
                    strategy_id.as_ref(),
                    account_id.as_ref(),
                    side,
                )
                .into_iter()
                .map(|value| value.clone())
                .collect::<Vec<_>>()
        })?;

        orders
            .into_iter()
            .map(|order| order_any_to_pyobject(py, order))
            .collect()
    }

    #[pyo3(name = "orders_open", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_orders_open(
        &self,
        py: Python<'_>,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let orders = self.read(|cache| {
            cache
                .orders_open(
                    venue.as_ref(),
                    instrument_id.as_ref(),
                    strategy_id.as_ref(),
                    account_id.as_ref(),
                    side,
                )
                .into_iter()
                .map(|value| value.clone())
                .collect::<Vec<_>>()
        })?;

        orders
            .into_iter()
            .map(|order| order_any_to_pyobject(py, order))
            .collect()
    }

    #[pyo3(name = "orders_inflight", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_orders_inflight(
        &self,
        py: Python<'_>,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let orders = self.read(|cache| {
            cache
                .orders_inflight(
                    venue.as_ref(),
                    instrument_id.as_ref(),
                    strategy_id.as_ref(),
                    account_id.as_ref(),
                    side,
                )
                .into_iter()
                .map(|value| value.clone())
                .collect::<Vec<_>>()
        })?;

        orders
            .into_iter()
            .map(|order| order_any_to_pyobject(py, order))
            .collect()
    }

    #[pyo3(name = "orders_open_count", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_orders_open_count(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<OrderSide>,
    ) -> PyResult<usize> {
        self.read(|cache| {
            cache.orders_open_count(
                venue.as_ref(),
                instrument_id.as_ref(),
                strategy_id.as_ref(),
                account_id.as_ref(),
                side,
            )
        })
    }

    #[pyo3(name = "client_order_ids_open", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None))]
    fn py_client_order_ids_open(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
    ) -> PyResult<Vec<ClientOrderId>> {
        self.read(|cache| {
            cache
                .client_order_ids_open(
                    venue.as_ref(),
                    instrument_id.as_ref(),
                    strategy_id.as_ref(),
                    account_id.as_ref(),
                )
                .into_iter()
                .collect()
        })
    }

    #[pyo3(name = "positions_open", signature = (venue=None, instrument_id=None, strategy_id=None, account_id=None, side=None))]
    fn py_positions_open(
        &self,
        venue: Option<Venue>,
        instrument_id: Option<InstrumentId>,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        side: Option<PositionSide>,
    ) -> PyResult<Vec<Position>> {
        self.read(|cache| {
            cache
                .positions_open(
                    venue.as_ref(),
                    instrument_id.as_ref(),
                    strategy_id.as_ref(),
                    account_id.as_ref(),
                    side,
                )
                .into_iter()
                .map(|value| value.clone())
                .collect()
        })
    }
}

impl PyClientCache {
    fn read<T>(&self, read: impl FnOnce(&Cache) -> T) -> PyResult<T> {
        let view = self.view()?;
        let cache = view.try_borrow().map_err(to_pyruntime_err)?;
        Ok(read(&cache))
    }

    fn view(&self) -> PyResult<CacheView> {
        CLIENT_CACHES
            .with_borrow(|caches| caches.get(&self.id).cloned())
            .ok_or_else(|| {
                to_pyruntime_err("Client cache is disposed or accessed from a foreign thread")
            })
    }
}

/// Queues typed client output on its owner thread.
#[pyclass(
    name = "_ClientOutput",
    module = "nautilus_trader.live",
    frozen,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")]
#[derive(Debug, Clone)]
pub struct ClientOutput {
    state: Arc<Mutex<OutputState>>,
    owner: ThreadId,
}

#[derive(Debug, Default)]
struct OutputState {
    sender: Option<tokio::sync::mpsc::UnboundedSender<DataEvent>>,
    bound: bool,
    disposed: bool,
    claimed: bool,
    client_id: Option<ClientId>,
    exec_sender: Option<tokio::sync::mpsc::UnboundedSender<ExecutionEvent>>,
    event_factory: Option<OrderEventFactory>,
    venue: Option<Venue>,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ClientOutput {
    #[new]
    fn py_new() -> Self {
        Self {
            state: Arc::default(),
            owner: thread::current().id(),
        }
    }

    #[pyo3(name = "instrument")]
    fn py_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        self.sender()?;
        self.send(DataEvent::Instrument(pyobject_to_instrument_any(
            py, instrument,
        )?))
    }

    #[pyo3(name = "data")]
    fn py_data(&self, data: &Bound<'_, PyAny>) -> PyResult<()> {
        self.sender()?;
        self.send(DataEvent::Data(extract_data(data)?))
    }
    #[pyo3(name = "response")]
    fn py_response(&self, response: &Bound<'_, PyAny>) -> PyResult<()> {
        self.sender()?;
        let (client_id, response) = extract_response(response)?;
        if self.state.lock().client_id != Some(client_id) {
            return Err(to_pytype_err(
                "Response client identity does not match its owner",
            ));
        }

        self.send(DataEvent::Response(response))
    }

    #[pyo3(name = "event")]
    fn py_event(&self, py: Python<'_>, event: Py<PyAny>) -> PyResult<()> {
        self.exec_sender()?;

        let event = if event.bind(py).is_instance_of::<AccountState>() {
            ExecutionEvent::Account(event.extract(py)?)
        } else {
            ExecutionEvent::Order(pyobject_to_order_event(py, event)?)
        };

        self.send_exec(event)
    }

    #[pyo3(name = "report", signature = (report, fills=None))]
    fn py_report(&self, report: &Bound<'_, PyAny>, fills: Option<Vec<FillReport>>) -> PyResult<()> {
        self.exec_sender()?;

        let report = if report.is_instance_of::<OrderStatusReport>() {
            let order = Box::new(report.extract()?);

            match fills {
                Some(fills) => ExecutionReport::OrderWithFills(order, fills),
                None => ExecutionReport::Order(order),
            }
        } else if fills.is_some() {
            return Err(to_pytype_err(
                "Associated fills require an OrderStatusReport",
            ));
        } else if report.is_instance_of::<FillReport>() {
            ExecutionReport::Fill(Box::new(report.extract()?))
        } else if report.is_instance_of::<PositionStatusReport>() {
            ExecutionReport::Position(Box::new(report.extract()?))
        } else if report.is_instance_of::<ExecutionMassStatus>() {
            ExecutionReport::MassStatus(Box::new(report.extract()?))
        } else {
            return Err(to_pytype_err(
                "Expected a Nautilus execution report from the installed wheel",
            ));
        };

        self.send_exec(ExecutionEvent::Report(report))
    }

    #[pyo3(name = "account_state", signature = (balances, margins, reported, ts_event, ts_init, info=None))]
    #[expect(
        clippy::too_many_arguments,
        clippy::needless_pass_by_value,
        reason = "typed event fields cross the PyO3 boundary together"
    )]
    fn py_account_state(
        &self,
        py: Python<'_>,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: u64,
        ts_init: u64,
        info: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let factory = self.event_factory()?;
        let info = info
            .as_ref()
            .map(|info| pydict_to_params(py, info))
            .transpose()?
            .flatten();
        let state = factory.generate_account_state(
            balances,
            margins,
            reported,
            ts_event.into(),
            ts_init.into(),
            info,
        );
        self.send_exec(ExecutionEvent::Account(state))
    }

    #[pyo3(name = "order_submitted_batch")]
    fn py_order_submitted_batch(&self, events: Vec<OrderSubmitted>) -> PyResult<()> {
        self.send_exec(ExecutionEvent::OrderSubmittedBatch(
            OrderSubmittedBatch::new(events),
        ))
    }

    #[pyo3(name = "order_accepted_batch")]
    fn py_order_accepted_batch(&self, events: Vec<OrderAccepted>) -> PyResult<()> {
        self.send_exec(ExecutionEvent::OrderAcceptedBatch(OrderAcceptedBatch::new(
            events,
        )))
    }

    #[pyo3(name = "order_canceled_batch")]
    fn py_order_canceled_batch(&self, events: Vec<OrderCanceled>) -> PyResult<()> {
        self.send_exec(ExecutionEvent::OrderCanceledBatch(OrderCanceledBatch::new(
            events,
        )))
    }

    #[pyo3(name = "order_denied", signature = (order, reason, ts_init))]
    fn py_order_denied(
        &self,
        py: Python<'_>,
        order: Py<PyAny>,
        reason: &str,
        ts_init: u64,
    ) -> PyResult<()> {
        let factory = self.event_factory()?;
        let order = pyobject_to_order_any(py, order)?;
        let event = factory.generate_order_denied(&order, reason, ts_init.into());
        self.send_exec(ExecutionEvent::Order(event))
    }

    #[pyo3(name = "order_submitted", signature = (order, ts_init))]
    fn py_order_submitted(&self, py: Python<'_>, order: Py<PyAny>, ts_init: u64) -> PyResult<()> {
        let factory = self.event_factory()?;
        let order = pyobject_to_order_any(py, order)?;
        let event = factory.generate_order_submitted(&order, ts_init.into());
        self.send_exec(ExecutionEvent::Order(event))
    }

    #[pyo3(name = "order_rejected", signature = (order, reason, ts_event, ts_init, due_post_only))]
    fn py_order_rejected(
        &self,
        py: Python<'_>,
        order: Py<PyAny>,
        reason: &str,
        ts_event: u64,
        ts_init: u64,
        due_post_only: bool,
    ) -> PyResult<()> {
        let factory = self.event_factory()?;
        let order = pyobject_to_order_any(py, order)?;
        let event = factory.generate_order_rejected(
            &order,
            reason,
            ts_event.into(),
            ts_init.into(),
            due_post_only,
        );
        self.send_exec(ExecutionEvent::Order(event))
    }

    #[pyo3(name = "order_accepted", signature = (order, venue_order_id, ts_event, ts_init))]
    fn py_order_accepted(
        &self,
        py: Python<'_>,
        order: Py<PyAny>,
        venue_order_id: VenueOrderId,
        ts_event: u64,
        ts_init: u64,
    ) -> PyResult<()> {
        let factory = self.event_factory()?;
        let order = pyobject_to_order_any(py, order)?;
        let event = factory.generate_order_accepted(
            &order,
            venue_order_id,
            ts_event.into(),
            ts_init.into(),
        );
        self.send_exec(ExecutionEvent::Order(event))
    }

    #[pyo3(name = "order_modify_rejected", signature = (order, venue_order_id, reason, ts_event, ts_init))]
    fn py_order_modify_rejected(
        &self,
        py: Python<'_>,
        order: Py<PyAny>,
        venue_order_id: Option<VenueOrderId>,
        reason: &str,
        ts_event: u64,
        ts_init: u64,
    ) -> PyResult<()> {
        let factory = self.event_factory()?;
        let order = pyobject_to_order_any(py, order)?;
        let event = factory.generate_order_modify_rejected(
            &order,
            venue_order_id,
            reason,
            ts_event.into(),
            ts_init.into(),
        );
        self.send_exec(ExecutionEvent::Order(event))
    }

    #[pyo3(name = "order_cancel_rejected", signature = (order, venue_order_id, reason, ts_event, ts_init))]
    fn py_order_cancel_rejected(
        &self,
        py: Python<'_>,
        order: Py<PyAny>,
        venue_order_id: Option<VenueOrderId>,
        reason: &str,
        ts_event: u64,
        ts_init: u64,
    ) -> PyResult<()> {
        let factory = self.event_factory()?;
        let order = pyobject_to_order_any(py, order)?;
        let event = factory.generate_order_cancel_rejected(
            &order,
            venue_order_id,
            reason,
            ts_event.into(),
            ts_init.into(),
        );
        self.send_exec(ExecutionEvent::Order(event))
    }

    #[pyo3(name = "order_updated", signature = (order, venue_order_id, quantity, price, trigger_price, protection_price, ts_event, ts_init))]
    #[expect(
        clippy::too_many_arguments,
        reason = "typed event fields cross the PyO3 boundary together"
    )]
    fn py_order_updated(
        &self,
        py: Python<'_>,
        order: Py<PyAny>,
        venue_order_id: VenueOrderId,
        quantity: Quantity,
        price: Option<Price>,
        trigger_price: Option<Price>,
        protection_price: Option<Price>,
        ts_event: u64,
        ts_init: u64,
    ) -> PyResult<()> {
        let factory = self.event_factory()?;
        let order = pyobject_to_order_any(py, order)?;
        let event = factory.generate_order_updated(
            &order,
            venue_order_id,
            quantity,
            price,
            trigger_price,
            protection_price,
            ts_event.into(),
            ts_init.into(),
        );
        self.send_exec(ExecutionEvent::Order(event))
    }

    #[pyo3(name = "order_canceled", signature = (order, venue_order_id, ts_event, ts_init))]
    fn py_order_canceled(
        &self,
        py: Python<'_>,
        order: Py<PyAny>,
        venue_order_id: Option<VenueOrderId>,
        ts_event: u64,
        ts_init: u64,
    ) -> PyResult<()> {
        let factory = self.event_factory()?;
        let order = pyobject_to_order_any(py, order)?;
        let event = factory.generate_order_canceled(
            &order,
            venue_order_id,
            ts_event.into(),
            ts_init.into(),
        );
        self.send_exec(ExecutionEvent::Order(event))
    }

    #[pyo3(name = "order_triggered", signature = (order, venue_order_id, ts_event, ts_init))]
    fn py_order_triggered(
        &self,
        py: Python<'_>,
        order: Py<PyAny>,
        venue_order_id: Option<VenueOrderId>,
        ts_event: u64,
        ts_init: u64,
    ) -> PyResult<()> {
        let factory = self.event_factory()?;
        let order = pyobject_to_order_any(py, order)?;
        let event = factory.generate_order_triggered(
            &order,
            venue_order_id,
            ts_event.into(),
            ts_init.into(),
        );
        self.send_exec(ExecutionEvent::Order(event))
    }

    #[pyo3(name = "order_expired", signature = (order, venue_order_id, ts_event, ts_init))]
    fn py_order_expired(
        &self,
        py: Python<'_>,
        order: Py<PyAny>,
        venue_order_id: Option<VenueOrderId>,
        ts_event: u64,
        ts_init: u64,
    ) -> PyResult<()> {
        let factory = self.event_factory()?;
        let order = pyobject_to_order_any(py, order)?;
        let event =
            factory.generate_order_expired(&order, venue_order_id, ts_event.into(), ts_init.into());
        self.send_exec(ExecutionEvent::Order(event))
    }

    #[pyo3(name = "order_filled", signature = (order, venue_order_id, venue_position_id, trade_id, last_qty, last_px, quote_currency, commission, liquidity_side, ts_event, ts_init))]
    #[expect(
        clippy::too_many_arguments,
        reason = "typed event fields cross the PyO3 boundary together"
    )]
    fn py_order_filled(
        &self,
        py: Python<'_>,
        order: Py<PyAny>,
        venue_order_id: VenueOrderId,
        venue_position_id: Option<PositionId>,
        trade_id: TradeId,
        last_qty: Quantity,
        last_px: Price,
        quote_currency: Currency,
        commission: Option<Money>,
        liquidity_side: LiquiditySide,
        ts_event: u64,
        ts_init: u64,
    ) -> PyResult<()> {
        let factory = self.event_factory()?;
        let order = pyobject_to_order_any(py, order)?;
        let event = factory.generate_order_filled(
            &order,
            venue_order_id,
            venue_position_id,
            trade_id,
            last_qty,
            last_px,
            quote_currency,
            commission,
            liquidity_side,
            ts_event.into(),
            ts_init.into(),
        );
        self.send_exec(ExecutionEvent::Order(event))
    }
}

impl ClientOutput {
    fn bind(&self) -> PyResult<()> {
        if thread::current().id() != self.owner {
            return Err(to_pyruntime_err("Client output requires its owner thread"));
        }

        let mut state = self.state.lock();
        if state.bound || state.disposed {
            return Err(to_pyruntime_err("Client output cannot be rebound"));
        }

        state.sender = Some(get_data_event_sender());
        if state.event_factory.is_some() {
            state.exec_sender = Some(get_exec_event_sender());
        }

        state.bound = true;
        Ok(())
    }

    fn send(&self, event: DataEvent) -> PyResult<()> {
        self.sender()?.send(event).map_err(to_pyruntime_err)
    }

    fn sender(&self) -> PyResult<tokio::sync::mpsc::UnboundedSender<DataEvent>> {
        if thread::current().id() != self.owner {
            return Err(to_pyruntime_err("Client output requires its owner thread"));
        }

        self.state
            .lock()
            .sender
            .clone()
            .ok_or_else(|| to_pyruntime_err("Client output is not bound to an active node"))
    }

    fn event_factory(&self) -> PyResult<OrderEventFactory> {
        self.exec_sender()?;
        self.state
            .lock()
            .event_factory
            .clone()
            .ok_or_else(|| to_pyruntime_err("Client has no execution identity"))
    }

    fn exec_sender(&self) -> PyResult<tokio::sync::mpsc::UnboundedSender<ExecutionEvent>> {
        if thread::current().id() != self.owner {
            return Err(to_pyruntime_err("Client output requires its owner thread"));
        }

        self.state
            .lock()
            .exec_sender
            .clone()
            .ok_or_else(|| to_pyruntime_err("Execution output is not bound to an active node"))
    }

    fn send_exec(&self, event: ExecutionEvent) -> PyResult<()> {
        let factory = self.event_factory()?;
        let account_id = factory.account_id();
        let trader_id = factory.trader_id();

        let valid = match &event {
            ExecutionEvent::Account(event) => event.account_id == account_id,
            ExecutionEvent::Order(event) => {
                event.trader_id() == trader_id
                    && event.account_id().is_none_or(|id| id == account_id)
            }
            ExecutionEvent::OrderSubmittedBatch(batch) => batch
                .events
                .iter()
                .all(|event| event.trader_id == trader_id && event.account_id == account_id),
            ExecutionEvent::OrderAcceptedBatch(batch) => batch
                .events
                .iter()
                .all(|event| event.trader_id == trader_id && event.account_id == account_id),
            ExecutionEvent::OrderCanceledBatch(batch) => batch.events.iter().all(|event| {
                event.trader_id == trader_id && event.account_id.is_none_or(|id| id == account_id)
            }),
            ExecutionEvent::Report(report) => match report {
                ExecutionReport::Order(report) => report.account_id == account_id,
                ExecutionReport::Fill(report) => report.account_id == account_id,
                ExecutionReport::Position(report) => report.account_id == account_id,
                ExecutionReport::OrderWithFills(report, fills) => {
                    report.account_id == account_id
                        && fills.iter().all(|fill| fill.account_id == account_id)
                }
                ExecutionReport::MassStatus(report) => {
                    let state = self.state.lock();
                    mass_status_matches(report, state.client_id, account_id, state.venue)
                }
            },
        };

        if !valid {
            return Err(to_pytype_err(
                "Execution output identity does not match its owner",
            ));
        }

        self.exec_sender()?.send(event).map_err(to_pyruntime_err)
    }

    fn invalidate(&self) {
        let mut state = self.state.lock();
        state.sender = None;
        state.exec_sender = None;
        state.disposed = true;
    }
}

#[derive(Debug, Default, Clone)]
pub(crate) struct PythonClients(Rc<RefCell<Vec<ClientOwner>>>);

#[derive(Debug)]
struct ClientOwner {
    client: Py<PyAny>,
    runtime: Py<PyAny>,
    output: ClientOutput,
    cache_id: u64,
}

impl Drop for ClientOwner {
    fn drop(&mut self) {
        self.output.invalidate();
        CLIENT_CACHES.with_borrow_mut(|caches| caches.remove(&self.cache_id));
        Python::attach(|py| {
            if let Err(e) = self.runtime.call_method0(py, "dispose") {
                log::error!("Failed to dispose Python client: {e}");
            }
        });
    }
}

impl PythonClients {
    pub(crate) fn take(&self) -> Self {
        Self(Rc::new(RefCell::new(self.0.take())))
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }

    pub(crate) fn bind(&self, py: Python<'_>, event_loop: &Bound<'_, PyAny>) -> PyResult<()> {
        let runtimes: Vec<_> = self
            .0
            .borrow()
            .iter()
            .map(|owner| owner.runtime.clone_ref(py))
            .collect();

        for runtime in runtimes {
            runtime.call_method1(py, "bind", (event_loop,))?;
        }

        Ok(())
    }

    pub(crate) fn finish(&self, py: Python<'_>) -> PyResult<()> {
        let owners = self.0.take();

        let mut incomplete = Vec::new();
        let mut retained = Vec::new();
        let mut failure = None;

        for owner in owners {
            owner.output.invalidate();
            CLIENT_CACHES.with_borrow_mut(|caches| caches.remove(&owner.cache_id));

            let result = (|| {
                owner.runtime.call_method0(py, "dispose")?;
                if owner.runtime.getattr(py, "complete")?.extract::<bool>(py)? {
                    return Ok(true);
                }

                incomplete.push(
                    owner
                        .client
                        .getattr(py, "client_id")?
                        .bind(py)
                        .str()?
                        .to_string(),
                );
                Ok::<bool, PyErr>(false)
            })();

            match result {
                Ok(true) => {}
                Ok(false) => retained.push(owner),
                Err(e) => {
                    failure.get_or_insert(e);
                    retained.push(owner);
                }
            }
        }

        self.0.borrow_mut().extend(retained);

        if let Some(e) = failure {
            return Err(e);
        }

        if incomplete.is_empty() {
            Ok(())
        } else {
            Err(to_pyruntime_err(format!(
                "Python client cleanup is incomplete: {}",
                incomplete.join(", ")
            )))
        }
    }
}

#[derive(Debug)]
pub(crate) struct PythonClientConfig(pub(crate) Py<PyAny>);

impl ClientConfig for PythonClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
pub(crate) struct PythonDataFactory {
    pub(crate) factory: Py<PyAny>,
    pub(crate) clients: PythonClients,
}

impl DataClientFactory for PythonDataFactory {
    fn name(&self) -> &'static str {
        "Python"
    }
    fn config_type(&self) -> &'static str {
        "DataClientConfig"
    }

    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        Ok(Box::new(self.clients.create(
            &self.factory,
            name,
            config,
            cache,
            clock,
            None,
        )?))
    }
}

#[derive(Debug)]
pub(crate) struct PythonExecutionFactory {
    pub(crate) factory: Py<PyAny>,
    pub(crate) clients: PythonClients,
}

impl ExecutionClientFactory for PythonExecutionFactory {
    fn name(&self) -> &'static str {
        "Python"
    }
    fn config_type(&self) -> &'static str {
        "ExecutionClientConfig"
    }
    fn create(
        &self,
        trader_id: TraderId,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let mut client = self.clients.create(
            &self.factory,
            name,
            config,
            cache.clone(),
            clock,
            Some(trader_id),
        )?;
        let identity = client
            .execution
            .take()
            .ok_or_else(|| anyhow::anyhow!("Missing execution client identity"))?;
        Ok(Box::new(PythonExecutionClient {
            client,
            identity,
            cache,
        }))
    }
}

impl PythonClients {
    fn create(
        &self,
        factory: &Py<PyAny>,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
        clock: Rc<RefCell<dyn Clock>>,
        trader_id: Option<TraderId>,
    ) -> anyhow::Result<PythonClient> {
        let config = config
            .as_any()
            .downcast_ref::<PythonClientConfig>()
            .ok_or_else(|| anyhow::anyhow!("Expected Python client configuration"))?;
        Python::attach(|py| {
            let cache_id = CACHE_ID.fetch_add(1, Ordering::Relaxed);
            CLIENT_CACHES.with_borrow_mut(|caches| caches.insert(cache_id, cache));

            let result = (|| -> PyResult<PythonClient> {
                let kwargs = PyDict::new(py);
                kwargs.set_item("name", name)?;
                kwargs.set_item("config", &config.0)?;
                kwargs.set_item("cache", PyClientCache { id: cache_id })?;
                kwargs.set_item("clock", PyClock::from_rc(clock.clone()))?;
                if let Some(trader_id) = trader_id {
                    kwargs.set_item("trader_id", trader_id)?;
                }

                let client = py
                    .import("nautilus_trader.live.clients")?
                    .getattr("_create_client")?
                    .call1((factory, kwargs))?
                    .unbind();

                let base_name = if trader_id.is_some() {
                    "ExecutionClient"
                } else {
                    "DataClient"
                };

                let base = py
                    .import("nautilus_trader.live.clients")?
                    .getattr(base_name)?;
                if !client.bind(py).is_instance(&base)? {
                    return Err(to_pytype_err(format!("Factory must return a {base_name}")));
                }

                let client_id = client.getattr(py, "client_id")?.extract::<ClientId>(py)?;
                if client_id.as_str() != name {
                    return Err(to_pytype_err(
                        "Factory client identity does not match registration name",
                    ));
                }

                if !client.getattr(py, "config")?.is(&config.0) {
                    return Err(to_pytype_err(
                        "Client must retain the original factory config",
                    ));
                }

                let venue = client.getattr(py, "venue")?.extract::<Option<Venue>>(py)?;
                let output = client.getattr(py, "_output")?.extract::<ClientOutput>(py)?;
                if client
                    .getattr(py, "cache")?
                    .extract::<PyRef<'_, PyClientCache>>(py)?
                    .id
                    != cache_id
                {
                    return Err(to_pytype_err(
                        "Client must retain the owning factory cache view",
                    ));
                }

                let runtime = client.getattr(py, "_runtime")?;

                let execution = trader_id
                    .map(|trader_id| -> PyResult<ExecutionIdentity> {
                        if client.getattr(py, "trader_id")?.extract::<TraderId>(py)? != trader_id {
                            return Err(to_pytype_err(
                                "Execution client trader identity does not match its node",
                            ));
                        }

                        let tolerance = client
                            .getattr(py, "position_reconciliation_tolerance")?
                            .extract::<Option<Decimal>>(py)?
                            .unwrap_or(DEFAULT_POSITION_RECONCILIATION_TOLERANCE);

                        if tolerance.is_sign_negative() {
                            return Err(to_pytype_err(
                                "Position reconciliation tolerance must be nonnegative",
                            ));
                        }

                        Ok(ExecutionIdentity {
                            trader_id,
                            tolerance,
                            account_id: client.getattr(py, "account_id")?.extract(py)?,
                            account_type: client.getattr(py, "account_type")?.extract(py)?,
                            base_currency: client.getattr(py, "base_currency")?.extract(py)?,
                            oms_type: client.getattr(py, "oms_type")?.extract(py)?,
                            venue: venue.ok_or_else(|| {
                                to_pytype_err("Execution clients require a venue")
                            })?,
                        })
                    })
                    .transpose()?;

                {
                    let mut state = output.state.lock();
                    if state.claimed || state.disposed {
                        return Err(to_pytype_err(
                            "A client instance cannot be registered twice",
                        ));
                    }

                    state.claimed = true;
                    state.client_id = Some(client_id);
                    state.venue = venue;
                    state.event_factory = execution.as_ref().map(|identity| {
                        OrderEventFactory::new(
                            identity.trader_id,
                            identity.account_id,
                            identity.account_type,
                            identity.base_currency,
                        )
                    });
                }

                self.0.borrow_mut().push(ClientOwner {
                    client: client.clone_ref(py),
                    runtime: runtime.clone_ref(py),
                    output: output.clone(),
                    cache_id,
                });

                Ok(PythonClient {
                    client_id,
                    venue,
                    runtime,
                    output,
                    instance: client,
                    execution,
                    clock,
                })
            })();

            if result.is_err() {
                CLIENT_CACHES.with_borrow_mut(|caches| caches.remove(&cache_id));
            }

            result.map_err(Into::into)
        })
    }
}

struct ExecutionIdentity {
    tolerance: Decimal,
    trader_id: TraderId,
    account_id: AccountId,
    account_type: AccountType,
    base_currency: Option<Currency>,
    oms_type: OmsType,
    venue: Venue,
}

struct PythonClient {
    client_id: ClientId,
    venue: Option<Venue>,
    runtime: Py<PyAny>,
    output: ClientOutput,
    instance: Py<PyAny>,
    execution: Option<ExecutionIdentity>,
    clock: Rc<RefCell<dyn Clock>>,
}

#[async_trait(?Send)]
impl DataClient for PythonClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }
    fn venue(&self) -> Option<Venue> {
        self.venue
    }
    fn start(&mut self) -> anyhow::Result<()> {
        self.output.bind().map_err(Into::into)
    }
    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    fn reset(&mut self) -> anyhow::Result<()> {
        anyhow::bail!("Python clients are single-use")
    }
    fn dispose(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    fn is_connected(&self) -> bool {
        Python::attach(|py| {
            self.runtime
                .getattr(py, "connected")
                .and_then(|v| v.extract(py))
                .unwrap_or(false)
        })
    }
    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }
    async fn connect(&mut self) -> anyhow::Result<()> {
        self.lifecycle("connect").await
    }
    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.lifecycle("disconnect").await
    }
    fn subscribe(&mut self, command: SubscribeCustomData) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_subscribe", (command,)))
        })?;

        Ok(())
    }
    fn subscribe_instruments(&mut self, command: SubscribeInstruments) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_subscribe_instruments", (command,)))
        })?;

        Ok(())
    }
    fn subscribe_instrument(&mut self, command: SubscribeInstrument) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_subscribe_instrument", (command,)))
        })?;

        Ok(())
    }
    fn subscribe_book_deltas(&mut self, command: SubscribeBookDeltas) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_subscribe_book_deltas", (command,)))
        })?;

        Ok(())
    }
    fn subscribe_book_depth10(&mut self, command: SubscribeBookDepth10) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_subscribe_book_depth10", (command,)))
        })?;

        Ok(())
    }
    fn subscribe_quotes(&mut self, command: SubscribeQuotes) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_subscribe_quotes", (command,)))
        })?;

        Ok(())
    }
    fn subscribe_trades(&mut self, command: SubscribeTrades) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_subscribe_trades", (command,)))
        })?;

        Ok(())
    }
    fn subscribe_mark_prices(&mut self, command: SubscribeMarkPrices) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_subscribe_mark_prices", (command,)))
        })?;

        Ok(())
    }
    fn subscribe_index_prices(&mut self, command: SubscribeIndexPrices) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_subscribe_index_prices", (command,)))
        })?;

        Ok(())
    }
    fn subscribe_funding_rates(&mut self, command: SubscribeFundingRates) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_subscribe_funding_rates", (command,)))
        })?;

        Ok(())
    }
    fn subscribe_bars(&mut self, command: SubscribeBars) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_subscribe_bars", (command,)))
        })?;

        Ok(())
    }
    fn subscribe_instrument_status(
        &mut self,
        command: SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_subscribe_instrument_status", (command,)))
        })?;

        Ok(())
    }
    fn subscribe_instrument_close(
        &mut self,
        command: SubscribeInstrumentClose,
    ) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_subscribe_instrument_close", (command,)))
        })?;

        Ok(())
    }
    fn subscribe_option_greeks(&mut self, command: SubscribeOptionGreeks) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_subscribe_option_greeks", (command,)))
        })?;

        Ok(())
    }
    fn unsubscribe(&mut self, command: &UnsubscribeCustomData) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_unsubscribe", (command.clone(),)))
        })?;

        Ok(())
    }
    fn unsubscribe_instruments(&mut self, command: &UnsubscribeInstruments) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "admit",
                ("_unsubscribe_instruments", (command.clone(),)),
            )
        })?;

        Ok(())
    }
    fn unsubscribe_instrument(&mut self, command: &UnsubscribeInstrument) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_unsubscribe_instrument", (command.clone(),)))
        })?;

        Ok(())
    }
    fn unsubscribe_book_deltas(&mut self, command: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "admit",
                ("_unsubscribe_book_deltas", (command.clone(),)),
            )
        })?;

        Ok(())
    }
    fn unsubscribe_book_depth10(&mut self, command: &UnsubscribeBookDepth10) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "admit",
                ("_unsubscribe_book_depth10", (command.clone(),)),
            )
        })?;

        Ok(())
    }
    fn unsubscribe_quotes(&mut self, command: &UnsubscribeQuotes) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_unsubscribe_quotes", (command.clone(),)))
        })?;

        Ok(())
    }
    fn unsubscribe_trades(&mut self, command: &UnsubscribeTrades) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_unsubscribe_trades", (command.clone(),)))
        })?;

        Ok(())
    }
    fn unsubscribe_mark_prices(&mut self, command: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "admit",
                ("_unsubscribe_mark_prices", (command.clone(),)),
            )
        })?;

        Ok(())
    }
    fn unsubscribe_index_prices(&mut self, command: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "admit",
                ("_unsubscribe_index_prices", (command.clone(),)),
            )
        })?;

        Ok(())
    }
    fn unsubscribe_funding_rates(
        &mut self,
        command: &UnsubscribeFundingRates,
    ) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "admit",
                ("_unsubscribe_funding_rates", (command.clone(),)),
            )
        })?;

        Ok(())
    }
    fn unsubscribe_bars(&mut self, command: &UnsubscribeBars) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "admit", ("_unsubscribe_bars", (command.clone(),)))
        })?;

        Ok(())
    }
    fn unsubscribe_instrument_status(
        &mut self,
        command: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "admit",
                ("_unsubscribe_instrument_status", (command.clone(),)),
            )
        })?;

        Ok(())
    }
    fn unsubscribe_instrument_close(
        &mut self,
        command: &UnsubscribeInstrumentClose,
    ) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "admit",
                ("_unsubscribe_instrument_close", (command.clone(),)),
            )
        })?;

        Ok(())
    }
    fn unsubscribe_option_greeks(
        &mut self,
        command: &UnsubscribeOptionGreeks,
    ) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "admit",
                ("_unsubscribe_option_greeks", (command.clone(),)),
            )
        })?;

        Ok(())
    }

    fn request_data(&self, request: RequestCustomData) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "call",
                ("_request_data", (PyRequestCustomData { request },)),
            )
        })?;

        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "call",
                ("_request_instruments", (PyRequestInstruments { request },)),
            )
        })?;

        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "call",
                ("_request_instrument", (PyRequestInstrument { request },)),
            )
        })?;

        Ok(())
    }

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "call",
                (
                    "_request_book_snapshot",
                    (PyRequestBookSnapshot { request },),
                ),
            )
        })?;

        Ok(())
    }

    fn request_quotes(&self, request: RequestQuotes) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "call",
                ("_request_quotes", (PyRequestQuotes { request },)),
            )
        })?;

        Ok(())
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "call",
                ("_request_trades", (PyRequestTrades { request },)),
            )
        })?;

        Ok(())
    }

    fn request_funding_rates(&self, request: RequestFundingRates) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "call",
                (
                    "_request_funding_rates",
                    (PyRequestFundingRates { request },),
                ),
            )
        })?;

        Ok(())
    }

    fn request_option_chain_reference_price(
        &self,
        request: RequestOptionChainReferencePrice,
    ) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "call",
                (
                    "_request_option_chain_reference_price",
                    (PyRequestOptionChainReferencePrice { request },),
                ),
            )
        })?;

        Ok(())
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime
                .call_method1(py, "call", ("_request_bars", (PyRequestBars { request },)))
        })?;

        Ok(())
    }

    fn request_book_depth(&self, request: RequestBookDepth) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "call",
                ("_request_book_depth", (PyRequestBookDepth { request },)),
            )
        })?;

        Ok(())
    }

    fn request_book_deltas(&self, request: RequestBookDeltas) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.runtime.call_method1(
                py,
                "call",
                ("_request_book_deltas", (PyRequestBookDeltas { request },)),
            )
        })?;

        Ok(())
    }
}

impl PythonClient {
    async fn lifecycle(&self, operation: &str) -> anyhow::Result<()> {
        let future = Python::attach(|py| {
            let task = self.runtime.call_method1(py, "lifecycle", (operation,))?;

            PythonOperation::new(py, task, self.runtime.clone_ref(py))
        })?;

        future.await.map(|_| ()).map_err(Into::into)
    }
}

fn extract_data(data: &Bound<'_, PyAny>) -> PyResult<Data> {
    macro_rules! extract {
        ($($ty:ty),+ $(,)?) => {
            $(if data.is_instance_of::<$ty>() {
                return Ok(data.extract::<$ty>()?.into());
            })+
        };
    }
    extract!(
        QuoteTick,
        TradeTick,
        Bar,
        OrderBookDelta,
        OrderBookDeltas,
        OrderBookDepth10,
        MarkPriceUpdate,
        IndexPriceUpdate,
        FundingRateUpdate,
        OptionGreeks,
        InstrumentStatus,
        InstrumentClose
    );

    if data.is_instance_of::<CustomData>() {
        return Ok(Data::Custom(data.extract()?));
    }

    Err(to_pytype_err(
        "Expected a Nautilus data object from the installed wheel",
    ))
}

struct PythonExecutionClient {
    client: PythonClient,
    identity: ExecutionIdentity,
    cache: CacheView,
}

#[async_trait(?Send)]
impl ExecutionClient for PythonExecutionClient {
    fn client_id(&self) -> ClientId {
        self.client.client_id
    }
    fn account_id(&self) -> AccountId {
        self.identity.account_id
    }
    fn venue(&self) -> Venue {
        self.identity.venue
    }
    fn oms_type(&self) -> OmsType {
        self.identity.oms_type
    }
    fn position_reconciliation_tolerance(&self) -> Decimal {
        self.identity.tolerance
    }
    fn handles_order_venue(&self, venue: Venue) -> bool {
        Python::attach(|py| {
            self.client
                .instance
                .call_method1(py, "_handles_order_venue", (venue,))?
                .extract::<bool>(py)
        })
        .unwrap_or_else(|e| {
            log::error!(
                "Client {} venue coverage failed: {e}",
                self.client.client_id
            );
            false
        })
    }
    fn provides_bulk_position_coverage(&self, instrument_id: InstrumentId) -> bool {
        Python::attach(|py| {
            self.client
                .instance
                .call_method1(py, "_provides_bulk_position_coverage", (instrument_id,))?
                .extract::<bool>(py)
        })
        .unwrap_or_else(|e| {
            log::error!(
                "Client {} bulk position coverage failed: {e}",
                self.client.client_id
            );
            false
        })
    }
    fn calculate_commission(
        &self,
        instrument: &InstrumentAny,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
    ) -> anyhow::Result<Option<Money>> {
        Python::attach(|py| -> PyResult<Option<Money>> {
            let instrument = instrument_any_to_pyobject(py, instrument.clone())?;
            Ok(self
                .client
                .instance
                .call_method1(
                    py,
                    "_calculate_commission",
                    (instrument, last_qty, last_px, liquidity_side),
                )?
                .extract(py)?)
        })
        .map_err(Into::into)
    }
    fn register_external_order(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        ts_init: UnixNanos,
    ) {
        if let Err(e) = Python::attach(|py| {
            self.client.runtime.call_method1(
                py,
                "admit",
                (
                    "_register_external_order",
                    (
                        client_order_id,
                        venue_order_id,
                        instrument_id,
                        strategy_id,
                        ts_init.as_u64(),
                    ),
                ),
            )
        }) {
            log::error!(
                "Client {} external order registration failed: {e}",
                self.client.client_id
            );
        }
    }
    fn on_instrument(&mut self, instrument: InstrumentAny) {
        if let Err(e) = Python::attach(|py| -> PyResult<()> {
            let instrument = instrument_any_to_pyobject(py, instrument)?;
            self.client
                .runtime
                .call_method1(py, "admit", ("_on_instrument", (instrument,)))?;
            Ok(())
        }) {
            log::error!(
                "Client {} instrument update failed: {e}",
                self.client.client_id
            );
        }
    }
    fn get_account(&self) -> Option<AccountAny> {
        match self.cache.try_borrow() {
            Ok(cache) => cache.account_owned(&self.identity.account_id),
            Err(e) => {
                log::error!("Cannot read Python execution client account: {e}");
                None
            }
        }
    }
    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
        info: Option<Params>,
    ) -> anyhow::Result<()> {
        let ts_init = self.client.clock.borrow().timestamp_ns();
        let factory = self.client.output.event_factory()?;
        let event =
            factory.generate_account_state(balances, margins, reported, ts_event, ts_init, info);
        self.client
            .output
            .send_exec(ExecutionEvent::Account(event))
            .map_err(Into::into)
    }
    fn start(&mut self) -> anyhow::Result<()> {
        self.client.output.bind().map_err(Into::into)
    }
    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    fn reset(&mut self) -> anyhow::Result<()> {
        anyhow::bail!("Python clients are single-use")
    }
    fn dispose(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    fn is_connected(&self) -> bool {
        DataClient::is_connected(&self.client)
    }
    async fn connect(&mut self) -> anyhow::Result<()> {
        self.client.lifecycle("connect").await
    }
    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.client.lifecycle("disconnect").await
    }
    fn submit_order(&self, command: SubmitOrder) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.client
                .runtime
                .call_method1(py, "admit", ("_submit_order", (command,)))
        })?;

        Ok(())
    }
    fn submit_order_list(&self, command: SubmitOrderList) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.client
                .runtime
                .call_method1(py, "admit", ("_submit_order_list", (command,)))
        })?;

        Ok(())
    }
    fn modify_order(&self, command: ModifyOrder) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.client
                .runtime
                .call_method1(py, "admit", ("_modify_order", (command,)))
        })?;

        Ok(())
    }
    fn batch_modify_orders(&self, command: BatchModifyOrders) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.client
                .runtime
                .call_method1(py, "admit", ("_batch_modify_orders", (command,)))
        })?;

        Ok(())
    }
    fn cancel_order(&self, command: CancelOrder) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.client
                .runtime
                .call_method1(py, "admit", ("_cancel_order", (command,)))
        })?;

        Ok(())
    }
    fn cancel_all_orders(&self, command: CancelAllOrders) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.client
                .runtime
                .call_method1(py, "admit", ("_cancel_all_orders", (command,)))
        })?;

        Ok(())
    }
    fn batch_cancel_orders(&self, command: BatchCancelOrders) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.client
                .runtime
                .call_method1(py, "admit", ("_batch_cancel_orders", (command,)))
        })?;

        Ok(())
    }
    fn query_account(&self, command: QueryAccount) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.client
                .runtime
                .call_method1(py, "admit", ("_query_account", (command,)))
        })?;

        Ok(())
    }
    fn query_order(&self, command: QueryOrder) -> anyhow::Result<()> {
        Python::attach(|py| {
            self.client
                .runtime
                .call_method1(py, "admit", ("_query_order", (command,)))
        })?;

        Ok(())
    }
    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        let operation = Python::attach(|py| {
            let task = self.client.runtime.call_method1(
                py,
                "call_awaited",
                ("_generate_mass_status", (lookback_mins,)),
            )?;

            PythonOperation::new(py, task, self.client.runtime.clone_ref(py))
        })?;

        let result = operation.await?;
        let default = Python::attach(|py| result.is(py.NotImplemented()));
        if default {
            let ts_init = self.client.clock.borrow().timestamp_ns();
            generate_mass_status(self, lookback_mins, ts_init).await
        } else {
            let report = Python::attach(|py| -> PyResult<Option<ExecutionMassStatus>> {
                Ok(result.extract(py)?)
            })?;

            if report.as_ref().is_some_and(|report| {
                !mass_status_matches(
                    report,
                    Some(self.client.client_id),
                    self.identity.account_id,
                    Some(self.identity.venue),
                )
            }) {
                anyhow::bail!("Mass status identity does not match its client");
            }

            Ok(report)
        }
    }
    async fn generate_order_status_report(
        &self,
        command: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let operation = Python::attach(|py| {
            let task = self.client.runtime.call_method1(
                py,
                "call_awaited",
                ("_generate_order_status_report", (command.clone(),)),
            )?;

            PythonOperation::new(py, task, self.client.runtime.clone_ref(py))
        })?;

        let result = operation.await?;

        let report = Python::attach(|py| -> PyResult<Option<OrderStatusReport>> {
            Ok(result.extract(py)?)
        })?;

        if report
            .as_ref()
            .is_some_and(|report| report.account_id != self.identity.account_id)
        {
            anyhow::bail!("Report account identity does not match its client");
        }

        Ok(report)
    }
    async fn generate_order_status_reports(
        &self,
        command: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let operation = Python::attach(|py| {
            let task = self.client.runtime.call_method1(
                py,
                "call_awaited",
                ("_generate_order_status_reports", (command.clone(),)),
            )?;

            PythonOperation::new(py, task, self.client.runtime.clone_ref(py))
        })?;

        let result = operation.await?;
        let reports = Python::attach(|py| result.extract::<Vec<OrderStatusReport>>(py))?;
        if reports
            .iter()
            .any(|report| report.account_id != self.identity.account_id)
        {
            anyhow::bail!("Report account identity does not match its client");
        }

        Ok(reports)
    }
    async fn generate_fill_reports(
        &self,
        command: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let operation = Python::attach(|py| {
            let task = self.client.runtime.call_method1(
                py,
                "call_awaited",
                ("_generate_fill_reports", (command,)),
            )?;

            PythonOperation::new(py, task, self.client.runtime.clone_ref(py))
        })?;

        let result = operation.await?;
        let reports = Python::attach(|py| result.extract::<Vec<FillReport>>(py))?;
        if reports
            .iter()
            .any(|report| report.account_id != self.identity.account_id)
        {
            anyhow::bail!("Report account identity does not match its client");
        }

        Ok(reports)
    }
    async fn generate_position_status_reports(
        &self,
        command: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let operation = Python::attach(|py| {
            let task = self.client.runtime.call_method1(
                py,
                "call_awaited",
                ("_generate_position_status_reports", (command.clone(),)),
            )?;

            PythonOperation::new(py, task, self.client.runtime.clone_ref(py))
        })?;

        let result = operation.await?;
        let reports = Python::attach(|py| result.extract::<Vec<PositionStatusReport>>(py))?;
        if reports
            .iter()
            .any(|report| report.account_id != self.identity.account_id)
        {
            anyhow::bail!("Report account identity does not match its client");
        }

        Ok(reports)
    }
}

fn mass_status_matches(
    report: &ExecutionMassStatus,
    client_id: Option<ClientId>,
    account_id: AccountId,
    venue: Option<Venue>,
) -> bool {
    report.account_id == account_id
        && Some(report.client_id) == client_id
        && Some(report.venue) == venue
        && report
            .order_reports()
            .values()
            .all(|report| report.account_id == account_id)
        && report
            .fill_reports()
            .values()
            .flatten()
            .all(|report| report.account_id == account_id)
        && report
            .position_reports()
            .values()
            .flatten()
            .all(|report| report.account_id == account_id)
}

#[cfg(test)]
mod tests {
    use nautilus_common::{clock::TestClock, enums::LogLevel};
    use nautilus_core::python::to_pyvalue_err;
    use nautilus_model::{
        enums::{OrderStatus, OrderType, TimeInForce},
        events::OrderEventAny,
    };
    use rstest::{fixture, rstest};

    use super::*;

    #[fixture]
    fn execution_output() -> (
        ClientOutput,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        Python::initialize();
        let output = ClientOutput::py_new();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        {
            let mut state = output.state.lock();
            state.exec_sender = Some(sender);
            state.bound = true;
            state.event_factory = Some(OrderEventFactory::new(
                TraderId::from("TESTER-001"),
                AccountId::from("SIM-001"),
                AccountType::Cash,
                None,
            ));
        }

        (output, receiver)
    }

    #[fixture]
    fn native_execution_client() -> PythonExecutionClient {
        Python::initialize();
        Python::attach(|py| {
            let module = PyModule::from_code(
                py,
                c"import asyncio
class Runtime:
    def __init__(self):
        self.loop = asyncio.new_event_loop()
        self.result = None
        self.failure = None
        self.cancelled = False
        self.calls = []
    def call_awaited(self, operation, args):
        self.calls.append((operation, args))
        task = self.loop.create_future()
        if self.cancelled:
            task.cancel()
        elif self.failure is not None:
            task.set_exception(self.failure)
        else:
            task.set_result(self.result)
        return task
    def call(self, operation, args):
        self.calls.append((operation, args))
    def _handles_order_venue(self, venue):
        self.calls.append(('venue', venue))
        if self.failure is not None:
            raise self.failure
        return self.result
    def _provides_bulk_position_coverage(self, instrument_id):
        self.calls.append(('positions', instrument_id))
        if self.failure is not None:
            raise self.failure
        return self.result
    def _calculate_commission(self, *args):
        self.calls.append(('commission', args))
        if self.failure is not None:
            raise self.failure
        return self.result
    def abandon(self, task):
        task.cancel()
    def __del__(self):
        self.loop.close()
",
                c"adapter_native_tests.py",
                c"adapter_native_tests",
            )
            .unwrap();
            let runtime = module.getattr("Runtime").unwrap().call0().unwrap().unbind();

            let cache = CacheView::new(Rc::new(RefCell::new(Cache::default())));

            PythonExecutionClient {
                client: PythonClient {
                    client_id: ClientId::from("SIM"),
                    venue: Some(Venue::from("SIM")),
                    instance: runtime.clone_ref(py),
                    runtime,
                    output: ClientOutput::py_new(),
                    execution: None,
                    clock: Rc::new(RefCell::new(TestClock::default())),
                },
                identity: ExecutionIdentity {
                    tolerance: Decimal::ZERO,
                    trader_id: TraderId::from("TESTER-001"),
                    account_id: AccountId::from("SIM-001"),
                    account_type: AccountType::Cash,
                    base_currency: None,
                    oms_type: OmsType::Netting,
                    venue: Venue::from("SIM"),
                },
                cache,
            }
        })
    }

    #[rstest]
    #[case("missing")]
    #[case("valid")]
    #[case("foreign")]
    #[case("wrong_type")]
    #[case("exception")]
    #[case("cancelled")]
    #[tokio::test]
    async fn test_single_order_report_validates_result_and_preserves_command(
        native_execution_client: PythonExecutionClient,
        #[case] result_kind: &str,
    ) {
        let client = native_execution_client;
        let mut expected = order_report();
        if result_kind == "foreign" {
            expected.account_id = AccountId::from("OTHER-001");
        }

        let command = GenerateOrderStatusReport::new(
            nautilus_core::UUID4::new(),
            163.into(),
            Some(InstrumentId::from("AUD/USD.SIM")),
            Some(ClientOrderId::from("ORDER-167")),
            Some(VenueOrderId::from("VENUE-173")),
            None,
            Some(nautilus_core::UUID4::new()),
        );
        Python::attach(|py| {
            let runtime = client.client.runtime.bind(py);

            match result_kind {
                "valid" | "foreign" => runtime
                    .setattr("result", Py::new(py, expected.clone()).unwrap())
                    .unwrap(),
                "wrong_type" => runtime.setattr("result", "not a report").unwrap(),
                "exception" => runtime
                    .setattr("failure", to_pyvalue_err("Report rejected").value(py))
                    .unwrap(),
                "cancelled" => runtime.setattr("cancelled", true).unwrap(),
                _ => {}
            }
        });

        let result = client.generate_order_status_report(&command).await;

        match result_kind {
            "missing" => assert_eq!(result.unwrap(), None),
            "valid" => assert_eq!(result.unwrap(), Some(expected)),
            "foreign" => assert_eq!(
                result.unwrap_err().to_string(),
                "Report account identity does not match its client"
            ),
            "wrong_type" => assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("OrderStatusReport")
            ),
            "exception" => assert_eq!(
                result.unwrap_err().to_string(),
                "ValueError: Report rejected"
            ),
            "cancelled" => assert!(result.unwrap_err().to_string().contains("CancelledError")),
            _ => unreachable!(),
        }

        Python::attach(|py| {
            let calls = client.client.runtime.getattr(py, "calls").unwrap();
            assert_eq!(calls.bind(py).len().unwrap(), 1);
            let call = calls.bind(py).get_item(0).unwrap();
            assert_eq!(
                call.get_item(0).unwrap().extract::<String>().unwrap(),
                "_generate_order_status_report"
            );
            let args = call.get_item(1).unwrap();
            assert_eq!(args.len().unwrap(), 1);
            let forwarded = args
                .get_item(0)
                .unwrap()
                .extract::<PyRef<'_, GenerateOrderStatusReport>>()
                .unwrap();
            assert_eq!(*forwarded, command);
        });
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_output_rebinding_is_rejected(#[case] disposed: bool) {
        Python::initialize();
        let output = ClientOutput::py_new();
        if disposed {
            output.invalidate();
        } else {
            output.state.lock().bound = true;
        }

        assert_eq!(
            output.bind().unwrap_err().to_string(),
            "RuntimeError: Client output cannot be rebound"
        );
    }

    #[rstest]
    fn test_output_owner_thread_guards(
        execution_output: (
            ClientOutput,
            tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        ),
    ) {
        let (output, _) = execution_output;
        let other = output.clone();
        let unbound = ClientOutput::py_new();

        let errors = std::thread::spawn(move || {
            [
                other.sender().unwrap_err().to_string(),
                other.exec_sender().unwrap_err().to_string(),
                other.bind().unwrap_err().to_string(),
                unbound.bind().unwrap_err().to_string(),
            ]
        })
        .join()
        .unwrap();

        assert_eq!(
            errors,
            [
                "RuntimeError: Client output requires its owner thread",
                "RuntimeError: Client output requires its owner thread",
                "RuntimeError: Client output requires its owner thread",
                "RuntimeError: Client output requires its owner thread",
            ]
        );
        assert!(output.exec_sender().is_ok());
    }

    #[rstest]
    fn test_output_invalidation_removes_both_senders(
        execution_output: (
            ClientOutput,
            tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        ),
    ) {
        let (output, _) = execution_output;
        let (sender, _) = tokio::sync::mpsc::unbounded_channel();
        output.state.lock().sender = Some(sender);
        output.invalidate();
        output.invalidate();
        assert_eq!(
            output.sender().unwrap_err().to_string(),
            "RuntimeError: Client output is not bound to an active node"
        );
        assert_eq!(
            output.exec_sender().unwrap_err().to_string(),
            "RuntimeError: Execution output is not bound to an active node"
        );
    }

    #[rstest]
    #[case("submitted")]
    #[case("accepted")]
    #[case("canceled")]
    fn test_output_batches_preserve_events_and_reject_foreign_identity(
        execution_output: (
            ClientOutput,
            tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        ),
        #[case] kind: &str,
    ) {
        let (output, mut receiver) = execution_output;

        match kind {
            "submitted" => {
                let first = OrderSubmitted {
                    trader_id: TraderId::from("TESTER-001"),
                    account_id: AccountId::from("SIM-001"),
                    ..Default::default()
                };

                let mut second = first.clone();
                second.client_order_id = ClientOrderId::from("SECOND");
                second.ts_event = 113.into();
                let events = vec![first, second];
                output.py_order_submitted_batch(events.clone()).unwrap();

                let ExecutionEvent::OrderSubmittedBatch(batch) = receiver.try_recv().unwrap()
                else {
                    panic!("Expected submitted batch");
                };

                assert_eq!(batch.events, events);
                let mut foreign = events[0].clone();
                foreign.account_id = AccountId::from("OTHER-001");
                assert_eq!(
                    output
                        .py_order_submitted_batch(vec![events[0].clone(), foreign])
                        .unwrap_err()
                        .to_string(),
                    "TypeError: Execution output identity does not match its owner"
                );
            }
            "accepted" => {
                let first = OrderAccepted {
                    trader_id: TraderId::from("TESTER-001"),
                    account_id: AccountId::from("SIM-001"),
                    ..Default::default()
                };

                let mut second = first.clone();
                second.client_order_id = ClientOrderId::from("SECOND");
                second.ts_event = 127.into();
                let events = vec![first, second];
                output.py_order_accepted_batch(events.clone()).unwrap();

                let ExecutionEvent::OrderAcceptedBatch(batch) = receiver.try_recv().unwrap() else {
                    panic!("Expected accepted batch");
                };

                assert_eq!(batch.events, events);
                let mut foreign = events[0].clone();
                foreign.trader_id = TraderId::from("OTHER-001");
                assert_eq!(
                    output
                        .py_order_accepted_batch(vec![events[0].clone(), foreign])
                        .unwrap_err()
                        .to_string(),
                    "TypeError: Execution output identity does not match its owner"
                );
            }
            "canceled" => {
                let first = OrderCanceled {
                    trader_id: TraderId::from("TESTER-001"),
                    account_id: Some(AccountId::from("SIM-001")),
                    ..Default::default()
                };

                let mut second = first.clone();
                second.client_order_id = ClientOrderId::from("SECOND");
                second.account_id = None;
                second.ts_event = 131.into();
                let events = vec![first, second];
                output.py_order_canceled_batch(events.clone()).unwrap();

                let ExecutionEvent::OrderCanceledBatch(batch) = receiver.try_recv().unwrap() else {
                    panic!("Expected canceled batch");
                };

                assert_eq!(batch.events, events);
                let mut foreign = events[0].clone();
                foreign.account_id = Some(AccountId::from("OTHER-001"));
                assert_eq!(
                    output
                        .py_order_canceled_batch(vec![events[0].clone(), foreign])
                        .unwrap_err()
                        .to_string(),
                    "TypeError: Execution output identity does not match its owner"
                );
            }
            _ => unreachable!(),
        }

        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

    #[rstest]
    fn test_closed_execution_channel_reports_failure(
        execution_output: (
            ClientOutput,
            tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        ),
    ) {
        let (output, receiver) = execution_output;
        drop(receiver);

        let event = OrderSubmitted {
            trader_id: TraderId::from("TESTER-001"),
            account_id: AccountId::from("SIM-001"),
            ..Default::default()
        };

        let error = output
            .send_exec(ExecutionEvent::Order(OrderEventAny::Submitted(event)))
            .unwrap_err();
        assert_eq!(error.to_string(), "RuntimeError: channel closed");
    }

    #[rstest]
    fn test_cache_view_rejects_incompatible_borrow_and_recovers() {
        Python::initialize();

        let cache = Rc::new(RefCell::new(Cache::default()));
        let id = CACHE_ID.fetch_add(1, Ordering::Relaxed);
        CLIENT_CACHES.with_borrow_mut(|caches| caches.insert(id, CacheView::new(cache.clone())));

        let view = PyClientCache { id };
        let held = cache.borrow_mut();
        assert!(view.py_instrument_ids(None).is_err());
        drop(held);
        assert_eq!(
            view.py_instrument_ids(None).unwrap(),
            Vec::<InstrumentId>::new()
        );
        CLIENT_CACHES.with_borrow_mut(|caches| caches.remove(&id));
        assert_eq!(
            view.py_instrument_ids(None).unwrap_err().to_string(),
            "RuntimeError: Client cache is disposed or accessed from a foreign thread"
        );
    }
    #[rstest]
    #[case(None)]
    #[case(Some(Price::from("123.456")))]
    fn test_option_reference_request_and_response_preserve_identity(
        native_execution_client: PythonExecutionClient,
        #[case] price: Option<Price>,
    ) {
        use nautilus_common::messages::data::DataResponse;
        use nautilus_model::identifiers::OptionSeriesId;

        use crate::python::client::responses::PyOptionChainReferencePriceResponse;

        let client = native_execution_client.client;

        let series =
            OptionSeriesId::new(Venue::from("SIM"), "BTC".into(), "USD".into(), 197.into());

        let request = RequestOptionChainReferencePrice::new(
            series,
            InstrumentId::from("BTC/USD.SIM"),
            Some(ClientId::from("SIM")),
            nautilus_core::UUID4::new(),
            199.into(),
            None,
        );
        client
            .request_option_chain_reference_price(request.clone())
            .unwrap();
        Python::attach(|py| {
            let calls = client.runtime.getattr(py, "calls").unwrap();
            assert_eq!(calls.bind(py).len().unwrap(), 1);
            let call = calls.bind(py).get_item(0).unwrap();
            assert_eq!(
                call.get_item(0).unwrap().extract::<String>().unwrap(),
                "_request_option_chain_reference_price"
            );
            let args = call.get_item(1).unwrap();
            assert_eq!(args.len().unwrap(), 1);
            let forwarded = args
                .get_item(0)
                .unwrap()
                .extract::<PyRef<'_, PyRequestOptionChainReferencePrice>>()
                .unwrap();
            assert_eq!(
                args.get_item(0)
                    .unwrap()
                    .getattr("ts_init")
                    .unwrap()
                    .extract::<u64>()
                    .unwrap(),
                199
            );
            assert_eq!(forwarded.request.series_id, request.series_id);
            assert_eq!(forwarded.request.instrument_id, request.instrument_id);
            assert_eq!(forwarded.request.client_id, request.client_id);
            assert_eq!(forwarded.request.request_id, request.request_id);
            assert_eq!(forwarded.request.ts_init, request.ts_init);
            assert_eq!(forwarded.request.params, request.params);

            let output = ClientOutput::py_new();
            let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
            {
                let mut state = output.state.lock();
                state.sender = Some(sender);
                state.client_id = Some(ClientId::from("SIM"));
            }

            let response = py
                .get_type::<PyOptionChainReferencePriceResponse>()
                .call1((
                    ClientId::from("SIM"),
                    series,
                    price,
                    request.request_id,
                    211_u64,
                ))
                .unwrap();
            output.py_response(&response).unwrap();

            let DataEvent::Response(DataResponse::OptionChainReferencePrice(response)) =
                receiver.try_recv().unwrap()
            else {
                panic!("Expected option reference response");
            };

            assert_eq!(response.client_id, ClientId::from("SIM"));
            assert_eq!(response.series_id, series);
            assert_eq!(response.price, price);
            assert_eq!(response.correlation_id, request.request_id);
            assert_eq!(response.ts_init, UnixNanos::from(211));
            assert_eq!(response.params, None);
            let foreign = py
                .get_type::<PyOptionChainReferencePriceResponse>()
                .call1((
                    ClientId::from("OTHER"),
                    series,
                    price,
                    request.request_id,
                    223_u64,
                ))
                .unwrap();
            assert_eq!(
                output.py_response(&foreign).unwrap_err().to_string(),
                "TypeError: Response client identity does not match its owner"
            );
            assert!(matches!(
                receiver.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ));
        });
    }

    #[rstest]
    fn test_native_account_output_preserves_fields_and_owner_clock(
        mut native_execution_client: PythonExecutionClient,
        execution_output: (
            ClientOutput,
            tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        ),
    ) {
        let (output, mut receiver) = execution_output;
        native_execution_client.client.output = output;
        let balances = vec![AccountBalance::new(
            Money::from("1000 USD"),
            Money::from("17 USD"),
            Money::from("983 USD"),
        )];
        native_execution_client
            .generate_account_state(balances.clone(), vec![], false, 233.into(), None)
            .unwrap();

        let ExecutionEvent::Account(event) = receiver.try_recv().unwrap() else {
            panic!("Expected account state");
        };

        assert_eq!(event.account_id, AccountId::from("SIM-001"));
        assert_eq!(event.account_type, AccountType::Cash);
        assert_eq!(event.base_currency, None);
        assert_eq!(event.balances, balances);
        assert_eq!(event.margins, vec![]);
        assert!(!event.is_reported);
        assert_eq!(event.ts_event, UnixNanos::from(233));
        assert_eq!(event.ts_init, UnixNanos::from(0));
        assert_eq!(event.info, None);
        Python::attach(|py| {
            let owned = Py::new(py, event.clone()).unwrap().into_any();
            native_execution_client
                .client
                .output
                .py_event(py, owned)
                .unwrap();
        });

        let ExecutionEvent::Account(forwarded) = receiver.try_recv().unwrap() else {
            panic!("Expected forwarded account state");
        };

        assert_eq!(forwarded, event);
    }

    #[rstest]
    fn test_native_client_identity_and_single_use_lifecycle(
        mut native_execution_client: PythonExecutionClient,
    ) {
        let client = &mut native_execution_client;
        assert_eq!(client.client_id(), ClientId::from("SIM"));
        assert_eq!(client.account_id(), AccountId::from("SIM-001"));
        assert_eq!(client.venue(), Venue::from("SIM"));
        assert_eq!(client.oms_type(), OmsType::Netting);
        assert_eq!(client.position_reconciliation_tolerance(), Decimal::ZERO);
        assert_eq!(
            client.reset().unwrap_err().to_string(),
            "Python clients are single-use"
        );
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        nautilus_common::live::runner::replace_data_event_sender(sender);
        assert!(client.start().is_ok());
        assert!(client.stop().is_ok());
        assert!(client.dispose().is_ok());
        assert_eq!(
            client.client.reset().unwrap_err().to_string(),
            "Python clients are single-use"
        );
        assert_eq!(
            client.client.start().unwrap_err().to_string(),
            "RuntimeError: Client output cannot be rebound"
        );
        assert!(client.client.stop().is_ok());
        assert!(client.client.dispose().is_ok());
        assert_eq!(client.client.venue(), Some(Venue::from("SIM")));
        assert!(client.client.is_disconnected());

        let cache = Rc::new(RefCell::new(Cache::default()));

        client.cache = CacheView::new(cache.clone());
        assert_eq!(client.get_account(), None);
        let held = cache.borrow_mut();
        assert_eq!(client.get_account(), None);
        drop(held);
        let account = AccountAny::default();
        client.identity.account_id = account.id();
        cache.borrow_mut().add_account(account.clone()).unwrap();
        assert_eq!(client.get_account(), Some(account));
    }

    #[rstest]
    #[case("true")]
    #[case("false")]
    #[case("wrong_type")]
    #[case("exception")]
    fn test_native_coverage_hooks_fail_closed_and_preserve_arguments(
        native_execution_client: PythonExecutionClient,
        #[case] mode: &str,
    ) {
        let client = native_execution_client;
        Python::attach(|py| {
            let runtime = client.client.runtime.bind(py);

            match mode {
                "true" => runtime.setattr("result", true).unwrap(),
                "false" => runtime.setattr("result", false).unwrap(),
                "wrong_type" => runtime.setattr("result", "invalid").unwrap(),
                "exception" => runtime
                    .setattr("failure", to_pyvalue_err("Coverage unavailable").value(py))
                    .unwrap(),
                _ => unreachable!(),
            }
        });

        let venue = Venue::from("OTHER");
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        assert_eq!(client.handles_order_venue(venue), mode == "true");
        assert_eq!(
            client.provides_bulk_position_coverage(instrument_id),
            mode == "true"
        );
        Python::attach(|py| {
            let calls = client.client.runtime.getattr(py, "calls").unwrap();
            assert_eq!(calls.bind(py).len().unwrap(), 2);
            let first = calls.bind(py).get_item(0).unwrap();
            assert_eq!(
                first.get_item(0).unwrap().extract::<String>().unwrap(),
                "venue"
            );
            assert_eq!(
                first.get_item(1).unwrap().extract::<Venue>().unwrap(),
                venue
            );
            let second = calls.bind(py).get_item(1).unwrap();
            assert_eq!(
                second.get_item(0).unwrap().extract::<String>().unwrap(),
                "positions"
            );
            assert_eq!(
                second
                    .get_item(1)
                    .unwrap()
                    .extract::<InstrumentId>()
                    .unwrap(),
                instrument_id
            );
        });
    }

    #[rstest]
    #[case("none")]
    #[case("money")]
    #[case("exception")]
    fn test_native_commission_hook_preserves_inputs_and_propagates_failure(
        native_execution_client: PythonExecutionClient,
        #[case] mode: &str,
    ) {
        let client = native_execution_client;
        let commission = Money::from("0.17 USD");
        Python::attach(|py| {
            let runtime = client.client.runtime.bind(py);
            if mode == "money" {
                runtime.setattr("result", commission).unwrap();
            } else if mode == "exception" {
                runtime
                    .setattr("failure", to_pyvalue_err("Fee unavailable").value(py))
                    .unwrap();
            }
        });

        let instrument =
            InstrumentAny::CurrencyPair(nautilus_model::instruments::stubs::audusd_sim());
        let quantity = Quantity::from("17");
        let price = Price::from("0.71231");
        let result =
            client.calculate_commission(&instrument, quantity, price, LiquiditySide::Maker);

        if mode == "exception" {
            assert_eq!(
                result.unwrap_err().to_string(),
                "ValueError: Fee unavailable"
            );
        } else {
            assert_eq!(
                result.unwrap(),
                if mode == "money" {
                    Some(commission)
                } else {
                    None
                }
            );
        }

        Python::attach(|py| {
            let calls = client.client.runtime.getattr(py, "calls").unwrap();
            assert_eq!(calls.bind(py).len().unwrap(), 1);
            let call = calls.bind(py).get_item(0).unwrap();
            assert_eq!(
                call.get_item(0).unwrap().extract::<String>().unwrap(),
                "commission"
            );
            let args = call.get_item(1).unwrap();
            assert_eq!(args.len().unwrap(), 4);
            assert_eq!(
                pyobject_to_instrument_any(py, args.get_item(0).unwrap().unbind()).unwrap(),
                instrument
            );
            assert_eq!(
                args.get_item(1).unwrap().extract::<Quantity>().unwrap(),
                quantity
            );
            assert_eq!(args.get_item(2).unwrap().extract::<Price>().unwrap(), price);
            assert_eq!(
                args.get_item(3)
                    .unwrap()
                    .extract::<LiquiditySide>()
                    .unwrap(),
                LiquiditySide::Maker
            );
        });
    }
    macro_rules! report_sequence_test {
        ($name:ident, $method:ident, $command_type:ty, [$($borrow:tt)*], $command:expr, $report:expr) => {
            #[rstest]
            #[case("empty")]
            #[case("valid")]
            #[case("foreign")]
            #[case("wrong_type")]
            #[tokio::test]
            async fn $name(native_execution_client: PythonExecutionClient, #[case] mode: &str) {
                let client = native_execution_client;
                let mut command = $command;
                command.correlation_id = Some(nautilus_core::UUID4::new());
                command.causation_id = Some(nautilus_core::UUID4::new());
                command.log_receipt_level = LogLevel::Debug;
                let mut params = Params::default();
                params.insert("probe".to_owned(), serde_json::Value::from(17));
                command.params = Some(params);
                let first = $report;
                let mut second = first.clone();
                second.ts_init = 251.into();
                let expected = if mode == "empty" { vec![] } else { vec![first, second] };
                Python::attach(|py| {
                    if mode != "wrong_type" {
                        let mut reports = expected.clone();
                        if mode == "foreign" {
                            reports[1].account_id = AccountId::from("OTHER-001");
                        }
                        let reports = reports.into_iter().map(|report| Py::new(py, report).unwrap()).collect::<Vec<_>>();
                        client.client.runtime.bind(py).setattr("result", reports).unwrap();
                    }
                });
                let result = client.$method($($borrow)* command.clone()).await;
                if mode == "foreign" {
                    assert_eq!(result.unwrap_err().to_string(), "Report account identity does not match its client");
                } else if mode == "wrong_type" {
                    assert!(result.unwrap_err().to_string().starts_with("TypeError:"));
                } else {
                    assert_eq!(result.unwrap(), expected);
                }
                Python::attach(|py| {
                    let calls = client.client.runtime.getattr(py, "calls").unwrap();
                    assert_eq!(calls.bind(py).len().unwrap(), 1);
                    let call = calls.bind(py).get_item(0).unwrap();
                    assert_eq!(call.get_item(0).unwrap().extract::<String>().unwrap(), concat!("_", stringify!($method)));
                    let args = call.get_item(1).unwrap();
                    assert_eq!(args.len().unwrap(), 1);
                    let argument = args.get_item(0).unwrap();
                    let forwarded = argument.extract::<PyRef<'_, $command_type>>().unwrap();
                    assert_eq!(*forwarded, command);
                    assert_eq!(argument.getattr("command_id").unwrap().extract::<nautilus_core::UUID4>().unwrap(), command.command_id);
                    assert_eq!(argument.getattr("ts_init").unwrap().extract::<u64>().unwrap(), 241);
                    assert_eq!(argument.getattr("correlation_id").unwrap().extract::<Option<nautilus_core::UUID4>>().unwrap(), command.correlation_id);
                    assert_eq!(argument.getattr("causation_id").unwrap().extract::<Option<nautilus_core::UUID4>>().unwrap(), command.causation_id);
                    assert_eq!(argument.getattr("instrument_id").unwrap().extract::<Option<InstrumentId>>().unwrap(), command.instrument_id);
                    assert_eq!(argument.getattr("start").unwrap().extract::<u64>().unwrap(), 229);
                    assert_eq!(argument.getattr("end").unwrap().extract::<u64>().unwrap(), 239);
                    assert_eq!(argument.getattr("log_receipt_level").unwrap().extract::<LogLevel>().unwrap(), command.log_receipt_level);
                    let params = argument.getattr("params").unwrap();
                    assert_eq!(params.len().unwrap(), 1);
                    assert_eq!(params.get_item("probe").unwrap().extract::<i64>().unwrap(), 17);
                });
            }
        };
    }

    report_sequence_test!(
        test_order_report_sequences_validate_every_member,
        generate_order_status_reports, GenerateOrderStatusReports, [&],
        GenerateOrderStatusReports::new(nautilus_core::UUID4::new(), 241.into(), true,
            Some(InstrumentId::from("AUD/USD.SIM")), Some(229.into()), Some(239.into()), None, None),
        order_report()
    );

    report_sequence_test!(
        test_fill_report_sequences_validate_every_member,
        generate_fill_reports,
        GenerateFillReports,
        [],
        GenerateFillReports::new(
            nautilus_core::UUID4::new(),
            241.into(),
            Some(InstrumentId::from("AUD/USD.SIM")),
            Some(VenueOrderId::from("VENUE-173")),
            Some(229.into()),
            Some(239.into()),
            None,
            None
        ),
        fill_report()
    );

    report_sequence_test!(
        test_position_report_sequences_validate_every_member,
        generate_position_status_reports, GeneratePositionStatusReports, [&],
        GeneratePositionStatusReports::new(nautilus_core::UUID4::new(), 241.into(),
            Some(InstrumentId::from("AUD/USD.SIM")), Some(229.into()), Some(239.into()), None, None),
        position_report()
    );
    #[rstest]
    #[case("order")]
    #[case("fill")]
    #[case("position")]
    #[case("mass")]
    #[case("order_with_fills")]
    fn test_report_output_preserves_each_payload_family(
        execution_output: (
            ClientOutput,
            tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        ),
        order_report: OrderStatusReport,
        fill_report: FillReport,
        position_report: PositionStatusReport,
        #[case] kind: &str,
    ) {
        let (output, mut receiver) = execution_output;
        {
            let mut state = output.state.lock();
            state.client_id = Some(ClientId::from("SIM"));
            state.venue = Some(Venue::from("SIM"));
        }

        let mut mass = ExecutionMassStatus::new(
            ClientId::from("SIM"),
            AccountId::from("SIM-001"),
            Venue::from("SIM"),
            257.into(),
            None,
        );
        mass.add_order_reports(vec![order_report.clone()]);
        mass.add_fill_reports(vec![fill_report.clone()]);
        mass.add_position_reports(vec![position_report.clone()]);
        Python::attach(|py| {
            let report = match kind {
                "order" | "order_with_fills" => {
                    Py::new(py, order_report.clone()).unwrap().into_any()
                }
                "fill" => Py::new(py, fill_report.clone()).unwrap().into_any(),
                "position" => Py::new(py, position_report.clone()).unwrap().into_any(),
                "mass" => Py::new(py, mass.clone()).unwrap().into_any(),
                _ => unreachable!(),
            };

            output
                .py_report(
                    report.bind(py),
                    if kind == "order_with_fills" {
                        Some(vec![fill_report.clone()])
                    } else {
                        None
                    },
                )
                .unwrap();
        });

        let ExecutionEvent::Report(report) = receiver.try_recv().unwrap() else {
            panic!("Expected execution report");
        };

        match (kind, report) {
            ("order", ExecutionReport::Order(report)) => assert_eq!(*report, order_report),
            ("fill", ExecutionReport::Fill(report)) => assert_eq!(*report, fill_report),
            ("position", ExecutionReport::Position(report)) => assert_eq!(*report, position_report),
            ("mass", ExecutionReport::MassStatus(report)) => assert_eq!(*report, mass),
            ("order_with_fills", ExecutionReport::OrderWithFills(report, fills)) => {
                assert_eq!(*report, order_report);
                assert_eq!(fills, vec![fill_report]);
            }
            _ => panic!("Wrong execution report variant"),
        }
    }

    #[fixture]
    fn order_report() -> OrderStatusReport {
        OrderStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUD/USD.SIM"),
            Some(ClientOrderId::from("ORDER-167")),
            VenueOrderId::from("VENUE-173"),
            Some(OrderSide::Buy),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from("17"),
            Quantity::from("3"),
            149.into(),
            151.into(),
            157.into(),
            None,
        )
    }

    #[fixture]
    fn fill_report() -> FillReport {
        FillReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUD/USD.SIM"),
            VenueOrderId::from("VENUE-173"),
            TradeId::from("TRADE-179"),
            OrderSide::Buy,
            Quantity::from("3"),
            Price::from("0.71231"),
            Money::from("0.17 USD"),
            LiquiditySide::Maker,
            Some(ClientOrderId::from("ORDER-167")),
            Some(PositionId::from("POSITION-181")),
            191.into(),
            193.into(),
            None,
        )
    }

    #[fixture]
    fn position_report() -> PositionStatusReport {
        PositionStatusReport::new(
            AccountId::from("SIM-001"),
            InstrumentId::from("AUD/USD.SIM"),
            PositionSide::Short,
            Quantity::from("17"),
            191.into(),
            193.into(),
            None,
            Some(PositionId::from("POSITION-181")),
            Some(Decimal::new(71231, 5)),
        )
    }
    #[rstest]
    #[case("none")]
    #[case("valid")]
    #[case("client")]
    #[case("account")]
    #[case("venue")]
    #[case("order")]
    #[case("fill")]
    #[case("position")]
    #[case("wrong_type")]
    #[tokio::test]
    async fn test_mass_report_checks_outer_and_nested_identity(
        native_execution_client: PythonExecutionClient,
        mut order_report: OrderStatusReport,
        mut fill_report: FillReport,
        mut position_report: PositionStatusReport,
        #[case] kind: &str,
    ) {
        let client = native_execution_client;
        let account = AccountId::from("OTHER-001");
        if kind == "order" {
            order_report.account_id = account;
        }

        if kind == "fill" {
            fill_report.account_id = account;
        }

        if kind == "position" {
            position_report.account_id = account;
        }

        let mut mass = ExecutionMassStatus::new(
            ClientId::from(if kind == "client" { "OTHER" } else { "SIM" }),
            if kind == "account" {
                account
            } else {
                AccountId::from("SIM-001")
            },
            Venue::from(if kind == "venue" { "OTHER" } else { "SIM" }),
            263.into(),
            None,
        );

        mass.add_order_reports(vec![order_report]);
        mass.add_fill_reports(vec![fill_report]);
        mass.add_position_reports(vec![position_report]);
        Python::attach(|py| {
            if kind == "wrong_type" {
                client
                    .client
                    .runtime
                    .bind(py)
                    .setattr("result", 17)
                    .unwrap();
            } else if kind != "none" {
                client
                    .client
                    .runtime
                    .bind(py)
                    .setattr("result", Py::new(py, mass.clone()).unwrap())
                    .unwrap();
            }
        });

        let result = client.generate_mass_status(Some(23)).await;

        match kind {
            "none" => assert_eq!(result.unwrap(), None),
            "valid" => assert_eq!(result.unwrap(), Some(mass)),
            "wrong_type" => assert!(result.unwrap_err().to_string().starts_with("TypeError:")),
            _ => assert_eq!(
                result.unwrap_err().to_string(),
                "Mass status identity does not match its client"
            ),
        }

        Python::attach(|py| {
            let calls = client.client.runtime.getattr(py, "calls").unwrap();
            assert_eq!(calls.bind(py).len().unwrap(), 1);
            let call = calls.bind(py).get_item(0).unwrap();
            assert_eq!(
                call.get_item(0).unwrap().extract::<String>().unwrap(),
                "_generate_mass_status"
            );
            assert_eq!(
                call.get_item(1).unwrap().extract::<(u64,)>().unwrap(),
                (23,)
            );
        });
    }
    #[rstest]
    fn test_cache_bytes_and_order_lists_are_owned_snapshots() {
        Python::initialize();

        let cache = Rc::new(RefCell::new(Cache::default()));
        let id = CACHE_ID.fetch_add(1, Ordering::Relaxed);
        CLIENT_CACHES.with_borrow_mut(|caches| caches.insert(id, CacheView::new(cache.clone())));

        let view = PyClientCache { id };

        let list = OrderList::new(
            OrderListId::from("OL-SNAPSHOT"),
            InstrumentId::from("AUD/USD.SIM"),
            StrategyId::from("S-SNAPSHOT"),
            vec![
                ClientOrderId::from("O-FIRST"),
                ClientOrderId::from("O-SECOND"),
            ],
            UnixNanos::from(173),
        );
        cache
            .borrow_mut()
            .add("probe", vec![0, 17, 255].into())
            .unwrap();
        cache.borrow_mut().add_order_list(list.clone()).unwrap();
        let mut bytes = view.py_get("probe").unwrap().unwrap();
        let snapshot = view.py_order_list(list.id).unwrap().unwrap();
        bytes[1] = 29;
        cache
            .borrow_mut()
            .add("probe", vec![31, 0, 127].into())
            .unwrap();
        assert_eq!(bytes, vec![0, 29, 255]);
        assert_eq!(view.py_get("probe").unwrap(), Some(vec![31, 0, 127]));
        assert_eq!(snapshot, list);
        assert_eq!(view.py_order_list(list.id).unwrap(), Some(list));
        assert_eq!(view.py_get("missing").unwrap(), None);
        assert_eq!(
            view.py_order_list(OrderListId::from("OL-MISSING")).unwrap(),
            None
        );
        CLIENT_CACHES.with_borrow_mut(|caches| caches.remove(&id));
    }
    #[rstest]
    fn test_factory_metadata() {
        Python::initialize();
        Python::attach(|py| {
            let data = PythonDataFactory {
                factory: py.None(),
                clients: PythonClients::default(),
            };

            let execution = PythonExecutionFactory {
                factory: py.None(),
                clients: PythonClients::default(),
            };

            assert_eq!(data.name(), "Python");
            assert_eq!(data.config_type(), "DataClientConfig");
            assert_eq!(execution.name(), "Python");
            assert_eq!(execution.config_type(), "ExecutionClientConfig");
        });
    }

    #[rstest]
    #[case(None)]
    #[case(Some(nautilus_core::UUID4::new()))]
    fn test_subscription_metadata_preserves_timestamp_and_correlation(
        #[case] correlation_id: Option<nautilus_core::UUID4>,
    ) {
        Python::initialize();
        Python::attach(|py| {
            let command_id = nautilus_core::UUID4::new();

            let command = SubscribeQuotes::new(
                InstrumentId::from("AUD/USD.SIM"),
                Some(ClientId::from("SIM")),
                Some(Venue::from("SIM")),
                command_id,
                283.into(),
                correlation_id,
                None,
            );
            let command_py = Py::new(py, command).unwrap();
            assert_eq!(
                command_py
                    .getattr(py, "command_id")
                    .unwrap()
                    .extract::<nautilus_core::UUID4>(py)
                    .unwrap(),
                command_id
            );
            assert_eq!(
                command_py
                    .getattr(py, "ts_init")
                    .unwrap()
                    .extract::<u64>(py)
                    .unwrap(),
                283
            );
            assert_eq!(
                command_py
                    .getattr(py, "correlation_id")
                    .unwrap()
                    .extract::<Option<nautilus_core::UUID4>>(py)
                    .unwrap(),
                correlation_id
            );
        });
    }
}
