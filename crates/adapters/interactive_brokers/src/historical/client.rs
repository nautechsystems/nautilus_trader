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

//! Historical data client for Interactive Brokers.

use std::{fmt::Debug, str::FromStr, sync::Arc};

use anyhow::Context;
use ibapi::{
    client::Client,
    contracts::{Contract, SecurityType},
    market_data::{IgnoreSize, TradingHours, historical},
    prelude::{StreamExt, SubscriptionItemStreamExt},
};
use jiff::Timestamp;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, BarSpecification, BarType, Data, QuoteTick},
    enums::{AggregationSource, BarAggregation, PriceType},
    identifiers::InstrumentId,
    instruments::{Instrument, any::InstrumentAny},
    types::{Price, Quantity},
};

use crate::{
    common::{
        enums::IbHistoricalTickType,
        shared_client::{self, SharedClientHandle},
        symbology::is_crypto_contract,
    },
    config::{InteractiveBrokersDataClientConfig, MarketDataType},
    data::{
        convert::{
            apply_bar_price_magnifier, apply_price_magnifier, bar_request_segments,
            bar_type_to_ib_bar_size, calculate_duration_segments, extend_historical_tick_batch,
            ib_bar_to_nautilus_bar, ib_timestamp_to_unix_nanos, jiff_to_ib_datetime,
            price_type_to_ib_what_to_show_for_security, retain_historical_ticks_in_range,
        },
        parse::parse_trade_tick,
    },
    providers::instruments::InteractiveBrokersInstrumentProvider,
};

const HISTORICAL_TICK_DEFAULT_LIMIT: usize = 10_000;

/// Historical data client for Interactive Brokers.
///
/// This client provides methods for requesting historical bars and ticks
/// for backtesting and research purposes.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.adapters.interactive_brokers",
        subclass,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(
        module = "nautilus_trader.adapters.interactive_brokers"
    )
)]
pub struct HistoricalInteractiveBrokersClient {
    /// IB API client.
    ib_client: Arc<Client>,
    /// Instrument provider.
    instrument_provider: Arc<InteractiveBrokersInstrumentProvider>,
    /// Shared client handle, when this client owns the connection lifecycle.
    _shared_client: Option<Arc<SharedClientHandle>>,
}

impl Clone for HistoricalInteractiveBrokersClient {
    fn clone(&self) -> Self {
        Self {
            ib_client: Arc::clone(&self.ib_client),
            instrument_provider: Arc::clone(&self.instrument_provider),
            _shared_client: self._shared_client.clone(),
        }
    }
}

impl Debug for HistoricalInteractiveBrokersClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(HistoricalInteractiveBrokersClient))
            .field("ib_client", &"<Client>")
            .field("instrument_provider", &"<InstrumentProvider>")
            .finish()
    }
}

impl HistoricalInteractiveBrokersClient {
    /// Create a new historical data client.
    pub fn new(
        ib_client: Arc<Client>,
        instrument_provider: Arc<InteractiveBrokersInstrumentProvider>,
    ) -> Self {
        Self {
            ib_client,
            instrument_provider,
            _shared_client: None,
        }
    }

    /// Connect to Interactive Brokers and create a historical data client.
    ///
    /// This initializes an instrument provider from `config.instrument_provider` and acquires the
    /// shared IB client for the configured host, port, and client ID.
    ///
    /// # Errors
    ///
    /// Returns an error if provider initialization or the IB connection fails.
    pub async fn connect(config: InteractiveBrokersDataClientConfig) -> anyhow::Result<Self> {
        let instrument_provider = Arc::new(InteractiveBrokersInstrumentProvider::new(
            config.instrument_provider.clone(),
        ));
        let shared_client = shared_client::get_or_connect(
            &config.host,
            config.port,
            config.client_id,
            config.connection_timeout,
        )
        .await?;
        let client = shared_client.as_arc();

        if config.market_data_type != MarketDataType::Realtime {
            let market_data_type: ibapi::market_data::MarketDataType =
                config.market_data_type.into();
            client.switch_market_data_type(market_data_type).await?;
        }
        instrument_provider
            .initialize_with_client(client.as_ref())
            .await?;

        Ok(Self::from_shared_client(shared_client, instrument_provider))
    }

    pub(crate) fn from_shared_client(
        shared_client: SharedClientHandle,
        instrument_provider: Arc<InteractiveBrokersInstrumentProvider>,
    ) -> Self {
        let ib_client = Arc::clone(shared_client.as_arc());

        Self {
            ib_client,
            instrument_provider,
            _shared_client: Some(Arc::new(shared_client)),
        }
    }

    /// Connect a historical data client with a supplied provider using the shared IB client registry.
    ///
    /// This keeps standalone Rust callers from needing to acquire `common::shared_client`
    /// directly before requesting instruments or historical data.
    ///
    /// # Errors
    ///
    /// Returns an error if the shared client cannot connect or the instrument provider cannot
    /// initialize.
    pub async fn connect_with_provider(
        instrument_provider: InteractiveBrokersInstrumentProvider,
        config: InteractiveBrokersDataClientConfig,
    ) -> anyhow::Result<Self> {
        let shared_client = shared_client::get_or_connect(
            &config.host,
            config.port,
            config.client_id,
            config.connection_timeout,
        )
        .await?;
        let client = shared_client.as_arc();

        if config.market_data_type != MarketDataType::Realtime {
            let market_data_type: ibapi::market_data::MarketDataType =
                config.market_data_type.into();
            client.switch_market_data_type(market_data_type).await?;
        }
        instrument_provider
            .initialize_with_client(client.as_ref())
            .await?;

        Ok(Self::from_shared_client(
            shared_client,
            Arc::new(instrument_provider),
        ))
    }

    /// Request historical bars.
    ///
    /// # Continuous futures
    ///
    /// Continuous futures (`CONTFUT`) reject an explicit end date/time with IB
    /// error 10339. For these contracts the end date is dropped and only the
    /// first duration segment is requested, anchored to the current time, so
    /// the returned bars may fall outside `[start_date_time, end_date_time]`.
    /// A warning is logged when the requested end date/time is in the past or
    /// the range spans more than one duration segment.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn request_bars(
        &self,
        bar_specifications: Vec<&str>,
        end_date_time: Timestamp,
        start_date_time: Option<Timestamp>,
        duration: Option<&str>,
        contracts: Option<Vec<Contract>>,
        instrument_ids: Option<Vec<InstrumentId>>,
        use_rth: bool,
        timeout: u64,
    ) -> anyhow::Result<Vec<Bar>> {
        // Validate inputs
        if start_date_time.is_some() && duration.is_some() {
            anyhow::bail!("Either start_date_time or duration should be provided, not both");
        }

        if let Some(start) = start_date_time
            && start >= end_date_time
        {
            anyhow::bail!("Start date must be before end date");
        }

        let duration = duration
            .map(str::parse::<historical::Duration>)
            .transpose()
            .with_context(|| {
                format!(
                    "duration must be in format: 'int S|D|W|M|Y', was '{}'",
                    duration.unwrap_or_default()
                )
            })?;

        let contracts = contracts.unwrap_or_default();
        let instrument_ids = instrument_ids.unwrap_or_default();

        if contracts.is_empty() && instrument_ids.is_empty() {
            anyhow::bail!("Either contracts or instrument_ids must be provided");
        }

        // Convert instrument IDs to contracts using instrument provider
        let mut all_contracts = contracts;

        for instrument_id in instrument_ids {
            // Try to find instrument in provider first
            if self.instrument_provider.find(&instrument_id).is_none() {
                // Auto-fetch if not cached
                if let Err(e) = self
                    .instrument_provider
                    .fetch_contract_details(&self.ib_client, instrument_id, false, None)
                    .await
                {
                    tracing::warn!(
                        "Failed to auto-fetch contract details for {}: {}",
                        instrument_id,
                        e
                    );
                }
            }

            // Try to convert instrument ID to contract
            if let Ok(contract) = self
                .instrument_provider
                .resolve_contract_for_instrument_async(&self.ib_client, instrument_id)
                .await
            {
                all_contracts.push(contract);
            } else {
                tracing::warn!(
                    "Failed to convert instrument_id {} to IB contract, skipping",
                    instrument_id
                );
            }
        }

        // Auto-fetch contracts if not cached (by contract ID)
        for contract in &all_contracts {
            if let Some(instrument_id) = self
                .instrument_provider
                .get_instrument_id_by_contract_id(contract.contract_id)
                && self.instrument_provider.find(&instrument_id).is_none()
                && let Err(e) = self
                    .instrument_provider
                    .fetch_contract_details(&self.ib_client, instrument_id, false, None)
                    .await
            {
                tracing::warn!(
                    "Failed to auto-fetch contract details for contract ID {}: {}",
                    contract.contract_id,
                    e
                );
            }
        }

        if all_contracts.is_empty() {
            anyhow::bail!("No valid contracts found after conversion");
        }

        let trading_hours = if use_rth {
            TradingHours::Regular
        } else {
            TradingHours::Extended
        };

        let mut all_bars = Vec::new();

        for contract in all_contracts {
            for bar_spec_str in &bar_specifications {
                // Parse bar spec (e.g., "1-HOUR-LAST")
                let parts: Vec<&str> = bar_spec_str.split('-').collect();
                if parts.len() != 3 {
                    anyhow::bail!("Invalid bar specification format: {bar_spec_str}");
                }

                let step = parts[0].parse::<usize>()?;
                let aggregation = parts[1].to_lowercase();
                let price_type = parts[2].to_uppercase();
                let price_type = PriceType::from_str(&price_type)
                    .with_context(|| format!("Invalid bar price type: {}", parts[2]))?;

                let bar_spec = match aggregation.as_str() {
                    "second" => BarSpecification::new(step, BarAggregation::Second, price_type),
                    "minute" => BarSpecification::new(step, BarAggregation::Minute, price_type),
                    "hour" => BarSpecification::new(step, BarAggregation::Hour, price_type),
                    "day" => BarSpecification::new(step, BarAggregation::Day, price_type),
                    "week" => BarSpecification::new(step, BarAggregation::Week, price_type),
                    _ => anyhow::bail!("Unsupported aggregation: {aggregation}"),
                };

                let instrument_id = self.resolve_instrument_id(&contract).await?;
                let bar_type_with_id =
                    BarType::new(instrument_id, bar_spec, AggregationSource::External);

                // Convert bar type to IB parameters. Crypto trade-price bars must
                // request AGGTRADES, not TRADES (TWS rejects TRADES for crypto,
                // error 10299) - same rule as the live data client's historical path.
                let ib_bar_size = bar_type_to_ib_bar_size(&bar_type_with_id)?;
                let is_crypto = is_crypto_contract(&contract);
                let ib_what_to_show =
                    price_type_to_ib_what_to_show_for_security(bar_spec.price_type, is_crypto);

                // Omit the end date for continuous futures (IB error 10339).
                let is_continuous_future = contract.security_type == SecurityType::ContinuousFuture;
                let segments = bar_request_segments(
                    calculate_duration_segments(start_date_time, Some(end_date_time), duration),
                    is_continuous_future,
                );

                for (segment_end, segment_duration) in segments {
                    tracing::debug!(
                        "Requesting historical bars ending on {:?} with duration {}",
                        segment_end,
                        segment_duration
                    );

                    let mut request = self
                        .ib_client
                        .historical_data(&contract, ib_bar_size)
                        .duration(segment_duration)
                        .what_to_show(ib_what_to_show)
                        .trading_hours(trading_hours);

                    if let Some(end) = segment_end {
                        request = request.ending(jiff_to_ib_datetime(&end));
                    }

                    let historical_data = tokio::time::timeout(
                        std::time::Duration::from_secs(timeout),
                        request.fetch(),
                    )
                    .await
                    .context(format!(
                        "Historical data request timed out after {timeout} seconds"
                    ))??;

                    let instrument =
                        self.instrument_provider
                            .find(&instrument_id)
                            .with_context(|| {
                                format!("Instrument {instrument_id} is missing from the provider")
                            })?;
                    let price_precision = instrument.price_precision();
                    let size_precision = instrument.size_precision();
                    let price_magnifier =
                        self.instrument_provider.get_price_magnifier(&instrument_id);

                    // Create new bar_type with correct instrument_id
                    for ib_bar in &historical_data.bars {
                        let ib_bar = apply_bar_price_magnifier(ib_bar, price_magnifier);
                        let nautilus_bar = ib_bar_to_nautilus_bar(
                            &ib_bar,
                            bar_type_with_id,
                            price_precision,
                            size_precision,
                        )?;
                        all_bars.push(nautilus_bar);
                    }

                    tracing::debug!("Retrieved {} bars in batch", historical_data.bars.len());
                }
            }
        }

        // Sort by timestamp
        all_bars.sort_by_key(|b| b.ts_event);

        Ok(all_bars)
    }

    /// Request historical ticks with pagination support.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn request_ticks(
        &self,
        tick_type: IbHistoricalTickType,
        start_date_time: Timestamp,
        end_date_time: Timestamp,
        contracts: Option<Vec<Contract>>,
        instrument_ids: Option<Vec<InstrumentId>>,
        use_rth: bool,
        timeout: u64,
        limit: usize,
    ) -> anyhow::Result<Vec<Data>> {
        if start_date_time >= end_date_time {
            anyhow::bail!("Start date must be before end date");
        }

        let limit = Some(if limit > 0 {
            limit
        } else {
            HISTORICAL_TICK_DEFAULT_LIMIT
        });

        if end_date_time.duration_since(start_date_time) > jiff::SignedDuration::from_hours(24) {
            tracing::warn!(
                "Requesting tick data for more than 1 day may take a long time, particularly for liquid instruments"
            );
        }

        let contracts = contracts.unwrap_or_default();
        let instrument_ids = instrument_ids.unwrap_or_default();

        if contracts.is_empty() && instrument_ids.is_empty() {
            anyhow::bail!("Either contracts or instrument_ids must be provided");
        }

        let trading_hours = if use_rth {
            TradingHours::Regular
        } else {
            TradingHours::Extended
        };

        // Convert instrument IDs to contracts and auto-fetch if not cached
        let mut all_contracts = contracts;

        for instrument_id in instrument_ids {
            // Auto-fetch if not cached
            if self.instrument_provider.find(&instrument_id).is_none()
                && let Err(e) = self
                    .instrument_provider
                    .fetch_contract_details(&self.ib_client, instrument_id, false, None)
                    .await
            {
                tracing::warn!(
                    "Failed to auto-fetch contract details for {}: {}",
                    instrument_id,
                    e
                );
            }

            if let Ok(contract) = self
                .instrument_provider
                .resolve_contract_for_instrument_async(&self.ib_client, instrument_id)
                .await
            {
                all_contracts.push(contract);
            } else {
                tracing::warn!(
                    "Failed to convert instrument_id {} to IB contract, skipping",
                    instrument_id
                );
            }
        }

        // Auto-fetch contracts if not cached
        for contract in &all_contracts {
            if let Some(instrument_id) = self
                .instrument_provider
                .get_instrument_id_by_contract_id(contract.contract_id)
                && self.instrument_provider.find(&instrument_id).is_none()
                && let Err(e) = self
                    .instrument_provider
                    .fetch_contract_details(&self.ib_client, instrument_id, false, None)
                    .await
            {
                tracing::warn!(
                    "Failed to auto-fetch contract details for contract ID {}: {}",
                    contract.contract_id,
                    e
                );
            }
        }

        if all_contracts.is_empty() {
            anyhow::bail!("No valid contracts found after conversion");
        }

        let mut all_ticks = Vec::new();

        for contract in all_contracts {
            let instrument_id = self.resolve_instrument_id(&contract).await?;

            let instrument = self
                .instrument_provider
                .find(&instrument_id)
                .with_context(|| {
                    format!("Instrument {instrument_id} is missing from the provider")
                })?;
            let price_precision = instrument.price_precision();
            let size_precision = instrument.size_precision();
            let price_magnifier = self.instrument_provider.get_price_magnifier(&instrument_id);
            let mut contract_ticks = Vec::new();

            // Pagination loop for ticks (similar to Python _handle_timestamp_iteration)
            let mut current_end_date = Some(end_date_time);
            let current_start_date = Some(start_date_time);
            let start_date_time_ns = UnixNanos::from(
                u64::try_from(start_date_time.as_nanosecond())
                    .context("Historical tick start date precedes the Unix epoch")?,
            );
            let end_date_time_ns = UnixNanos::from(
                u64::try_from(end_date_time.as_nanosecond())
                    .context("Historical tick end date precedes the Unix epoch")?,
            );

            match tick_type {
                IbHistoricalTickType::Trades => {
                    while let Some(request_end) = current_end_date {
                        // Make request for this batch
                        let subscription = tokio::time::timeout(
                            std::time::Duration::from_secs(timeout),
                            self.ib_client
                                .historical_ticks(&contract, 1000)
                                .ending(jiff_to_ib_datetime(&request_end))
                                .trading_hours(trading_hours)
                                .trade(),
                        )
                        .await
                        .context(format!(
                            "Historical trades request timed out after {timeout} seconds"
                        ))??;

                        let mut subscription = subscription.filter_data();
                        let mut batch_ticks = Vec::new();

                        while let Some(tick_result) = subscription.next().await {
                            let tick = match tick_result {
                                Ok(tick) => tick,
                                Err(e) => {
                                    tracing::warn!("Historical trade ticks stream error: {e:?}");
                                    continue;
                                }
                            };
                            let ts_event = ib_timestamp_to_unix_nanos(&tick.timestamp);

                            if ts_event < start_date_time_ns || ts_event > end_date_time_ns {
                                continue;
                            }

                            let ts_init = ts_event;

                            let converted_price =
                                apply_price_magnifier(tick.price, price_magnifier);
                            let Some(raw_size) = tick.size else {
                                tracing::warn!(
                                    "Skipping historical trade tick with no size for {}",
                                    instrument_id
                                );
                                continue;
                            };

                            if raw_size == 0.0 {
                                tracing::warn!(
                                    "Skipping historical trade tick with zero size for {instrument_id}"
                                );
                                continue;
                            }
                            let trade_tick = parse_trade_tick(
                                instrument_id,
                                converted_price,
                                raw_size,
                                price_precision,
                                size_precision,
                                ts_event,
                                ts_init,
                                None,
                            )
                            .with_context(|| {
                                format!("Invalid historical trade tick for {instrument_id}")
                            })?;

                            batch_ticks.push(Data::Trade(trade_tick));
                        }

                        if !extend_historical_tick_batch(
                            &mut contract_ticks,
                            batch_ticks,
                            current_start_date,
                            &mut current_end_date,
                            Some(start_date_time_ns),
                            Some(end_date_time_ns),
                            limit,
                            data_ts_event,
                        ) {
                            break;
                        }
                    }
                }
                IbHistoricalTickType::BidAsk => {
                    while let Some(request_end) = current_end_date {
                        // Make request for this batch
                        let subscription = tokio::time::timeout(
                            std::time::Duration::from_secs(timeout),
                            self.ib_client
                                .historical_ticks(&contract, 1000)
                                .ending(jiff_to_ib_datetime(&request_end))
                                .trading_hours(trading_hours)
                                .bid_ask(IgnoreSize::No),
                        )
                        .await
                        .context(format!(
                            "Historical bid/ask ticks request timed out after {timeout} seconds"
                        ))??;

                        let mut subscription = subscription.filter_data();
                        let mut batch_ticks = Vec::new();

                        while let Some(tick_result) = subscription.next().await {
                            let tick = match tick_result {
                                Ok(tick) => tick,
                                Err(e) => {
                                    tracing::warn!("Historical bid/ask ticks stream error: {e:?}");
                                    continue;
                                }
                            };
                            let ts_event = ib_timestamp_to_unix_nanos(&tick.timestamp);

                            if ts_event < start_date_time_ns || ts_event > end_date_time_ns {
                                continue;
                            }

                            let ts_init = ts_event;

                            let raw_bid_price =
                                apply_price_magnifier(tick.price_bid, price_magnifier);
                            let bid_price = Price::new_checked(raw_bid_price, price_precision)
                                .with_context(|| {
                                    format!(
                                        "Invalid historical bid price {raw_bid_price} for {instrument_id}"
                                    )
                                })?;
                            let (Some(raw_bid_size), Some(raw_ask_size)) =
                                (tick.size_bid, tick.size_ask)
                            else {
                                tracing::warn!(
                                    "Skipping historical quote tick with an absent size for {}",
                                    instrument_id
                                );
                                continue;
                            };
                            let bid_size = Quantity::new_checked(raw_bid_size, size_precision)
                                .with_context(|| {
                                    format!(
                                        "Invalid historical bid size {raw_bid_size} for {instrument_id}"
                                    )
                                })?;
                            let raw_ask_price =
                                apply_price_magnifier(tick.price_ask, price_magnifier);
                            let ask_price = Price::new_checked(raw_ask_price, price_precision)
                                .with_context(|| {
                                    format!(
                                        "Invalid historical ask price {raw_ask_price} for {instrument_id}"
                                    )
                                })?;
                            let ask_size = Quantity::new_checked(raw_ask_size, size_precision)
                                .with_context(|| {
                                    format!(
                                        "Invalid historical ask size {raw_ask_size} for {instrument_id}"
                                    )
                                })?;

                            let quote_tick = QuoteTick::new(
                                instrument_id,
                                bid_price,
                                ask_price,
                                bid_size,
                                ask_size,
                                ts_event,
                                ts_init,
                            );

                            batch_ticks.push(Data::Quote(quote_tick));
                        }

                        if !extend_historical_tick_batch(
                            &mut contract_ticks,
                            batch_ticks,
                            current_start_date,
                            &mut current_end_date,
                            Some(start_date_time_ns),
                            Some(end_date_time_ns),
                            limit,
                            data_ts_event,
                        ) {
                            break;
                        }
                    }
                }
            }

            retain_historical_ticks_in_range(
                &mut contract_ticks,
                Some(start_date_time_ns),
                Some(end_date_time_ns),
                data_ts_event,
            );
            contract_ticks.sort_by_key(data_ts_event);

            if let Some(limit) = limit
                && contract_ticks.len() > limit
            {
                contract_ticks = contract_ticks.split_off(contract_ticks.len() - limit);
            }
            all_ticks.extend(contract_ticks);
        }

        // Sort by timestamp
        all_ticks.sort_by_key(data_ts_event);

        Ok(all_ticks)
    }

    /// Request instruments given instrument IDs or contracts.
    ///
    /// This method uses the instrument provider to load and return instruments.
    ///
    /// # Returns
    ///
    /// Returns a list of instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if loading fails.
    pub async fn request_instruments(
        &self,
        instrument_ids: Option<Vec<InstrumentId>>,
        contracts: Option<Vec<Contract>>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let instrument_ids = instrument_ids.unwrap_or_default();
        let contracts = contracts.unwrap_or_default();

        if instrument_ids.is_empty() && contracts.is_empty() {
            anyhow::bail!("Either instrument_ids or contracts must be provided");
        }

        let loaded_ids = self
            .instrument_provider
            .load_ids_with_return_async(&self.ib_client, instrument_ids, None)
            .await?;
        let mut loaded_instruments = self.instrument_provider.find_all(&loaded_ids);

        // Load instruments from contracts (equivalent to Python's _fetch_instruments_if_not_cached)
        for contract in contracts {
            match self
                .instrument_provider
                .get_instrument(&self.ib_client, &contract)
                .await
            {
                Ok(Some(instrument)) => {
                    if !loaded_instruments.iter().any(|i| i.id() == instrument.id()) {
                        loaded_instruments.push(instrument);
                    }
                    continue;
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!(
                        "Failed to fetch contract details from original contract {:?}: {}",
                        contract,
                        e
                    );
                }
            }

            // Try to find instrument by contract ID first
            let instrument_id = if let Some(cached_id) = self
                .instrument_provider
                .get_instrument_id_by_contract_id(contract.contract_id)
            {
                Some(cached_id)
            } else {
                // Convert contract to instrument ID using provider's venue determination
                // This matches Python's logic: venue = instrument_provider.determine_venue_from_contract(contract)
                let venue = self.instrument_provider.determine_venue(&contract, None);
                self.instrument_provider
                    .instrument_id_from_contract(&contract, venue)
                    .ok()
            };

            if let Some(instrument_id) = instrument_id {
                // Check if already loaded (skip if already in results)
                if loaded_instruments.iter().any(|i| i.id() == instrument_id) {
                    continue;
                }

                // Fetch if not cached (matching Python: if not self._client._cache.instrument(instrument_id))
                if self.instrument_provider.find(&instrument_id).is_none() {
                    tracing::debug!("Fetching Instrument for: {}", instrument_id);

                    if let Err(e) = self
                        .instrument_provider
                        .fetch_contract_details(&self.ib_client, instrument_id, false, None)
                        .await
                    {
                        tracing::warn!(
                            "Failed to fetch contract details for {}: {}",
                            instrument_id,
                            e
                        );
                        continue;
                    }
                }

                if let Some(instrument) = self.instrument_provider.find(&instrument_id) {
                    loaded_instruments.push(instrument);
                }
            } else {
                // Fallback: try using get_instrument which handles BAG contracts
                if let Ok(Some(instrument)) = self
                    .instrument_provider
                    .get_instrument(&self.ib_client, &contract)
                    .await
                    && !loaded_instruments.iter().any(|i| i.id() == instrument.id())
                {
                    loaded_instruments.push(instrument);
                }
            }
        }

        tracing::debug!("Loaded {} instruments", loaded_instruments.len());

        Ok(loaded_instruments)
    }

    async fn resolve_instrument_id(&self, contract: &Contract) -> anyhow::Result<InstrumentId> {
        if let Some(instrument_id) = self
            .instrument_provider
            .get_instrument_id_by_contract_id(contract.contract_id)
        {
            return Ok(instrument_id);
        }

        let venue = self.instrument_provider.determine_venue(contract, None);
        let parsed = self
            .instrument_provider
            .instrument_id_from_contract(contract, venue)
            .ok();

        if let Some(instrument_id) = parsed {
            return Ok(instrument_id);
        }

        if let Ok(Some(instrument)) = self
            .instrument_provider
            .get_instrument(&self.ib_client, contract)
            .await
        {
            return Ok(instrument.id());
        }

        anyhow::bail!(
            "Failed to resolve instrument ID for contract {}:{}:{}",
            contract.symbol,
            contract.security_type,
            contract.exchange
        );
    }
}

fn data_ts_event(data: &Data) -> UnixNanos {
    match data {
        Data::Trade(tick) => tick.ts_event,
        Data::Quote(tick) => tick.ts_event,
        _ => UnixNanos::default(),
    }
}
