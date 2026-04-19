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

//! Interactive Brokers instrument provider implementation.

use std::{collections::HashMap, fs, path::Path, str::FromStr, sync::Arc};

use anyhow::Context;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use ibapi::contracts::{ComboLegOpenClose, Contract, Exchange, SecurityType, Symbol};
#[cfg(test)]
use nautilus_model::instruments::Instrument;
use nautilus_model::{
    identifiers::{InstrumentId, Venue},
    instruments::InstrumentAny,
};
use serde::{Deserialize, Serialize};

use crate::{
    common::parse::{
        determine_venue_from_contract, instrument_id_to_ib_contract, is_spread_instrument_id,
        parse_spread_instrument_id_to_legs, possible_exchanges_for_venue,
    },
    config::InteractiveBrokersInstrumentProviderConfig,
    providers::parse::{
        create_spread_instrument_id, parse_ib_contract_to_instrument, parse_spread_instrument_any,
    },
};

/// Cache structure for persistent instrument caching.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstrumentCache {
    /// Timestamp when cache was created.
    cache_timestamp: DateTime<Utc>,
    /// Contract ID to Instrument ID mappings.
    contract_id_to_instrument_id: Vec<(i32, String)>,
    /// Instrument ID to Price Magnifier mappings.
    price_magnifiers: Vec<(String, i32)>,
    /// Instruments serialized as JSON strings (since InstrumentAny is serializable).
    instruments: Vec<(String, String)>, // (instrument_id, json)
}

/// Interactive Brokers instrument provider.
///
/// This provider fetches contract details from Interactive Brokers using the `rust-ibapi` library
/// and converts them to NautilusTrader instruments.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        unsendable,
        from_py_object
    )
)]
#[derive(Debug, Clone)]
pub struct InteractiveBrokersInstrumentProvider {
    /// Configuration for the provider.
    config: InteractiveBrokersInstrumentProviderConfig,
    /// Cache mapping contract IDs to instrument IDs.
    contract_id_to_instrument_id: Arc<DashMap<i32, InstrumentId>>,
    /// Cache mapping instrument IDs to instruments.
    instruments: Arc<DashMap<InstrumentId, InstrumentAny>>,
    /// Cache mapping instrument IDs to contract details.
    contract_details: Arc<DashMap<InstrumentId, ibapi::contracts::ContractDetails>>,
    /// Cache mapping instrument IDs to IB contracts.
    contracts: Arc<DashMap<InstrumentId, Contract>>,
    /// Dedicated cache for price magnifiers for fast lookups.
    price_magnifiers: Arc<DashMap<InstrumentId, i32>>,
}

impl InteractiveBrokersInstrumentProvider {
    /// Create a new `InteractiveBrokersInstrumentProvider`.
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration for the provider
    pub fn new(config: InteractiveBrokersInstrumentProviderConfig) -> Self {
        Self {
            config,
            contract_id_to_instrument_id: Arc::new(DashMap::new()),
            instruments: Arc::new(DashMap::new()),
            contract_details: Arc::new(DashMap::new()),
            contracts: Arc::new(DashMap::new()),
            price_magnifiers: Arc::new(DashMap::new()),
        }
    }

    #[cfg(test)]
    pub(crate) fn insert_test_instrument(
        &self,
        instrument: InstrumentAny,
        contract_id: i32,
        price_magnifier: i32,
    ) {
        let instrument_id = instrument.id();
        self.instruments.insert(instrument_id, instrument);
        self.contract_id_to_instrument_id
            .insert(contract_id, instrument_id);
        self.contracts.insert(
            instrument_id,
            Contract {
                contract_id,
                ..Default::default()
            },
        );
        self.price_magnifiers.insert(instrument_id, price_magnifier);
    }

    /// Initialize the provider by loading cache if configured.
    ///
    /// This is equivalent to Python's `provider.initialize()` method.
    /// It loads instruments from cache if `cache_path` is configured and cache is valid.
    ///
    /// # Errors
    ///
    /// Returns an error if cache loading fails.
    pub async fn initialize(&self) -> anyhow::Result<()> {
        if let Some(ref cache_path) = self.config.cache_path {
            match self.load_cache(cache_path).await {
                Ok(cache_loaded) => {
                    if cache_loaded {
                        tracing::info!(
                            "Initialized provider with {} instruments from cache",
                            self.count()
                        );
                    } else {
                        tracing::debug!(
                            "Cache file not found or expired, starting with empty cache"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to load cache during initialization: {}", e);
                }
            }
        }
        Ok(())
    }

    /// Determine venue from contract using provider configuration.
    ///
    /// This is equivalent to Python's `determine_venue_from_contract` method.
    /// It uses the config's symbol-to-venue mapping and exchange-to-venue conversion settings.
    ///
    /// # Arguments
    ///
    /// * `contract` - The IB contract
    ///
    /// # Returns
    ///
    /// The determined venue.
    pub fn determine_venue(
        &self,
        contract: &Contract,
        contract_details: Option<&ibapi::contracts::ContractDetails>,
    ) -> Venue {
        let valid_exchanges = contract_details.map(|details| details.valid_exchanges.join(","));
        let venue_str = determine_venue_from_contract(
            contract,
            &self.config.symbol_to_mic_venue,
            self.config.convert_exchange_to_mic_venue,
            valid_exchanges.as_deref(),
        );
        Venue::from(venue_str.as_str())
    }

    /// Get the symbology method from the provider configuration.
    pub fn symbology_method(&self) -> crate::config::SymbologyMethod {
        self.config.symbology_method
    }

    /// Get an instrument by its ID.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to look up
    ///
    /// # Returns
    ///
    /// Returns the instrument if found, `None` otherwise.
    #[must_use]
    pub fn find(&self, instrument_id: &InstrumentId) -> Option<InstrumentAny> {
        self.instruments
            .get(instrument_id)
            .map(|entry| entry.value().clone())
    }

    /// Get an instrument by contract ID.
    ///
    /// # Arguments
    ///
    /// * `contract_id` - The IB contract ID to look up
    ///
    /// # Returns
    ///
    /// Returns the instrument if found, `None` otherwise.
    #[must_use]
    pub fn find_by_contract_id(&self, contract_id: i32) -> Option<InstrumentAny> {
        self.contract_id_to_instrument_id
            .get(&contract_id)
            .and_then(|entry| self.find(entry.value()))
    }

    /// Get an instrument ID by contract ID.
    ///
    /// # Arguments
    ///
    /// * `contract_id` - The IB contract ID to look up
    ///
    /// # Returns
    ///
    /// Returns the instrument ID if found, `None` otherwise.
    #[must_use]
    pub fn get_instrument_id_by_contract_id(&self, contract_id: i32) -> Option<InstrumentId> {
        self.contract_id_to_instrument_id
            .get(&contract_id)
            .map(|entry| *entry.value())
    }

    /// Check if a security type should be filtered.
    ///
    /// # Arguments
    ///
    /// * `sec_type` - The security type to check
    ///
    /// # Returns
    ///
    /// Returns `true` if the security type should be filtered.
    #[must_use]
    pub fn is_filtered_sec_type(&self, sec_type: &str) -> bool {
        self.config.filter_sec_types.contains(sec_type)
    }

    /// Get all cached instruments.
    ///
    /// # Returns
    ///
    /// Returns a vector of all cached instruments.
    #[must_use]
    pub fn get_all(&self) -> Vec<InstrumentAny> {
        self.instruments
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Get the number of cached instruments.
    ///
    /// # Returns
    ///
    /// Returns the number of cached instruments.
    #[must_use]
    pub fn count(&self) -> usize {
        self.instruments.len()
    }

    /// Get price magnifier for an instrument ID.
    ///
    /// Price magnifier allows execution and strike prices to be reported consistently
    /// with market data and historical data.
    ///
    /// This method first checks the dedicated price magnifier cache for fast lookup.
    /// If not found, it falls back to checking contract details. If still not found,
    /// it returns the default value of 1 and logs a warning if the instrument exists.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to look up
    ///
    /// # Returns
    ///
    /// Returns the price magnifier if found, otherwise 1.
    #[must_use]
    pub fn get_price_magnifier(&self, instrument_id: &InstrumentId) -> i32 {
        // First try dedicated price magnifier cache for fast lookup
        if let Some(magnifier) = self.price_magnifiers.get(instrument_id) {
            return normalize_price_magnifier(*magnifier.value());
        }

        // Fall back to contract details lookup
        if let Some(details) = self.contract_details.get(instrument_id) {
            let magnifier = normalize_price_magnifier(details.value().price_magnifier);
            // Cache it for future fast lookups
            self.price_magnifiers.insert(*instrument_id, magnifier);
            return magnifier;
        }

        // Not found - check if instrument exists (might not have contract details loaded yet)
        if self.instruments.contains_key(instrument_id) {
            tracing::debug!(
                "Price magnifier not found for instrument {} (has instrument but no contract details), using default 1",
                instrument_id
            );
        } else {
            tracing::trace!(
                "Price magnifier not found for instrument {} (instrument not loaded), using default 1",
                instrument_id
            );
        }

        // Default to 1 if not found
        1
    }

    /// Get an instrument by IB Contract.
    ///
    /// This is equivalent to Python's `get_instrument` method.
    /// Supports BAG contracts by auto-loading legs.
    ///
    /// # Arguments
    ///
    /// * `client` - The IB API client
    /// * `contract` - The IB contract to get instrument for
    ///
    /// # Returns
    ///
    /// Returns the instrument if found, `None` otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching fails.
    pub async fn get_instrument(
        &self,
        client: &ibapi::Client,
        contract: &Contract,
    ) -> anyhow::Result<Option<InstrumentAny>> {
        log::debug!(
            "IB get_instrument request sec_type={:?} con_id={} symbol={} local_symbol={} exchange={} expiry={}",
            contract.security_type,
            contract.contract_id,
            contract.symbol.as_str(),
            contract.local_symbol.as_str(),
            contract.exchange.as_str(),
            contract.last_trade_date_or_contract_month.as_str()
        );
        // Check if security type is filtered
        let sec_type_str = format!("{:?}", contract.security_type);
        if self.is_filtered_sec_type(&sec_type_str) {
            tracing::warn!(
                "Skipping filtered security type {} for contract",
                sec_type_str
            );
            return Ok(None);
        }

        let contract_id = contract.contract_id;

        // Check if we already have this instrument by contract ID
        if let Some(cached_instrument_id) = self.contract_id_to_instrument_id.get(&contract_id) {
            log::debug!(
                "IB get_instrument cache hit for contract_id={} -> {}",
                contract_id,
                cached_instrument_id.value()
            );

            if let Some(instrument) = self.find(cached_instrument_id.value()) {
                return Ok(Some(instrument));
            }
        }

        // Special handling for BAG contracts
        if contract.security_type == SecurityType::Spread && !contract.combo_legs.is_empty() {
            // Load BAG contract (which auto-loads legs and creates spread instrument)
            self.fetch_bag_contract(client, contract).await?;

            // Get the spread instrument ID that was created
            if let Some(spread_instrument_id) = self.contract_id_to_instrument_id.get(&contract_id)
            {
                return Ok(self.find(spread_instrument_id.value()));
            }
        }

        // For non-BAG contracts, fetch contract details and load
        let details_vec = client
            .contract_details(contract)
            .await
            .context("Failed to fetch contract details from IB")?;

        log::debug!(
            "IB get_instrument received {} contract details for sec_type={:?} symbol={} local_symbol={}",
            details_vec.len(),
            contract.security_type,
            contract.symbol.as_str(),
            contract.local_symbol.as_str()
        );

        if details_vec.is_empty() {
            tracing::warn!("No contract details returned for contract {}", contract_id);
            return Ok(None);
        }

        let details = &details_vec[0];
        log::debug!(
            "IB get_instrument using first detail sec_type={:?} con_id={} local_symbol={} exchange={} under_con_id={}",
            details.contract.security_type,
            details.contract.contract_id,
            details.contract.local_symbol.as_str(),
            details.contract.exchange.as_str(),
            details.under_contract_id
        );
        let venue = self.determine_venue(&details.contract, Some(details));
        let instrument_id = match self.config.symbology_method {
            crate::config::SymbologyMethod::Simplified => {
                crate::common::parse::ib_contract_to_instrument_id_simplified(
                    &details.contract,
                    Some(venue),
                )
            }
            crate::config::SymbologyMethod::Raw => {
                crate::common::parse::ib_contract_to_instrument_id_raw(
                    &details.contract,
                    Some(venue),
                )
            }
        }
        .context("Failed to convert contract to instrument ID")?;

        log::debug!(
            "IB get_instrument mapped to instrument_id={}",
            instrument_id
        );

        // Parse and cache the instrument
        let instrument = parse_ib_contract_to_instrument(details, instrument_id)
            .context("Failed to parse instrument")?;

        self.instruments.insert(instrument_id, instrument.clone());
        self.contract_details.insert(instrument_id, details.clone());
        self.contracts
            .insert(instrument_id, details.contract.clone());
        self.contract_id_to_instrument_id
            .insert(details.contract.contract_id, instrument_id);
        self.price_magnifiers
            .insert(instrument_id, details.price_magnifier);

        Ok(Some(instrument))
    }

    /// Convert an instrument ID to IB contract details.
    ///
    /// This is equivalent to Python's `instrument_id_to_ib_contract_details` method.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument ID to convert
    ///
    /// # Returns
    ///
    /// Returns the contract details if found, `None` otherwise.
    #[must_use]
    pub fn instrument_id_to_ib_contract_details(
        &self,
        instrument_id: &InstrumentId,
    ) -> Option<ibapi::contracts::ContractDetails> {
        self.contract_details
            .get(instrument_id)
            .map(|entry| entry.value().clone())
    }

    #[must_use]
    pub fn instrument_id_to_ib_contract(&self, instrument_id: &InstrumentId) -> Option<Contract> {
        self.contracts
            .get(instrument_id)
            .map(|entry| entry.value().clone())
    }

    pub fn resolve_contract_for_instrument(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<Contract> {
        if let Some(contract) = self.instrument_id_to_ib_contract(&instrument_id) {
            return Ok(contract);
        }

        if let Some(details) = self.instrument_id_to_ib_contract_details(&instrument_id) {
            return Ok(details.contract);
        }

        instrument_id_to_ib_contract(instrument_id, None)
    }

    /// Load a single instrument (does not return loaded IDs).
    ///
    /// This is equivalent to Python's `load_async` method.
    ///
    /// # Arguments
    ///
    /// * `client` - The IB API client
    /// * `instrument_id` - The instrument ID to load
    /// * `force_instrument_update` - If true, force re-fetch even if already cached
    ///
    /// # Errors
    ///
    /// Returns an error if loading fails.
    pub async fn load_async(
        &self,
        client: &ibapi::Client,
        instrument_id: InstrumentId,
        filters: Option<HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let filters: Option<HashMap<String, String>> = filters;
        let force_instrument_update = filters
            .as_ref()
            .and_then(|f| f.get("force_instrument_update"))
            .map(|v| v == "true")
            .unwrap_or(false);

        self.fetch_contract_details(client, instrument_id, force_instrument_update, filters)
            .await
    }

    /// Load a single instrument and return the loaded instrument ID.
    ///
    /// This is equivalent to Python's `load_with_return_async` method.
    ///
    /// # Arguments
    ///
    /// * `client` - The IB API client
    /// * `instrument_id` - The instrument ID to load
    /// * `force_instrument_update` - If true, force re-fetch even if already cached
    ///
    /// # Returns
    ///
    /// Returns the loaded instrument ID if successful, `None` otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error if loading fails.
    pub async fn load_with_return_async(
        &self,
        client: &ibapi::Client,
        instrument_id: InstrumentId,
        filters: Option<HashMap<String, String>>,
    ) -> anyhow::Result<Option<InstrumentId>> {
        let filters: Option<HashMap<String, String>> = filters;
        let force_instrument_update = filters
            .as_ref()
            .and_then(|f| f.get("force_instrument_update"))
            .map(|v| v == "true")
            .unwrap_or(false);

        if is_spread_instrument_id(&instrument_id) {
            self.fetch_spread_instrument(client, instrument_id, force_instrument_update, filters)
                .await?;
        } else {
            self.fetch_contract_details(client, instrument_id, force_instrument_update, filters)
                .await?;
        }

        if self.instruments.contains_key(&instrument_id) {
            Ok(Some(instrument_id))
        } else {
            Ok(None)
        }
    }

    /// Load multiple instruments (does not return loaded IDs).
    ///
    /// This is equivalent to Python's `load_ids_async` method.
    ///
    /// # Arguments
    ///
    /// * `client` - The IB API client
    /// * `instrument_ids` - Vector of instrument IDs to load
    /// * `force_instrument_update` - If true, force re-fetch even if already cached
    ///
    /// # Errors
    ///
    /// Returns an error if loading fails.
    pub async fn load_ids_async(
        &self,
        client: &ibapi::Client,
        instrument_ids: Vec<InstrumentId>,
        filters: Option<HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let filters: Option<HashMap<String, String>> = filters;
        let force_instrument_update = filters
            .as_ref()
            .and_then(|f| f.get("force_instrument_update"))
            .map(|v| v == "true")
            .unwrap_or(false);

        for instrument_id in instrument_ids {
            let load_result = if is_spread_instrument_id(&instrument_id) {
                self.fetch_spread_instrument(
                    client,
                    instrument_id,
                    force_instrument_update,
                    filters.clone(),
                )
                .await
                .map(|_| ())
            } else {
                self.fetch_contract_details(
                    client,
                    instrument_id,
                    force_instrument_update,
                    filters.clone(),
                )
                .await
            };

            if let Err(e) = load_result {
                tracing::warn!("Failed to load instrument {}: {}", instrument_id, e);
            }
        }
        Ok(())
    }

    /// Load multiple instruments and return the loaded instrument IDs.
    ///
    /// This is equivalent to Python's `load_ids_with_return_async` method.
    ///
    /// # Arguments
    ///
    /// * `client` - The IB API client
    /// * `instrument_ids` - Vector of instrument IDs to load
    /// * `force_instrument_update` - If true, force re-fetch even if already cached
    ///
    /// # Returns
    ///
    /// Returns a vector of successfully loaded instrument IDs.
    ///
    /// # Errors
    ///
    /// Returns an error if loading fails.
    pub async fn load_ids_with_return_async(
        &self,
        client: &ibapi::Client,
        instrument_ids: Vec<InstrumentId>,
        filters: Option<HashMap<String, String>>,
    ) -> anyhow::Result<Vec<InstrumentId>> {
        let mut loaded_ids = Vec::new();

        let force_instrument_update = filters
            .as_ref()
            .and_then(|f| f.get("force_instrument_update"))
            .map(|v| v == "true")
            .unwrap_or(false);

        for instrument_id in instrument_ids {
            let load_result = if is_spread_instrument_id(&instrument_id) {
                self.fetch_spread_instrument(
                    client,
                    instrument_id,
                    force_instrument_update,
                    filters.clone(),
                )
                .await
                .map(|loaded| loaded.then_some(()))
            } else {
                self.fetch_contract_details(
                    client,
                    instrument_id,
                    force_instrument_update,
                    filters.clone(),
                )
                .await
                .map(Some)
            };

            if load_result.is_ok() {
                loaded_ids.push(instrument_id);
            }
        }

        Ok(loaded_ids)
    }

    fn create_bag_contract_from_legs(
        &self,
        leg_contract_details: &[(ibapi::contracts::ContractDetails, i32)],
        instrument_id: Option<InstrumentId>,
        bag_contract: Option<&Contract>,
    ) -> anyhow::Result<Contract> {
        if let Some(bag_contract) = bag_contract {
            return Ok(bag_contract.clone());
        }

        let (first_details, _) = leg_contract_details
            .first()
            .ok_or_else(|| anyhow::anyhow!("Cannot create BAG contract without leg details"))?;

        let combo_legs = leg_contract_details
            .iter()
            .map(|(details, ratio)| ibapi::contracts::ComboLeg {
                contract_id: details.contract.contract_id,
                ratio: ratio.abs(),
                action: if *ratio > 0 {
                    "BUY".to_string()
                } else {
                    "SELL".to_string()
                },
                exchange: details.contract.exchange.to_string(),
                open_close: ComboLegOpenClose::Same,
                short_sale_slot: 0,
                designated_location: String::new(),
                exempt_code: -1,
            })
            .collect();

        Ok(Contract {
            contract_id: 0,
            symbol: first_details.contract.symbol.clone(),
            security_type: SecurityType::Spread,
            exchange: Exchange::from("SMART"),
            currency: first_details.contract.currency.clone(),
            local_symbol: instrument_id.map_or_else(String::new, |id| id.symbol.to_string()),
            combo_legs_description: instrument_id
                .map(|id| format!("Spread: {}", id.symbol))
                .unwrap_or_else(|| "Spread".to_string()),
            combo_legs,
            ..Default::default()
        })
    }

    /// Fetch a spread instrument by loading its individual legs.
    ///
    /// This is equivalent to Python's `_fetch_spread_instrument` method.
    /// It parses the spread instrument ID to extract leg tuples, loads each leg,
    /// and then creates the spread instrument.
    ///
    /// # Arguments
    ///
    /// * `client` - The IB API client
    /// * `spread_instrument_id` - The spread instrument ID to fetch
    /// * `force_instrument_update` - If true, force re-fetch even if already cached
    ///
    /// # Returns
    ///
    /// Returns `true` if the spread instrument was successfully loaded, `false` otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing or loading fails.
    pub async fn fetch_spread_instrument(
        &self,
        client: &ibapi::Client,
        spread_instrument_id: InstrumentId,
        force_instrument_update: bool,
        filters: Option<HashMap<String, String>>,
    ) -> anyhow::Result<bool> {
        // Check if already cached (unless forcing update)
        if !force_instrument_update && self.instruments.contains_key(&spread_instrument_id) {
            tracing::debug!("Spread instrument {} already cached", spread_instrument_id);
            return Ok(true);
        }

        // Parse the spread ID to get individual legs
        let leg_tuples = parse_spread_instrument_id_to_legs(&spread_instrument_id)
            .context("Failed to parse spread instrument ID to leg tuples")?;

        if leg_tuples.is_empty() {
            tracing::error!("Spread instrument {} has no legs", spread_instrument_id);
            return Ok(false);
        }

        tracing::info!(
            "Loading spread instrument {} with {} legs",
            spread_instrument_id,
            leg_tuples.len()
        );

        // First, load all individual leg instruments to get their contract details
        let mut leg_contract_details = Vec::new();

        for (leg_instrument_id, ratio) in &leg_tuples {
            tracing::info!(
                "Loading leg instrument: {} (ratio: {})",
                leg_instrument_id,
                ratio
            );

            // Load the individual leg instrument
            self.fetch_contract_details(
                client,
                *leg_instrument_id,
                force_instrument_update,
                filters.clone(),
            )
            .await
            .with_context(|| format!("Failed to load leg instrument: {}", leg_instrument_id))?;

            // Get the contract details for this leg
            let leg_details = self
                .contract_details
                .get(leg_instrument_id)
                .map(|entry| entry.value().clone())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Leg instrument {} not found in contract details after loading",
                        leg_instrument_id
                    )
                })?;

            leg_contract_details.push((leg_details, *ratio));
        }

        // Create the spread instrument
        let timestamp = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();
        let leg_details_refs: Vec<(&ibapi::contracts::ContractDetails, i32)> =
            leg_contract_details.iter().map(|(d, r)| (d, *r)).collect();

        let bag_contract = self.create_bag_contract_from_legs(
            &leg_contract_details,
            Some(spread_instrument_id),
            None,
        )?;
        let spread_instrument = parse_spread_instrument_any(
            spread_instrument_id,
            &leg_details_refs,
            Some(&bag_contract),
            Some(timestamp),
        )
        .context("Failed to parse spread instrument")?;

        // Cache the spread instrument
        self.instruments
            .insert(spread_instrument_id, spread_instrument);
        self.contracts.insert(spread_instrument_id, bag_contract);

        if let Some((first_details, _)) = leg_contract_details.first() {
            self.price_magnifiers
                .insert(spread_instrument_id, first_details.price_magnifier);
        }

        tracing::info!(
            "Successfully loaded spread instrument {}",
            spread_instrument_id
        );
        Ok(true)
    }

    /// Load all instruments from provided IDs and contracts.
    ///
    /// This is equivalent to Python's `load_all_async` method.
    /// Python version loads from config's `_load_ids_on_start` and `_load_contracts_on_start`.
    /// Rust version accepts these as parameters for flexibility.
    ///
    /// # Arguments
    ///
    /// * `client` - The IB API client
    /// * `instrument_ids` - Optional vector of instrument IDs to load
    /// * `contracts` - Optional vector of IB contracts to load
    /// * `force_instrument_update` - If true, force re-fetch even if already cached
    ///
    /// # Errors
    ///
    /// Returns an error if loading fails.
    pub async fn load_all_async(
        &self,
        client: &ibapi::Client,
        instrument_ids: Option<Vec<InstrumentId>>,
        contracts: Option<Vec<Contract>>,
        force_instrument_update: bool,
    ) -> anyhow::Result<Vec<InstrumentId>> {
        let mut loaded_ids = Vec::new();

        // Load from instrument IDs
        let ids_to_load =
            instrument_ids.unwrap_or_else(|| self.config.load_ids.iter().cloned().collect());

        if !ids_to_load.is_empty() {
            let mut filters = std::collections::HashMap::new();

            if force_instrument_update {
                filters.insert("force_instrument_update".to_string(), "true".to_string());
            }
            let filters = if filters.is_empty() {
                None
            } else {
                Some(filters)
            };

            let ids_result = self
                .load_ids_with_return_async(client, ids_to_load, filters)
                .await
                .context("Failed to load instruments from IDs")?;
            loaded_ids.extend(ids_result);
        }

        // Load from contracts
        let mut contracts_to_load = contracts.unwrap_or_default();
        if contracts_to_load.is_empty() {
            for contract_json in &self.config.load_contracts {
                let contract = crate::common::contracts::parse_contract_from_json(contract_json)
                    .context("Failed to parse contract from config JSON")?;
                contracts_to_load.push(contract);
            }
        }

        if !contracts_to_load.is_empty() {
            for contract in contracts_to_load {
                match self.get_instrument(client, &contract).await {
                    Ok(Some(instrument)) => {
                        use nautilus_model::instruments::Instrument;
                        let instrument_id = instrument.id();
                        loaded_ids.push(instrument_id);
                        tracing::debug!("Loaded instrument {} from contract", instrument_id);
                    }
                    Ok(None) => {
                        tracing::warn!("Failed to load instrument from contract: {:?}", contract);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Error loading instrument from contract {:?}: {}",
                            contract,
                            e
                        );
                    }
                }
            }
        }

        if loaded_ids.is_empty() {
            tracing::debug!("load_all_async called but no instruments were loaded");
        } else {
            tracing::info!("load_all_async loaded {} instruments", loaded_ids.len());
        }

        Ok(loaded_ids)
    }
}

fn normalize_price_magnifier(price_magnifier: i32) -> i32 {
    if price_magnifier > 0 {
        price_magnifier
    } else {
        1
    }
}

impl InteractiveBrokersInstrumentProvider {
    /// Fetch and cache contract details for an instrument ID using the provided IB client.
    ///
    /// # Arguments
    ///
    /// * `client` - The IB API client
    /// * `instrument_id` - The instrument ID to fetch
    ///
    /// # Errors
    ///
    /// Returns an error if fetching fails.
    pub async fn fetch_contract_details(
        &self,
        client: &ibapi::Client,
        instrument_id: InstrumentId,
        force_instrument_update: bool,
        filters: Option<HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        if !force_instrument_update {
            if self.instruments.contains_key(&instrument_id) {
                tracing::debug!(
                    "Instrument {} already cached, skipping fetch",
                    instrument_id
                );
                return Ok(());
            }
        }
        // Convert instrument ID to IB contract
        let exchange = filters
            .as_ref()
            .and_then(|f| f.get("exchange"))
            .map(|s| s.as_str());

        let exchanges_to_try: Vec<String> = if let Some(exchange) = exchange {
            vec![exchange.to_string()]
        } else {
            possible_exchanges_for_venue(instrument_id.venue.as_str())
        };

        let mut details_vec = Vec::new();
        let mut last_error = None;

        for candidate_exchange in exchanges_to_try {
            let contract = instrument_id_to_ib_contract(instrument_id, Some(candidate_exchange.as_str()))
                .with_context(|| format!("Failed to convert instrument_id {} to IB contract. Check that the instrument ID format is correct and the venue/symbol are valid.", instrument_id))?;

            match client.contract_details(&contract).await {
                Ok(result) if !result.is_empty() => {
                    details_vec = result;
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    last_error = Some((candidate_exchange.clone(), e.to_string()));
                }
            }
        }

        if details_vec.is_empty() {
            if let Some((candidate_exchange, error)) = last_error {
                tracing::warn!(
                    "Failed to fetch contract details for {} on {}: {}",
                    instrument_id,
                    candidate_exchange,
                    error
                );
            } else {
                tracing::warn!(
                    "No contract details returned for {} - instrument may not exist in IB or contract specification is incomplete",
                    instrument_id
                );
            }
            return Ok(());
        }

        // Process the first contract detail (usually there's only one)
        let details = &details_vec[0];

        // Check if security type is filtered
        let sec_type_str = format!("{:?}", details.contract.security_type);
        if self.is_filtered_sec_type(&sec_type_str) {
            tracing::warn!("Skipping filtered security type: {}", sec_type_str);
            return Ok(());
        }

        // Parse to Nautilus instrument
        let parsed_instrument = parse_ib_contract_to_instrument(details, instrument_id)
            .context("Failed to parse IB contract to Nautilus instrument")?;

        // TODO: Filter callable support (Python feature)
        // Python's filter_callable allows custom filtering of instruments via a Python callable.
        // This requires calling Python functions from Rust, which needs GIL handling and Python interop.
        // For now, users can filter instruments in Python after loading if needed.
        // To implement: Accept PyObject callable in config, call it here with parsed_instrument,
        // skip instrument if callable returns False.

        // Cache the instrument and mappings (force update if requested)
        if force_instrument_update || !self.instruments.contains_key(&instrument_id) {
            self.instruments.insert(instrument_id, parsed_instrument);
            self.contract_details.insert(instrument_id, details.clone());
            self.contracts
                .insert(instrument_id, details.contract.clone());
            self.contract_id_to_instrument_id
                .insert(details.contract.contract_id, instrument_id);
            self.price_magnifiers
                .insert(instrument_id, details.price_magnifier);
        }

        tracing::info!(
            "Successfully loaded instrument: {} (price_magnifier: {})",
            instrument_id,
            details.price_magnifier
        );
        Ok(())
    }

    /// Batch load multiple instrument IDs.
    ///
    /// This method fetches and caches contract details for multiple instrument IDs in parallel.
    ///
    /// # Arguments
    ///
    /// * `client` - The IB API client
    /// * `instrument_ids` - Vector of instrument IDs to load
    /// * `filters` - Optional filters to apply (not yet implemented, reserved for future use)
    ///
    /// # Returns
    ///
    /// Returns a vector of successfully loaded instrument IDs.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching fails.
    pub async fn batch_load(
        &self,
        client: &ibapi::Client,
        instrument_ids: Vec<InstrumentId>,
        filters: Option<&[String]>,
    ) -> anyhow::Result<Vec<InstrumentId>> {
        let mut loaded_ids = Vec::new();

        // Apply filters if provided
        let filtered_ids: Vec<InstrumentId> = if let Some(filter_list) = filters {
            // Filter instrument IDs by matching against filter patterns
            // Filters can be:
            // - Security type filters (e.g., "STK", "OPT", "FUT")
            // - Venue filters (e.g., "SMART", "NASDAQ")
            // - Symbol patterns (partial matching)
            instrument_ids
                .into_iter()
                .filter(|instrument_id| {
                    // Check if instrument matches any filter
                    for filter in filter_list {
                        // Check symbol match (case-insensitive partial match)
                        if instrument_id
                            .symbol
                            .as_str()
                            .to_lowercase()
                            .contains(&filter.to_lowercase())
                        {
                            return true;
                        }

                        // Check venue match
                        if instrument_id.venue.as_str() == filter {
                            return true;
                        }

                        // Check security type (try to infer from instrument)
                        if let Some(contract_details) = self.contract_details.get(instrument_id) {
                            let sec_type_str =
                                format!("{:?}", contract_details.contract.security_type);

                            if sec_type_str.to_uppercase().contains(&filter.to_uppercase()) {
                                return true;
                            }
                        }
                    }
                    false
                })
                .collect()
        } else {
            instrument_ids
        };

        // Load instruments sequentially (can be parallelized in future if needed)
        let filtered_count = filtered_ids.len();
        for instrument_id in filtered_ids {
            match self
                .fetch_contract_details(client, instrument_id, false, None)
                .await
            {
                Ok(()) => loaded_ids.push(instrument_id),
                Err(e) => {
                    tracing::warn!("Failed to load instrument {}: {}", instrument_id, e);
                }
            }
        }

        tracing::info!(
            "Batch loaded {} instruments ({} after filtering)",
            loaded_ids.len(),
            filtered_count
        );

        // Save cache if cache_path is configured
        if !loaded_ids.is_empty()
            && let Some(ref cache_path) = self.config.cache_path
            && let Err(e) = self.save_cache(cache_path).await
        {
            tracing::warn!("Failed to save instrument cache to {}: {}", cache_path, e);
        }

        Ok(loaded_ids)
    }

    /// Fetch option chain for a given underlying contract with expiry filtering.
    ///
    /// This is equivalent to Python's `get_option_chain_details_by_range`.
    /// It uses `contract_details` to fetch options with precise expiry filtering,
    /// which is more flexible than the basic `option_chain` API.
    ///
    /// # Arguments
    ///
    /// * `client` - The IB API client
    /// * `underlying` - The underlying contract
    /// * `expiry_min` - Minimum expiry date string (YYYYMMDD format, can be None for no min)
    /// * `expiry_max` - Maximum expiry date string (YYYYMMDD format, can be None for no max)
    ///
    /// # Returns
    ///
    /// Returns the number of option instruments loaded.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching fails.
    pub async fn fetch_option_chain_by_range(
        &self,
        client: &ibapi::Client,
        underlying: &Contract,
        expiry_min: Option<&str>,
        expiry_max: Option<&str>,
    ) -> anyhow::Result<usize> {
        tracing::info!(
            "Building option chain for {}.{} (sec_type={:?}, contract_id={}, expiry_min={:?}, expiry_max={:?}, config_min_days={:?}, config_max_days={:?})",
            underlying.symbol.as_str(),
            underlying.exchange.as_str(),
            underlying.security_type,
            underlying.contract_id,
            expiry_min,
            expiry_max,
            self.config.min_expiry_days,
            self.config.max_expiry_days,
        );

        // First, get option chain metadata to determine expirations
        let symbol = underlying.symbol.as_str();
        let exchange = underlying.exchange.as_str();

        let mut option_chain_stream = client
            .option_chain(
                symbol,
                exchange,
                underlying.security_type.clone(),
                underlying.contract_id,
            )
            .await
            .context("Failed to request option chain from IB")?;

        let mut total_loaded = 0;

        // Get current time for expiry day calculation
        let now = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();

        // Collect all expirations from the metadata
        let mut all_expirations = Vec::new();

        while let Some(result) = option_chain_stream.next().await {
            if let Ok(chain) = result {
                tracing::debug!(
                    "Received option chain metadata exchange={} trading_class={} expirations={} strikes={}",
                    chain.exchange,
                    chain.trading_class,
                    chain.expirations.len(),
                    chain.strikes.len(),
                );

                for expiration in &chain.expirations {
                    // Filter by expiry date string if specified
                    let date_filter_pass = match (expiry_min, expiry_max) {
                        (Some(min), Some(max)) => {
                            expiration.as_str() >= min && expiration.as_str() <= max
                        }
                        (Some(min), None) => expiration.as_str() >= min,
                        (None, Some(max)) => expiration.as_str() <= max,
                        (None, None) => true,
                    };

                    // Filter by expiry days from config if specified
                    let days_filter_pass = {
                        let expiry_ns = crate::providers::parse::expiry_timestring_to_unix_nanos(
                            expiration.as_str(),
                            None,
                        )
                        .unwrap_or(now);
                        let days_until_expiry = (expiry_ns.as_u64().saturating_sub(now.as_u64()))
                            / (24 * 60 * 60 * 1_000_000_000);

                        let min_days_ok = self
                            .config
                            .min_expiry_days
                            .is_none_or(|min| days_until_expiry >= min as u64);
                        let max_days_ok = self
                            .config
                            .max_expiry_days
                            .is_none_or(|max| days_until_expiry <= max as u64);

                        min_days_ok && max_days_ok
                    };

                    if date_filter_pass && days_filter_pass && !all_expirations.contains(expiration)
                    {
                        all_expirations.push(expiration.clone());
                    }
                }
            }
        }

        tracing::info!(
            "Filtered {} option expirations for {}.{}",
            all_expirations.len(),
            underlying.symbol.as_str(),
            underlying.exchange.as_str(),
        );

        // Now fetch contract details for each expiry using contract_details
        for expiration in all_expirations {
            tracing::info!(
                "Requesting option contract details for {}.{} expiry {}",
                underlying.symbol.as_str(),
                underlying.exchange.as_str(),
                expiration,
            );

            let option_contract = Contract {
                contract_id: 0,
                symbol: underlying.symbol.clone(),
                security_type: if underlying.security_type == SecurityType::Future {
                    SecurityType::FuturesOption
                } else {
                    SecurityType::Option
                },
                last_trade_date_or_contract_month: expiration.clone(),
                strike: 0.0,
                right: String::new(),
                multiplier: String::new(),
                exchange: underlying.exchange.clone(),
                currency: underlying.currency.clone(),
                local_symbol: String::new(),
                primary_exchange: Exchange::default(),
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

            match client.contract_details(&option_contract).await {
                Ok(details_vec) => {
                    tracing::info!(
                        "Received {} raw option contract details for {}.{} expiry {}",
                        details_vec.len(),
                        underlying.symbol.as_str(),
                        underlying.exchange.as_str(),
                        expiration,
                    );

                    for details in details_vec {
                        // Filter by underlying contract ID
                        if details.under_contract_id != underlying.contract_id {
                            continue;
                        }

                        let contract_id = details.contract.contract_id;

                        if self.contract_id_to_instrument_id.contains_key(&contract_id) {
                            continue;
                        }

                        let venue = self.determine_venue(&details.contract, Some(&details));
                        let instrument_id = match self.config.symbology_method {
                            crate::config::SymbologyMethod::Simplified => {
                                crate::common::parse::ib_contract_to_instrument_id_simplified(
                                    &details.contract,
                                    Some(venue),
                                )
                            }
                            crate::config::SymbologyMethod::Raw => {
                                crate::common::parse::ib_contract_to_instrument_id_raw(
                                    &details.contract,
                                    Some(venue),
                                )
                            }
                        }
                        .context("Failed to convert IB contract to instrument ID")?;

                        let sec_type_str = format!("{:?}", details.contract.security_type);
                        if self.is_filtered_sec_type(&sec_type_str) {
                            continue;
                        }

                        match parse_ib_contract_to_instrument(&details, instrument_id) {
                            Ok(parsed_instrument) => {
                                self.instruments.insert(instrument_id, parsed_instrument);
                                self.contract_details.insert(instrument_id, details.clone());
                                self.contracts
                                    .insert(instrument_id, details.contract.clone());
                                self.contract_id_to_instrument_id
                                    .insert(contract_id, instrument_id);
                                self.price_magnifiers
                                    .insert(instrument_id, details.price_magnifier);
                                total_loaded += 1;
                            }
                            Err(e) => {
                                tracing::warn!("Failed to parse option instrument: {}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to fetch contract details for expiration {}: {}",
                        expiration,
                        e
                    );
                }
            }
        }

        tracing::info!(
            "Successfully loaded {} option instruments from chain for {}.{}",
            total_loaded,
            underlying.symbol.as_str(),
            underlying.exchange.as_str(),
        );

        // Save cache if cache_path is configured
        if total_loaded > 0
            && let Some(ref cache_path) = self.config.cache_path
            && let Err(e) = self.save_cache(cache_path).await
        {
            tracing::warn!("Failed to save instrument cache to {}: {}", cache_path, e);
        }

        Ok(total_loaded)
    }

    /// Fetch and cache futures chain (all futures contracts for a symbol).
    ///
    /// This method fetches all futures contracts for a given underlying symbol
    /// and populates the cache with all individual futures instruments.
    ///
    /// # Arguments
    ///
    /// * `client` - The IB API client
    /// * `symbol` - The underlying symbol
    /// * `exchange` - The exchange (use "" for all exchanges)
    /// * `currency` - The currency (use USD as default)
    ///
    /// # Returns
    ///
    /// Returns the number of futures instruments loaded.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching fails.
    pub async fn fetch_futures_chain(
        &self,
        client: &ibapi::Client,
        symbol: &str,
        exchange: &str,
        currency: &str,
        min_expiry_days: Option<u32>,
        max_expiry_days: Option<u32>,
    ) -> anyhow::Result<usize> {
        tracing::info!(
            "Building futures chain for {}.{} (currency={}, min_days={:?}, max_days={:?}, config_min_days={:?}, config_max_days={:?})",
            symbol,
            exchange,
            currency,
            min_expiry_days,
            max_expiry_days,
            self.config.min_expiry_days,
            self.config.max_expiry_days,
        );

        // Build futures contract for lookup
        let futures_contract = Contract {
            contract_id: 0, // 0 for lookup by specification
            symbol: Symbol::from(symbol.to_string()),
            security_type: SecurityType::Future,
            last_trade_date_or_contract_month: String::new(),
            strike: 0.0,
            right: String::new(),
            multiplier: String::new(),
            exchange: Exchange::from(exchange.to_string()),
            currency: ibapi::contracts::Currency::from(currency.to_string()),
            local_symbol: String::new(),
            primary_exchange: Exchange::default(),
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

        // Fetch contract details for all matching futures
        let details_vec = client
            .contract_details(&futures_contract)
            .await
            .context("Failed to fetch futures chain from IB")?;

        tracing::info!(
            "Received {} raw futures contract details for {}.{}",
            details_vec.len(),
            symbol,
            exchange,
        );

        let mut total_loaded = 0;
        let now = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();

        for details in details_vec {
            let contract_id = details.contract.contract_id;

            // Check if already cached
            if self.contract_id_to_instrument_id.contains_key(&contract_id) {
                continue;
            }

            // Generate instrument ID using configured symbology method
            let venue = self.determine_venue(&details.contract, Some(&details));
            let instrument_id = match self.config.symbology_method {
                crate::config::SymbologyMethod::Simplified => {
                    crate::common::parse::ib_contract_to_instrument_id_simplified(
                        &details.contract,
                        Some(venue),
                    )
                }
                crate::config::SymbologyMethod::Raw => {
                    crate::common::parse::ib_contract_to_instrument_id_raw(
                        &details.contract,
                        Some(venue),
                    )
                }
            }
            .context("Failed to convert IB contract to instrument ID")?;

            // Check if security type is filtered
            let sec_type_str = format!("{:?}", details.contract.security_type);
            if self.is_filtered_sec_type(&sec_type_str) {
                continue;
            }

            // Filter by expiry days for futures
            if !details
                .contract
                .last_trade_date_or_contract_month
                .is_empty()
                && let Ok(expiry_ns) = crate::providers::parse::expiry_timestring_to_unix_nanos(
                    &details.contract.last_trade_date_or_contract_month,
                    Some(&details),
                )
            {
                let days_until_expiry = (expiry_ns.as_u64().saturating_sub(now.as_u64()))
                    / (24 * 60 * 60 * 1_000_000_000);

                let min_days_ok = min_expiry_days
                    .or(self.config.min_expiry_days)
                    .is_none_or(|min| days_until_expiry >= min as u64);
                let max_days_ok = max_expiry_days
                    .or(self.config.max_expiry_days)
                    .is_none_or(|max| days_until_expiry <= max as u64);

                if !min_days_ok || !max_days_ok {
                    continue;
                }
            }

            // Parse to Nautilus instrument
            match parse_ib_contract_to_instrument(&details, instrument_id) {
                Ok(parsed_instrument) => {
                    // Cache the instrument and mappings
                    self.instruments.insert(instrument_id, parsed_instrument);
                    self.contract_details.insert(instrument_id, details.clone());
                    self.contracts
                        .insert(instrument_id, details.contract.clone());
                    self.contract_id_to_instrument_id
                        .insert(contract_id, instrument_id);
                    self.price_magnifiers
                        .insert(instrument_id, details.price_magnifier);
                    total_loaded += 1;
                }
                Err(e) => {
                    tracing::warn!("Failed to parse futures instrument: {}", e);
                }
            }
        }

        tracing::info!(
            "Successfully loaded {} futures instruments from chain",
            total_loaded
        );

        // Save cache if cache_path is configured
        if total_loaded > 0
            && let Some(ref cache_path) = self.config.cache_path
            && let Err(e) = self.save_cache(cache_path).await
        {
            tracing::warn!("Failed to save instrument cache to {}: {}", cache_path, e);
        }

        Ok(total_loaded)
    }

    /// Fetch and cache a BAG (spread) contract.
    ///
    /// This method fetches contract details for a spread contract by requesting
    /// contract details with a BAG contract. The BAG contract should have its
    /// combo_legs populated with the individual leg contract IDs.
    ///
    /// # Arguments
    ///
    /// * `client` - The IB API client
    /// * `bag_contract` - The BAG contract with populated combo_legs
    ///
    /// # Returns
    ///
    /// Returns the number of spread instruments loaded (0 or 1).
    ///
    /// # Errors
    ///
    /// Returns an error if fetching fails.
    ///
    /// # Notes
    ///
    /// This method now auto-loads all leg instruments from combo_legs and creates
    /// a proper spread instrument, matching Python's `_load_bag_contract` behavior.
    pub async fn fetch_bag_contract(
        &self,
        client: &ibapi::Client,
        bag_contract: &Contract,
    ) -> anyhow::Result<usize> {
        // Validate BAG contract
        if bag_contract.security_type != SecurityType::Spread || bag_contract.combo_legs.is_empty()
        {
            anyhow::bail!(
                "Invalid BAG contract: must have security_type=Spread and non-empty combo_legs"
            );
        }

        tracing::info!(
            "Loading BAG contract with {} legs",
            bag_contract.combo_legs.len()
        );

        // First, load all individual leg instruments and collect their details
        let mut leg_contract_details = Vec::new();
        let mut leg_tuples = Vec::new();

        for combo_leg in &bag_contract.combo_legs {
            // Create a leg contract using information from the combo leg
            let leg_contract = Contract {
                contract_id: combo_leg.contract_id,  // Use conId from combo_leg
                symbol: bag_contract.symbol.clone(), // Use underlying symbol from BAG
                security_type: SecurityType::Option, // Default to Option, will be determined from contract details
                last_trade_date_or_contract_month: String::new(),
                strike: 0.0,
                right: String::new(),
                multiplier: String::new(),
                exchange: Exchange::from(combo_leg.exchange.as_str()),
                currency: bag_contract.currency.clone(), // Use currency from BAG
                local_symbol: String::new(),
                primary_exchange: Exchange::default(),
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

            // Fetch contract details for this leg
            let leg_details_vec =
                client
                    .contract_details(&leg_contract)
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to fetch contract details for leg conId {}",
                            combo_leg.contract_id
                        )
                    })?;

            if leg_details_vec.is_empty() {
                tracing::warn!(
                    "No contract details returned for leg conId {}",
                    combo_leg.contract_id
                );
                continue;
            }

            let leg_details = &leg_details_vec[0];
            let leg_contract_id = leg_details.contract.contract_id;

            // Check if leg is already cached
            let leg_instrument_id =
                if let Some(cached_id) = self.contract_id_to_instrument_id.get(&leg_contract_id) {
                    *cached_id.value()
                } else {
                    // Load the leg instrument
                    let leg_venue = self.determine_venue(&leg_details.contract, Some(leg_details));
                    let leg_instrument_id = match self.config.symbology_method {
                        crate::config::SymbologyMethod::Simplified => {
                            crate::common::parse::ib_contract_to_instrument_id_simplified(
                                &leg_details.contract,
                                Some(leg_venue),
                            )
                        }
                        crate::config::SymbologyMethod::Raw => {
                            crate::common::parse::ib_contract_to_instrument_id_raw(
                                &leg_details.contract,
                                Some(leg_venue),
                            )
                        }
                    }
                    .context("Failed to convert leg contract to instrument ID")?;

                    // Parse and cache the leg instrument
                    let leg_instrument =
                        parse_ib_contract_to_instrument(leg_details, leg_instrument_id)
                            .context("Failed to parse leg instrument")?;

                    self.instruments.insert(leg_instrument_id, leg_instrument);
                    self.contract_details
                        .insert(leg_instrument_id, leg_details.clone());
                    self.contracts
                        .insert(leg_instrument_id, leg_details.contract.clone());
                    self.contract_id_to_instrument_id
                        .insert(leg_contract_id, leg_instrument_id);
                    self.price_magnifiers
                        .insert(leg_instrument_id, leg_details.price_magnifier);

                    leg_instrument_id
                };

            // Determine ratio (positive for BUY, negative for SELL)
            let ratio = if combo_leg.action == "BUY" {
                combo_leg.ratio
            } else {
                -combo_leg.ratio
            };

            // Get the contract details for this leg (should be cached now)
            let leg_details_clone = self
                .contract_details
                .get(&leg_instrument_id)
                .map(|entry| entry.value().clone())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Contract details not found for leg {} after loading",
                        leg_instrument_id
                    )
                })?;

            leg_contract_details.push((leg_details_clone, ratio));
            leg_tuples.push((leg_instrument_id, ratio));
        }

        if leg_tuples.is_empty() {
            anyhow::bail!("No valid legs loaded for BAG contract");
        }

        // Create spread instrument ID from leg tuples
        let spread_instrument_id = create_spread_instrument_id(&leg_tuples)
            .context("Failed to create spread instrument ID from leg tuples")?;

        // Check if spread is already cached
        if self.instruments.contains_key(&spread_instrument_id) {
            tracing::info!("Spread instrument {} already cached", spread_instrument_id);
            return Ok(0);
        }

        // Fetch BAG contract details (for storing the mapping)
        let bag_details_vec = client
            .contract_details(bag_contract)
            .await
            .context("Failed to fetch BAG contract details from IB")?;

        if bag_details_vec.is_empty() {
            tracing::warn!("No contract details returned for BAG contract");
            return Ok(0);
        }

        let bag_details = &bag_details_vec[0];
        let bag_contract_id = bag_details.contract.contract_id;

        // Create the spread instrument
        let timestamp = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();

        // Convert leg_contract_details to the format needed by parse_spread_instrument_id
        let leg_details_refs: Vec<(&ibapi::contracts::ContractDetails, i32)> =
            leg_contract_details.iter().map(|(d, r)| (d, *r)).collect();

        let spread_instrument = parse_spread_instrument_any(
            spread_instrument_id,
            &leg_details_refs,
            Some(&bag_details.contract),
            Some(timestamp),
        )
        .context("Failed to parse spread instrument")?;

        // Cache the spread instrument and mappings
        self.instruments
            .insert(spread_instrument_id, spread_instrument);
        self.contract_details
            .insert(spread_instrument_id, bag_details.clone());
        self.contracts
            .insert(spread_instrument_id, bag_details.contract.clone());
        self.contract_id_to_instrument_id
            .insert(bag_contract_id, spread_instrument_id);
        self.price_magnifiers
            .insert(spread_instrument_id, bag_details.price_magnifier);

        tracing::info!(
            "Successfully loaded spread instrument {} with {} legs",
            spread_instrument_id,
            leg_tuples.len()
        );

        // Save cache if cache_path is configured
        if let Some(ref cache_path) = self.config.cache_path
            && let Err(e) = self.save_cache(cache_path).await
        {
            tracing::warn!("Failed to save instrument cache to {}: {}", cache_path, e);
        }

        Ok(1)
    }

    /// Save the current instrument cache to disk.
    ///
    /// # Arguments
    ///
    /// * `cache_path` - Path to the cache file
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or file I/O fails.
    pub async fn save_cache(&self, cache_path: &str) -> anyhow::Result<()> {
        let cache = InstrumentCache {
            cache_timestamp: Utc::now(),
            contract_id_to_instrument_id: self
                .contract_id_to_instrument_id
                .iter()
                .map(|entry| (*entry.key(), entry.value().to_string()))
                .collect(),
            price_magnifiers: self
                .price_magnifiers
                .iter()
                .map(|entry| (entry.key().to_string(), *entry.value()))
                .collect(),
            instruments: self
                .instruments
                .iter()
                .map(|entry| {
                    let instrument_id = entry.key().to_string();
                    let json =
                        serde_json::to_string(entry.value()).unwrap_or_else(|_| String::new());
                    (instrument_id, json)
                })
                .collect(),
        };

        // Ensure parent directory exists
        if let Some(parent) = Path::new(cache_path).parent() {
            fs::create_dir_all(parent)?;
        }

        // Write cache to file
        let json = serde_json::to_string_pretty(&cache)?;
        fs::write(cache_path, json)?;
        tracing::info!(
            "Saved instrument cache to {} ({} instruments)",
            cache_path,
            cache.instruments.len()
        );
        Ok(())
    }

    /// Load instrument cache from disk if valid.
    ///
    /// # Arguments
    ///
    /// * `cache_path` - Path to the cache file
    ///
    /// # Returns
    ///
    /// Returns `true` if cache was loaded successfully and is valid, `false` otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error if deserialization or file I/O fails (but treats missing file as non-error).
    pub async fn load_cache(&self, cache_path: &str) -> anyhow::Result<bool> {
        // Check if cache file exists
        if !Path::new(cache_path).exists() {
            tracing::debug!("Cache file does not exist: {}", cache_path);
            return Ok(false);
        }

        // Load cache from file
        let json = fs::read_to_string(cache_path)?;
        let cache: InstrumentCache = serde_json::from_str(&json)?;

        // Check cache validity
        if let Some(validity_days) = self.config.cache_validity_days {
            let cache_age = Utc::now() - cache.cache_timestamp;
            let max_age = chrono::Duration::days(validity_days as i64);
            if cache_age > max_age {
                tracing::info!(
                    "Cache is expired (age: {} days, max: {} days). Ignoring cache",
                    cache_age.num_days(),
                    validity_days
                );
                return Ok(false);
            }
        }

        // Deserialize and restore instruments
        let mut loaded_count = 0;

        for (instrument_id_str, instrument_json) in &cache.instruments {
            match InstrumentId::from_str(instrument_id_str) {
                Ok(instrument_id) => match serde_json::from_str::<InstrumentAny>(instrument_json) {
                    Ok(instrument) => {
                        self.instruments.insert(instrument_id, instrument);

                        if let Ok(value) =
                            serde_json::from_str::<serde_json::Value>(instrument_json)
                            && let Some(contract_json) =
                                value.get("info").and_then(|info| info.get("contract"))
                            && let Ok(contract) =
                                crate::common::contracts::parse_contract_from_json(contract_json)
                        {
                            self.contracts.insert(instrument_id, contract);
                        }
                        loaded_count += 1;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to deserialize instrument {}: {}",
                            instrument_id_str,
                            e
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to parse instrument ID {}: {}", instrument_id_str, e);
                }
            }
        }

        // Restore contract ID mappings
        for (contract_id, instrument_id_str) in &cache.contract_id_to_instrument_id {
            if let Ok(instrument_id) = InstrumentId::from_str(instrument_id_str) {
                self.contract_id_to_instrument_id
                    .insert(*contract_id, instrument_id);
            }
        }

        // Restore price magnifiers
        for (instrument_id_str, magnifier) in &cache.price_magnifiers {
            if let Ok(instrument_id) = InstrumentId::from_str(instrument_id_str) {
                self.price_magnifiers.insert(instrument_id, *magnifier);
            }
        }

        tracing::info!(
            "Loaded instrument cache from {} ({} instruments, created at {})",
            cache_path,
            loaded_count,
            cache.cache_timestamp
        );
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use nautilus_core::UnixNanos;
    use nautilus_model::{
        identifiers::{Symbol, Venue},
        instruments::CurrencyPair,
        types::{Price, Quantity, currency::Currency},
    };
    use tempfile::TempDir;

    use super::*;

    fn create_test_provider_with_cache() -> (InteractiveBrokersInstrumentProvider, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir
            .path()
            .join("test_cache.json")
            .to_str()
            .unwrap()
            .to_string();

        let config = InteractiveBrokersInstrumentProviderConfig::builder()
            .cache_path(cache_path)
            .cache_validity_days(7u32)
            .build();

        let provider = InteractiveBrokersInstrumentProvider::new(config);
        (provider, temp_dir)
    }

    fn create_test_instrument(instrument_id: InstrumentId) -> InstrumentAny {
        CurrencyPair::new(
            instrument_id,
            Symbol::from("EUR/USD"),
            Currency::from("EUR"),
            Currency::from("USD"),
            4,
            0,
            Price::from("0.0001"),
            Quantity::from(1),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .into()
    }

    #[tokio::test]
    async fn test_save_cache() {
        let (provider, _temp_dir) = create_test_provider_with_cache();
        let cache_path = provider.config.cache_path.as_ref().unwrap().clone();

        // Add some test instruments
        let instrument_id1 = InstrumentId::new(Symbol::from("EUR/USD"), Venue::from("IDEALPRO"));
        let instrument_id2 = InstrumentId::new(Symbol::from("GBP/USD"), Venue::from("IDEALPRO"));

        let instrument1 = create_test_instrument(instrument_id1);
        let instrument2 = create_test_instrument(instrument_id2);

        provider.instruments.insert(instrument_id1, instrument1);
        provider.instruments.insert(instrument_id2, instrument2);
        provider
            .contract_id_to_instrument_id
            .insert(100, instrument_id1);
        provider
            .contract_id_to_instrument_id
            .insert(200, instrument_id2);
        provider.price_magnifiers.insert(instrument_id1, 1);
        provider.price_magnifiers.insert(instrument_id2, 1);

        // Save cache
        let result = provider.save_cache(&cache_path).await;
        assert!(result.is_ok(), "save_cache should succeed");

        // Verify file exists
        assert!(Path::new(&cache_path).exists(), "Cache file should exist");

        // Verify file contains JSON
        let contents = fs::read_to_string(&cache_path).unwrap();
        assert!(
            contents.contains("EUR/USD"),
            "Cache should contain instrument data"
        );
        assert!(
            contents.contains("cache_timestamp"),
            "Cache should contain timestamp"
        );
    }

    #[tokio::test]
    async fn test_load_cache_valid() {
        let (provider, _temp_dir) = create_test_provider_with_cache();
        let cache_path = provider.config.cache_path.as_ref().unwrap().clone();

        // First save a cache
        let instrument_id = InstrumentId::new(Symbol::from("EUR/USD"), Venue::from("IDEALPRO"));
        let instrument = create_test_instrument(instrument_id);

        provider
            .instruments
            .insert(instrument_id, instrument.clone());
        provider
            .contract_id_to_instrument_id
            .insert(100, instrument_id);
        provider.price_magnifiers.insert(instrument_id, 1);

        provider.save_cache(&cache_path).await.unwrap();

        // Create a new provider and load the cache
        let new_config = InteractiveBrokersInstrumentProviderConfig::builder()
            .cache_path(cache_path.clone())
            .cache_validity_days(7u32)
            .build();

        let new_provider = InteractiveBrokersInstrumentProvider::new(new_config);

        let result = new_provider.load_cache(&cache_path).await;
        assert!(result.is_ok(), "load_cache should succeed");
        assert!(
            result.unwrap(),
            "load_cache should return true for valid cache"
        );

        // Verify instrument was loaded
        assert!(
            new_provider.find(&instrument_id).is_some(),
            "Instrument should be loaded from cache"
        );
        assert_eq!(new_provider.count(), 1, "Provider should have 1 instrument");
    }

    #[tokio::test]
    async fn test_load_cache_missing_file() {
        let (provider, _temp_dir) = create_test_provider_with_cache();
        let cache_path = "/nonexistent/path/cache.json";

        let result = provider.load_cache(cache_path).await;
        assert!(
            result.is_ok(),
            "load_cache should not error on missing file"
        );
        assert!(
            !result.unwrap(),
            "load_cache should return false for missing file"
        );
    }

    #[tokio::test]
    async fn test_load_cache_expired() {
        let (provider, _temp_dir) = create_test_provider_with_cache();
        let cache_path = provider.config.cache_path.as_ref().unwrap().clone();

        // Create an expired cache manually
        let old_timestamp = Utc::now() - chrono::Duration::days(10);
        let expired_cache = InstrumentCache {
            cache_timestamp: old_timestamp,
            contract_id_to_instrument_id: vec![],
            price_magnifiers: vec![],
            instruments: vec![],
        };

        let json = serde_json::to_string_pretty(&expired_cache).unwrap();
        fs::write(&cache_path, json).unwrap();

        // Try to load with validity_days = 7
        let result = provider.load_cache(&cache_path).await;
        assert!(
            result.is_ok(),
            "load_cache should not error on expired cache"
        );
        assert!(
            !result.unwrap(),
            "load_cache should return false for expired cache"
        );
    }
}
