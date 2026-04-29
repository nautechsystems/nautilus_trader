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

//! Core data client implementation for Interactive Brokers.

#[path = "core_streams.rs"]
mod streams;

use std::{
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use anyhow::Context;
#[cfg(feature = "python")]
use chrono::{DateTime, Utc};
use ibapi::{
    contracts::{Contract, Currency as IBCurrency, Exchange as IBExchange, SecurityType, Symbol},
    market_data::historical::ToDuration,
};
use nautilus_common::{
    clients::DataClient,
    live::{get_runtime, runner::get_data_event_sender},
    messages::{
        DataEvent, DataResponse,
        data::{
            BarsResponse, InstrumentResponse, InstrumentsResponse, QuotesResponse, RequestBars,
            RequestInstrument, RequestInstruments, RequestQuotes, RequestTrades, SubscribeBars,
            SubscribeBookDeltas, SubscribeIndexPrices, SubscribeOptionGreeks, SubscribeQuotes,
            SubscribeTrades, TradesResponse, UnsubscribeBars, UnsubscribeBookDeltas,
            UnsubscribeIndexPrices, UnsubscribeOptionGreeks, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
#[cfg(feature = "python")]
use nautilus_core::{Params, UUID4};
use nautilus_core::{
    UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
#[cfg(feature = "python")]
use nautilus_model::data::{Bar, BarType, Data, QuoteTick, TradeTick};
use nautilus_model::{
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, any::InstrumentAny},
};
#[cfg(feature = "python")]
use pyo3::{IntoPyObjectExt, prelude::*};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use self::streams::{
    handle_historical_bars_subscription, handle_index_price_subscription,
    handle_market_depth_subscription, handle_option_greeks_subscription, handle_quote_subscription,
    handle_realtime_bars_subscription, handle_tick_by_tick_quote_subscription,
    handle_trade_subscription,
};
use super::{
    cache::{OptionGreeksCache, QuoteCache},
    convert::{
        bar_type_to_ib_bar_size, calculate_duration, calculate_duration_segments,
        chrono_to_ib_datetime, ib_bar_to_nautilus_bar, price_type_to_ib_what_to_show,
    },
};
use crate::{
    common::{consts::IB_VENUE, shared_client::SharedClientHandle},
    config::InteractiveBrokersDataClientConfig,
    providers::instruments::InteractiveBrokersInstrumentProvider,
};

/// Interactive Brokers data client.
///
/// This client provides market data functionality using the `rust-ibapi` library.
/// It manages subscriptions, handles historical data requests, and streams
/// market data to NautilusTrader.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers")
)]
pub struct InteractiveBrokersDataClient {
    /// Client identifier.
    client_id: ClientId,
    /// Configuration for the client.
    config: InteractiveBrokersDataClientConfig,
    /// Instrument provider.
    instrument_provider: Arc<InteractiveBrokersInstrumentProvider>,
    /// Connection state.
    is_connected: AtomicBool,
    /// Cancellation token for stopping tasks.
    cancellation_token: CancellationToken,
    /// Active task handles.
    tasks: Vec<JoinHandle<()>>,
    /// Data event sender.
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    /// Active subscriptions mapped by instrument ID.
    subscriptions: Arc<tokio::sync::Mutex<AHashMap<InstrumentId, SubscriptionInfo>>>,
    /// Active option greeks subscriptions mapped by instrument ID.
    option_greeks_subscriptions: Arc<tokio::sync::Mutex<AHashMap<InstrumentId, CancellationToken>>>,
    /// Quote cache for accumulating tick updates.
    quote_cache: Arc<tokio::sync::Mutex<QuoteCache>>,
    /// Option greeks cache for merging IB option-computation ticks.
    option_greeks_cache: Arc<tokio::sync::Mutex<OptionGreeksCache>>,
    /// Clock for timestamping.
    clock: &'static AtomicTime,
    /// IB API client (shared per host/port/client_id when both data and execution connect).
    ib_client: Option<SharedClientHandle>,
    /// Last bar for each bar type (for bar completion timeout tracking).
    last_bars: Arc<tokio::sync::Mutex<AHashMap<String, ibapi::market_data::realtime::Bar>>>,
    /// Active timeout tasks for bar completion.
    bar_timeout_tasks: Arc<tokio::sync::Mutex<AHashMap<String, tokio::task::JoinHandle<()>>>>,
}

/// Information about an active subscription.
#[derive(Debug)]
#[allow(dead_code)]
struct SubscriptionInfo {
    /// Instrument ID for the subscription.
    instrument_id: InstrumentId,
    /// Subscription type.
    subscription_type: SubscriptionType,
    /// Cancellation token for this specific subscription.
    cancellation_token: CancellationToken,
}

/// Type of subscription.
#[derive(Debug, Clone)]
enum SubscriptionType {
    /// Quote subscription.
    Quotes,
    /// Index price subscription.
    IndexPrices,
    /// Trade subscription.
    Trades,
    /// Bar subscription.
    Bars,
    /// Order book delta subscription.
    BookDeltas,
}

#[cfg(feature = "python")]
static DATA_EVENT_CALLBACK: std::sync::OnceLock<std::sync::Mutex<Option<Py<PyAny>>>> =
    std::sync::OnceLock::new();

#[cfg(feature = "python")]
thread_local! {
    static DATA_EVENT_BRIDGE_INITIALIZED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(feature = "python")]
fn data_event_callback() -> &'static std::sync::Mutex<Option<Py<PyAny>>> {
    DATA_EVENT_CALLBACK.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(feature = "python")]
fn string_hash_map_to_params(
    map: Option<std::collections::HashMap<String, String>>,
) -> Option<Params> {
    map.map(|m| {
        let mut params = Params::new();
        for (key, value) in m {
            params.insert(key, serde_json::Value::String(value));
        }
        params
    })
}

#[cfg(feature = "python")]
fn unix_nanos_to_datetime(nanos: Option<u64>) -> Option<DateTime<Utc>> {
    nanos.and_then(|value| {
        let secs = value / 1_000_000_000;
        let nanos_part = (value % 1_000_000_000) as u32;
        DateTime::from_timestamp(secs as i64, nanos_part)
    })
}

#[cfg(feature = "python")]
fn u64_to_nonzero_usize(value: u64) -> Option<std::num::NonZeroUsize> {
    std::num::NonZeroUsize::new(value as usize)
}

#[cfg(feature = "python")]
fn u16_to_nonzero_usize(value: u16) -> Option<std::num::NonZeroUsize> {
    std::num::NonZeroUsize::new(value as usize)
}

fn parse_start_ns(params: Option<&nautilus_core::Params>) -> Option<UnixNanos> {
    params
        .and_then(|params| params.get_u64("start_ns"))
        .or_else(|| {
            params
                .and_then(|params| params.get_str("start_ns"))
                .and_then(|value| value.parse::<u64>().ok())
        })
        .map(UnixNanos::from)
}

#[cfg(feature = "python")]
fn py_list_from_quotes(py: Python<'_>, values: Vec<QuoteTick>) -> PyResult<Py<PyAny>> {
    let items: PyResult<Vec<Py<PyAny>>> = values
        .into_iter()
        .map(|value| value.into_py_any(py))
        .collect();
    items?.into_py_any(py)
}

#[cfg(feature = "python")]
fn py_list_from_trades(py: Python<'_>, values: Vec<TradeTick>) -> PyResult<Py<PyAny>> {
    let items: PyResult<Vec<Py<PyAny>>> = values
        .into_iter()
        .map(|value| value.into_py_any(py))
        .collect();
    items?.into_py_any(py)
}

#[cfg(feature = "python")]
fn py_list_from_bars(py: Python<'_>, values: Vec<Bar>) -> PyResult<Py<PyAny>> {
    let items: PyResult<Vec<Py<PyAny>>> = values
        .into_iter()
        .map(|value| value.into_py_any(py))
        .collect();
    items?.into_py_any(py)
}

#[cfg(feature = "python")]
fn dispatch_python_data_event(
    py: Python<'_>,
    callback: &Py<PyAny>,
    event: DataEvent,
) -> PyResult<()> {
    let (kind, correlation_id, payload) = match event {
        DataEvent::Data(data) => match data {
            Data::Quote(quote) => ("quote", None, quote.into_py_any(py)?),
            Data::Trade(trade) => ("trade", None, trade.into_py_any(py)?),
            Data::Bar(bar) => ("bar", None, bar.into_py_any(py)?),
            Data::Delta(delta) => ("delta", None, delta.into_py_any(py)?),
            Data::IndexPriceUpdate(index_price) => {
                ("index_price", None, index_price.into_py_any(py)?)
            }
            other => {
                tracing::debug!("Ignoring unsupported IB data event payload: {:?}", other);
                return Ok(());
            }
        },
        DataEvent::OptionGreeks(greeks) => ("option_greeks", None, greeks.into_py_any(py)?),
        DataEvent::Instrument(instrument) => (
            "instrument",
            None,
            nautilus_model::python::instruments::instrument_any_to_pyobject(py, instrument)?,
        ),
        DataEvent::Response(response) => match response {
            DataResponse::Instrument(response) => (
                "instrument_response",
                Some(response.correlation_id.to_string()),
                nautilus_model::python::instruments::instrument_any_to_pyobject(py, response.data)?,
            ),
            DataResponse::Instruments(response) => (
                "instruments_response",
                Some(response.correlation_id.to_string()),
                response
                    .data
                    .into_iter()
                    .map(|instrument| {
                        nautilus_model::python::instruments::instrument_any_to_pyobject(
                            py, instrument,
                        )
                    })
                    .collect::<PyResult<Vec<_>>>()?
                    .into_py_any(py)?,
            ),
            DataResponse::Quotes(response) => (
                "quotes_response",
                Some(response.correlation_id.to_string()),
                py_list_from_quotes(py, response.data)?,
            ),
            DataResponse::Trades(response) => (
                "trades_response",
                Some(response.correlation_id.to_string()),
                py_list_from_trades(py, response.data)?,
            ),
            DataResponse::Bars(response) => (
                "bars_response",
                Some(response.correlation_id.to_string()),
                py_list_from_bars(py, response.data)?,
            ),
            other => {
                tracing::debug!("Ignoring unsupported IB data response payload: {:?}", other);
                return Ok(());
            }
        },
        other => {
            tracing::debug!("Ignoring unsupported IB data event variant: {:?}", other);
            return Ok(());
        }
    };

    callback.call1(py, (kind, correlation_id, payload))?;
    Ok(())
}

impl InteractiveBrokersDataClient {
    /// Create a new `InteractiveBrokersDataClient`.
    ///
    /// # Arguments
    ///
    /// * `client_id` - Client identifier
    /// * `config` - Configuration for the client
    /// * `instrument_provider` - Instrument provider
    ///
    /// # Errors
    ///
    /// Returns an error if client creation fails.
    pub fn new(
        client_id: ClientId,
        config: InteractiveBrokersDataClientConfig,
        instrument_provider: Arc<InteractiveBrokersInstrumentProvider>,
    ) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        Ok(Self {
            client_id,
            config,
            instrument_provider,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            subscriptions: Arc::new(tokio::sync::Mutex::new(AHashMap::new())),
            option_greeks_subscriptions: Arc::new(tokio::sync::Mutex::new(AHashMap::new())),
            quote_cache: Arc::new(tokio::sync::Mutex::new(QuoteCache::new())),
            option_greeks_cache: Arc::new(tokio::sync::Mutex::new(OptionGreeksCache::new())),
            clock,
            ib_client: None,
            last_bars: Arc::new(tokio::sync::Mutex::new(AHashMap::new())),
            bar_timeout_tasks: Arc::new(tokio::sync::Mutex::new(AHashMap::new())),
        })
    }

    #[cfg(feature = "python")]
    pub(crate) fn new_for_python(
        config: InteractiveBrokersDataClientConfig,
        instrument_provider: crate::providers::instruments::InteractiveBrokersInstrumentProvider,
    ) -> anyhow::Result<Self> {
        Self::ensure_python_event_bridge();
        let client_id = ClientId::from(format!("IB-{:03}", config.client_id));
        Self::new(client_id, config, Arc::new(instrument_provider))
    }

    #[cfg(feature = "python")]
    pub(crate) fn register_python_event_callback(&self, callback: Py<PyAny>) {
        *data_event_callback()
            .lock()
            .expect("data event callback mutex poisoned") = Some(callback);
    }

    #[cfg(feature = "python")]
    fn ensure_python_event_bridge() {
        if nautilus_common::live::runner::try_get_data_event_sender().is_some() {
            return;
        }

        DATA_EVENT_BRIDGE_INITIALIZED.with(|initialized| {
            if initialized.replace(true) {
                return;
            }

            let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
            nautilus_common::live::runner::set_data_event_sender(sender);

            get_runtime().spawn(async move {
                while let Some(event) = receiver.recv().await {
                    Python::attach(|py| {
                        let callback_guard = data_event_callback()
                            .lock()
                            .expect("data event callback mutex poisoned");

                        let Some(callback) = callback_guard.as_ref() else {
                            return;
                        };

                        if let Err(e) = dispatch_python_data_event(py, callback, event) {
                            tracing::error!("Failed to dispatch IB data event to Python: {e}");
                        }
                    });
                }
            });
        });
    }

    #[cfg(feature = "python")]
    pub(crate) fn subscribe_quotes_for_python(
        &mut self,
        instrument_id: InstrumentId,
        params: Option<std::collections::HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let cmd = SubscribeQuotes {
            instrument_id,
            client_id: Some(self.client_id()),
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.clock.get_time_ns(),
            correlation_id: None,
            params: string_hash_map_to_params(params),
        };
        DataClient::subscribe_quotes(self, cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) fn subscribe_index_prices_for_python(
        &mut self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        let cmd = SubscribeIndexPrices {
            instrument_id,
            client_id: Some(self.client_id()),
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.clock.get_time_ns(),
            correlation_id: None,
            params: None,
        };
        DataClient::subscribe_index_prices(self, cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) fn subscribe_option_greeks_for_python(
        &mut self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        let cmd = SubscribeOptionGreeks {
            instrument_id,
            client_id: Some(self.client_id()),
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.clock.get_time_ns(),
            correlation_id: None,
            params: None,
        };
        DataClient::subscribe_option_greeks(self, cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) fn subscribe_trades_for_python(
        &mut self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        let cmd = SubscribeTrades {
            instrument_id,
            client_id: Some(self.client_id()),
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.clock.get_time_ns(),
            correlation_id: None,
            params: None,
        };
        DataClient::subscribe_trades(self, cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) fn subscribe_bars_for_python(
        &mut self,
        bar_type: BarType,
        params: Option<std::collections::HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let cmd = SubscribeBars {
            bar_type,
            client_id: Some(self.client_id()),
            venue: Some(bar_type.instrument_id().venue),
            command_id: UUID4::new(),
            ts_init: self.clock.get_time_ns(),
            correlation_id: None,
            params: string_hash_map_to_params(params),
        };
        DataClient::subscribe_bars(self, cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) fn subscribe_book_deltas_for_python(
        &mut self,
        instrument_id: InstrumentId,
        depth: Option<u16>,
        params: Option<std::collections::HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let cmd = SubscribeBookDeltas {
            instrument_id,
            book_type: BookType::L2_MBP,
            client_id: Some(self.client_id()),
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.clock.get_time_ns(),
            depth: u16_to_nonzero_usize(depth.unwrap_or(20)),
            managed: true,
            correlation_id: None,
            params: string_hash_map_to_params(params),
        };
        DataClient::subscribe_book_deltas(self, cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) fn unsubscribe_quotes_for_python(
        &mut self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        let cmd = UnsubscribeQuotes {
            instrument_id,
            client_id: Some(self.client_id()),
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.clock.get_time_ns(),
            correlation_id: None,
            params: None,
        };
        DataClient::unsubscribe_quotes(self, &cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) fn unsubscribe_index_prices_for_python(
        &mut self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        let cmd = UnsubscribeIndexPrices {
            instrument_id,
            client_id: Some(self.client_id()),
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.clock.get_time_ns(),
            correlation_id: None,
            params: None,
        };
        DataClient::unsubscribe_index_prices(self, &cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) fn unsubscribe_option_greeks_for_python(
        &mut self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        let cmd = UnsubscribeOptionGreeks {
            instrument_id,
            client_id: Some(self.client_id()),
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.clock.get_time_ns(),
            correlation_id: None,
            params: None,
        };
        DataClient::unsubscribe_option_greeks(self, &cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) fn unsubscribe_trades_for_python(
        &mut self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        let cmd = UnsubscribeTrades {
            instrument_id,
            client_id: Some(self.client_id()),
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.clock.get_time_ns(),
            correlation_id: None,
            params: None,
        };
        DataClient::unsubscribe_trades(self, &cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) fn unsubscribe_bars_for_python(&mut self, bar_type: BarType) -> anyhow::Result<()> {
        let cmd = UnsubscribeBars {
            bar_type,
            client_id: Some(self.client_id()),
            venue: Some(bar_type.instrument_id().venue),
            command_id: UUID4::new(),
            ts_init: self.clock.get_time_ns(),
            correlation_id: None,
            params: None,
        };
        DataClient::unsubscribe_bars(self, &cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) fn unsubscribe_book_deltas_for_python(
        &mut self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        let cmd = UnsubscribeBookDeltas {
            instrument_id,
            client_id: Some(self.client_id()),
            venue: Some(instrument_id.venue),
            command_id: UUID4::new(),
            ts_init: self.clock.get_time_ns(),
            correlation_id: None,
            params: None,
        };
        DataClient::unsubscribe_book_deltas(self, &cmd)
    }

    #[cfg(feature = "python")]
    pub(crate) fn request_quotes_for_python(
        &self,
        instrument_id: InstrumentId,
        limit: Option<u64>,
        start: Option<u64>,
        end: Option<u64>,
        request_id: Option<String>,
    ) -> anyhow::Result<()> {
        let req = RequestQuotes {
            instrument_id,
            start: unix_nanos_to_datetime(start),
            end: unix_nanos_to_datetime(end),
            limit: u64_to_nonzero_usize(limit.unwrap_or(10_000)),
            client_id: Some(self.client_id()),
            request_id: request_id.map_or_else(UUID4::new, UUID4::from),
            ts_init: self.clock.get_time_ns(),
            params: None,
        };
        DataClient::request_quotes(self, req)
    }

    #[cfg(feature = "python")]
    pub(crate) fn request_trades_for_python(
        &self,
        instrument_id: InstrumentId,
        limit: Option<u64>,
        start: Option<u64>,
        end: Option<u64>,
        request_id: Option<String>,
    ) -> anyhow::Result<()> {
        let req = RequestTrades {
            instrument_id,
            start: unix_nanos_to_datetime(start),
            end: unix_nanos_to_datetime(end),
            limit: u64_to_nonzero_usize(limit.unwrap_or(10_000)),
            client_id: Some(self.client_id()),
            request_id: request_id.map_or_else(UUID4::new, UUID4::from),
            ts_init: self.clock.get_time_ns(),
            params: None,
        };
        DataClient::request_trades(self, req)
    }

    #[cfg(feature = "python")]
    pub(crate) fn request_bars_for_python(
        &self,
        bar_type: BarType,
        limit: Option<u64>,
        start: Option<u64>,
        end: Option<u64>,
        request_id: Option<String>,
    ) -> anyhow::Result<()> {
        let req = RequestBars {
            bar_type,
            start: unix_nanos_to_datetime(start),
            end: unix_nanos_to_datetime(end),
            limit: u64_to_nonzero_usize(limit.unwrap_or(1_000)),
            client_id: Some(self.client_id()),
            request_id: request_id.map_or_else(UUID4::new, UUID4::from),
            ts_init: self.clock.get_time_ns(),
            params: None,
        };
        DataClient::request_bars(self, req)
    }

    #[cfg(feature = "python")]
    pub(crate) fn request_instrument_for_python(
        &self,
        instrument_id: InstrumentId,
        params: Option<std::collections::HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let req = RequestInstrument {
            client_id: Some(self.client_id()),
            instrument_id,
            start: None,
            end: None,
            request_id: UUID4::new(),
            ts_init: self.clock.get_time_ns(),
            params: string_hash_map_to_params(params),
        };
        DataClient::request_instrument(self, req)
    }

    #[cfg(feature = "python")]
    pub(crate) fn request_instruments_for_python(
        &self,
        venue: Option<Venue>,
        params: Option<std::collections::HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let req = RequestInstruments {
            client_id: Some(self.client_id()),
            venue,
            start: None,
            end: None,
            request_id: UUID4::new(),
            ts_init: self.clock.get_time_ns(),
            params: string_hash_map_to_params(params),
        };
        DataClient::request_instruments(self, req)
    }

    fn venue_id(&self) -> Venue {
        *IB_VENUE
    }

    /// Get a reference to the IB client if connected.
    /// This is used internally for provider method calls.
    #[allow(dead_code)] // Library API - may be used by other modules or PyO3 bindings
    pub(crate) fn get_ib_client(&self) -> Option<&Arc<ibapi::Client>> {
        self.ib_client.as_ref().map(|h| h.as_arc())
    }

    /// Get a reference to the instrument provider.
    #[allow(dead_code)] // Library API - may be used by other modules or PyO3 bindings
    pub(crate) fn instrument_provider(&self) -> Arc<InteractiveBrokersInstrumentProvider> {
        Arc::clone(&self.instrument_provider)
    }

    /// Batch load multiple instrument IDs using the internal IB client.
    ///
    /// This method calls the provider's batch_load with the data client's IB client.
    ///
    /// # Arguments
    ///
    /// * `instrument_ids` - Vector of instrument IDs to load
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not connected
    /// - The provider batch_load fails
    pub async fn batch_load_instruments(
        &self,
        instrument_ids: Vec<InstrumentId>,
    ) -> anyhow::Result<Vec<InstrumentId>> {
        log::debug!(
            "Batch loading {} IB instruments through data client",
            instrument_ids.len()
        );
        let client = self
            .ib_client
            .as_ref()
            .context("IB client not connected. Call connect() first")?;

        let loaded = self
            .instrument_provider
            .batch_load(client, instrument_ids, None)
            .await?;
        log::debug!("Batch loaded {} IB instruments", loaded.len());
        Ok(loaded)
    }

    /// Fetch option chain for an underlying contract with expiry filtering.
    ///
    /// This method calls the provider's fetch_option_chain_by_range with the data client's IB client.
    ///
    /// # Arguments
    ///
    /// * `underlying_symbol` - The underlying symbol (e.g., "AAPL")
    /// * `exchange` - The exchange (defaults to "SMART")
    /// * `currency` - The currency (defaults to "USD")
    /// * `expiry_min` - Minimum expiry date string (YYYYMMDD format, optional)
    /// * `expiry_max` - Maximum expiry date string (YYYYMMDD format, optional)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not connected
    /// - The provider method fails
    pub async fn fetch_option_chain_by_range(
        &self,
        underlying_symbol: &str,
        exchange: Option<&str>,
        currency: Option<&str>,
        expiry_min: Option<&str>,
        expiry_max: Option<&str>,
    ) -> anyhow::Result<usize> {
        log::debug!(
            "Fetching IB option chain by range (symbol={}, exchange={:?}, currency={:?}, expiry_min={:?}, expiry_max={:?})",
            underlying_symbol,
            exchange,
            currency,
            expiry_min,
            expiry_max
        );
        let client = self
            .ib_client
            .as_ref()
            .context("IB client not connected. Call connect() first")?;

        let underlying = Contract {
            contract_id: 0,
            symbol: Symbol::from(underlying_symbol.to_string()),
            security_type: SecurityType::Stock,
            last_trade_date_or_contract_month: String::new(),
            strike: 0.0,
            right: String::new(),
            multiplier: String::new(),
            exchange: IBExchange::from(exchange.unwrap_or("SMART")),
            currency: IBCurrency::from(currency.unwrap_or("USD")),
            local_symbol: String::new(),
            primary_exchange: IBExchange::default(),
            trading_class: String::new(),
            include_expired: false,
            security_id_type: String::new(),
            security_id: String::new(),
            combo_legs_description: String::new(),
            combo_legs: Vec::new(),
            delta_neutral_contract: None,
            issuer_id: String::new(),
            description: String::new(),
            last_trade_date: None,
        };

        let count = self
            .instrument_provider
            .fetch_option_chain_by_range(client, &underlying, expiry_min, expiry_max)
            .await?;
        log::debug!(
            "Fetched {} IB option instruments for {}",
            count,
            underlying_symbol
        );
        Ok(count)
    }

    /// Fetch futures chain for a given underlying symbol.
    ///
    /// This method calls the provider's fetch_futures_chain with the data client's IB client.
    ///
    /// # Arguments
    ///
    /// * `symbol` - The underlying symbol (e.g., "ES")
    /// * `exchange` - The exchange (defaults to empty string for all exchanges)
    /// * `currency` - The currency (defaults to "USD")
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not connected
    /// - The provider method fails
    pub async fn fetch_futures_chain(
        &self,
        symbol: &str,
        exchange: Option<&str>,
        currency: Option<&str>,
        min_expiry_days: Option<u32>,
        max_expiry_days: Option<u32>,
    ) -> anyhow::Result<usize> {
        log::debug!(
            "Fetching IB futures chain (symbol={}, exchange={:?}, currency={:?}, min_days={:?}, max_days={:?})",
            symbol,
            exchange,
            currency,
            min_expiry_days,
            max_expiry_days
        );
        let client = self
            .ib_client
            .as_ref()
            .context("IB client not connected. Call connect() first")?;

        let count = self
            .instrument_provider
            .fetch_futures_chain(
                client,
                symbol,
                exchange.unwrap_or(""),
                currency.unwrap_or("USD"),
                min_expiry_days,
                max_expiry_days,
            )
            .await?;
        log::debug!("Fetched {} IB futures instruments for {}", count, symbol);
        Ok(count)
    }

    /// Fetch BAG (spread) contract details.
    ///
    /// This method calls the provider's fetch_bag_contract with the data client's IB client.
    ///
    /// # Arguments
    ///
    /// * `bag_contract` - The BAG contract with populated combo_legs
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not connected
    /// - The provider method fails
    pub async fn fetch_bag_contract(
        &self,
        bag_contract: &ibapi::contracts::Contract,
    ) -> anyhow::Result<usize> {
        log::debug!(
            "Fetching IB BAG contract details (contract_id={}, exchange={}, symbol={})",
            bag_contract.contract_id,
            bag_contract.exchange.as_str(),
            bag_contract.symbol.as_str()
        );
        let client = self
            .ib_client
            .as_ref()
            .context("IB client not connected. Call connect() first")?;

        let count = self
            .instrument_provider
            .fetch_bag_contract(client, bag_contract)
            .await?;
        log::debug!("Fetched {} BAG instruments", count);
        Ok(count)
    }
}

impl Debug for InteractiveBrokersDataClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(InteractiveBrokersDataClient))
            .field("client_id", &self.client_id)
            .field("config", &self.config)
            .field("is_connected", &self.is_connected.load(Ordering::Relaxed))
            .field("has_ib_client", &self.ib_client.is_some())
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for InteractiveBrokersDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(self.venue_id())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            client_id = %self.client_id,
            "Starting Interactive Brokers data client"
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            "Stopping Interactive Brokers data client {id}",
            id = self.client_id
        );
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);

        // Cancel all tasks
        for task in &self.tasks {
            task.abort();
        }
        self.tasks.clear();

        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        tracing::debug!(
            "Resetting Interactive Brokers data client {id}",
            id = self.client_id
        );
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        self.tasks.clear();

        // Clear subscriptions and cache
        {
            let mut subscriptions = self.subscriptions.blocking_lock();
            subscriptions.clear();
        }
        {
            let mut subscriptions = self.option_greeks_subscriptions.blocking_lock();
            subscriptions.clear();
        }
        {
            let mut cache = self.quote_cache.blocking_lock();
            cache.clear();
        }
        {
            let mut cache = self.option_greeks_cache.blocking_lock();
            cache.clear();
        }

        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.stop()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        tracing::debug!("Connecting Interactive Brokers data client...");

        let handle = crate::common::shared_client::get_or_connect(
            &self.config.host,
            self.config.port,
            self.config.client_id,
            self.config.connection_timeout,
        )
        .await
        .context("Failed to connect to IB Gateway/TWS")?;

        let client = handle.as_arc();

        tracing::info!(
            "Connected to IB Gateway/TWS at {}:{} (client_id: {})",
            self.config.host,
            self.config.port,
            self.config.client_id
        );

        // Set market data type if not default
        if self.config.market_data_type != crate::config::MarketDataType::Realtime {
            let ib_data_type: ibapi::market_data::MarketDataType =
                self.config.market_data_type.into();
            client
                .switch_market_data_type(ib_data_type)
                .await
                .context("Failed to switch market data type")?;
            tracing::info!("Set market data type to {:?}", self.config.market_data_type);
        }

        self.ib_client = Some(handle);
        self.is_connected.store(true, Ordering::Relaxed);

        // Initialize provider and load instruments from cache if configured
        tracing::debug!("Initializing IB data instrument provider");
        if let Err(e) = self.instrument_provider.initialize().await {
            tracing::warn!("Failed to initialize instrument provider: {}", e);
        }

        tracing::debug!("Loading configured IB data instruments");

        if let Err(e) = self
            .instrument_provider
            .load_all_async(
                self.ib_client.as_ref().unwrap().as_arc().as_ref(),
                None,
                None,
                false,
            )
            .await
        {
            tracing::warn!("Failed to load instruments on startup: {}", e);
        }

        let instrument_count = self.instrument_provider.count();
        if instrument_count > 0 {
            tracing::info!(
                "Data client connected with {} instruments in provider cache",
                instrument_count
            );
        }

        tracing::info!("Connected Interactive Brokers data client");
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        tracing::debug!("Disconnecting Interactive Brokers data client...");

        self.stop()?;
        self.ib_client = None;
        self.is_connected.store(false, Ordering::Relaxed);
        tracing::info!("Disconnected Interactive Brokers data client");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    // Subscription handlers
    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        tracing::debug!("Subscribing to quotes for {}", cmd.instrument_id);

        let client = self
            .ib_client
            .as_ref()
            .context("IB client not connected. Call connect() first")?;

        // Get instrument from provider
        let instrument = self
            .instrument_provider
            .find(&cmd.instrument_id)
            .context(format!(
                "Instrument {} not found in provider",
                cmd.instrument_id
            ))?;

        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        // Convert instrument_id to IB contract
        let contract = self
            .instrument_provider
            .resolve_contract_for_instrument(cmd.instrument_id)
            .context("Failed to convert instrument_id to IB contract")?;

        // Check if contract is BAG (spread) or if batch_quotes parameter is set
        // BAG contracts have SecurityType::Spread or combo_legs populated
        let is_bag = matches!(
            contract.security_type,
            ibapi::contracts::SecurityType::Spread
        ) || !contract.combo_legs.is_empty();
        let batch_quotes = cmd
            .params
            .as_ref()
            .and_then(|params| params.get_str("batch_quotes"))
            .map_or(self.config.batch_quotes, |s| {
                s == "true" || s == "True" || s == "1"
            });

        let use_market_data = is_bag || batch_quotes;

        let instrument_id = cmd.instrument_id;
        let data_sender = self.data_sender.clone();
        let quote_cache = Arc::clone(&self.quote_cache);
        let clock = self.clock;

        // Get price magnifier from instrument provider
        let price_magnifier = self.instrument_provider.get_price_magnifier(&instrument_id) as f64;

        // Create subscription-specific cancellation token
        let subscription_token = CancellationToken::new();

        // Spawn subscription task
        let client_clone = client.as_arc().clone();
        let subscription_token_clone = subscription_token.clone();
        let ignore_size_updates = self.config.ignore_quote_tick_size_updates;

        let task = get_runtime().spawn(async move {
            if use_market_data {
                // Use market_data (reqMktData) for BAG contracts or when batch_quotes is requested
                tracing::debug!(
                    "Using market_data subscription for {} (BAG: {}, batch_quotes: {})",
                    instrument_id,
                    is_bag,
                    batch_quotes
                );

                if let Err(e) = handle_quote_subscription(
                    client_clone,
                    contract,
                    instrument_id,
                    price_precision,
                    size_precision,
                    data_sender,
                    quote_cache,
                    clock,
                    subscription_token_clone,
                    ignore_size_updates,
                )
                .await
                {
                    tracing::error!("Quote subscription error for {}: {:?}", instrument_id, e);
                }
            } else {
                // Try tick_by_tick_bid_ask first for regular contracts (better performance)
                // Fallback to market_data if it fails (e.g., for BAG contracts not detected upfront)
                tracing::debug!(
                    "Attempting tick_by_tick_bid_ask subscription for {}",
                    instrument_id
                );

                match handle_tick_by_tick_quote_subscription(
                    client_clone.clone(),
                    contract.clone(),
                    instrument_id,
                    price_precision,
                    size_precision,
                    data_sender.clone(),
                    clock,
                    subscription_token_clone.clone(),
                    price_magnifier,
                )
                .await
                {
                    Ok(()) => {
                        // Success - subscription is active
                    }
                    Err(e) => {
                        tracing::warn!(
                            "tick_by_tick_bid_ask failed for {} (may be BAG contract), falling back to market_data: {:?}",
                            instrument_id,
                            e
                        );
                        // Fallback to market_data (reqMktData) - works for BAG contracts
                        if let Err(fallback_err) = handle_quote_subscription(
                            client_clone,
                            contract,
                            instrument_id,
                            price_precision,
                            size_precision,
                            data_sender,
                            quote_cache,
                            clock,
                            subscription_token_clone,
                            ignore_size_updates,
                        )
                        .await
                        {
                            tracing::error!(
                                "Quote subscription fallback also failed for {}: {:?}",
                                instrument_id,
                                fallback_err
                            );
                        } else {
                            tracing::info!(
                                "Successfully subscribed to {} using market_data fallback",
                                instrument_id
                            );
                        }
                    }
                }
            }
        });

        self.tasks.push(task);

        // Record subscription
        let mut subscriptions = self.subscriptions.blocking_lock();
        subscriptions.insert(
            cmd.instrument_id,
            SubscriptionInfo {
                instrument_id: cmd.instrument_id,
                subscription_type: SubscriptionType::Quotes,
                cancellation_token: subscription_token,
            },
        );

        tracing::info!(
            "Quote subscription started for {} (method: {})",
            cmd.instrument_id,
            if use_market_data {
                "market_data"
            } else {
                "tick_by_tick_bid_ask"
            }
        );
        Ok(())
    }

    fn subscribe_index_prices(&mut self, cmd: SubscribeIndexPrices) -> anyhow::Result<()> {
        tracing::debug!("Subscribing to index prices for {}", cmd.instrument_id);

        let client = self
            .ib_client
            .as_ref()
            .context("IB client not connected. Call connect() first")?;

        let instrument = self
            .instrument_provider
            .find(&cmd.instrument_id)
            .context(format!(
                "Instrument {} not found in provider",
                cmd.instrument_id
            ))?;

        let contract = self
            .instrument_provider
            .resolve_contract_for_instrument(cmd.instrument_id)
            .context("Failed to convert instrument_id to IB contract")?;

        if !matches!(contract.security_type, SecurityType::Index) {
            tracing::warn!(
                "Index price subscription not supported for security type {:?} on {}",
                contract.security_type,
                cmd.instrument_id
            );
            return Ok(());
        }

        let price_precision = instrument.price_precision();
        let price_magnifier = self
            .instrument_provider
            .get_price_magnifier(&cmd.instrument_id);
        let instrument_id = cmd.instrument_id;
        let data_sender = self.data_sender.clone();
        let clock = self.clock;

        let subscription_token = CancellationToken::new();

        let client_clone = client.as_arc().clone();
        let subscription_token_clone = subscription_token.clone();

        let task = get_runtime().spawn(async move {
            if let Err(e) = handle_index_price_subscription(
                client_clone,
                contract,
                instrument_id,
                price_precision,
                price_magnifier,
                data_sender,
                clock,
                subscription_token_clone,
            )
            .await
            {
                tracing::error!(
                    "Index price subscription error for {}: {:?}",
                    instrument_id,
                    e
                );
            }
        });

        self.tasks.push(task);

        let mut subscriptions = self.subscriptions.blocking_lock();
        subscriptions.insert(
            cmd.instrument_id,
            SubscriptionInfo {
                instrument_id: cmd.instrument_id,
                subscription_type: SubscriptionType::IndexPrices,
                cancellation_token: subscription_token,
            },
        );

        tracing::info!("Index price subscription started for {}", cmd.instrument_id);
        Ok(())
    }

    fn subscribe_option_greeks(&mut self, cmd: SubscribeOptionGreeks) -> anyhow::Result<()> {
        tracing::debug!("Subscribing to option greeks for {}", cmd.instrument_id);

        let client = self
            .ib_client
            .as_ref()
            .context("IB client not connected. Call connect() first")?;

        let instrument = self
            .instrument_provider
            .find(&cmd.instrument_id)
            .context(format!(
                "Instrument {} not found in provider",
                cmd.instrument_id
            ))?;

        if !matches!(
            instrument,
            InstrumentAny::OptionContract(_)
                | InstrumentAny::FuturesContract(_)
                | InstrumentAny::CryptoOption(_)
        ) && !matches!(
            self.instrument_provider
                .resolve_contract_for_instrument(cmd.instrument_id)?
                .security_type,
            SecurityType::Option | SecurityType::FuturesOption
        ) {
            tracing::warn!(
                "Option greeks subscription is only supported for option instruments: {}",
                cmd.instrument_id
            );
            return Ok(());
        }

        let contract = self
            .instrument_provider
            .resolve_contract_for_instrument(cmd.instrument_id)
            .context("Failed to convert instrument_id to IB contract")?;

        let instrument_id = cmd.instrument_id;
        let data_sender = self.data_sender.clone();
        let option_greeks_cache = Arc::clone(&self.option_greeks_cache);
        let clock = self.clock;
        let subscription_token = CancellationToken::new();
        let subscription_token_clone = subscription_token.clone();
        let client_clone = client.as_arc().clone();

        let task = get_runtime().spawn(async move {
            if let Err(e) = handle_option_greeks_subscription(
                client_clone,
                contract,
                instrument_id,
                data_sender,
                option_greeks_cache,
                clock,
                subscription_token_clone,
            )
            .await
            {
                tracing::error!(
                    "Option greeks subscription error for {}: {:?}",
                    instrument_id,
                    e
                );
            }
        });

        self.tasks.push(task);

        let mut subscriptions = self.option_greeks_subscriptions.blocking_lock();
        if let Some(existing) = subscriptions.insert(cmd.instrument_id, subscription_token) {
            existing.cancel();
        }

        tracing::info!(
            "Option greeks subscription started for {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        tracing::debug!("Unsubscribing from quotes for {}", cmd.instrument_id);

        let mut subscriptions = self.subscriptions.blocking_lock();
        if let Some(sub_info) = subscriptions.remove(&cmd.instrument_id) {
            sub_info.cancellation_token.cancel();
            tracing::info!("Unsubscribed from quotes for {}", cmd.instrument_id);
        } else {
            tracing::warn!(
                "No active quote subscription found for {}",
                cmd.instrument_id
            );
        }

        // Clear quote cache for this instrument
        {
            // Quote cache doesn't have per-instrument clear, but we can clear all
            // In practice, the cache will naturally expire as new quotes arrive
        }

        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        tracing::debug!("Unsubscribing from index prices for {}", cmd.instrument_id);

        let mut subscriptions = self.subscriptions.blocking_lock();
        if let Some(sub_info) = subscriptions.remove(&cmd.instrument_id) {
            sub_info.cancellation_token.cancel();
            tracing::info!("Unsubscribed from index prices for {}", cmd.instrument_id);
        } else {
            tracing::warn!(
                "No active index price subscription found for {}",
                cmd.instrument_id
            );
        }

        Ok(())
    }

    fn unsubscribe_option_greeks(&mut self, cmd: &UnsubscribeOptionGreeks) -> anyhow::Result<()> {
        tracing::debug!("Unsubscribing from option greeks for {}", cmd.instrument_id);

        let mut subscriptions = self.option_greeks_subscriptions.blocking_lock();
        if let Some(subscription_token) = subscriptions.remove(&cmd.instrument_id) {
            subscription_token.cancel();
            tracing::info!("Unsubscribed from option greeks for {}", cmd.instrument_id);
        } else {
            tracing::warn!(
                "No active option greeks subscription found for {}",
                cmd.instrument_id
            );
        }

        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        tracing::debug!("Subscribing to trades for {}", cmd.instrument_id);

        let client = self
            .ib_client
            .as_ref()
            .context("IB client not connected. Call connect() first")?;

        // Get instrument from provider
        let instrument = self
            .instrument_provider
            .find(&cmd.instrument_id)
            .context(format!(
                "Instrument {} not found in provider",
                cmd.instrument_id
            ))?;

        // Check if instrument is a CurrencyPair (IB doesn't support trades for CurrencyPair)
        if matches!(instrument, InstrumentAny::CurrencyPair(_)) {
            tracing::error!(
                "Interactive Brokers does not support trades for CurrencyPair instruments: {}",
                cmd.instrument_id
            );
            return Ok(());
        }

        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        // Convert instrument_id to IB contract
        let contract = self
            .instrument_provider
            .resolve_contract_for_instrument(cmd.instrument_id)
            .context("Failed to convert instrument_id to IB contract")?;

        let instrument_id = cmd.instrument_id;
        let data_sender = self.data_sender.clone();
        let clock = self.clock;

        // Create subscription-specific cancellation token
        let subscription_token = CancellationToken::new();

        // Spawn subscription task
        let client_clone = client.as_arc().clone();
        let subscription_token_clone = subscription_token.clone();

        let task = get_runtime().spawn(async move {
            if let Err(e) = handle_trade_subscription(
                client_clone,
                contract,
                instrument_id,
                price_precision,
                size_precision,
                data_sender,
                clock,
                subscription_token_clone,
            )
            .await
            {
                tracing::error!("Trade subscription error for {}: {:?}", instrument_id, e);
            }
        });

        self.tasks.push(task);

        // Record subscription
        let mut subscriptions = self.subscriptions.blocking_lock();
        subscriptions.insert(
            cmd.instrument_id,
            SubscriptionInfo {
                instrument_id: cmd.instrument_id,
                subscription_type: SubscriptionType::Trades,
                cancellation_token: subscription_token,
            },
        );

        tracing::info!("Trade subscription started for {}", cmd.instrument_id);
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        tracing::debug!("Unsubscribing from trades for {}", cmd.instrument_id);

        let mut subscriptions = self.subscriptions.blocking_lock();
        if let Some(sub_info) = subscriptions.remove(&cmd.instrument_id) {
            sub_info.cancellation_token.cancel();
            tracing::info!("Unsubscribed from trades for {}", cmd.instrument_id);
        } else {
            tracing::warn!(
                "No active trade subscription found for {}",
                cmd.instrument_id
            );
        }

        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: SubscribeBars) -> anyhow::Result<()> {
        tracing::debug!("Subscribing to bars for {}", cmd.bar_type);

        let client = self
            .ib_client
            .as_ref()
            .context("IB client not connected. Call connect() first")?;

        // Get instrument from provider
        let instrument_id = cmd.bar_type.instrument_id();
        let instrument = self
            .instrument_provider
            .find(&instrument_id)
            .context(format!("Instrument {instrument_id} not found in provider"))?;

        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        // Convert instrument_id to IB contract
        let contract = self
            .instrument_provider
            .resolve_contract_for_instrument(instrument_id)
            .context("Failed to convert instrument_id to IB contract")?;

        let bar_type = cmd.bar_type;
        let bar_type_str = bar_type.to_string();
        let data_sender = self.data_sender.clone();
        let clock = self.clock;
        let last_bars = Arc::clone(&self.last_bars);
        let bar_timeout_tasks = Arc::clone(&self.bar_timeout_tasks);
        let handle_revised_bars = self.config.handle_revised_bars;
        let use_rth = self.config.use_regular_trading_hours;
        let start_ns = parse_start_ns(cmd.params.as_ref());

        // Create subscription-specific cancellation token
        let subscription_token = CancellationToken::new();

        // Spawn subscription task
        let client_clone = client.as_arc().clone();
        let subscription_token_clone = subscription_token.clone();

        let task = get_runtime().spawn(async move {
            let result = if bar_type.spec().timedelta().num_seconds() == 5 {
                handle_realtime_bars_subscription(
                    client_clone,
                    contract,
                    bar_type,
                    bar_type_str,
                    instrument_id,
                    price_precision,
                    size_precision,
                    data_sender,
                    clock,
                    last_bars,
                    bar_timeout_tasks,
                    handle_revised_bars,
                    subscription_token_clone,
                )
                .await
            } else {
                handle_historical_bars_subscription(
                    client_clone,
                    contract,
                    bar_type,
                    price_type_to_ib_what_to_show(bar_type.spec().price_type),
                    price_precision,
                    size_precision,
                    use_rth,
                    start_ns,
                    data_sender,
                    handle_revised_bars,
                    clock,
                    subscription_token_clone,
                )
                .await
            };

            if let Err(e) = result {
                tracing::error!("Bars subscription error for {}: {:?}", bar_type, e);
            }
        });

        self.tasks.push(task);

        // Record subscription
        let mut subscriptions = self.subscriptions.blocking_lock();
        subscriptions.insert(
            instrument_id,
            SubscriptionInfo {
                instrument_id,
                subscription_type: SubscriptionType::Bars,
                cancellation_token: subscription_token,
            },
        );

        tracing::info!("Real-time bars subscription started for {}", bar_type);
        Ok(())
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        tracing::debug!("Unsubscribing from bars for {}", cmd.bar_type);

        let instrument_id = cmd.bar_type.instrument_id();
        let mut subscriptions = self.subscriptions.blocking_lock();
        if let Some(sub_info) = subscriptions.remove(&instrument_id) {
            sub_info.cancellation_token.cancel();
            tracing::info!("Unsubscribed from bars for {}", cmd.bar_type);
        } else {
            tracing::warn!("No active bar subscription found for {}", cmd.bar_type);
        }

        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        tracing::debug!("Subscribing to book deltas for {}", cmd.instrument_id);

        // Validate book type (IB doesn't support L3_MBO)
        if cmd.book_type == BookType::L3_MBO {
            tracing::error!(
                "Cannot subscribe to order book deltas: L3_MBO data is not published by Interactive Brokers. Valid book types are L1_MBP, L2_MBP"
            );
            return Ok(());
        }

        let client = self
            .ib_client
            .as_ref()
            .context("IB client not connected. Call connect() first")?;

        // Get instrument from provider
        let instrument = self
            .instrument_provider
            .find(&cmd.instrument_id)
            .context(format!(
                "Instrument {} not found in provider",
                cmd.instrument_id
            ))?;

        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        // Convert instrument_id to IB contract
        let contract = self
            .instrument_provider
            .resolve_contract_for_instrument(cmd.instrument_id)
            .context("Failed to convert instrument_id to IB contract")?;

        let instrument_id = cmd.instrument_id;
        let data_sender = self.data_sender.clone();
        let clock = self.clock;

        // Create subscription-specific cancellation token
        let subscription_token = CancellationToken::new();

        // Get depth from command or default to 20 (Python default)
        let depth_rows = cmd.depth.map_or(20, |d| d.get() as i32);

        // Get is_smart_depth from params or default to true
        let is_smart_depth = cmd
            .params
            .as_ref()
            .and_then(|params| params.get_str("is_smart_depth"))
            .is_none_or(|s| s == "true" || s == "True" || s == "1");

        // Spawn subscription task
        let client_clone = client.as_arc().clone();
        let subscription_token_clone = subscription_token.clone();

        let task = get_runtime().spawn(async move {
            if let Err(e) = handle_market_depth_subscription(
                client_clone,
                contract,
                instrument_id,
                price_precision,
                size_precision,
                depth_rows,
                is_smart_depth,
                data_sender,
                clock,
                subscription_token_clone,
            )
            .await
            {
                tracing::error!(
                    "Market depth subscription error for {}: {:?}",
                    instrument_id,
                    e
                );
            }
        });

        self.tasks.push(task);

        // Record subscription
        let mut subscriptions = self.subscriptions.blocking_lock();
        subscriptions.insert(
            cmd.instrument_id,
            SubscriptionInfo {
                instrument_id: cmd.instrument_id,
                subscription_type: SubscriptionType::BookDeltas,
                cancellation_token: subscription_token,
            },
        );

        tracing::info!(
            "Market depth subscription started for {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        tracing::debug!("Unsubscribing from book deltas for {}", cmd.instrument_id);

        let mut subscriptions = self.subscriptions.blocking_lock();
        if let Some(sub_info) = subscriptions.remove(&cmd.instrument_id) {
            sub_info.cancellation_token.cancel();
            tracing::info!("Unsubscribed from book deltas for {}", cmd.instrument_id);
        } else {
            tracing::warn!(
                "No active book delta subscription found for {}",
                cmd.instrument_id
            );
        }

        Ok(())
    }

    // Request handlers
    fn request_instrument(&self, cmd: RequestInstrument) -> anyhow::Result<()> {
        tracing::debug!("Requesting instrument: {}", cmd.instrument_id);

        // Check if force_instrument_update is requested
        let force_update = cmd
            .params
            .as_ref()
            .and_then(|params| params.get_str("force_instrument_update"))
            .is_some_and(|s| s == "true" || s == "True" || s == "1");

        // Get instrument from provider (or load if not found or force_update)
        let instrument =
            if force_update || self.instrument_provider.find(&cmd.instrument_id).is_none() {
                // Need to load instrument - spawn async task
                let client = self
                    .ib_client
                    .as_ref()
                    .context("IB client not connected. Call connect() first")?;
                let instrument_provider = Arc::clone(&self.instrument_provider);
                let instrument_id = cmd.instrument_id;
                let data_sender = self.data_sender.clone();
                let clock = self.clock;
                let request_id = cmd.request_id;
                let client_id = cmd.client_id.unwrap_or(self.client_id);
                let params = cmd.params.clone();
                let start_nanos = cmd.start.map(|dt| {
                    UnixNanos::from(
                        dt.timestamp_nanos_opt()
                            .unwrap_or_else(|| dt.timestamp() * 1_000_000_000)
                            as u64,
                    )
                });
                let end_nanos = cmd.end.map(|dt| {
                    UnixNanos::from(
                        dt.timestamp_nanos_opt()
                            .unwrap_or_else(|| dt.timestamp() * 1_000_000_000)
                            as u64,
                    )
                });

                let client_clone = client.as_arc().clone();

                get_runtime().spawn(async move {
                    if let Err(e) = instrument_provider
                        .fetch_contract_details(&client_clone, instrument_id, false, None)
                        .await
                    {
                        tracing::error!(
                            "Failed to fetch contract details for {}: {:?}",
                            instrument_id,
                            e
                        );
                        return;
                    }

                    if let Some(instrument) = instrument_provider.find(&instrument_id) {
                        let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                            request_id,
                            client_id,
                            instrument_id,
                            instrument,
                            start_nanos,
                            end_nanos,
                            clock.get_time_ns(),
                            params,
                        )));

                        if let Err(e) = data_sender.send(DataEvent::Response(response)) {
                            tracing::error!("Failed to send instrument response: {e}");
                        }
                    }
                });

                // Return early, response will be sent async
                return Ok(());
            } else {
                // Instrument already in provider
                self.instrument_provider
                    .find(&cmd.instrument_id)
                    .context(format!(
                        "Instrument {} not found in provider",
                        cmd.instrument_id
                    ))?
            };

        let start_nanos = cmd.start.map(|dt| {
            UnixNanos::from(
                dt.timestamp_nanos_opt()
                    .unwrap_or_else(|| dt.timestamp() * 1_000_000_000) as u64,
            )
        });
        let end_nanos = cmd.end.map(|dt| {
            UnixNanos::from(
                dt.timestamp_nanos_opt()
                    .unwrap_or_else(|| dt.timestamp() * 1_000_000_000) as u64,
            )
        });

        let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
            cmd.request_id,
            cmd.client_id.unwrap_or(self.client_id),
            cmd.instrument_id,
            instrument,
            start_nanos,
            end_nanos,
            self.clock.get_time_ns(),
            cmd.params,
        )));

        if let Err(e) = self.data_sender.send(DataEvent::Response(response)) {
            tracing::error!("Failed to send instrument response: {e}");
        }

        Ok(())
    }

    fn request_instruments(&self, cmd: RequestInstruments) -> anyhow::Result<()> {
        tracing::debug!("Requesting all instruments for venue: {:?}", cmd.venue);

        let client = self
            .ib_client
            .as_ref()
            .context("IB client not connected. Call connect() first")?;

        // Check for force_instrument_update
        let force_update = cmd
            .params
            .as_ref()
            .and_then(|params| params.get_str("force_instrument_update"))
            .is_some_and(|s| s == "true" || s == "True" || s == "1");

        // Check if ib_contracts parameter is provided for batch loading
        let mut contracts_to_load: Vec<ibapi::contracts::Contract> = Vec::new();

        if let Some(params) = &cmd.params
            && let Some(ib_contracts_json_str) = params.get_str("ib_contracts")
        {
            // Parse JSON string containing array of contracts
            match crate::common::contracts::parse_contracts_from_json_array(ib_contracts_json_str) {
                Ok(contracts) => {
                    tracing::info!(
                        "Parsed {} contracts from ib_contracts JSON",
                        contracts.len()
                    );
                    log::debug!("Parsed ib_contracts payload: {}", ib_contracts_json_str);
                    contracts_to_load = contracts;
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse ib_contracts JSON: {}. Continuing without contracts",
                        e
                    );
                }
            }
        }

        // If force_update is requested or we need to batch load, spawn async task
        let instrument_provider = Arc::clone(&self.instrument_provider);
        let client_clone = client.as_arc().clone();
        let data_sender = self.data_sender.clone();
        let clock = self.clock;
        let request_id = cmd.request_id;
        let client_id = cmd.client_id.unwrap_or(self.client_id);
        let venue = cmd.venue.unwrap_or(*IB_VENUE);
        let params = cmd.params.clone();
        let start_nanos = cmd.start.map(|dt| {
            UnixNanos::from(
                dt.timestamp_nanos_opt()
                    .unwrap_or_else(|| dt.timestamp() * 1_000_000_000) as u64,
            )
        });
        let end_nanos = cmd.end.map(|dt| {
            UnixNanos::from(
                dt.timestamp_nanos_opt()
                    .unwrap_or_else(|| dt.timestamp() * 1_000_000_000) as u64,
            )
        });

        // Handle batch loading if contracts are provided or force_update is requested
        if !contracts_to_load.is_empty() || force_update {
            let contracts_to_load_clone = contracts_to_load;

            get_runtime().spawn(async move {
                let mut loaded_instrument_ids = Vec::new();

                // Load instruments from contracts if provided
                if !contracts_to_load_clone.is_empty() {
                    for contract in contracts_to_load_clone {
                        log::debug!(
                            "Loading instrument from IB contract spec (sec_type={:?}, symbol={}, local_symbol={}, exchange={}, expiry={})",
                            contract.security_type,
                            contract.symbol.as_str(),
                            contract.local_symbol.as_str(),
                            contract.exchange.as_str(),
                            contract.last_trade_date_or_contract_month.as_str()
                        );
                        // Convert contract to instrument ID and load
                        if let Ok(instrument_id) =
                            crate::common::parse::ib_contract_to_instrument_id_simple(&contract)
                        {
                            if instrument_provider.find(&instrument_id).is_none() {
                                if let Err(e) = instrument_provider
                                    .fetch_contract_details(
                                        &client_clone,
                                        instrument_id,
                                        false,
                                        None,
                                    )
                                    .await
                                {
                                    tracing::warn!(
                                        "Failed to load contract for {}: {}",
                                        instrument_id,
                                        e
                                    );
                                } else {
                                    loaded_instrument_ids.push(instrument_id);
                                }
                            } else {
                                loaded_instrument_ids.push(instrument_id);
                            }
                        }
                    }
                }

                // If force_update, also reload all existing instruments
                if force_update {
                    let all_instrument_ids: Vec<InstrumentId> = instrument_provider
                        .get_all()
                        .into_iter()
                        .map(|inst| inst.id())
                        .collect();

                    if !all_instrument_ids.is_empty()
                        && let Ok(mut reloaded_ids) = instrument_provider
                            .batch_load(&client_clone, all_instrument_ids, None)
                            .await
                    {
                        loaded_instrument_ids.append(&mut reloaded_ids);
                    }
                }

                // Get all instruments from provider after loading
                let instruments = instrument_provider.get_all();

                let response = DataResponse::Instruments(InstrumentsResponse::new(
                    request_id,
                    client_id,
                    venue,
                    instruments,
                    start_nanos,
                    end_nanos,
                    clock.get_time_ns(),
                    params,
                ));

                if let Err(e) = data_sender.send(DataEvent::Response(response)) {
                    tracing::error!("Failed to send instruments response: {e}");
                } else {
                    tracing::info!(
                        "Successfully sent {} instruments response (loaded {} new instruments)",
                        instrument_provider.count(),
                        loaded_instrument_ids.len()
                    );
                }
            });
        } else {
            // Get all instruments from provider (no loading needed)
            let instruments = self.instrument_provider.get_all();

            let response = DataResponse::Instruments(InstrumentsResponse::new(
                cmd.request_id,
                cmd.client_id.unwrap_or(self.client_id),
                venue,
                instruments,
                start_nanos,
                end_nanos,
                self.clock.get_time_ns(),
                cmd.params,
            ));

            if let Err(e) = self.data_sender.send(DataEvent::Response(response)) {
                tracing::error!("Failed to send instruments response: {e}");
            } else {
                tracing::info!(
                    "Successfully sent {} instruments response",
                    self.instrument_provider.count()
                );
            }
        }

        Ok(())
    }

    fn request_quotes(&self, cmd: RequestQuotes) -> anyhow::Result<()> {
        tracing::debug!("Requesting quotes for {}", cmd.instrument_id);

        let client = self
            .ib_client
            .as_ref()
            .context("IB client not connected. Call connect() first")?;

        // Get instrument from provider
        let instrument = self
            .instrument_provider
            .find(&cmd.instrument_id)
            .context(format!(
                "Instrument {} not found in provider",
                cmd.instrument_id
            ))?;

        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        // Convert instrument_id to IB contract
        let contract = self
            .instrument_provider
            .resolve_contract_for_instrument(cmd.instrument_id)
            .context("Failed to convert instrument_id to IB contract")?;

        // Determine number of ticks from limit or default to 1000
        let number_of_ticks = cmd.limit.map_or(1000, |l| l.get() as i32).min(1000);

        let instrument_id = cmd.instrument_id;
        let data_sender = self.data_sender.clone();
        let clock = self.clock;
        let request_id = cmd.request_id;
        let client_id = cmd.client_id.unwrap_or(self.client_id);
        let params = cmd.params.clone();
        let start_nanos = cmd.start.map(|dt| {
            UnixNanos::from(
                dt.timestamp_nanos_opt()
                    .unwrap_or_else(|| dt.timestamp() * 1_000_000_000) as u64,
            )
        });
        let end_nanos = cmd.end.map(|dt| {
            UnixNanos::from(
                dt.timestamp_nanos_opt()
                    .unwrap_or_else(|| dt.timestamp() * 1_000_000_000) as u64,
            )
        });

        // Spawn async task to handle the request with pagination
        let client_clone = client.as_arc().clone();
        let limit = cmd.limit.map_or(1000, |l| l.get());
        let start_nanos_clone = start_nanos;
        let end_nanos_clone = end_nanos;
        let cmd_start = cmd.start;
        let cmd_end = cmd.end;

        get_runtime().spawn(async move {
            let mut all_quotes = Vec::new();
            // Work backwards from end_date, updating end to the earliest tick received
            let mut current_end_date = cmd_end;
            if current_end_date.is_none() {
                current_end_date = Some(chrono::Utc::now());
            }
            let current_start_date = cmd_start;

            // Pagination loop: continue while (start exists and end > start) or (len < limit)
            loop {
                let should_continue =
                    if let (Some(start), Some(end)) = (current_start_date, current_end_date) {
                        end > start
                    } else {
                        false
                    };

                if !should_continue && all_quotes.len() >= limit {
                    break;
                }

                let current_end_ib = current_end_date.as_ref().map(chrono_to_ib_datetime);

                // Make request for this batch
                match client_clone
                    .historical_ticks_bid_ask(
                        &contract,
                        current_start_date.as_ref().map(chrono_to_ib_datetime),
                        current_end_ib,
                        number_of_ticks,
                        ibapi::market_data::TradingHours::Regular,
                        false, // ignore_size
                    )
                    .await
                {
                    Ok(mut subscription) => {
                        let mut batch_quotes = Vec::new();

                        while let Some(tick) = subscription.next().await {
                            let ts_event =
                                super::convert::ib_timestamp_to_unix_nanos(&tick.timestamp);
                            let ts_init = clock.get_time_ns();

                            match super::parse::parse_quote_tick(
                                instrument_id,
                                Some(tick.price_bid),
                                Some(tick.price_ask),
                                Some(tick.size_bid as f64),
                                Some(tick.size_ask as f64),
                                price_precision,
                                size_precision,
                                ts_event,
                                ts_init,
                            ) {
                                Ok(quote_tick) => batch_quotes.push(quote_tick),
                                Err(e) => {
                                    tracing::warn!("Failed to parse quote tick: {:?}", e);
                                }
                            }
                        }

                        if batch_quotes.is_empty() {
                            break;
                        }

                        // Update current_end_date to the minimum ts_init from this batch for next iteration
                        // This works backwards in time
                        if let Some(min_tick) = batch_quotes.iter().min_by_key(|t| t.ts_init) {
                            let min_ts_nanos = min_tick.ts_init.as_u64();
                            // Convert UnixNanos to DateTime<Utc>
                            let min_ts_seconds = (min_ts_nanos / 1_000_000_000) as i64;
                            let min_ts_nanos_remainder = (min_ts_nanos % 1_000_000_000) as u32;
                            current_end_date = chrono::DateTime::from_timestamp(
                                min_ts_seconds,
                                min_ts_nanos_remainder,
                            );
                        }

                        all_quotes.extend(batch_quotes);

                        // Check if we should continue - need start and current_end > start
                        if let (Some(start_dt), Some(end_dt)) =
                            (current_start_date, current_end_date)
                            && end_dt <= start_dt
                        {
                            // Filter out quotes after end_date_time and before start
                            if let Some(end_limit) = end_nanos_clone {
                                all_quotes.retain(|q| q.ts_init <= end_limit);
                            }

                            if let Some(start_limit) = start_nanos_clone {
                                all_quotes.retain(|q| q.ts_init >= start_limit);
                            }
                            break;
                        }

                        // Break if we've reached the limit
                        if all_quotes.len() >= limit {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            "Historical quotes request failed for {}: {:?}",
                            instrument_id,
                            e
                        );
                        break;
                    }
                }
            }

            // Filter out ticks after end_date_time if specified
            if let Some(end_limit) = end_nanos_clone {
                all_quotes.retain(|q| q.ts_init <= end_limit);
            }

            // Sort by ts_init
            all_quotes.sort_by_key(|q| q.ts_init);

            let quotes_count = all_quotes.len();
            let response = DataResponse::Quotes(QuotesResponse::new(
                request_id,
                client_id,
                instrument_id,
                all_quotes,
                start_nanos_clone,
                end_nanos_clone,
                clock.get_time_ns(),
                params,
            ));

            if let Err(e) = data_sender.send(DataEvent::Response(response)) {
                tracing::error!("Failed to send quotes response: {e}");
            } else {
                tracing::info!(
                    "Successfully sent {} quotes for {}",
                    quotes_count,
                    instrument_id
                );
            }
        });

        Ok(())
    }

    fn request_trades(&self, cmd: RequestTrades) -> anyhow::Result<()> {
        tracing::debug!("Requesting trades for {}", cmd.instrument_id);

        let client = self
            .ib_client
            .as_ref()
            .context("IB client not connected. Call connect() first")?;

        // Get instrument from provider
        let instrument = self
            .instrument_provider
            .find(&cmd.instrument_id)
            .context(format!(
                "Instrument {} not found in provider",
                cmd.instrument_id
            ))?;

        // Check if instrument is a CurrencyPair (IB doesn't support trades for CurrencyPair)
        if matches!(instrument, InstrumentAny::CurrencyPair(_)) {
            tracing::error!(
                "Interactive Brokers does not support trades for CurrencyPair instruments: {}",
                cmd.instrument_id
            );
            return Ok(());
        }

        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        // Convert instrument_id to IB contract
        let contract = self
            .instrument_provider
            .resolve_contract_for_instrument(cmd.instrument_id)
            .context("Failed to convert instrument_id to IB contract")?;

        // Determine number of ticks from limit or default to 1000
        let number_of_ticks = cmd.limit.map_or(1000, |l| l.get() as i32).min(1000);

        let instrument_id = cmd.instrument_id;
        let data_sender = self.data_sender.clone();
        let clock = self.clock;
        let request_id = cmd.request_id;
        let client_id = cmd.client_id.unwrap_or(self.client_id);
        let params = cmd.params.clone();
        let start_nanos = cmd.start.map(|dt| {
            UnixNanos::from(
                dt.timestamp_nanos_opt()
                    .unwrap_or_else(|| dt.timestamp() * 1_000_000_000) as u64,
            )
        });
        let end_nanos = cmd.end.map(|dt| {
            UnixNanos::from(
                dt.timestamp_nanos_opt()
                    .unwrap_or_else(|| dt.timestamp() * 1_000_000_000) as u64,
            )
        });

        // Spawn async task to handle the request with pagination
        let client_clone = client.as_arc().clone();
        let limit = cmd.limit.map_or(1000, |l| l.get());
        let start_nanos_clone = start_nanos;
        let end_nanos_clone = end_nanos;
        let cmd_start = cmd.start;
        let cmd_end = cmd.end;

        get_runtime().spawn(async move {
            let mut all_trades = Vec::new();
            // Work backwards from end_date, updating end to the earliest tick received
            let mut current_end_date = cmd_end;
            if current_end_date.is_none() {
                current_end_date = Some(chrono::Utc::now());
            }
            let current_start_date = cmd_start;

            // Pagination loop: continue while (start exists and end > start) or (len < limit)
            loop {
                let should_continue =
                    if let (Some(start), Some(end)) = (current_start_date, current_end_date) {
                        end > start
                    } else {
                        false
                    };

                if !should_continue && all_trades.len() >= limit {
                    break;
                }

                let current_end_ib = current_end_date.as_ref().map(chrono_to_ib_datetime);

                // Make request for this batch
                match client_clone
                    .historical_ticks_trade(
                        &contract,
                        current_start_date.as_ref().map(chrono_to_ib_datetime),
                        current_end_ib,
                        number_of_ticks,
                        ibapi::market_data::TradingHours::Regular,
                    )
                    .await
                {
                    Ok(mut subscription) => {
                        let mut batch_trades = Vec::new();

                        while let Some(tick) = subscription.next().await {
                            let ts_event =
                                super::convert::ib_timestamp_to_unix_nanos(&tick.timestamp);
                            let ts_init = clock.get_time_ns();

                            // Generate trade ID from exchange and special conditions if available
                            let trade_id = None;

                            match super::parse::parse_trade_tick(
                                instrument_id,
                                tick.price,
                                tick.size as f64,
                                price_precision,
                                size_precision,
                                ts_event,
                                ts_init,
                                trade_id,
                            ) {
                                Ok(trade_tick) => batch_trades.push(trade_tick),
                                Err(e) => {
                                    tracing::warn!("Failed to parse trade tick: {:?}", e);
                                }
                            }
                        }

                        if batch_trades.is_empty() {
                            break;
                        }

                        // Update current_end_date to the minimum ts_init from this batch for next iteration
                        // This works backwards in time
                        if let Some(min_tick) = batch_trades.iter().min_by_key(|t| t.ts_init) {
                            let min_ts_nanos = min_tick.ts_init.as_u64();
                            // Convert UnixNanos to DateTime<Utc>
                            let min_ts_seconds = (min_ts_nanos / 1_000_000_000) as i64;
                            let min_ts_nanos_remainder = (min_ts_nanos % 1_000_000_000) as u32;
                            current_end_date = chrono::DateTime::from_timestamp(
                                min_ts_seconds,
                                min_ts_nanos_remainder,
                            );
                        }

                        all_trades.extend(batch_trades);

                        // Check if we should continue - need start and current_end > start
                        if let (Some(start_dt), Some(end_dt)) =
                            (current_start_date, current_end_date)
                            && end_dt <= start_dt
                        {
                            // Filter out trades after end_date_time and before start
                            if let Some(end_limit) = end_nanos_clone {
                                all_trades.retain(|t| t.ts_init <= end_limit);
                            }

                            if let Some(start_limit) = start_nanos_clone {
                                all_trades.retain(|t| t.ts_init >= start_limit);
                            }
                            break;
                        }

                        // Break if we've reached the limit
                        if all_trades.len() >= limit {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            "Historical trades request failed for {}: {:?}",
                            instrument_id,
                            e
                        );
                        break;
                    }
                }
            }

            // Filter out ticks after end_date_time if specified
            if let Some(end_limit) = end_nanos_clone {
                all_trades.retain(|t| t.ts_init <= end_limit);
            }

            // Sort by ts_init
            all_trades.sort_by_key(|t| t.ts_init);

            let trades_count = all_trades.len();
            let response = DataResponse::Trades(TradesResponse::new(
                request_id,
                client_id,
                instrument_id,
                all_trades,
                start_nanos_clone,
                end_nanos_clone,
                clock.get_time_ns(),
                params,
            ));

            if let Err(e) = data_sender.send(DataEvent::Response(response)) {
                tracing::error!("Failed to send trades response: {e}");
            } else {
                tracing::info!(
                    "Successfully sent {} trades for {}",
                    trades_count,
                    instrument_id
                );
            }
        });

        Ok(())
    }

    fn request_bars(&self, cmd: RequestBars) -> anyhow::Result<()> {
        tracing::debug!("Requesting bars for {}", cmd.bar_type);

        // Validate bar spec (only time-aggregated bars are supported)
        if !cmd.bar_type.spec().is_time_aggregated() {
            tracing::error!(
                "Cannot request {} bars: only time bars are aggregated by Interactive Brokers",
                cmd.bar_type
            );
            return Ok(());
        }

        let client = self
            .ib_client
            .as_ref()
            .context("IB client not connected. Call connect() first")?;

        // Get instrument from provider
        let instrument_id = cmd.bar_type.instrument_id();
        let instrument = self
            .instrument_provider
            .find(&instrument_id)
            .context(format!("Instrument {instrument_id} not found in provider"))?;

        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        // Convert instrument_id to IB contract
        let contract = self
            .instrument_provider
            .resolve_contract_for_instrument(instrument_id)
            .context("Failed to convert instrument_id to IB contract")?;

        // Convert bar type to IB formats
        let ib_bar_size = bar_type_to_ib_bar_size(&cmd.bar_type)
            .context("Failed to convert bar type to IB bar size")?;
        let ib_what_to_show = price_type_to_ib_what_to_show(cmd.bar_type.spec().price_type);

        // Calculate segments to break down the request if needed
        let segments = if let (Some(start), Some(end)) = (cmd.start, cmd.end) {
            calculate_duration_segments(start, end)
        } else {
            let end_date = cmd.end.unwrap_or_else(chrono::Utc::now);
            let duration = calculate_duration(cmd.start, cmd.end).unwrap_or_else(|_| 1i32.days());
            vec![(end_date, duration)]
        };

        let bar_type = cmd.bar_type;
        let data_sender = self.data_sender.clone();
        let clock = self.clock;
        let request_id = cmd.request_id;
        let client_id = cmd.client_id.unwrap_or(self.client_id);
        let params = cmd.params.clone();
        let start_nanos = cmd.start.map(|dt| {
            UnixNanos::from(
                dt.timestamp_nanos_opt()
                    .unwrap_or_else(|| dt.timestamp() * 1_000_000_000) as u64,
            )
        });
        let end_nanos = cmd.end.map(|dt| {
            UnixNanos::from(
                dt.timestamp_nanos_opt()
                    .unwrap_or_else(|| dt.timestamp() * 1_000_000_000) as u64,
            )
        });

        // Spawn async task to handle the request with segmentation
        let client_clone = client.as_arc().clone();

        get_runtime().spawn(async move {
            let mut all_bars = Vec::new();

            for (seg_end, seg_duration) in segments {
                let end_ib = chrono_to_ib_datetime(&seg_end);

                match client_clone
                    .historical_data(
                        &contract,
                        Some(end_ib),
                        seg_duration,
                        ib_bar_size,
                        Some(ib_what_to_show),
                        ibapi::market_data::TradingHours::Regular,
                    )
                    .await
                {
                    Ok(historical_data) => {
                        // Convert IB bars to Nautilus bars
                        for ib_bar in &historical_data.bars {
                            match ib_bar_to_nautilus_bar(
                                ib_bar,
                                bar_type,
                                price_precision,
                                size_precision,
                            ) {
                                Ok(bar) => all_bars.push(bar),
                                Err(e) => {
                                    tracing::warn!(
                                        "Failed to convert IB bar to Nautilus bar: {:?}",
                                        e
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            "Historical data request failed for {} segment: {:?}",
                            bar_type,
                            e
                        );
                        // We continue with other segments if one fails?
                        // For now keep going to return what we have
                    }
                }
            }

            // Return aggregated results
            let bars_count = all_bars.len();
            if bars_count == 0 {
                tracing::warn!("No bar data received for {}", bar_type);
            }

            // Sort bars by timestamp as segments might overlap or be out of order from IB
            all_bars.sort_by_key(|b| b.ts_event);

            let response = DataResponse::Bars(BarsResponse::new(
                request_id,
                client_id,
                bar_type,
                all_bars,
                start_nanos,
                end_nanos,
                clock.get_time_ns(),
                params,
            ));

            if let Err(e) = data_sender.send(DataEvent::Response(response)) {
                tracing::error!("Failed to send bars response: {e}");
            } else {
                tracing::info!(
                    "Successfully sent {} bars for {} (segmented)",
                    bars_count,
                    bar_type
                );
            }
        });

        Ok(())
    }
}

impl Drop for InteractiveBrokersDataClient {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
