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

//! WebSocket client for dYdX v4 API.
//!
//! This client provides streaming connectivity to dYdX's WebSocket API for both
//! public market data and private account updates.
//!
//! # Authentication
//!
//! dYdX v4 uses Cosmos SDK wallet-based authentication. Unlike traditional exchanges:
//! - **Public channels** require no authentication.
//! - **Private channels** (subaccounts) only require the wallet address in the subscription message.
//! - No signature or API key is needed for WebSocket connections themselves.
//!
//! # References
//!
//! <https://docs.dydx.trade/developers/indexer/websockets>

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use dashmap::DashMap;
use nautilus_model::{
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
};
use ustr::Ustr;

use super::error::{DydxWsError, DydxWsResult};
use crate::common::credential::DydxCredential;

/// WebSocket client for dYdX v4 market data and account streams.
///
/// # Authentication
///
/// dYdX v4 does not require traditional API key signatures for WebSocket connections.
/// Public channels work without any credentials. Private channels (subaccounts) only
/// need the wallet address included in the subscription message.
///
/// The [`DydxCredential`] stored in this client is used for:
/// - Providing the wallet address for private channel subscriptions
/// - Transaction signing (when placing orders via the validator node)
///
/// It is **NOT** used for WebSocket message signing or authentication.
#[derive(Debug)]
#[allow(dead_code)] // TODO: Remove once implementation is complete
pub struct DydxWebSocketClient {
    /// The WebSocket connection URL.
    url: String,
    /// Optional credential for private channels (only wallet address is used).
    credential: Option<DydxCredential>,
    /// Whether authentication is required for this client.
    requires_auth: bool,
    /// Whether the client is currently connected.
    is_connected: Arc<AtomicBool>,
    /// Cached instruments for parsing market data.
    instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
    /// Optional account ID for account message parsing.
    account_id: Option<AccountId>,
}

impl DydxWebSocketClient {
    /// Creates a new public WebSocket client for market data.
    #[must_use]
    pub fn new_public(url: String, _heartbeat: Option<u64>) -> Self {
        Self {
            url,
            credential: None,
            requires_auth: false,
            is_connected: Arc::new(AtomicBool::new(false)),
            instruments_cache: Arc::new(DashMap::new()),
            account_id: None,
        }
    }

    /// Creates a new private WebSocket client for account updates.
    #[must_use]
    pub fn new_private(
        url: String,
        credential: DydxCredential,
        account_id: AccountId,
        _heartbeat: Option<u64>,
    ) -> Self {
        Self {
            url,
            credential: Some(credential),
            requires_auth: true,
            is_connected: Arc::new(AtomicBool::new(false)),
            instruments_cache: Arc::new(DashMap::new()),
            account_id: Some(account_id),
        }
    }

    /// Returns the credential associated with this client, if any.
    #[must_use]
    pub fn credential(&self) -> Option<&DydxCredential> {
        self.credential.as_ref()
    }

    /// Returns `true` when the client is connected.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    /// Sets the account ID for account message parsing.
    pub fn set_account_id(&mut self, account_id: AccountId) {
        self.account_id = Some(account_id);
    }

    /// Returns the account ID if set.
    #[must_use]
    pub fn account_id(&self) -> Option<AccountId> {
        self.account_id
    }

    /// Caches a single instrument.
    ///
    /// Any existing instrument with the same ID will be replaced.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        let symbol = instrument.id().symbol.inner();
        self.instruments_cache.insert(symbol, instrument);
    }

    /// Caches multiple instruments.
    ///
    /// Any existing instruments with the same IDs will be replaced.
    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        for instrument in instruments {
            self.instruments_cache
                .insert(instrument.id().symbol.inner(), instrument);
        }
    }

    /// Returns a reference to the instruments cache.
    #[must_use]
    pub fn instruments(&self) -> &Arc<DashMap<Ustr, InstrumentAny>> {
        &self.instruments_cache
    }

    /// Retrieves an instrument from the cache by symbol.
    ///
    /// Returns `None` if the instrument is not found.
    #[must_use]
    pub fn get_instrument(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments_cache.get(symbol).map(|r| r.clone())
    }

    /// Subscribes to public trade updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#trades-channel>
    pub async fn subscribe_trades(&self, instrument_id: InstrumentId) -> DydxWsResult<()> {
        // TODO: Implement subscription
        tracing::debug!("Subscribe to trades for {instrument_id}");
        Ok(())
    }

    /// Unsubscribes from public trade updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_trades(&self, instrument_id: InstrumentId) -> DydxWsResult<()> {
        // TODO: Implement unsubscription
        tracing::debug!("Unsubscribe from trades for {instrument_id}");
        Ok(())
    }

    /// Subscribes to orderbook updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#orderbook-channel>
    pub async fn subscribe_orderbook(&self, instrument_id: InstrumentId) -> DydxWsResult<()> {
        // TODO: Implement subscription
        tracing::debug!("Subscribe to orderbook for {instrument_id}");
        Ok(())
    }

    /// Unsubscribes from orderbook updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_orderbook(&self, instrument_id: InstrumentId) -> DydxWsResult<()> {
        // TODO: Implement unsubscription
        tracing::debug!("Unsubscribe from orderbook for {instrument_id}");
        Ok(())
    }

    /// Subscribes to candle/kline updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#candles-channel>
    pub async fn subscribe_candles(
        &self,
        instrument_id: InstrumentId,
        resolution: &str,
    ) -> DydxWsResult<()> {
        // TODO: Implement subscription
        tracing::debug!("Subscribe to candles for {instrument_id} with resolution {resolution}");
        Ok(())
    }

    /// Unsubscribes from candle/kline updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_candles(
        &self,
        instrument_id: InstrumentId,
        resolution: &str,
    ) -> DydxWsResult<()> {
        // TODO: Implement unsubscription
        tracing::debug!(
            "Unsubscribe from candles for {instrument_id} with resolution {resolution}"
        );
        Ok(())
    }

    /// Subscribes to market updates for all instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#markets-channel>
    pub async fn subscribe_markets(&self) -> DydxWsResult<()> {
        // TODO: Implement subscription
        tracing::debug!("Subscribe to markets");
        Ok(())
    }

    /// Unsubscribes from market updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_markets(&self) -> DydxWsResult<()> {
        // TODO: Implement unsubscription
        tracing::debug!("Unsubscribe from markets");
        Ok(())
    }

    /// Subscribes to subaccount updates (orders, fills, positions, balances).
    ///
    /// This requires authentication and will only work for private WebSocket clients
    /// created with [`Self::new_private`].
    ///
    /// # Errors
    ///
    /// Returns an error if the client was not created with credentials or if the
    /// subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#subaccounts-channel>
    pub async fn subscribe_subaccount(
        &self,
        address: &str,
        subaccount_number: u32,
    ) -> DydxWsResult<()> {
        if !self.requires_auth {
            return Err(DydxWsError::Authentication(
                "Subaccount subscriptions require authentication. Use new_private() to create an authenticated client".to_string(),
            ));
        }

        // TODO: Implement subscription
        tracing::debug!("Subscribe to subaccount {address}/{subaccount_number}");
        Ok(())
    }

    /// Unsubscribes from subaccount updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_subaccount(
        &self,
        address: &str,
        subaccount_number: u32,
    ) -> DydxWsResult<()> {
        // TODO: Implement unsubscription
        tracing::debug!("Unsubscribe from subaccount {address}/{subaccount_number}");
        Ok(())
    }

    /// Subscribes to block height updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#block-height-channel>
    pub async fn subscribe_block_height(&self) -> DydxWsResult<()> {
        // TODO: Implement subscription
        tracing::debug!("Subscribe to block height");
        Ok(())
    }

    /// Unsubscribes from block height updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_block_height(&self) -> DydxWsResult<()> {
        // TODO: Implement unsubscription
        tracing::debug!("Unsubscribe from block height");
        Ok(())
    }
}
