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
use chrono::{DateTime, Utc};
use ibapi::{
    client::Client,
    contracts::Contract,
    market_data::{TradingHours, historical},
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, BarSpecification, BarType, Data, QuoteTick, TradeTick},
    enums::{AggregationSource, AggressorSide, BarAggregation, PriceType},
    identifiers::InstrumentId,
    instruments::{Instrument, any::InstrumentAny},
    types::{Price, Quantity},
};

use crate::{
    data::convert::{
        bar_type_to_ib_bar_size, chrono_to_ib_datetime, ib_bar_to_nautilus_bar,
        ib_timestamp_to_unix_nanos, price_type_to_ib_what_to_show,
    },
    providers::instruments::InteractiveBrokersInstrumentProvider,
};

/// Historical data client for Interactive Brokers.
///
/// This client provides methods for requesting historical bars and ticks
/// for backtesting and research purposes.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        subclass,
        from_py_object
    )
)]
pub struct HistoricalInteractiveBrokersClient {
    /// IB API client.
    ib_client: Arc<Client>,
    /// Instrument provider.
    instrument_provider: Arc<InteractiveBrokersInstrumentProvider>,
}

impl Clone for HistoricalInteractiveBrokersClient {
    fn clone(&self) -> Self {
        Self {
            ib_client: Arc::clone(&self.ib_client),
            instrument_provider: Arc::clone(&self.instrument_provider),
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
    ///
    /// # Arguments
    ///
    /// * `ib_client` - The IB API client
    /// * `instrument_provider` - The instrument provider
    pub fn new(
        ib_client: Arc<Client>,
        instrument_provider: Arc<InteractiveBrokersInstrumentProvider>,
    ) -> Self {
        Self {
            ib_client,
            instrument_provider,
        }
    }

    /// Request historical bars.
    ///
    /// # Arguments
    ///
    /// * `bar_specifications` - List of bar specifications (e.g., "1-HOUR-LAST")
    /// * `end_date_time` - End date for bars
    /// * `start_date_time` - Optional start date
    /// * `duration` - Optional duration string (e.g., "1 D")
    /// * `contracts` - List of IB contracts
    /// * `instrument_ids` - List of instrument IDs
    /// * `use_rth` - Use regular trading hours only
    /// * `timeout` - Request timeout in seconds
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn request_bars(
        &self,
        bar_specifications: Vec<&str>,
        end_date_time: DateTime<Utc>,
        start_date_time: Option<DateTime<Utc>>,
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
                .resolve_contract_for_instrument(instrument_id)
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
                    anyhow::bail!("Invalid bar specification format: {}", bar_spec_str);
                }

                let step = parts[0].parse::<usize>()?;
                let aggregation = parts[1].to_lowercase();
                let price_type = parts[2].to_uppercase();

                let bar_spec = match aggregation.as_str() {
                    "second" => BarSpecification::new(
                        step,
                        BarAggregation::Second,
                        PriceType::from_str(&price_type).unwrap_or(PriceType::Last),
                    ),
                    "minute" => BarSpecification::new(
                        step,
                        BarAggregation::Minute,
                        PriceType::from_str(&price_type).unwrap_or(PriceType::Last),
                    ),
                    "hour" => BarSpecification::new(
                        step,
                        BarAggregation::Hour,
                        PriceType::from_str(&price_type).unwrap_or(PriceType::Last),
                    ),
                    "day" => BarSpecification::new(
                        step,
                        BarAggregation::Day,
                        PriceType::from_str(&price_type).unwrap_or(PriceType::Last),
                    ),
                    "week" => BarSpecification::new(
                        step,
                        BarAggregation::Week,
                        PriceType::from_str(&price_type).unwrap_or(PriceType::Last),
                    ),
                    _ => anyhow::bail!("Unsupported aggregation: {}", aggregation),
                };

                let instrument_id = self.resolve_instrument_id(&contract).await?;
                let bar_type_with_id =
                    BarType::new(instrument_id, bar_spec, AggregationSource::External);

                // Convert bar type to IB parameters
                let ib_bar_size = bar_type_to_ib_bar_size(&bar_type_with_id)?;
                let ib_what_to_show = price_type_to_ib_what_to_show(bar_spec.price_type);

                // Calculate duration segments
                let segments =
                    self.calculate_duration_segments(start_date_time, end_date_time, duration);

                for (segment_end, segment_duration) in segments {
                    tracing::info!(
                        "Requesting historical bars ending on {} with duration {}",
                        segment_end,
                        segment_duration
                    );

                    let historical_data = tokio::time::timeout(
                        std::time::Duration::from_secs(timeout),
                        self.ib_client.historical_data(
                            &contract,
                            Some(chrono_to_ib_datetime(&segment_end)),
                            segment_duration,
                            ib_bar_size,
                            Some(ib_what_to_show),
                            trading_hours,
                        ),
                    )
                    .await
                    .context(format!(
                        "Historical data request timed out after {} seconds",
                        timeout
                    ))??;

                    // Get precision from instrument if available
                    let (price_precision, size_precision) =
                        if let Some(instrument) = self.instrument_provider.find(&instrument_id) {
                            (instrument.price_precision(), instrument.size_precision())
                        } else {
                            (5, 0) // Default fallback
                        };

                    // Create new bar_type with correct instrument_id
                    for ib_bar in &historical_data.bars {
                        let nautilus_bar = ib_bar_to_nautilus_bar(
                            ib_bar,
                            bar_type_with_id,
                            price_precision,
                            size_precision,
                        )?;
                        all_bars.push(nautilus_bar);
                    }

                    tracing::info!("Retrieved {} bars in batch", historical_data.bars.len());
                }
            }
        }

        // Sort by timestamp
        all_bars.sort_by_key(|b| b.ts_event);

        Ok(all_bars)
    }

    /// Request historical ticks with pagination support.
    ///
    /// # Arguments
    ///
    /// * `tick_type` - "TRADES" or "BID_ASK"
    /// * `start_date_time` - Start date
    /// * `end_date_time` - End date
    /// * `contracts` - List of IB contracts
    /// * `instrument_ids` - List of instrument IDs
    /// * `use_rth` - Use regular trading hours only
    /// * `timeout` - Request timeout in seconds
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn request_ticks(
        &self,
        tick_type: &str,
        start_date_time: DateTime<Utc>,
        end_date_time: DateTime<Utc>,
        contracts: Option<Vec<Contract>>,
        instrument_ids: Option<Vec<InstrumentId>>,
        use_rth: bool,
        _timeout: u64,
    ) -> anyhow::Result<Vec<Data>> {
        if tick_type != "TRADES" && tick_type != "BID_ASK" {
            anyhow::bail!("tick_type must be 'TRADES' or 'BID_ASK'");
        }

        if start_date_time >= end_date_time {
            anyhow::bail!("Start date must be before end date");
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
                .resolve_contract_for_instrument(instrument_id)
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

            // Get precision from instrument if available
            let (price_precision, size_precision) =
                if let Some(instrument) = self.instrument_provider.find(&instrument_id) {
                    (instrument.price_precision(), instrument.size_precision())
                } else {
                    (5, 0) // Default fallback
                };

            // Pagination loop for ticks (similar to Python _handle_timestamp_iteration)
            let mut current_end_date = end_date_time;
            let current_start_date = start_date_time;
            let end_date_time_ns = UnixNanos::from(
                end_date_time
                    .timestamp_nanos_opt()
                    .unwrap_or_else(|| end_date_time.timestamp() * 1_000_000_000)
                    as u64,
            );

            match tick_type {
                "TRADES" => {
                    loop {
                        // Make request for this batch
                        let mut subscription = self
                            .ib_client
                            .historical_ticks_trade(
                                &contract,
                                Some(chrono_to_ib_datetime(&current_start_date)),
                                Some(chrono_to_ib_datetime(&current_end_date)),
                                1000, // Number of ticks per request
                                trading_hours,
                            )
                            .await?;

                        let mut batch_ticks = Vec::new();

                        while let Some(tick) = subscription.next().await {
                            let ts_event = ib_timestamp_to_unix_nanos(&tick.timestamp);

                            // Filter out ticks after end_date_time
                            if ts_event > end_date_time_ns {
                                continue;
                            }

                            let ts_init = ts_event;

                            let price = Price::new(tick.price, price_precision);
                            let size = Quantity::new(tick.size as f64, size_precision);

                            let trade_tick = TradeTick::new(
                                instrument_id,
                                price,
                                size,
                                AggressorSide::NoAggressor,
                                crate::common::parse::generate_ib_trade_id(
                                    ts_event,
                                    tick.price,
                                    tick.size as f64,
                                ),
                                ts_event,
                                ts_init,
                            );

                            batch_ticks.push(Data::Trade(trade_tick));
                        }

                        if batch_ticks.is_empty() {
                            break;
                        }

                        // Update current_end_date to the minimum ts_event from this batch for next iteration
                        // This works backwards in time
                        if let Some(min_tick) = batch_ticks.iter().min_by_key(|t| match t {
                            Data::Trade(t) => t.ts_event,
                            _ => UnixNanos::default(),
                        }) {
                            let min_ts_nanos = match min_tick {
                                Data::Trade(t) => t.ts_event.as_u64(),
                                _ => break,
                            };

                            if let Some(new_end) = retreat_end_datetime(min_ts_nanos) {
                                current_end_date = new_end;
                            } else {
                                break;
                            }
                        }

                        all_ticks.extend(batch_ticks);

                        // Check if we should continue - need current_end > current_start
                        if !should_continue_backward_pagination(
                            current_end_date,
                            current_start_date,
                        ) {
                            break;
                        }

                        // Filter out ticks after end_date_time if needed
                        all_ticks.retain(|t| match t {
                            Data::Trade(t) => t.ts_event <= end_date_time_ns,
                            Data::Quote(q) => q.ts_event <= end_date_time_ns,
                            _ => true,
                        });
                    }
                }
                "BID_ASK" => {
                    loop {
                        // Make request for this batch
                        let mut subscription = self
                            .ib_client
                            .historical_ticks_bid_ask(
                                &contract,
                                Some(chrono_to_ib_datetime(&current_start_date)),
                                Some(chrono_to_ib_datetime(&current_end_date)),
                                1000,
                                trading_hours,
                                false, // ignore_size
                            )
                            .await?;

                        let mut batch_ticks = Vec::new();

                        while let Some(tick) = subscription.next().await {
                            let ts_event = ib_timestamp_to_unix_nanos(&tick.timestamp);

                            // Filter out ticks after end_date_time
                            if ts_event > end_date_time_ns {
                                continue;
                            }

                            let ts_init = ts_event;

                            let bid_price = Price::new(tick.price_bid, price_precision);
                            let bid_size = Quantity::new(tick.size_bid as f64, size_precision);
                            let ask_price = Price::new(tick.price_ask, price_precision);
                            let ask_size = Quantity::new(tick.size_ask as f64, size_precision);

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

                        if batch_ticks.is_empty() {
                            break;
                        }

                        // Update current_end_date to the minimum ts_event from this batch for next iteration
                        if let Some(min_tick) = batch_ticks.iter().min_by_key(|t| match t {
                            Data::Quote(q) => q.ts_event,
                            _ => UnixNanos::default(),
                        }) {
                            let min_ts_nanos = match min_tick {
                                Data::Quote(q) => q.ts_event.as_u64(),
                                _ => break,
                            };

                            if let Some(new_end) = retreat_end_datetime(min_ts_nanos) {
                                current_end_date = new_end;
                            } else {
                                break;
                            }
                        }

                        all_ticks.extend(batch_ticks);

                        // Check if we should continue
                        if !should_continue_backward_pagination(
                            current_end_date,
                            current_start_date,
                        ) {
                            break;
                        }

                        // Filter out ticks after end_date_time if needed
                        all_ticks.retain(|t| match t {
                            Data::Trade(t) => t.ts_event <= end_date_time_ns,
                            Data::Quote(q) => q.ts_event <= end_date_time_ns,
                            _ => true,
                        });
                    }
                }
                _ => unreachable!(),
            }
        }

        // Sort by timestamp
        all_ticks.sort_by_key(|tick| match tick {
            Data::Trade(t) => t.ts_event,
            Data::Quote(q) => q.ts_event,
            _ => UnixNanos::default(),
        });

        Ok(all_ticks)
    }

    /// Request instruments given instrument IDs or contracts.
    ///
    /// This method uses the instrument provider to load and return instruments.
    ///
    /// # Arguments
    ///
    /// * `instrument_ids` - Optional list of instrument IDs
    /// * `contracts` - Optional list of IB contracts
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

        let mut loaded_instruments = Vec::new();

        // Load instruments from instrument IDs
        for instrument_id in instrument_ids {
            // Try fetching from provider if not already loaded
            if self.instrument_provider.find(&instrument_id).is_none()
                && let Err(e) = self
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

            if let Some(instrument) = self.instrument_provider.find(&instrument_id) {
                loaded_instruments.push(instrument);
            }
        }

        // Load instruments from contracts (equivalent to Python's _fetch_instruments_if_not_cached)
        for contract in contracts {
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
                match self.instrument_provider.symbology_method() {
                    crate::config::SymbologyMethod::Simplified => {
                        crate::common::parse::ib_contract_to_instrument_id_simplified(
                            &contract,
                            Some(venue),
                        )
                        .ok()
                    }
                    crate::config::SymbologyMethod::Raw => {
                        crate::common::parse::ib_contract_to_instrument_id_raw(
                            &contract,
                            Some(venue),
                        )
                        .ok()
                    }
                }
            };

            if let Some(instrument_id) = instrument_id {
                // Check if already loaded (skip if already in results)
                if loaded_instruments.iter().any(|i| i.id() == instrument_id) {
                    continue;
                }

                // Fetch if not cached (matching Python: if not self._client._cache.instrument(instrument_id))
                if self.instrument_provider.find(&instrument_id).is_none() {
                    tracing::info!("Fetching Instrument for: {}", instrument_id);

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
                {
                    if !loaded_instruments.iter().any(|i| i.id() == instrument.id()) {
                        loaded_instruments.push(instrument);
                    }
                }
            }
        }

        tracing::info!("Loaded {} instruments", loaded_instruments.len());

        Ok(loaded_instruments)
    }

    /// Calculate duration segments for a time range.
    ///
    /// This breaks down large date ranges into smaller segments that IB can handle.
    ///
    /// # Arguments
    ///
    /// * `start_date` - Optional start date
    /// * `end_date` - End date
    /// * `duration` - Optional duration string
    ///
    /// # Returns
    ///
    /// Returns a list of (end_date, duration) tuples.
    fn calculate_duration_segments(
        &self,
        start_date: Option<DateTime<Utc>>,
        end_date: DateTime<Utc>,
        duration: Option<&str>,
    ) -> Vec<(DateTime<Utc>, historical::Duration)> {
        // If duration is specified, use it directly
        if let Some(dur_str) = duration {
            if let Ok(dur) = dur_str.parse::<historical::Duration>() {
                return vec![(end_date, dur)];
            } else {
                tracing::warn!("Invalid duration format: {}, using default", dur_str);
            }
        }

        // Calculate from start/end dates - matching Python's comprehensive breakdown
        if let Some(start) = start_date {
            let total_delta = end_date.signed_duration_since(start);
            let total_days = total_delta.num_days();

            let mut segments = Vec::new();

            // Calculate full years in the time delta (matching Python: years = total_delta.days // 365)
            let years = total_days / 365;
            let minus_years_date = if years > 0 {
                end_date - chrono::Duration::days(365 * years)
            } else {
                end_date
            };

            // Calculate remaining days after subtracting full years (matching Python logic)
            let days = if years > 0 {
                let remaining_delta = minus_years_date.signed_duration_since(start);
                remaining_delta.num_days()
            } else {
                total_days
            };

            let minus_days_date = if days > 0 {
                minus_years_date - chrono::Duration::days(days)
            } else {
                minus_years_date
            };

            // Calculate remaining time in seconds after subtracting years and days
            // Matching Python: hours*3600 + minutes*60 + seconds + subsecond
            let remaining_delta = minus_days_date.signed_duration_since(start);
            // Extract time components from the remaining delta
            let total_secs = remaining_delta.num_seconds();
            let hours = total_secs / 3600;
            let minutes = (total_secs % 3600) / 60;
            let secs = total_secs % 60;
            // Check for subsecond precision (milliseconds, microseconds, nanoseconds)
            let subsecond = if remaining_delta.num_milliseconds() % 1000 > 0
                || remaining_delta.num_microseconds().unwrap_or(0) % 1000 > 0
                || remaining_delta.num_nanoseconds().unwrap_or(0) % 1000 > 0
            {
                1
            } else {
                0
            };
            let seconds = hours * 3600 + minutes * 60 + secs + subsecond;

            // Build segments in order: years, days, seconds (matching Python order)
            if years > 0 {
                segments.push((end_date, historical::Duration::years(years as i32)));
            }

            if days > 0 {
                segments.push((minus_years_date, historical::Duration::days(days as i32)));
            }

            if seconds > 0 {
                segments.push((
                    minus_days_date,
                    historical::Duration::seconds(seconds as i32),
                ));
            }

            if segments.is_empty() {
                // Default to 1 day if calculation results in nothing
                segments.push((end_date, historical::Duration::days(1)));
            }

            segments
        } else {
            // Default to 1 day if no start date
            vec![(end_date, historical::Duration::days(1))]
        }
    }

    async fn resolve_instrument_id(&self, contract: &Contract) -> anyhow::Result<InstrumentId> {
        if let Some(instrument_id) = self
            .instrument_provider
            .get_instrument_id_by_contract_id(contract.contract_id)
        {
            return Ok(instrument_id);
        }

        let venue = self.instrument_provider.determine_venue(contract, None);
        let parsed = match self.instrument_provider.symbology_method() {
            crate::config::SymbologyMethod::Simplified => {
                crate::common::parse::ib_contract_to_instrument_id_simplified(contract, Some(venue))
                    .ok()
            }
            crate::config::SymbologyMethod::Raw => {
                crate::common::parse::ib_contract_to_instrument_id_raw(contract, Some(venue)).ok()
            }
        };

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

fn retreat_end_datetime(min_ts_nanos: u64) -> Option<DateTime<Utc>> {
    let new_end_nanos = min_ts_nanos.saturating_sub(1_000_000); // 1ms
    let seconds = (new_end_nanos / 1_000_000_000) as i64;
    let nanos = (new_end_nanos % 1_000_000_000) as u32;
    chrono::DateTime::from_timestamp(seconds, nanos)
}

fn should_continue_backward_pagination(
    current_end_date: DateTime<Utc>,
    current_start_date: DateTime<Utc>,
) -> bool {
    current_end_date > current_start_date
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use rstest::rstest;

    use super::{retreat_end_datetime, should_continue_backward_pagination};

    #[rstest]
    fn test_retreat_end_datetime_subtracts_one_millisecond() {
        let ts_nanos = 1_700_000_000_123_456_789_u64;
        let result = retreat_end_datetime(ts_nanos).unwrap();
        assert_eq!(
            result.timestamp_nanos_opt().unwrap() as u64,
            ts_nanos - 1_000_000
        );
    }

    #[rstest]
    fn test_retreat_end_datetime_saturates_at_zero() {
        let result = retreat_end_datetime(500_000).unwrap();
        assert_eq!(result.timestamp_nanos_opt().unwrap(), 0);
    }

    #[rstest]
    fn test_should_continue_backward_pagination_true_when_end_after_start() {
        let start = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 1).unwrap();
        assert!(should_continue_backward_pagination(end, start));
    }

    #[rstest]
    fn test_should_continue_backward_pagination_false_when_end_equal_start() {
        let start = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        assert!(!should_continue_backward_pagination(start, start));
    }
}
