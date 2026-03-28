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

//! Rithmic instrument provider implementation.

use std::{fmt::Debug, sync::Arc};

use dashmap::DashMap;
use rithmic_rs::rti::ResponseFrontMonthContract;
use rithmic_rs::rti::messages::RithmicMessage;
use rithmic_rs::rti::request_search_symbols::{InstrumentType, Pattern};

use crate::common::consts::exchanges::KNOWN_EXCHANGES;
use crate::error::{Result, RithmicError};
use crate::gateway::RithmicGateway;

use super::parse::response_to_instrument;

/// Instrument definition from Rithmic.
#[derive(Debug, Clone)]
pub struct RithmicInstrument {
    /// Full symbol (e.g., "ESZ4").
    pub symbol: String,
    /// Exchange code (e.g., "CME").
    pub exchange: String,
    /// Product code (e.g., "ES").
    pub product_code: String,
    /// Instrument description.
    pub description: String,
    /// Tick size (minimum price increment).
    pub tick_size: f64,
    /// Point value (dollar value per point).
    pub point_value: f64,
    /// Currency code.
    pub currency: String,
    /// Contract size/multiplier.
    pub contract_size: f64,
    /// Price precision (decimal places).
    pub price_precision: u8,
    /// Size precision (decimal places).
    pub size_precision: u8,
    /// Expiration date (Unix timestamp in nanoseconds).
    pub expiration_ts: Option<u64>,
    /// Whether instrument is tradeable.
    pub is_tradeable: bool,
}

/// Provides instrument definitions from Rithmic.
///
/// This provider connects to Rithmic's ticker plant to fetch instrument
/// reference data including tick sizes, point values, and contract specifications.
///
/// # Example
///
/// ```rust,ignore
/// use nautilus_rithmic::{RithmicGateway, RithmicInstrumentProvider, GatewayConfig};
///
/// let config = GatewayConfig::from_env()?;
/// let gateway = Arc::new(tokio::sync::RwLock::new(RithmicGateway::new(config)));
/// gateway.write().await.connect().await?;
/// let provider = RithmicInstrumentProvider::new(Arc::clone(&gateway));
/// let instrument = provider.load_instrument_async("ESH5", "CME").await?;
/// println!("Tick size: {}", instrument.tick_size);
/// ```
pub struct RithmicInstrumentProvider {
    gateway: Arc<tokio::sync::RwLock<RithmicGateway>>,
    instruments: DashMap<String, RithmicInstrument>,
    loaded_exchanges: tokio::sync::RwLock<Vec<String>>,
}

impl RithmicInstrumentProvider {
    fn front_month_contract(front_month: &ResponseFrontMonthContract) -> Result<(&str, &str)> {
        let symbol = front_month
            .trading_symbol
            .as_deref()
            .or(front_month.symbol.as_deref())
            .ok_or_else(|| {
                RithmicError::Instrument("No symbol in front month response".to_string())
            })?;

        let exchange = front_month
            .trading_exchange
            .as_deref()
            .or(front_month.exchange.as_deref())
            .ok_or_else(|| {
                RithmicError::Instrument("No exchange in front month response".to_string())
            })?;

        Ok((symbol, exchange))
    }

    /// Creates a new instrument provider with the given gateway.
    ///
    /// The gateway should be connected before calling the async loading methods.
    pub fn new(gateway: Arc<tokio::sync::RwLock<RithmicGateway>>) -> Self {
        Self {
            gateway,
            instruments: DashMap::new(),
            loaded_exchanges: tokio::sync::RwLock::new(Vec::new()),
        }
    }

    /// Returns a reference to the gateway.
    pub fn gateway(&self) -> &Arc<tokio::sync::RwLock<RithmicGateway>> {
        &self.gateway
    }

    // Async loading methods.

    /// Loads all available instruments from all known exchanges.
    ///
    /// This iterates through `KNOWN_EXCHANGES` and loads instruments from each.
    /// This can be a slow operation depending on how many instruments are available.
    ///
    /// # Returns
    ///
    /// The total number of instruments loaded across all exchanges.
    pub async fn load_all_async(&self) -> Result<usize> {
        let mut total_loaded = 0;

        for exchange in KNOWN_EXCHANGES {
            match self.load_exchange_async(exchange).await {
                Ok(instruments) => {
                    tracing::debug!("Loaded {} instruments from {}", instruments.len(), exchange);
                    total_loaded += instruments.len();
                }
                Err(e) => {
                    tracing::warn!("Failed to load instruments from {}: {}", exchange, e);
                    // Continue with other exchanges
                }
            }
        }

        Ok(total_loaded)
    }

    /// Loads instruments for a specific exchange.
    ///
    /// Uses `search_symbols` to find all futures instruments on the exchange,
    /// then loads reference data for each one.
    ///
    /// # Arguments
    ///
    /// * `exchange` - The exchange code (e.g., "CME", "NYMEX")
    ///
    /// # Returns
    ///
    /// A vector of loaded instruments.
    pub async fn load_exchange_async(&self, exchange: &str) -> Result<Vec<RithmicInstrument>> {
        let responses = {
            let gateway = self.gateway.read().await;
            let ticker = gateway.ticker_handle().ok_or(RithmicError::NotConnected)?;

            ticker
                .search_symbols(
                    "",                           // Empty search text = all
                    Some(exchange),               // Filter by exchange
                    None,                         // No product code filter
                    Some(InstrumentType::Future), // Futures only
                    Some(Pattern::Contains),      // Contains pattern
                )
                .await
                .map_err(|e| RithmicError::Api(format!("Symbol search failed: {e}")))?
        };

        let mut instruments = Vec::new();
        let mut symbols_to_load: Vec<String> = Vec::new();

        // Collect symbols from search results
        for response in &responses {
            if let Some(error) = &response.error {
                tracing::warn!("Search response error: {}", error);
                continue;
            }

            if let RithmicMessage::ResponseSearchSymbols(search_result) = &response.message
                && let Some(symbol) = &search_result.symbol
            {
                symbols_to_load.push(symbol.clone());
            }
        }

        tracing::debug!(
            "Found {} symbols on {}, loading reference data...",
            symbols_to_load.len(),
            exchange
        );

        // Load reference data for each symbol
        for symbol in symbols_to_load {
            match self.load_instrument_async(&symbol, exchange).await {
                Ok(instrument) => {
                    instruments.push(instrument);
                }
                Err(e) => {
                    tracing::debug!("Failed to load reference data for {}: {}", symbol, e);
                    // Continue with other symbols
                }
            }
        }

        // Mark exchange as loaded
        {
            let mut loaded = self.loaded_exchanges.write().await;
            if !loaded.contains(&exchange.to_string()) {
                loaded.push(exchange.to_string());
            }
        }

        Ok(instruments)
    }

    /// Loads a single instrument by symbol and exchange.
    ///
    /// If the instrument is already cached, returns the cached version.
    /// Otherwise, fetches reference data from Rithmic.
    ///
    /// # Arguments
    ///
    /// * `symbol` - The instrument symbol (e.g., "ESH5")
    /// * `exchange` - The exchange code (e.g., "CME")
    ///
    /// # Returns
    ///
    /// The loaded instrument.
    pub async fn load_instrument_async(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> Result<RithmicInstrument> {
        // Check cache first
        let cache_key = format!("{exchange}:{symbol}");
        if let Some(instrument) = self.instruments.get(&cache_key) {
            return Ok(instrument.clone());
        }

        // Request reference data from Rithmic
        let response = {
            let gateway = self.gateway.read().await;
            let ticker = gateway.ticker_handle().ok_or(RithmicError::NotConnected)?;

            ticker
                .get_reference_data(symbol, exchange)
                .await
                .map_err(|e| RithmicError::Api(format!("Reference data request failed: {e}")))?
        };

        // Check for errors in response
        if let Some(error) = &response.error {
            return Err(RithmicError::Instrument(format!(
                "Reference data error for {symbol}: {error}"
            )));
        }

        // Parse the response
        match &response.message {
            RithmicMessage::ResponseReferenceData(ref_data) => {
                let instrument = response_to_instrument(ref_data)?;
                self.cache_instrument(instrument.clone());
                Ok(instrument)
            }
            other => Err(RithmicError::Instrument(format!(
                "Unexpected response type for reference data: {:?}",
                std::mem::discriminant(other)
            ))),
        }
    }

    /// Loads the current front month contract for a product.
    ///
    /// This is useful for getting the active contract without knowing
    /// the specific expiration month.
    ///
    /// # Arguments
    ///
    /// * `product` - The product code (e.g., "ES", "NQ", "CL")
    /// * `exchange` - The exchange code (e.g., "CME", "NYMEX")
    ///
    /// # Returns
    ///
    /// The front month instrument with full reference data.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Get the current front month ES contract
    /// let es_front = provider.load_front_month("ES", "CME").await?;
    /// println!("Front month: {}", es_front.symbol); // e.g., "ESH5"
    /// ```
    pub async fn load_front_month(
        &self,
        product: &str,
        exchange: &str,
    ) -> Result<RithmicInstrument> {
        // Request front month contract
        let response = {
            let gateway = self.gateway.read().await;
            let ticker = gateway.ticker_handle().ok_or(RithmicError::NotConnected)?;

            ticker
                .get_front_month_contract(product, exchange, false)
                .await
                .map_err(|e| RithmicError::Api(format!("Front month request failed: {e}")))?
        };

        // Check for errors
        if let Some(error) = &response.error {
            return Err(RithmicError::Instrument(format!(
                "Front month error for {product}: {error}"
            )));
        }

        // Parse the front month response to get the symbol
        match &response.message {
            RithmicMessage::ResponseFrontMonthContract(front_month) => {
                let (symbol, resolved_exchange) = Self::front_month_contract(front_month)?;

                // Now load the full reference data for this symbol
                self.load_instrument_async(symbol, resolved_exchange).await
            }
            other => Err(RithmicError::Instrument(format!(
                "Unexpected response type for front month: {:?}",
                std::mem::discriminant(other)
            ))),
        }
    }

    // Instrument cache methods (NautilusTrader standard interface).

    /// Caches multiple instruments.
    ///
    /// This is the standard NautilusTrader method for batch caching.
    pub fn cache_instruments(&self, instruments: Vec<RithmicInstrument>) {
        for instrument in instruments {
            self.cache_instrument(instrument);
        }
    }

    /// Caches a single instrument.
    ///
    /// This is the standard NautilusTrader method for single instrument caching.
    pub fn cache_instrument(&self, instrument: RithmicInstrument) {
        let key = format!("{}:{}", instrument.exchange, instrument.symbol);
        self.instruments.insert(key, instrument.clone());
        // Also store by symbol alone for convenience
        self.instruments
            .insert(instrument.symbol.clone(), instrument);
    }

    /// Gets an instrument by symbol.
    ///
    /// This is the standard NautilusTrader method for instrument retrieval.
    pub fn get_instrument(&self, symbol: &str) -> Option<RithmicInstrument> {
        self.instruments.get(symbol).map(|r| r.clone())
    }

    // Additional convenience methods.

    /// Returns a loaded instrument by symbol (alias for `get_instrument`).
    pub fn get(&self, symbol: &str) -> Option<RithmicInstrument> {
        self.get_instrument(symbol)
    }

    /// Returns a loaded instrument by symbol and exchange.
    pub fn get_by_exchange(&self, symbol: &str, exchange: &str) -> Option<RithmicInstrument> {
        let key = format!("{exchange}:{symbol}");
        self.instruments
            .get(&key)
            .or_else(|| self.instruments.get(symbol))
            .map(|r| r.clone())
    }

    /// Returns all loaded instruments.
    ///
    /// Note: Each instrument is stored under two keys internally for fast lookup,
    /// but this method returns each unique instrument only once.
    pub fn instruments(&self) -> Vec<RithmicInstrument> {
        // Only return entries keyed by "exchange:symbol" to avoid duplicates
        self.instruments
            .iter()
            .filter(|r| r.key().contains(':'))
            .map(|r| r.clone())
            .collect()
    }

    /// Returns all instruments for a specific exchange.
    pub fn instruments_for_exchange(&self, exchange: &str) -> Vec<RithmicInstrument> {
        let prefix = format!("{exchange}:");
        self.instruments
            .iter()
            .filter(|r| r.key().starts_with(&prefix))
            .map(|r| r.clone())
            .collect()
    }

    /// Returns the number of unique loaded instruments.
    pub fn count(&self) -> usize {
        // Count only "exchange:symbol" keys to get unique count
        self.instruments
            .iter()
            .filter(|r| r.key().contains(':'))
            .count()
    }

    /// Returns the list of loaded exchanges.
    pub async fn loaded_exchanges(&self) -> Vec<String> {
        self.loaded_exchanges.read().await.clone()
    }

    /// Clears all cached instruments.
    pub async fn clear(&self) {
        self.instruments.clear();
        self.loaded_exchanges.write().await.clear();
    }

    /// Adds an instrument to the cache.
    ///
    /// Deprecated: Use `cache_instrument` instead for NautilusTrader compatibility.
    #[deprecated(since = "0.1.0", note = "Use cache_instrument instead")]
    pub fn add_instrument(&self, instrument: RithmicInstrument) {
        self.cache_instrument(instrument);
    }
}

impl Debug for RithmicInstrumentProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(RithmicInstrumentProvider))
            .field("instrument_count", &self.instruments.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RithmicEnv;
    use crate::gateway::GatewayConfig;

    fn test_gateway() -> Arc<tokio::sync::RwLock<RithmicGateway>> {
        let config = GatewayConfig::new(
            RithmicEnv::Demo,
            "user",
            "pass",
            "system",
            "fcm",
            "ib",
            "account",
        );
        Arc::new(tokio::sync::RwLock::new(RithmicGateway::new(config)))
    }

    fn test_instrument() -> RithmicInstrument {
        RithmicInstrument {
            symbol: "ESZ4".to_string(),
            exchange: "CME".to_string(),
            product_code: "ES".to_string(),
            description: "E-mini S&P 500 Dec 2024".to_string(),
            tick_size: 0.25,
            point_value: 50.0,
            currency: "USD".to_string(),
            contract_size: 1.0,
            price_precision: 2,
            size_precision: 0,
            expiration_ts: None,
            is_tradeable: true,
        }
    }

    #[rstest::rstest]
    fn test_instrument_provider_creation() {
        let gateway = test_gateway();
        let provider = RithmicInstrumentProvider::new(gateway);
        assert_eq!(provider.count(), 0);
    }

    #[rstest::rstest]
    fn test_cache_instrument() {
        let gateway = test_gateway();
        let provider = RithmicInstrumentProvider::new(gateway);
        let instrument = test_instrument();

        provider.cache_instrument(instrument);
        assert_eq!(provider.count(), 1);

        let retrieved = provider.get_instrument("ESZ4").unwrap();
        assert_eq!(retrieved.symbol, "ESZ4");
        assert_eq!(retrieved.tick_size, 0.25);
    }

    #[rstest::rstest]
    fn test_cache_instruments_batch() {
        let gateway = test_gateway();
        let provider = RithmicInstrumentProvider::new(gateway);

        let instruments = vec![
            RithmicInstrument {
                symbol: "ESZ4".to_string(),
                exchange: "CME".to_string(),
                product_code: "ES".to_string(),
                description: "E-mini S&P 500".to_string(),
                tick_size: 0.25,
                point_value: 50.0,
                currency: "USD".to_string(),
                contract_size: 1.0,
                price_precision: 2,
                size_precision: 0,
                expiration_ts: None,
                is_tradeable: true,
            },
            RithmicInstrument {
                symbol: "NQZ4".to_string(),
                exchange: "CME".to_string(),
                product_code: "NQ".to_string(),
                description: "E-mini NASDAQ 100".to_string(),
                tick_size: 0.25,
                point_value: 20.0,
                currency: "USD".to_string(),
                contract_size: 1.0,
                price_precision: 2,
                size_precision: 0,
                expiration_ts: None,
                is_tradeable: true,
            },
        ];

        provider.cache_instruments(instruments);
        assert_eq!(provider.count(), 2);

        assert!(provider.get_instrument("ESZ4").is_some());
        assert!(provider.get_instrument("NQZ4").is_some());
    }

    #[rstest::rstest]
    fn test_get_by_exchange() {
        let gateway = test_gateway();
        let provider = RithmicInstrumentProvider::new(gateway);
        provider.cache_instrument(test_instrument());

        let retrieved = provider.get_by_exchange("ESZ4", "CME").unwrap();
        assert_eq!(retrieved.exchange, "CME");
    }

    #[rstest::rstest]
    fn test_instruments_for_exchange() {
        let gateway = test_gateway();
        let provider = RithmicInstrumentProvider::new(gateway);
        provider.cache_instrument(test_instrument());

        let cme_instruments = provider.instruments_for_exchange("CME");
        assert_eq!(cme_instruments.len(), 1);

        let nymex_instruments = provider.instruments_for_exchange("NYMEX");
        assert!(nymex_instruments.is_empty());
    }

    #[rstest::rstest]
    fn test_gateway_reference() {
        let gateway = test_gateway();
        let provider = RithmicInstrumentProvider::new(Arc::clone(&gateway));

        // Provider should hold reference to same gateway
        assert!(!provider.gateway().blocking_read().is_connected());
    }

    #[rstest::rstest]
    fn test_front_month_contract_prefers_trading_symbol_and_exchange() {
        let response = ResponseFrontMonthContract {
            template_id: 0,
            user_msg: Vec::new(),
            rp_code: Vec::new(),
            symbol: Some("MNQ".to_string()),
            exchange: Some("CME".to_string()),
            is_front_month_symbol: Some(true),
            symbol_name: None,
            trading_symbol: Some("MNQM26".to_string()),
            trading_exchange: Some("CME".to_string()),
        };

        let (symbol, exchange) = RithmicInstrumentProvider::front_month_contract(&response)
            .expect("front month contract should parse");

        assert_eq!(symbol, "MNQM26");
        assert_eq!(exchange, "CME");
    }
}
