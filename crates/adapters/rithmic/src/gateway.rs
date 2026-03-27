//! Central gateway for managing Rithmic plant connections.
//!
//! The `RithmicGateway` provides a single point of connection management for all
//! Rithmic plants (ticker, order, pnl, history). It handles:
//! - Plant lifecycle (connect, disconnect, reconnect)
//! - Background message processing tasks
//! - Event channels for downstream consumers
//! - Shared instrument state
//!
//! # Example
//!
//! ```rust,ignore
//! use nautilus_rithmic::{RithmicGateway, GatewayConfig};
//!
//! let config = GatewayConfig::from_env()?;
//! let mut gateway = RithmicGateway::new(config);
//! gateway.connect().await?;
//! ```

use ahash::AHashMap;
use arc_swap::ArcSwap;
use dashmap::DashMap;
use std::{env, sync::Arc, time::Duration};
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{debug, error, info, warn};

use crate::{
    common::enums::ConnectionState,
    config::{RithmicEnv, optional_env_var, parse_rithmic_env, required_env_var},
    data::{MarketDataEvent, RithmicBarType},
    error::{Result, RithmicError},
    execution::{ExecutionEvent, ExecutionHandler, RithmicExecutionClient},
    providers::{AccountEvent, PositionEvent},
};

use rithmic_rs::{
    RithmicConfig,
    api::RithmicResponse,
    plants::history_plant::{RithmicHistoryPlant, RithmicHistoryPlantHandle},
    plants::order_plant::{RithmicOrderPlant, RithmicOrderPlantHandle},
    plants::pnl_plant::{RithmicPnlPlant, RithmicPnlPlantHandle},
    plants::ticker_plant::{RithmicTickerPlant, RithmicTickerPlantHandle},
    rti::messages::RithmicMessage,
    rti::request_time_bar_replay::BarType as TimeBarType,
    rti::request_time_bar_update::{BarType as LiveTimeBarType, Request as LiveTimeBarRequest},
    ws::ConnectStrategy,
};

/// Maximum reconnection attempts before giving up.
const MAX_RECONNECT_ATTEMPTS: u32 = 100;

const DEFAULT_APP_NAME: &str = "fufo:fund-forge";
const DEFAULT_APP_VERSION: &str = "1.0";

/// Initial backoff duration for reconnection.
const INITIAL_BACKOFF_MS: u64 = 1000;

/// Maximum backoff duration for reconnection.
const MAX_BACKOFF_MS: u64 = 30000;

/// Maximum time to wait for an individual plant logout before aborting it.
const DISCONNECT_TIMEOUT_SECS: u64 = 3;

fn normalize_server_name(server: &str) -> String {
    server
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn resolve_server_endpoint(server: &str) -> Result<&'static str> {
    match normalize_server_name(server).as_str() {
        "chicago" => Ok("wss://rprotocol.rithmic.com:443"),
        "sydney" => Ok("wss://rprotocol-au.rithmic.com:443"),
        "saopaulo" => Ok("wss://rprotocol-br.rithmic.com:443"),
        "colo75" => Ok("wss://protocol-colo75.rithmic.com:443"),
        "frankfurt" => Ok("wss://rprotocol-de.rithmic.com:443"),
        "hongkong" => Ok("wss://rprotocol-hk.rithmic.com:443"),
        "ireland" => Ok("wss://rprotocol-ie.rithmic.com:443"),
        "mumbai" => Ok("wss://rprotocol-in.rithmic.com:443"),
        "seoul" => Ok("wss://rprotocol-kr.rithmic.com:443"),
        "capetown" => Ok("wss://rprotocol-za.rithmic.com:443"),
        "tokyo" => Ok("wss://rprotocol-jp.rithmic.com:443"),
        "singapore" => Ok("wss://rprotocol-sg.rithmic.com:443"),
        "test" => Ok("wss://rituz00100.rithmic.com:443"),
        _ => Err(RithmicError::Config(format!(
            "Unknown Rithmic server {server:?}. Expected one of: Chicago, Sydney, Sao Paulo, Colo75, Frankfurt, Hong Kong, Ireland, Mumbai, Seoul, Cape Town, Tokyo, Singapore, Test"
        ))),
    }
}

/// Configuration for the Rithmic gateway.
///
/// This unified configuration contains all credentials needed to connect
/// to any Rithmic plant. Use `from_env()` to load from environment variables.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// Rithmic environment (Demo, Live, Test).
    pub environment: RithmicEnv,
    /// Rithmic username.
    pub username: String,
    /// Rithmic password.
    pub password: String,
    /// System name for Rithmic connection.
    pub system_name: String,
    /// Application name sent during login.
    pub app_name: String,
    /// Application version sent during login.
    pub app_version: String,
    /// FCM ID (Futures Commission Merchant).
    pub fcm_id: String,
    /// IB ID (Introducing Broker).
    pub ib_id: String,
    /// Trading account ID.
    pub account_id: String,
    /// Optional named primary Rithmic server.
    pub server: Option<String>,
    /// Optional named alternate Rithmic server.
    pub alt_server: Option<String>,
    /// Optional primary WebSocket URL override.
    pub url_override: Option<String>,
    /// Optional alternate WebSocket URL override.
    pub beta_url_override: Option<String>,
    /// Whether to connect the ticker plant.
    pub enable_ticker: bool,
    /// Whether to connect the order plant.
    pub enable_order: bool,
    /// Whether to connect the PnL plant.
    pub enable_pnl: bool,
    /// Whether to connect the history plant (lazy by default).
    pub enable_history: bool,
}

impl GatewayConfig {
    /// Creates a new gateway configuration.
    pub fn new(
        environment: RithmicEnv,
        username: impl Into<String>,
        password: impl Into<String>,
        system_name: impl Into<String>,
        fcm_id: impl Into<String>,
        ib_id: impl Into<String>,
        account_id: impl Into<String>,
    ) -> Self {
        Self {
            environment,
            username: username.into(),
            password: password.into(),
            system_name: system_name.into(),
            app_name: DEFAULT_APP_NAME.to_string(),
            app_version: DEFAULT_APP_VERSION.to_string(),
            fcm_id: fcm_id.into(),
            ib_id: ib_id.into(),
            account_id: account_id.into(),
            server: None,
            alt_server: None,
            url_override: None,
            beta_url_override: None,
            enable_ticker: true,
            enable_order: true,
            enable_pnl: true,
            enable_history: false, // Lazy by default
        }
    }

    /// Creates configuration from environment variables.
    ///
    /// Required environment variables:
    /// - `RITHMIC_USERNAME`
    /// - `RITHMIC_PASSWORD`
    /// - `RITHMIC_SYSTEM_NAME`
    /// - `RITHMIC_ACCOUNT_ID`
    /// - `RITHMIC_ENV` (optional, defaults to "demo")
    ///
    /// Optional environment variables:
    /// - `RITHMIC_APP_NAME`
    /// - `RITHMIC_APP_VERSION`
    /// - `RITHMIC_FCM_ID`
    /// - `RITHMIC_IB_ID`
    pub fn from_env() -> Result<Self> {
        Self::from_env_with_profile(None)
    }

    /// Creates configuration from environment variables, optionally scoped by profile.
    pub fn from_env_with_profile(profile: Option<&str>) -> Result<Self> {
        let environment = optional_env_var("ENV", profile)?
            .map(|s| parse_rithmic_env(&s))
            .unwrap_or(Ok(RithmicEnv::Demo))?;

        Ok(Self {
            environment,
            username: required_env_var("USERNAME", profile)?,
            password: required_env_var("PASSWORD", profile)?,
            system_name: required_env_var("SYSTEM_NAME", profile)?,
            app_name: optional_env_var("APP_NAME", profile)?
                .unwrap_or_else(|| DEFAULT_APP_NAME.to_string()),
            app_version: optional_env_var("APP_VERSION", profile)?
                .unwrap_or_else(|| DEFAULT_APP_VERSION.to_string()),
            fcm_id: optional_env_var("FCM_ID", profile)?.unwrap_or_default(),
            ib_id: optional_env_var("IB_ID", profile)?.unwrap_or_default(),
            account_id: required_env_var("ACCOUNT_ID", profile)?,
            server: optional_env_var("SERVER", profile)?,
            alt_server: optional_env_var("ALT_SERVER", profile)?,
            url_override: None,
            beta_url_override: None,
            enable_ticker: true,
            enable_order: true,
            enable_pnl: true,
            enable_history: false,
        })
    }

    /// Enables or disables the ticker plant.
    pub fn with_ticker(mut self, enable: bool) -> Self {
        self.enable_ticker = enable;
        self
    }

    /// Enables or disables the order plant.
    pub fn with_order(mut self, enable: bool) -> Self {
        self.enable_order = enable;
        self
    }

    /// Enables or disables the PnL plant.
    pub fn with_pnl(mut self, enable: bool) -> Self {
        self.enable_pnl = enable;
        self
    }

    /// Enables or disables the history plant.
    pub fn with_history(mut self, enable: bool) -> Self {
        self.enable_history = enable;
        self
    }

    /// Sets the application name.
    pub fn with_app_name(mut self, app_name: impl Into<String>) -> Self {
        self.app_name = app_name.into();
        self
    }

    /// Sets the application version.
    pub fn with_app_version(mut self, app_version: impl Into<String>) -> Self {
        self.app_version = app_version.into();
        self
    }

    /// Sets the named primary Rithmic server.
    pub fn with_server(mut self, server: impl Into<String>) -> Self {
        self.server = Some(server.into());
        self
    }

    /// Sets the named alternate Rithmic server.
    pub fn with_alt_server(mut self, alt_server: impl Into<String>) -> Self {
        self.alt_server = Some(alt_server.into());
        self
    }

    /// Sets the primary WebSocket URL override.
    pub fn with_url_override(mut self, url: impl Into<String>) -> Self {
        self.url_override = Some(url.into());
        self
    }

    /// Sets the alternate WebSocket URL override.
    pub fn with_beta_url_override(mut self, beta_url: impl Into<String>) -> Self {
        self.beta_url_override = Some(beta_url.into());
        self
    }

    /// Converts to rithmic-rs RithmicConfig.
    pub(crate) fn to_rithmic_config(&self) -> Result<RithmicConfig> {
        let env = self.environment;

        // Map environment to the required connection fields
        let (url_var, beta_url_var, default_system_name) = match env {
            RithmicEnv::Demo => (
                "RITHMIC_DEMO_URL",
                "RITHMIC_DEMO_ALT_URL",
                "Rithmic Paper Trading".to_string(),
            ),
            RithmicEnv::Live => (
                "RITHMIC_LIVE_URL",
                "RITHMIC_LIVE_ALT_URL",
                "Rithmic 01".to_string(),
            ),
            RithmicEnv::Test => (
                "RITHMIC_TEST_URL",
                "RITHMIC_TEST_ALT_URL",
                "Rithmic Test".to_string(),
            ),
        };

        let named_url = self
            .server
            .as_deref()
            .map(resolve_server_endpoint)
            .transpose()?
            .map(str::to_string);
        let url = self
            .url_override
            .clone()
            .or_else(|| env::var(url_var).ok())
            .or(named_url)
            .ok_or_else(|| {
                RithmicError::Config(format!(
                    "{url_var} not set and no named Rithmic primary server configured"
                ))
            })?;

        let named_beta_url = self
            .alt_server
            .as_deref()
            .map(resolve_server_endpoint)
            .transpose()?
            .map(str::to_string);
        let beta_url = self
            .beta_url_override
            .clone()
            .or_else(|| env::var(beta_url_var).ok())
            .or(named_beta_url)
            .unwrap_or_default();

        let mut builder = RithmicConfig::builder(env)
            .account_id(self.account_id.clone())
            .fcm_id(self.fcm_id.clone())
            .ib_id(self.ib_id.clone())
            .user(self.username.clone())
            .password(self.password.clone())
            .app_name(self.app_name.clone())
            .app_version(self.app_version.clone())
            .url(url)
            .beta_url(beta_url);

        // Use provided system name override when present; otherwise fall back to rithmic-rs default
        let system_name = if self.system_name.is_empty() {
            default_system_name
        } else {
            self.system_name.clone()
        };
        builder = builder.system_name(system_name);

        builder
            .build()
            .map_err(|e| RithmicError::Config(e.to_string()))
    }
}

/// Instrument information cached from reference data.
#[derive(Debug, Clone)]
pub struct InstrumentInfo {
    /// Symbol.
    pub symbol: String,
    /// Exchange.
    pub exchange: String,
    /// Tick size.
    pub tick_size: Option<f64>,
    /// Point value (dollar value per point).
    pub point_value: Option<f64>,
    /// Product code.
    pub product_code: Option<String>,
    /// Description/name.
    pub description: Option<String>,
    /// Currency.
    pub currency: Option<String>,
    /// Whether tradeable.
    pub is_tradeable: bool,
}

/// P&L event emitted by the gateway.
#[derive(Debug, Clone)]
pub enum PnlEvent {
    /// Account-level P&L update.
    Account(AccountEvent),
    /// Position-level P&L update.
    Position(PositionEvent),
}

/// Central gateway for Rithmic connections.
///
/// Manages the lifecycle of all Rithmic plants and provides event channels
/// for downstream consumers.
///
/// # Plant Handles
///
/// After calling `connect()`, the ticker, order, and PnL plant handles are moved
/// to background processor tasks. Only the history plant handle remains accessible
/// via `history_handle()` since it uses a request/response pattern.
pub struct RithmicGateway {
    config: GatewayConfig,

    // Plants (owned, used to get handles for disconnect)
    ticker_plant: Option<RithmicTickerPlant>,
    order_plant: Option<RithmicOrderPlant>,
    pnl_plant: Option<RithmicPnlPlant>,
    history_plant: Option<RithmicHistoryPlant>,

    // Query handles - kept for request/response operations
    // (separate from handles moved to processor tasks)
    ticker_query_handle: Option<RithmicTickerPlantHandle>,
    history_handle: Option<RithmicHistoryPlantHandle>,

    // Shared state
    instruments: Arc<DashMap<String, InstrumentInfo>>,
    connection_state: Arc<ArcSwap<ConnectionState>>,
    order_updates_available: bool,

    // Event channels for downstream consumers
    market_data_tx: mpsc::UnboundedSender<MarketDataEvent>,
    market_data_rx: Option<mpsc::UnboundedReceiver<MarketDataEvent>>,
    execution_tx: mpsc::UnboundedSender<ExecutionEvent>,
    execution_rx: Option<mpsc::UnboundedReceiver<ExecutionEvent>>,
    pnl_tx: mpsc::UnboundedSender<PnlEvent>,
    pnl_rx: Option<mpsc::UnboundedReceiver<PnlEvent>>,

    // Background task handles
    task_handles: Vec<JoinHandle<()>>,

    // Shutdown signal
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl RithmicGateway {
    /// Creates a new gateway without connecting.
    ///
    /// Call `connect()` to establish connections to Rithmic plants.
    pub fn new(config: GatewayConfig) -> Self {
        let (market_data_tx, market_data_rx) = mpsc::unbounded_channel();
        let (execution_tx, execution_rx) = mpsc::unbounded_channel();
        let (pnl_tx, pnl_rx) = mpsc::unbounded_channel();

        Self {
            config,
            ticker_plant: None,
            order_plant: None,
            pnl_plant: None,
            history_plant: None,
            ticker_query_handle: None,
            history_handle: None,
            instruments: Arc::new(DashMap::new()),
            connection_state: Arc::new(ArcSwap::from_pointee(ConnectionState::Disconnected)),
            order_updates_available: false,
            market_data_tx,
            market_data_rx: Some(market_data_rx),
            execution_tx,
            execution_rx: Some(execution_rx),
            pnl_tx,
            pnl_rx: Some(pnl_rx),
            task_handles: Vec::new(),
            shutdown_tx: None,
        }
    }

    /// Returns the gateway configuration.
    pub fn config(&self) -> &GatewayConfig {
        &self.config
    }

    /// Returns the current connection state.
    pub fn connection_state(&self) -> ConnectionState {
        **self.connection_state.load()
    }

    /// Returns true if the order plant accepted the order-updates subscription.
    pub fn order_updates_available(&self) -> bool {
        self.order_updates_available
    }

    /// Returns true if the gateway is connected.
    pub fn is_connected(&self) -> bool {
        self.connection_state() == ConnectionState::Connected
    }

    /// Returns a reference to the shared instruments map.
    pub fn instruments(&self) -> &Arc<DashMap<String, InstrumentInfo>> {
        &self.instruments
    }

    /// Returns the ticker plant handle for queries if connected.
    ///
    /// This handle is separate from the one used by the background processor
    /// and can be used for request/response operations like getting reference data.
    pub fn ticker_handle(&self) -> Option<&RithmicTickerPlantHandle> {
        self.ticker_query_handle.as_ref()
    }

    /// Returns a mutable reference to the ticker plant handle for queries.
    pub fn ticker_handle_mut(&mut self) -> Option<&mut RithmicTickerPlantHandle> {
        self.ticker_query_handle.as_mut()
    }

    /// Returns a new order plant handle for sending commands.
    ///
    /// Each call creates a fresh handle with its own subscription receiver.
    /// This handle is separate from the one used by the background processor
    /// and can be used to place, modify, and cancel orders.
    ///
    /// Returns `None` if the order plant is not connected.
    pub fn order_handle(&self) -> Option<RithmicOrderPlantHandle> {
        self.order_plant.as_ref().map(|p| p.get_handle())
    }

    /// Returns a new PnL plant handle for request/response operations if connected.
    pub fn pnl_handle(&self) -> Option<RithmicPnlPlantHandle> {
        self.pnl_plant.as_ref().map(|p| p.get_handle())
    }

    /// Returns all trading accounts accessible to the current order session.
    pub async fn list_accounts(&self) -> Result<Vec<String>> {
        let handle = self
            .order_handle()
            .ok_or_else(|| RithmicError::Connection("Order plant not connected".to_string()))?;

        let responses = handle
            .get_account_list()
            .await
            .map_err(|e| RithmicError::Connection(format!("Account list request failed: {e}")))?;

        Ok(responses
            .into_iter()
            .filter_map(|response| match response.message {
                RithmicMessage::ResponseAccountList(resp) => resp.account_id,
                _ => None,
            })
            .collect())
    }

    /// Requests a PnL snapshot for the configured account.
    ///
    /// The snapshot payload is emitted asynchronously through the gateway's PnL event channel.
    pub async fn request_pnl_snapshot(&self) -> Result<()> {
        let handle = self
            .pnl_handle()
            .ok_or_else(|| RithmicError::Connection("PnL plant not connected".to_string()))?;

        handle
            .pnl_position_snapshots()
            .await
            .map_err(|e| RithmicError::Connection(format!("PnL snapshot request failed: {e}")))?;

        Ok(())
    }

    /// Returns the history plant handle if connected.
    ///
    /// This handle can be used for request/response operations like historical data queries.
    pub fn history_handle(&self) -> Option<&RithmicHistoryPlantHandle> {
        self.history_handle.as_ref()
    }

    /// Returns a mutable reference to the history plant handle if connected.
    pub fn history_handle_mut(&mut self) -> Option<&mut RithmicHistoryPlantHandle> {
        self.history_handle.as_mut()
    }

    /// Takes the market data event receiver.
    ///
    /// This can only be called once - subsequent calls will return None.
    pub fn take_market_data_receiver(
        &mut self,
    ) -> Option<mpsc::UnboundedReceiver<MarketDataEvent>> {
        self.market_data_rx.take()
    }

    /// Takes the execution event receiver.
    ///
    /// This can only be called once - subsequent calls will return None.
    pub fn take_execution_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<ExecutionEvent>> {
        self.execution_rx.take()
    }

    /// Convenience helper: pipe execution events from the gateway to a
    /// `RithmicExecutionClient`, keeping local order state in sync.
    pub fn pipe_execution_events(
        &mut self,
        client: Arc<RithmicExecutionClient>,
    ) -> Result<JoinHandle<()>> {
        let rx = self.take_execution_receiver().ok_or_else(|| {
            RithmicError::Connection("Execution receiver already taken".to_string())
        })?;

        Ok(client.spawn_event_pump(rx))
    }

    /// Takes the P&L event receiver.
    ///
    /// This can only be called once - subsequent calls will return None.
    pub fn take_pnl_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<PnlEvent>> {
        self.pnl_rx.take()
    }

    /// Subscribes to market data (quotes and trades) for an instrument.
    ///
    /// This sends a subscription request to the ticker plant. After subscription,
    /// `BestBidOffer` and `LastTrade` messages will be received and transformed
    /// into `MarketDataEvent::Quote` and `MarketDataEvent::Trade` events.
    ///
    /// # Arguments
    /// * `symbol` - The instrument symbol (e.g., "ESZ4")
    /// * `exchange` - The exchange code (e.g., "CME")
    ///
    /// # Returns
    /// `Ok(())` on successful subscription, or an error if not connected or subscription fails.
    pub async fn subscribe_market_data(&self, symbol: &str, exchange: &str) -> Result<()> {
        let handle = self
            .ticker_query_handle
            .as_ref()
            .ok_or_else(|| RithmicError::Connection("Ticker plant not connected".to_string()))?;

        let response = handle
            .subscribe(symbol, exchange)
            .await
            .map_err(|e| RithmicError::Connection(format!("Subscription failed: {e}")))?;

        if let Some(err) = response.error {
            return Err(RithmicError::Connection(format!(
                "Subscription rejected: {err} (source={})",
                response.source
            )));
        }

        debug!("Subscribed to market data for {symbol} on {exchange}");
        Ok(())
    }

    /// Subscribes to order book depth for an instrument.
    ///
    /// # Arguments
    /// * `symbol` - The instrument symbol (e.g., "ESZ4")
    /// * `exchange` - The exchange code (e.g., "CME")
    pub async fn subscribe_order_book(&self, symbol: &str, exchange: &str) -> Result<()> {
        let handle = self
            .ticker_query_handle
            .as_ref()
            .ok_or_else(|| RithmicError::Connection("Ticker plant not connected".to_string()))?;

        handle
            .subscribe_order_book(symbol, exchange)
            .await
            .map_err(|e| {
                RithmicError::Connection(format!("Order book subscription failed: {e}"))
            })?;

        debug!("Subscribed to order book for {symbol} on {exchange}");
        Ok(())
    }

    /// Unsubscribes from market data (quotes and trades) for an instrument.
    ///
    /// # Arguments
    /// * `symbol` - The instrument symbol (e.g., "ESZ4")
    /// * `exchange` - The exchange code (e.g., "CME")
    pub async fn unsubscribe_market_data(&self, symbol: &str, exchange: &str) -> Result<()> {
        let handle = self
            .ticker_query_handle
            .as_ref()
            .ok_or_else(|| RithmicError::Connection("Ticker plant not connected".to_string()))?;

        handle
            .unsubscribe(symbol, exchange)
            .await
            .map_err(|e| RithmicError::Connection(format!("Unsubscribe failed: {e}")))?;

        debug!("Unsubscribed from market data for {symbol} on {exchange}");
        Ok(())
    }

    /// Unsubscribes from order book depth for an instrument.
    ///
    /// # Arguments
    /// * `symbol` - The instrument symbol (e.g., "ESZ4")
    /// * `exchange` - The exchange code (e.g., "CME")
    pub async fn unsubscribe_order_book(&self, symbol: &str, exchange: &str) -> Result<()> {
        let handle = self
            .ticker_query_handle
            .as_ref()
            .ok_or_else(|| RithmicError::Connection("Ticker plant not connected".to_string()))?;

        handle
            .unsubscribe_order_book(symbol, exchange)
            .await
            .map_err(|e| RithmicError::Connection(format!("Order book unsubscribe failed: {e}")))?;

        debug!("Unsubscribed from order book for {symbol} on {exchange}");
        Ok(())
    }

    // ========================================================================
    // Historical Data Methods
    // ========================================================================

    /// Requests historical time bars from the history plant.
    ///
    /// The history plant must be enabled in the gateway configuration
    /// (`enable_history: true`) for this method to work.
    ///
    /// # Arguments
    ///
    /// * `symbol` - The instrument symbol (e.g., "ESH5")
    /// * `exchange` - The exchange code (e.g., "CME")
    /// * `bar_type` - The type of bar (SecondBar, MinuteBar, DailyBar, WeeklyBar)
    /// * `bar_period` - The period (e.g., 1 for 1-minute, 5 for 5-minute)
    /// * `start_time_sec` - Start time as Unix timestamp in seconds
    /// * `end_time_sec` - End time as Unix timestamp in seconds
    ///
    /// # Returns
    ///
    /// A vector of `RithmicResponse` containing the bar data.
    pub async fn request_bars(
        &self,
        symbol: &str,
        exchange: &str,
        bar_type: TimeBarType,
        bar_period: i32,
        start_time_sec: i32,
        end_time_sec: i32,
    ) -> Result<Vec<RithmicResponse>> {
        let handle = self
            .history_handle
            .as_ref()
            .ok_or_else(|| RithmicError::Connection("History plant not connected".to_string()))?;

        let responses = handle
            .load_time_bars(
                symbol.to_string(),
                exchange.to_string(),
                bar_type,
                bar_period,
                start_time_sec,
                end_time_sec,
            )
            .await
            .map_err(|e| RithmicError::Api(format!("Bar request failed: {e}")))?;

        debug!(
            "Received {} bar responses for {} on {}",
            responses.len(),
            symbol,
            exchange
        );
        Ok(responses)
    }

    /// Subscribes to live time-bar updates on the history plant.
    pub async fn subscribe_time_bars(
        &self,
        symbol: &str,
        exchange: &str,
        bar_type: TimeBarType,
        bar_period: i32,
    ) -> Result<()> {
        let handle = self
            .history_handle
            .as_ref()
            .ok_or_else(|| RithmicError::Connection("History plant not connected".to_string()))?;

        let response = handle
            .subscribe_time_bar_updates(
                symbol,
                exchange,
                replay_bar_type_to_live(bar_type),
                bar_period,
                LiveTimeBarRequest::Subscribe,
            )
            .await
            .map_err(|e| RithmicError::Api(format!("Live bar subscription failed: {e}")))?;

        if let Some(error) = response.error {
            return Err(RithmicError::Api(format!(
                "Live bar subscription failed: {error}"
            )));
        }

        debug!(
            "Subscribed to live {:?} bars(period={}) for {} on {}",
            bar_type, bar_period, symbol, exchange
        );
        Ok(())
    }

    /// Unsubscribes from live time-bar updates on the history plant.
    pub async fn unsubscribe_time_bars(
        &self,
        symbol: &str,
        exchange: &str,
        bar_type: TimeBarType,
        bar_period: i32,
    ) -> Result<()> {
        let handle = self
            .history_handle
            .as_ref()
            .ok_or_else(|| RithmicError::Connection("History plant not connected".to_string()))?;

        let response = handle
            .subscribe_time_bar_updates(
                symbol,
                exchange,
                replay_bar_type_to_live(bar_type),
                bar_period,
                LiveTimeBarRequest::Unsubscribe,
            )
            .await
            .map_err(|e| RithmicError::Api(format!("Live bar unsubscribe failed: {e}")))?;

        if let Some(error) = response.error {
            return Err(RithmicError::Api(format!(
                "Live bar unsubscribe failed: {error}"
            )));
        }

        debug!(
            "Unsubscribed from live {:?} bars(period={}) for {} on {}",
            bar_type, bar_period, symbol, exchange
        );
        Ok(())
    }

    /// Returns true if the history plant is connected.
    ///
    /// Use this to check before calling `request_bars`.
    pub fn has_history_plant(&self) -> bool {
        self.history_handle.is_some()
    }

    /// Connects to all enabled Rithmic plants.
    ///
    /// This method:
    /// 1. Connects to each enabled plant (ticker, order, pnl, history)
    /// 2. Authenticates with each plant
    /// 3. Spawns background processor tasks for message handling
    /// 4. Sets connection state to Connected on success
    pub async fn connect(&mut self) -> Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        self.set_connection_state(ConnectionState::Connecting);
        info!("Connecting to Rithmic gateway...");

        let rithmic_config = self.config.to_rithmic_config()?;
        println!(
            "RITHMIC CONFIG >>> env={:?} system={} account={} fcm={} ib={} user={} url={} alt={}",
            rithmic_config.env,
            rithmic_config.system_name,
            rithmic_config.account_id,
            rithmic_config.fcm_id,
            rithmic_config.ib_id,
            rithmic_config.user,
            rithmic_config.url,
            rithmic_config.beta_url
        );

        // Create shutdown channel
        let (shutdown_tx, _shutdown_rx) = mpsc::channel(1);
        self.shutdown_tx = Some(shutdown_tx);

        // Connect to each enabled plant and collect handles for processors
        let ticker_handle = if self.config.enable_ticker {
            Some(self.connect_ticker_plant(&rithmic_config).await?)
        } else {
            None
        };

        let order_handle = if self.config.enable_order {
            Some(self.connect_order_plant(&rithmic_config).await?)
        } else {
            None
        };

        let pnl_handle = if self.config.enable_pnl {
            Some(self.connect_pnl_plant(&rithmic_config).await?)
        } else {
            None
        };

        if self.config.enable_history {
            self.connect_history_plant(&rithmic_config).await?;
        }

        // Spawn processor tasks with the handles
        self.spawn_processors(ticker_handle, order_handle, pnl_handle);

        self.set_connection_state(ConnectionState::Connected);

        // Emit authenticated event per NautilusTrader convention
        let _ = self.market_data_tx.send(MarketDataEvent::Authenticated);
        let _ = self.execution_tx.send(ExecutionEvent::Authenticated);

        info!("Rithmic gateway connected successfully");

        Ok(())
    }

    /// Connects to the ticker plant and returns a handle for the processor.
    ///
    /// Also stores a separate query handle for request/response operations.
    async fn connect_ticker_plant(
        &mut self,
        config: &RithmicConfig,
    ) -> Result<RithmicTickerPlantHandle> {
        info!("Connecting to ticker plant...");

        let plant = RithmicTickerPlant::connect(config, ConnectStrategy::Simple)
            .await
            .map_err(|e| {
                RithmicError::Connection(format!("Ticker plant connection failed: {e}"))
            })?;

        // Get two handles: one for processor, one for queries
        let processor_handle = plant.get_handle();
        let query_handle = plant.get_handle();

        // Login to ticker plant (using processor handle, login state is shared)
        let response = processor_handle
            .login()
            .await
            .map_err(|e| RithmicError::Authentication(format!("Ticker plant login failed: {e}")))?;

        if let Some(error) = &response.error {
            return Err(RithmicError::Authentication(format!(
                "Ticker plant login error: {error} (source={})",
                response.source
            )));
        }

        debug!("Ticker plant connected and authenticated");

        self.ticker_plant = Some(plant);
        self.ticker_query_handle = Some(query_handle);
        Ok(processor_handle)
    }

    /// Connects to the order plant and returns a handle for the processor.
    async fn connect_order_plant(
        &mut self,
        config: &RithmicConfig,
    ) -> Result<RithmicOrderPlantHandle> {
        info!("Connecting to order plant...");
        debug!(
            system_name = %config.system_name,
            user = %config.user,
            account_id = %config.account_id,
            fcm_id = %config.fcm_id,
            ib_id = %config.ib_id,
            "order plant identity context"
        );

        let plant = RithmicOrderPlant::connect(config, ConnectStrategy::Simple)
            .await
            .map_err(|e| RithmicError::Connection(format!("Order plant connection failed: {e}")))?;

        // Get handle for processor
        let processor_handle = plant.get_handle();

        // Login to order plant
        let response = processor_handle
            .login()
            .await
            .map_err(|e| RithmicError::Authentication(format!("Order plant login failed: {e}")))?;

        if let Some(error) = &response.error {
            return Err(RithmicError::Authentication(format!(
                "Order plant login error: {error}"
            )));
        }

        // Subscribe to order updates
        debug!(
            template_id = 308,
            account_id = %config.account_id,
            fcm_id = %config.fcm_id,
            ib_id = %config.ib_id,
            "sending subscribe_order_updates request"
        );
        let subscribe_response = processor_handle
            .subscribe_order_updates()
            .await
            .map_err(|e| {
                RithmicError::Connection(format!("Order updates subscription failed: {e}"))
            })?;

        if let Some(error) = &subscribe_response.error {
            self.order_updates_available = false;
            warn!(
                source = %subscribe_response.source,
                error = %error,
                "order updates subscription unavailable; continuing with direct command responses only"
            );
        } else {
            self.order_updates_available = true;
            debug!(?subscribe_response, "subscribed to order updates");
        }

        debug!("Order plant connected and authenticated");

        // Store plant so we can create handles via order_handle()
        self.order_plant = Some(plant);
        Ok(processor_handle)
    }

    /// Connects to the PnL plant and returns a handle for the processor.
    async fn connect_pnl_plant(&mut self, config: &RithmicConfig) -> Result<RithmicPnlPlantHandle> {
        info!("Connecting to PnL plant...");

        let plant = RithmicPnlPlant::connect(config, ConnectStrategy::Simple)
            .await
            .map_err(|e| RithmicError::Connection(format!("PnL plant connection failed: {e}")))?;

        let handle = plant.get_handle();

        // Login to PnL plant
        let response = handle
            .login()
            .await
            .map_err(|e| RithmicError::Authentication(format!("PnL plant login failed: {e}")))?;

        if let Some(error) = &response.error {
            return Err(RithmicError::Authentication(format!(
                "PnL plant login error: {error}"
            )));
        }

        // Subscribe to PnL updates
        handle.subscribe_pnl_updates().await.map_err(|e| {
            RithmicError::Connection(format!("PnL updates subscription failed: {e}"))
        })?;

        debug!("PnL plant connected and authenticated");

        self.pnl_plant = Some(plant);
        Ok(handle)
    }

    /// Connects to the history plant.
    async fn connect_history_plant(&mut self, config: &RithmicConfig) -> Result<()> {
        info!("Connecting to history plant...");

        let plant = RithmicHistoryPlant::connect(config, ConnectStrategy::Simple)
            .await
            .map_err(|e| {
                RithmicError::Connection(format!("History plant connection failed: {e}"))
            })?;

        let handle = plant.get_handle();

        // Login to history plant
        let response = handle.login().await.map_err(|e| {
            RithmicError::Authentication(format!("History plant login failed: {e}"))
        })?;

        if let Some(error) = &response.error {
            return Err(RithmicError::Authentication(format!(
                "History plant login error: {error}"
            )));
        }

        debug!("History plant connected and authenticated");

        self.history_plant = Some(plant);
        self.history_handle = Some(handle);

        Ok(())
    }

    /// Spawns background processor tasks for each connected plant.
    fn spawn_processors(
        &mut self,
        ticker_handle: Option<RithmicTickerPlantHandle>,
        order_handle: Option<RithmicOrderPlantHandle>,
        pnl_handle: Option<RithmicPnlPlantHandle>,
    ) {
        // Spawn ticker processor
        if let Some(handle) = ticker_handle {
            let instruments = Arc::clone(&self.instruments);
            let event_tx = self.market_data_tx.clone();
            let connection_state = Arc::clone(&self.connection_state);

            let task = tokio::spawn(async move {
                ticker_processor(handle, instruments, event_tx, connection_state).await;
            });
            self.task_handles.push(task);
        }

        // Spawn order processor
        if let Some(handle) = order_handle {
            let event_tx = self.execution_tx.clone();
            let connection_state = Arc::clone(&self.connection_state);

            let task = tokio::spawn(async move {
                order_processor(handle, event_tx, connection_state).await;
            });
            self.task_handles.push(task);
        }

        // Spawn PnL processor
        if let Some(handle) = pnl_handle {
            let event_tx = self.pnl_tx.clone();
            let connection_state = Arc::clone(&self.connection_state);

            let task = tokio::spawn(async move {
                pnl_processor(handle, event_tx, connection_state).await;
            });
            self.task_handles.push(task);
        }

        if let Some(handle) = self.history_handle.clone() {
            let event_tx = self.market_data_tx.clone();
            let connection_state = Arc::clone(&self.connection_state);

            let task = tokio::spawn(async move {
                history_processor(handle, event_tx, connection_state).await;
            });
            self.task_handles.push(task);
        }
    }

    /// Disconnects from all Rithmic plants.
    ///
    /// This method:
    /// 1. Signals all processor tasks to stop
    /// 2. Disconnects from each plant
    /// 3. Cleans up resources
    pub async fn disconnect(&mut self) -> Result<()> {
        if !self.is_connected() && self.connection_state() != ConnectionState::Reconnecting {
            return Ok(());
        }

        info!("Disconnecting from Rithmic gateway...");

        // Signal shutdown
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }

        // Abort all processor tasks (they own the streaming handles)
        for handle in self.task_handles.drain(..) {
            handle.abort();
        }

        // Get fresh handles from plants and disconnect.
        // The processor tasks owned the streaming handles, but we can get new handles
        // from the plants to call disconnect and close the connections cleanly.
        if let Some(plant) = self.ticker_plant.take() {
            let handle = plant.get_handle();
            match tokio::time::timeout(
                Duration::from_secs(DISCONNECT_TIMEOUT_SECS),
                handle.disconnect(),
            )
            .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    warn!("Error disconnecting ticker plant: {e}; aborting plant");
                    handle.abort();
                }
                Err(_) => {
                    warn!("Timed out disconnecting ticker plant; aborting plant");
                    handle.abort();
                }
            }
        }

        if let Some(plant) = self.order_plant.take() {
            let handle = plant.get_handle();
            match tokio::time::timeout(
                Duration::from_secs(DISCONNECT_TIMEOUT_SECS),
                handle.disconnect(),
            )
            .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    warn!("Error disconnecting order plant: {e}; aborting plant");
                    handle.abort();
                }
                Err(_) => {
                    warn!("Timed out disconnecting order plant; aborting plant");
                    handle.abort();
                }
            }
        }

        if let Some(plant) = self.pnl_plant.take() {
            let handle = plant.get_handle();
            match tokio::time::timeout(
                Duration::from_secs(DISCONNECT_TIMEOUT_SECS),
                handle.disconnect(),
            )
            .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    warn!("Error disconnecting PnL plant: {e}; aborting plant");
                    handle.abort();
                }
                Err(_) => {
                    warn!("Timed out disconnecting PnL plant; aborting plant");
                    handle.abort();
                }
            }
        }

        if let Some(plant) = self.history_plant.take() {
            let handle = plant.get_handle();
            match tokio::time::timeout(
                Duration::from_secs(DISCONNECT_TIMEOUT_SECS),
                handle.disconnect(),
            )
            .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    warn!("Error disconnecting history plant: {e}; aborting plant");
                    handle.abort();
                }
                Err(_) => {
                    warn!("Timed out disconnecting history plant; aborting plant");
                    handle.abort();
                }
            }
        }

        // Also clear the stored query handles
        self.ticker_query_handle = None;
        self.history_handle = None;

        self.set_connection_state(ConnectionState::Disconnected);
        info!("Rithmic gateway disconnected");

        Ok(())
    }

    /// Attempts to reconnect to Rithmic with exponential backoff.
    pub async fn reconnect(&mut self) -> Result<()> {
        self.set_connection_state(ConnectionState::Reconnecting);
        info!("Attempting to reconnect to Rithmic...");

        let mut attempts = 0;
        let mut backoff_ms = INITIAL_BACKOFF_MS;

        while attempts < MAX_RECONNECT_ATTEMPTS {
            attempts += 1;

            // Clean up existing connections
            if let Err(e) = self.disconnect().await {
                warn!("Error during disconnect for reconnect: {e}");
            }

            // Wait before retry
            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;

            // Attempt reconnection
            match self.connect().await {
                Ok(()) => {
                    // Emit reconnected event per NautilusTrader convention
                    let _ = self.market_data_tx.send(MarketDataEvent::Reconnected);
                    let _ = self.execution_tx.send(ExecutionEvent::Reconnected);

                    info!("Reconnection successful after {attempts} attempts");
                    return Ok(());
                }
                Err(e) => {
                    warn!("Reconnection attempt {attempts} failed: {e}");
                    backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
                }
            }
        }

        self.set_connection_state(ConnectionState::Error);
        Err(RithmicError::Connection(format!(
            "Reconnection failed after {MAX_RECONNECT_ATTEMPTS} attempts"
        )))
    }

    /// Sets the connection state and emits state change events.
    fn set_connection_state(&self, state: ConnectionState) {
        self.connection_state.store(Arc::new(state));

        // Emit state change to all channels
        let _ = self
            .market_data_tx
            .send(MarketDataEvent::ConnectionState(state));
        let _ = self
            .execution_tx
            .send(ExecutionEvent::ConnectionState(state));
    }
}

impl std::fmt::Debug for RithmicGateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RithmicGateway")
            .field("environment", &self.config.environment)
            .field("connection_state", &self.connection_state())
            .field("ticker_enabled", &self.config.enable_ticker)
            .field("order_enabled", &self.config.enable_order)
            .field("pnl_enabled", &self.config.enable_pnl)
            .field("history_enabled", &self.config.enable_history)
            .field("instruments_count", &self.instruments.len())
            .finish()
    }
}

impl Drop for RithmicGateway {
    fn drop(&mut self) {
        // Abort all tasks on drop
        for handle in self.task_handles.drain(..) {
            handle.abort();
        }
    }
}

// =============================================================================
// Processor Tasks
// =============================================================================

/// Background task that processes ticker plant messages.
///
/// This task receives messages from the ticker plant's subscription receiver
/// and transforms them into `MarketDataEvent` types for downstream consumers.
async fn ticker_processor(
    mut handle: RithmicTickerPlantHandle,
    instruments: Arc<DashMap<String, InstrumentInfo>>,
    event_tx: mpsc::UnboundedSender<MarketDataEvent>,
    connection_state: Arc<ArcSwap<ConnectionState>>,
) {
    debug!("Ticker processor started");
    // Cache only lives for the lifetime of the active ticker task, so it is
    // dropped on disconnect/reconnect and cannot leak stale quotes across sessions.
    let mut quote_state: AHashMap<String, crate::data::QuoteTick> = AHashMap::new();

    loop {
        match handle.subscription_receiver.recv().await {
            Ok(response) => {
                // Check for connection issues
                if is_connection_issue(&response) {
                    warn!("Ticker plant connection issue detected");
                    quote_state.clear();
                    connection_state.store(Arc::new(ConnectionState::Reconnecting));
                    let _ = event_tx.send(MarketDataEvent::ConnectionState(
                        ConnectionState::Reconnecting,
                    ));
                    break;
                }

                // Check for errors
                if let Some(error) = &response.error {
                    warn!("Ticker plant error: {error}");
                    let _ = event_tx.send(MarketDataEvent::Error(error.clone()));
                    continue;
                }

                // Transform market data messages
                // Note: Actual transformation will be implemented in Phase 3
                if let Some(event) =
                    transform_market_data(&response, &instruments, &mut quote_state)
                {
                    let _ = event_tx.send(event);
                }
            }
            Err(e) => {
                // Channel closed or lagged
                error!("Ticker processor receive error: {e}");
                quote_state.clear();
                break;
            }
        }
    }

    debug!("Ticker processor stopped");
}

/// Background task that processes history-plant subscription updates.
async fn history_processor(
    mut handle: RithmicHistoryPlantHandle,
    event_tx: mpsc::UnboundedSender<MarketDataEvent>,
    connection_state: Arc<ArcSwap<ConnectionState>>,
) {
    debug!("History processor started");

    loop {
        match handle.subscription_receiver.recv().await {
            Ok(response) => {
                if is_connection_issue(&response) {
                    warn!("History plant connection issue detected");
                    connection_state.store(Arc::new(ConnectionState::Reconnecting));
                    let _ = event_tx.send(MarketDataEvent::ConnectionState(
                        ConnectionState::Reconnecting,
                    ));
                    break;
                }

                if let Some(error) = &response.error {
                    warn!("History plant error: {error}");
                    let _ = event_tx.send(MarketDataEvent::Error(error.clone()));
                    continue;
                }

                if let Some(event) = transform_history_market_data(&response) {
                    let _ = event_tx.send(event);
                }
            }
            Err(e) => {
                error!("History processor receive error: {e}");
                break;
            }
        }
    }

    debug!("History processor stopped");
}

/// Background task that processes order plant messages.
async fn order_processor(
    mut handle: RithmicOrderPlantHandle,
    event_tx: mpsc::UnboundedSender<ExecutionEvent>,
    connection_state: Arc<ArcSwap<ConnectionState>>,
) {
    debug!("Order processor started");

    let handler = ExecutionHandler::new();

    loop {
        match handle.subscription_receiver.recv().await {
            Ok(response) => {
                debug!(
                    request_id = %response.request_id,
                    source = %response.source,
                    is_update = response.is_update,
                    has_more = response.has_more,
                    multi_response = response.multi_response,
                    message_kind = ?std::mem::discriminant(&response.message),
                    "order processor received response"
                );

                // Check for connection issues
                if is_connection_issue(&response) {
                    warn!("Order plant connection issue detected");
                    connection_state.store(Arc::new(ConnectionState::Reconnecting));
                    let _ = event_tx.send(ExecutionEvent::ConnectionState(
                        ConnectionState::Reconnecting,
                    ));
                    break;
                }

                // Check for errors
                if let Some(error) = &response.error {
                    warn!("Order plant error: {error}");
                    let _ = event_tx.send(ExecutionEvent::Error(error.clone()));
                    continue;
                }

                // Transform execution messages
                if let Some(event) = handler.handle_response(&response) {
                    debug!(?event, "order processor emitting mapped execution event");
                    let _ = event_tx.send(event);
                } else {
                    debug!("order processor ignored response after execution mapping");
                }
            }
            Err(e) => {
                error!("Order processor receive error: {e}");
                break;
            }
        }
    }

    debug!("Order processor stopped");
}

/// Background task that processes PnL plant messages.
async fn pnl_processor(
    mut handle: RithmicPnlPlantHandle,
    event_tx: mpsc::UnboundedSender<PnlEvent>,
    connection_state: Arc<ArcSwap<ConnectionState>>,
) {
    debug!("PnL processor started");

    loop {
        match handle.subscription_receiver.recv().await {
            Ok(response) => {
                // Check for connection issues
                if is_connection_issue(&response) {
                    warn!("PnL plant connection issue detected");
                    connection_state.store(Arc::new(ConnectionState::Reconnecting));
                    break;
                }

                // Check for errors
                if let Some(error) = &response.error {
                    warn!("PnL plant error: {error}");
                    continue;
                }

                // Transform PnL messages
                // Note: Actual transformation will be implemented in Phase 5
                if let Some(event) = transform_pnl(&response) {
                    let _ = event_tx.send(event);
                }
            }
            Err(e) => {
                error!("PnL processor receive error: {e}");
                break;
            }
        }
    }

    debug!("PnL processor stopped");
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Checks if a response indicates a connection issue.
fn is_connection_issue(response: &RithmicResponse) -> bool {
    matches!(
        response.message,
        RithmicMessage::ConnectionError | RithmicMessage::HeartbeatTimeout
    )
}

/// Converts Rithmic's ssboe (seconds since beginning of epoch) and usecs to Unix nanoseconds.
///
/// Rithmic timestamps use:
/// - `ssboe`: Seconds since Unix epoch (1970-01-01)
/// - `usecs`: Microseconds component
#[inline]
fn rithmic_timestamp_to_nanos(ssboe: Option<i32>, usecs: Option<i32>) -> u64 {
    let secs = ssboe.unwrap_or(0) as u64;
    let micros = usecs.unwrap_or(0) as u64;
    secs * 1_000_000_000 + micros * 1_000
}

/// Returns current time in Unix nanoseconds.
#[inline]
fn now_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

#[inline]
fn replay_bar_type_to_live(bar_type: TimeBarType) -> LiveTimeBarType {
    match bar_type {
        TimeBarType::SecondBar => LiveTimeBarType::SecondBar,
        TimeBarType::MinuteBar => LiveTimeBarType::MinuteBar,
        TimeBarType::DailyBar => LiveTimeBarType::DailyBar,
        TimeBarType::WeeklyBar => LiveTimeBarType::WeeklyBar,
    }
}

#[inline]
fn live_bar_type_to_replay(value: i32) -> Option<TimeBarType> {
    match LiveTimeBarType::try_from(value).ok()? {
        LiveTimeBarType::SecondBar => Some(TimeBarType::SecondBar),
        LiveTimeBarType::MinuteBar => Some(TimeBarType::MinuteBar),
        LiveTimeBarType::DailyBar => Some(TimeBarType::DailyBar),
        LiveTimeBarType::WeeklyBar => Some(TimeBarType::WeeklyBar),
    }
}

#[inline]
fn time_bar_timestamp_to_nanos(marker: Option<i32>, period: Option<&str>) -> u64 {
    if let Some(marker) = marker.filter(|value| *value > 0) {
        return marker as u64 * 1_000_000_000;
    }

    period
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
        * 1_000_000_000
}

#[inline]
fn tick_bar_marker(data_bar_ssboe: &[i32]) -> Option<i64> {
    data_bar_ssboe.last().copied().map(i64::from)
}

#[inline]
fn tick_bar_timestamp_to_nanos(data_bar_ssboe: &[i32], data_bar_usecs: &[i32]) -> u64 {
    let secs = data_bar_ssboe.last().copied().unwrap_or_default() as u64;
    let micros = data_bar_usecs.last().copied().unwrap_or_default() as u64;
    secs * 1_000_000_000 + micros * 1_000
}

#[inline]
fn live_tick_bar_type_to_bar_type(value: i32) -> Option<RithmicBarType> {
    match rithmic_rs::rti::tick_bar::BarType::try_from(value).ok()? {
        rithmic_rs::rti::tick_bar::BarType::TickBar => Some(RithmicBarType::TickBar),
        rithmic_rs::rti::tick_bar::BarType::RangeBar
        | rithmic_rs::rti::tick_bar::BarType::VolumeBar => None,
    }
}

#[inline]
fn bbo_side_updated(bits: Option<u32>, bit: u32, price: Option<f64>, size: Option<i32>) -> bool {
    match bits {
        Some(bits) => bits & bit != 0,
        None => price.is_some_and(|value| value > 0.0) || size.is_some_and(|value| value > 0),
    }
}

#[inline]
fn quote_is_complete(quote: &crate::data::QuoteTick) -> bool {
    quote.bid_price > 0.0 && quote.ask_price > 0.0 && quote.bid_size > 0.0 && quote.ask_size > 0.0
}

/// Transforms a ticker plant response to a market data event.
fn transform_market_data(
    response: &RithmicResponse,
    _instruments: &DashMap<String, InstrumentInfo>,
    quote_state: &mut AHashMap<String, crate::data::QuoteTick>,
) -> Option<MarketDataEvent> {
    transform_market_data_message(&response.message, _instruments, quote_state)
}

fn transform_history_market_data(response: &RithmicResponse) -> Option<MarketDataEvent> {
    match &response.message {
        RithmicMessage::TimeBar(bar) => {
            let symbol = bar.symbol.as_ref()?;
            let exchange = bar.exchange.as_ref()?;
            let bar_type = RithmicBarType::from(live_bar_type_to_replay(bar.r#type?)?);
            let bar_period = bar.period.as_ref()?.parse::<i32>().ok()?;
            let ts_event = time_bar_timestamp_to_nanos(bar.marker, bar.period.as_deref());
            let ts_init = now_nanos();

            Some(MarketDataEvent::Bar(crate::data::TimeBar {
                symbol: symbol.clone(),
                exchange: exchange.clone(),
                bar_type,
                bar_period,
                open_price: bar.open_price.unwrap_or(0.0),
                high_price: bar.high_price.unwrap_or(0.0),
                low_price: bar.low_price.unwrap_or(0.0),
                close_price: bar.close_price.unwrap_or(0.0),
                volume: bar.volume.unwrap_or(0) as f64,
                marker: bar.marker.map(i64::from),
                ts_event,
                ts_init,
            }))
        }
        RithmicMessage::TickBar(bar) => {
            let symbol = bar.symbol.as_ref()?;
            let exchange = bar.exchange.as_ref()?;
            let bar_type = live_tick_bar_type_to_bar_type(bar.r#type?)?;
            let bar_period = bar.type_specifier.as_deref()?.parse::<i32>().ok()?;
            let ts_event = tick_bar_timestamp_to_nanos(&bar.data_bar_ssboe, &bar.data_bar_usecs);
            let ts_init = now_nanos();

            Some(MarketDataEvent::Bar(crate::data::TimeBar {
                symbol: symbol.clone(),
                exchange: exchange.clone(),
                bar_type,
                bar_period,
                open_price: bar.open_price.unwrap_or(0.0),
                high_price: bar.high_price.unwrap_or(0.0),
                low_price: bar.low_price.unwrap_or(0.0),
                close_price: bar.close_price.unwrap_or(0.0),
                volume: bar.volume.unwrap_or(0) as f64,
                marker: tick_bar_marker(&bar.data_bar_ssboe),
                ts_event,
                ts_init,
            }))
        }
        RithmicMessage::ForcedLogout(_) => {
            warn!("Forced logout from history plant");
            Some(MarketDataEvent::Error("Forced logout".to_string()))
        }
        _ => None,
    }
}

fn transform_market_data_message(
    message: &RithmicMessage,
    _instruments: &DashMap<String, InstrumentInfo>,
    quote_state: &mut AHashMap<String, crate::data::QuoteTick>,
) -> Option<MarketDataEvent> {
    use crate::data::{QuoteTick, TradeTick};

    match message {
        RithmicMessage::BestBidOffer(bbo) => {
            use rithmic_rs::rti::best_bid_offer::PresenceBits;

            let symbol = bbo.symbol.as_ref()?;
            let exchange = bbo.exchange.as_ref()?;

            let bid_updated = bbo_side_updated(
                bbo.presence_bits,
                PresenceBits::Bid as u32,
                bbo.bid_price,
                bbo.bid_size,
            );
            let ask_updated = bbo_side_updated(
                bbo.presence_bits,
                PresenceBits::Ask as u32,
                bbo.ask_price,
                bbo.ask_size,
            );

            if !bid_updated && !ask_updated {
                return None;
            }

            let ts_event = rithmic_timestamp_to_nanos(bbo.ssboe, bbo.usecs);
            let ts_init = now_nanos();
            let key = format!("{exchange}:{symbol}");
            let prior = quote_state.get(&key);

            let quote = QuoteTick {
                symbol: symbol.clone(),
                exchange: exchange.clone(),
                bid_price: if bid_updated {
                    bbo.bid_price
                        .or_else(|| prior.as_ref().map(|quote| quote.bid_price))
                        .unwrap_or(0.0)
                } else {
                    prior.as_ref().map(|quote| quote.bid_price).unwrap_or(0.0)
                },
                ask_price: if ask_updated {
                    bbo.ask_price
                        .or_else(|| prior.as_ref().map(|quote| quote.ask_price))
                        .unwrap_or(0.0)
                } else {
                    prior.as_ref().map(|quote| quote.ask_price).unwrap_or(0.0)
                },
                bid_size: if bid_updated {
                    bbo.bid_size
                        .map(|value| value as f64)
                        .or_else(|| prior.as_ref().map(|quote| quote.bid_size))
                        .unwrap_or(0.0)
                } else {
                    prior.as_ref().map(|quote| quote.bid_size).unwrap_or(0.0)
                },
                ask_size: if ask_updated {
                    bbo.ask_size
                        .map(|value| value as f64)
                        .or_else(|| prior.as_ref().map(|quote| quote.ask_size))
                        .unwrap_or(0.0)
                } else {
                    prior.as_ref().map(|quote| quote.ask_size).unwrap_or(0.0)
                },
                ts_event,
                ts_init,
            };

            quote_state.insert(key, quote.clone());

            if quote_is_complete(&quote) {
                Some(MarketDataEvent::Quote(quote))
            } else {
                None
            }
        }
        RithmicMessage::LastTrade(trade) => {
            let symbol = trade.symbol.as_ref()?;
            let exchange = trade.exchange.as_ref()?;
            let price = trade.trade_price?;
            let size = trade.trade_size.unwrap_or(0);

            // Skip trades with no price or zero size
            if size == 0 {
                return None;
            }

            // Determine aggressor side from transaction type
            // 1 = Buy (aggressor bought), 2 = Sell (aggressor sold)
            let aggressor_side = match trade.aggressor {
                Some(1) => "BUY",
                Some(2) => "SELL",
                _ => "UNKNOWN",
            };

            let ts_event = rithmic_timestamp_to_nanos(trade.ssboe, trade.usecs);
            let ts_init = now_nanos();

            Some(MarketDataEvent::Trade(TradeTick {
                symbol: symbol.clone(),
                exchange: exchange.clone(),
                price,
                size: size as f64,
                aggressor_side: aggressor_side.to_string(),
                trade_id: trade.exchange_order_id.clone().unwrap_or_default(),
                ts_event,
                ts_init,
            }))
        }
        RithmicMessage::DepthByOrder(_) => {
            // Order book depth - future enhancement
            debug!("Received DepthByOrder message (not yet implemented)");
            None
        }
        RithmicMessage::ForcedLogout(_) => {
            warn!("Forced logout from ticker plant");
            Some(MarketDataEvent::Error("Forced logout".to_string()))
        }
        _ => None,
    }
}

/// Transforms a PnL plant response to a PnL event.
///
/// Handles `AccountPnLPositionUpdate` for account-level balance/margin updates
/// and `InstrumentPnLPositionUpdate` for per-instrument position updates.
fn transform_pnl(response: &RithmicResponse) -> Option<PnlEvent> {
    transform_pnl_message(&response.message)
}

fn transform_pnl_message(message: &RithmicMessage) -> Option<PnlEvent> {
    use crate::providers::{AccountBalance, Position};

    match message {
        RithmicMessage::AccountPnLPositionUpdate(update) => {
            let account_id = update.account_id.clone()?;

            // Parse string fields to f64 (Rithmic sends numeric values as strings)
            let total = parse_optional_f64(&update.account_balance).unwrap_or(0.0);
            let available = parse_optional_f64(&update.cash_on_hand).unwrap_or(0.0);
            let locked = parse_optional_f64(&update.margin_balance).unwrap_or(0.0);
            let unrealized_pnl = parse_optional_f64(&update.open_position_pnl).unwrap_or(0.0);
            let realized_pnl = parse_optional_f64(&update.closed_position_pnl).unwrap_or(0.0);
            let ts_event = rithmic_timestamp_to_nanos(update.ssboe, update.usecs);

            debug!(
                "AccountPnLPositionUpdate: account={}, balance={}, available={}, margin={}",
                account_id, total, available, locked
            );

            Some(PnlEvent::Account(AccountEvent::BalanceUpdate(
                AccountBalance {
                    is_snapshot: update.is_snapshot.unwrap_or(false),
                    account_id,
                    currency: "USD".to_string(), // Futures typically USD
                    total,
                    available,
                    locked,
                    unrealized_pnl,
                    realized_pnl,
                    ts_event,
                },
            )))
        }
        RithmicMessage::InstrumentPnLPositionUpdate(update) => {
            let account_id = update.account_id.clone()?;
            let symbol = update.symbol.clone()?;
            let exchange = update.exchange.clone()?;

            // Net position is buy_qty - sell_qty
            let quantity = update.buy_qty.unwrap_or(0) as f64 - update.sell_qty.unwrap_or(0) as f64;
            let avg_price = update.avg_open_fill_price.unwrap_or(0.0);

            // InstrumentPnLPositionUpdate has f64 for day_open_pnl/day_closed_pnl
            // but string for open_position_pnl/closed_position_pnl
            let unrealized_pnl = update
                .day_open_pnl
                .or_else(|| parse_optional_f64(&update.open_position_pnl))
                .unwrap_or(0.0);
            let realized_pnl = update
                .day_closed_pnl
                .or_else(|| parse_optional_f64(&update.closed_position_pnl))
                .unwrap_or(0.0);

            let ts_event = rithmic_timestamp_to_nanos(update.ssboe, update.usecs);

            debug!(
                "InstrumentPnLPositionUpdate: {}:{} qty={}, avg_price={}, pnl={}",
                exchange, symbol, quantity, avg_price, unrealized_pnl
            );

            Some(PnlEvent::Position(PositionEvent::Updated(Position {
                is_snapshot: update.is_snapshot.unwrap_or(false),
                account_id,
                symbol,
                exchange,
                quantity,
                avg_price,
                unrealized_pnl,
                realized_pnl,
                ts_event,
            })))
        }
        RithmicMessage::ForcedLogout(_) => {
            warn!("Forced logout from PnL plant");
            Some(PnlEvent::Account(AccountEvent::Error(
                "Forced logout".to_string(),
            )))
        }
        _ => None,
    }
}

/// Parses an optional string field to f64.
#[inline]
fn parse_optional_f64(s: &Option<String>) -> Option<f64> {
    s.as_ref().and_then(|s| s.parse().ok())
}

#[cfg(test)]
mod tests {
    use std::sync::{LazyLock, Mutex};

    use super::*;

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn set_env(key: &str, value: Option<&str>) -> Option<String> {
        let previous = std::env::var(key).ok();
        match value {
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }
        previous
    }

    fn restore_env(entries: &[(&str, Option<String>)]) {
        for (key, value) in entries {
            match value {
                Some(value) => unsafe { std::env::set_var(key, value) },
                None => unsafe { std::env::remove_var(key) },
            }
        }
    }

    fn transform_market_data_for_test(
        message: RithmicMessage,
        instruments: &DashMap<String, InstrumentInfo>,
        quote_state: &mut AHashMap<String, crate::data::QuoteTick>,
    ) -> Option<MarketDataEvent> {
        transform_market_data_message(&message, instruments, quote_state)
    }

    fn transform_pnl_for_test(message: RithmicMessage) -> Option<PnlEvent> {
        transform_pnl_message(&message)
    }

    #[test]
    fn test_gateway_creation() {
        let config = GatewayConfig::new(
            RithmicEnv::Demo,
            "user",
            "pass",
            "system",
            "fcm",
            "ib",
            "account",
        );
        let gateway = RithmicGateway::new(config);

        assert_eq!(gateway.connection_state(), ConnectionState::Disconnected);
        assert!(!gateway.is_connected());
        // History handle is None before connect
        assert!(gateway.history_handle().is_none());
    }

    #[test]
    fn test_gateway_config_builder() {
        let config = GatewayConfig::new(
            RithmicEnv::Demo,
            "user",
            "pass",
            "system",
            "fcm",
            "ib",
            "account",
        )
        .with_ticker(true)
        .with_order(false)
        .with_pnl(true)
        .with_history(false);

        assert!(config.enable_ticker);
        assert!(!config.enable_order);
        assert!(config.enable_pnl);
        assert!(!config.enable_history);
        assert_eq!(config.app_name, DEFAULT_APP_NAME);
        assert_eq!(config.app_version, DEFAULT_APP_VERSION);
    }

    #[test]
    fn test_gateway_config_to_rithmic_config_uses_url_overrides() {
        let config = GatewayConfig::new(
            RithmicEnv::Demo,
            "user",
            "pass",
            "system",
            "fcm",
            "ib",
            "account",
        )
        .with_url_override("ws://127.0.0.1:12345")
        .with_beta_url_override("ws://127.0.0.1:12346");

        let rithmic = config.to_rithmic_config().unwrap();

        assert_eq!(rithmic.url, "ws://127.0.0.1:12345");
        assert_eq!(rithmic.beta_url, "ws://127.0.0.1:12346");
        assert_eq!(rithmic.system_name, "system");
    }

    #[test]
    fn test_gateway_config_to_rithmic_config_resolves_named_servers() {
        let config = GatewayConfig::new(
            RithmicEnv::Demo,
            "user",
            "pass",
            "system",
            "fcm",
            "ib",
            "account",
        )
        .with_server("Chicago")
        .with_alt_server("Sydney");

        let rithmic = config.to_rithmic_config().unwrap();

        assert_eq!(rithmic.url, "wss://rprotocol.rithmic.com:443");
        assert_eq!(rithmic.beta_url, "wss://rprotocol-au.rithmic.com:443");
    }

    #[test]
    fn test_gateway_config_from_env_uses_canonical_vars() {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = [
            ("RITHMIC_ENV", set_env("RITHMIC_ENV", Some("demo"))),
            (
                "RITHMIC_USERNAME",
                set_env("RITHMIC_USERNAME", Some("user")),
            ),
            (
                "RITHMIC_PASSWORD",
                set_env("RITHMIC_PASSWORD", Some("pass")),
            ),
            (
                "RITHMIC_SYSTEM_NAME",
                set_env("RITHMIC_SYSTEM_NAME", Some("system")),
            ),
            (
                "RITHMIC_APP_NAME",
                set_env("RITHMIC_APP_NAME", Some("MyApp")),
            ),
            (
                "RITHMIC_APP_VERSION",
                set_env("RITHMIC_APP_VERSION", Some("2.0")),
            ),
            ("RITHMIC_FCM_ID", set_env("RITHMIC_FCM_ID", Some("fcm"))),
            ("RITHMIC_IB_ID", set_env("RITHMIC_IB_ID", Some("ib"))),
            (
                "RITHMIC_ACCOUNT_ID",
                set_env("RITHMIC_ACCOUNT_ID", Some("account")),
            ),
            ("RITHMIC_SERVER", set_env("RITHMIC_SERVER", Some("Chicago"))),
            (
                "RITHMIC_ALT_SERVER",
                set_env("RITHMIC_ALT_SERVER", Some("Sydney")),
            ),
        ];

        let config = GatewayConfig::from_env().unwrap();

        assert_eq!(config.environment, RithmicEnv::Demo);
        assert_eq!(config.username, "user");
        assert_eq!(config.password, "pass");
        assert_eq!(config.system_name, "system");
        assert_eq!(config.app_name, "MyApp");
        assert_eq!(config.app_version, "2.0");
        assert_eq!(config.fcm_id, "fcm");
        assert_eq!(config.ib_id, "ib");
        assert_eq!(config.account_id, "account");
        assert_eq!(config.server.as_deref(), Some("Chicago"));
        assert_eq!(config.alt_server.as_deref(), Some("Sydney"));

        restore_env(&previous);
    }

    #[test]
    fn test_gateway_config_from_env_requires_canonical_username() {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = [
            ("RITHMIC_ENV", set_env("RITHMIC_ENV", Some("demo"))),
            ("RITHMIC_USERNAME", set_env("RITHMIC_USERNAME", None)),
            (
                "RITHMIC_PASSWORD",
                set_env("RITHMIC_PASSWORD", Some("pass")),
            ),
            (
                "RITHMIC_SYSTEM_NAME",
                set_env("RITHMIC_SYSTEM_NAME", Some("system")),
            ),
            ("RITHMIC_APP_NAME", set_env("RITHMIC_APP_NAME", None)),
            ("RITHMIC_APP_VERSION", set_env("RITHMIC_APP_VERSION", None)),
            ("RITHMIC_FCM_ID", set_env("RITHMIC_FCM_ID", None)),
            ("RITHMIC_IB_ID", set_env("RITHMIC_IB_ID", None)),
            ("RITHMIC_SERVER", set_env("RITHMIC_SERVER", None)),
            ("RITHMIC_ALT_SERVER", set_env("RITHMIC_ALT_SERVER", None)),
            (
                "RITHMIC_ACCOUNT_ID",
                set_env("RITHMIC_ACCOUNT_ID", Some("account")),
            ),
            (
                "RITHMIC_DEMO_USER",
                set_env("RITHMIC_DEMO_USER", Some("legacy-user")),
            ),
        ];

        let error = GatewayConfig::from_env().unwrap_err();
        assert!(error.to_string().contains("RITHMIC_USERNAME not set"));

        restore_env(&previous);
    }

    #[test]
    fn test_gateway_config_from_profile_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = [
            (
                "RITHMIC_APEX_ENV",
                set_env("RITHMIC_APEX_ENV", Some("live")),
            ),
            (
                "RITHMIC_APEX_USERNAME",
                set_env("RITHMIC_APEX_USERNAME", Some("user")),
            ),
            (
                "RITHMIC_APEX_PASSWORD",
                set_env("RITHMIC_APEX_PASSWORD", Some("pass")),
            ),
            (
                "RITHMIC_APEX_SYSTEM_NAME",
                set_env("RITHMIC_APEX_SYSTEM_NAME", Some("Apex")),
            ),
            (
                "RITHMIC_APEX_ACCOUNT_ID",
                set_env("RITHMIC_APEX_ACCOUNT_ID", Some("account")),
            ),
            (
                "RITHMIC_APEX_FCM_ID",
                set_env("RITHMIC_APEX_FCM_ID", Some("fcm")),
            ),
            (
                "RITHMIC_APEX_SERVER",
                set_env("RITHMIC_APEX_SERVER", Some("Frankfurt")),
            ),
        ];

        let config = GatewayConfig::from_env_with_profile(Some("Apex")).unwrap();

        assert_eq!(config.environment, RithmicEnv::Live);
        assert_eq!(config.username, "user");
        assert_eq!(config.password, "pass");
        assert_eq!(config.system_name, "Apex");
        assert_eq!(config.account_id, "account");
        assert_eq!(config.fcm_id, "fcm");
        assert_eq!(config.server.as_deref(), Some("Frankfurt"));

        restore_env(&previous);
    }

    #[test]
    fn test_connection_state_default() {
        let config = GatewayConfig::new(
            RithmicEnv::Demo,
            "user",
            "pass",
            "system",
            "fcm",
            "ib",
            "account",
        );
        let gateway = RithmicGateway::new(config);

        assert_eq!(gateway.connection_state(), ConnectionState::Disconnected);
    }

    #[test]
    fn test_instruments_initially_empty() {
        let config = GatewayConfig::new(
            RithmicEnv::Demo,
            "user",
            "pass",
            "system",
            "fcm",
            "ib",
            "account",
        );
        let gateway = RithmicGateway::new(config);

        assert!(gateway.instruments().is_empty());
    }

    #[test]
    fn test_event_receivers_can_be_taken() {
        let config = GatewayConfig::new(
            RithmicEnv::Demo,
            "user",
            "pass",
            "system",
            "fcm",
            "ib",
            "account",
        );
        let mut gateway = RithmicGateway::new(config);

        // First take should succeed
        assert!(gateway.take_market_data_receiver().is_some());
        assert!(gateway.take_execution_receiver().is_some());
        assert!(gateway.take_pnl_receiver().is_some());

        // Second take should return None
        assert!(gateway.take_market_data_receiver().is_none());
        assert!(gateway.take_execution_receiver().is_none());
        assert!(gateway.take_pnl_receiver().is_none());
    }

    #[test]
    fn test_rithmic_timestamp_to_nanos() {
        // Test with both ssboe and usecs
        let nanos = rithmic_timestamp_to_nanos(Some(1704067200), Some(123456));
        // 1704067200 * 1e9 + 123456 * 1e3 = 1704067200000000000 + 123456000
        assert_eq!(nanos, 1704067200123456000);

        // Test with only ssboe
        let nanos = rithmic_timestamp_to_nanos(Some(1704067200), None);
        assert_eq!(nanos, 1704067200000000000);

        // Test with None values
        let nanos = rithmic_timestamp_to_nanos(None, None);
        assert_eq!(nanos, 0);
    }

    #[test]
    fn test_transform_bbo_to_quote() {
        use rithmic_rs::rti::BestBidOffer;

        let instruments = DashMap::new();
        let mut quote_state = AHashMap::new();
        let bbo = BestBidOffer {
            template_id: 150,
            symbol: Some("ESZ4".to_string()),
            exchange: Some("CME".to_string()),
            presence_bits: None,
            clear_bits: None,
            is_snapshot: Some(false),
            bid_price: Some(5000.25),
            bid_size: Some(100),
            bid_orders: None,
            bid_implicit_size: None,
            bid_time: None,
            ask_price: Some(5000.50),
            ask_size: Some(150),
            ask_orders: None,
            ask_implicit_size: None,
            ask_time: None,
            lean_price: None,
            ssboe: Some(1704067200),
            usecs: Some(500000),
        };

        let event = transform_market_data_for_test(
            RithmicMessage::BestBidOffer(bbo),
            &instruments,
            &mut quote_state,
        );
        assert!(event.is_some());

        if let Some(MarketDataEvent::Quote(quote)) = event {
            assert_eq!(quote.symbol, "ESZ4");
            assert_eq!(quote.exchange, "CME");
            assert_eq!(quote.bid_price, 5000.25);
            assert_eq!(quote.ask_price, 5000.50);
            assert_eq!(quote.bid_size, 100.0);
            assert_eq!(quote.ask_size, 150.0);
            assert_eq!(quote.ts_event, 1704067200500000000);
        } else {
            panic!("Expected Quote event");
        }
    }

    #[test]
    fn test_transform_bbo_preserves_last_seen_opposite_side() {
        use rithmic_rs::rti::BestBidOffer;
        use rithmic_rs::rti::best_bid_offer::PresenceBits;

        let instruments = DashMap::new();
        let mut quote_state = AHashMap::new();

        let initial = RithmicMessage::BestBidOffer(BestBidOffer {
            template_id: 150,
            symbol: Some("ESM6".to_string()),
            exchange: Some("CME".to_string()),
            presence_bits: Some((PresenceBits::Bid as u32) | (PresenceBits::Ask as u32)),
            clear_bits: None,
            is_snapshot: Some(false),
            bid_price: Some(6619.00),
            bid_size: Some(9),
            bid_orders: None,
            bid_implicit_size: None,
            bid_time: None,
            ask_price: Some(6619.50),
            ask_size: Some(6),
            ask_orders: None,
            ask_implicit_size: None,
            ask_time: None,
            lean_price: None,
            ssboe: Some(1),
            usecs: Some(0),
        });

        let partial = RithmicMessage::BestBidOffer(BestBidOffer {
            template_id: 150,
            symbol: Some("ESM6".to_string()),
            exchange: Some("CME".to_string()),
            presence_bits: Some(PresenceBits::Ask as u32),
            clear_bits: None,
            is_snapshot: Some(false),
            bid_price: Some(0.0),
            bid_size: Some(0),
            bid_orders: None,
            bid_implicit_size: None,
            bid_time: None,
            ask_price: Some(6619.50),
            ask_size: Some(5),
            ask_orders: None,
            ask_implicit_size: None,
            ask_time: None,
            lean_price: None,
            ssboe: Some(2),
            usecs: Some(0),
        });

        let _ = transform_market_data_for_test(initial, &instruments, &mut quote_state);
        let event = transform_market_data_for_test(partial, &instruments, &mut quote_state);

        match event {
            Some(MarketDataEvent::Quote(quote)) => {
                assert_eq!(quote.bid_price, 6619.00);
                assert_eq!(quote.bid_size, 9.0);
                assert_eq!(quote.ask_price, 6619.50);
                assert_eq!(quote.ask_size, 5.0);
            }
            other => panic!("Expected Quote event, got {other:?}"),
        }
    }

    #[test]
    fn test_transform_bbo_preserves_last_seen_ask_when_bid_only_update_zeroes_ask_fields() {
        use rithmic_rs::rti::BestBidOffer;
        use rithmic_rs::rti::best_bid_offer::PresenceBits;

        let instruments = DashMap::new();
        let mut quote_state = AHashMap::new();

        let initial = RithmicMessage::BestBidOffer(BestBidOffer {
            template_id: 150,
            symbol: Some("ESM6".to_string()),
            exchange: Some("CME".to_string()),
            presence_bits: Some((PresenceBits::Bid as u32) | (PresenceBits::Ask as u32)),
            clear_bits: None,
            is_snapshot: Some(false),
            bid_price: Some(6617.00),
            bid_size: Some(3),
            bid_orders: None,
            bid_implicit_size: None,
            bid_time: None,
            ask_price: Some(6617.25),
            ask_size: Some(1),
            ask_orders: None,
            ask_implicit_size: None,
            ask_time: None,
            lean_price: None,
            ssboe: Some(1),
            usecs: Some(0),
        });

        let bid_only = RithmicMessage::BestBidOffer(BestBidOffer {
            template_id: 150,
            symbol: Some("ESM6".to_string()),
            exchange: Some("CME".to_string()),
            presence_bits: Some(PresenceBits::Bid as u32),
            clear_bits: None,
            is_snapshot: Some(false),
            bid_price: Some(6616.75),
            bid_size: Some(10),
            bid_orders: None,
            bid_implicit_size: None,
            bid_time: None,
            ask_price: Some(0.0),
            ask_size: Some(0),
            ask_orders: None,
            ask_implicit_size: None,
            ask_time: None,
            lean_price: None,
            ssboe: Some(2),
            usecs: Some(0),
        });

        let _ = transform_market_data_for_test(initial, &instruments, &mut quote_state);
        let event = transform_market_data_for_test(bid_only, &instruments, &mut quote_state);

        match event {
            Some(MarketDataEvent::Quote(quote)) => {
                assert_eq!(quote.bid_price, 6616.75);
                assert_eq!(quote.bid_size, 10.0);
                assert_eq!(quote.ask_price, 6617.25);
                assert_eq!(quote.ask_size, 1.0);
            }
            other => panic!("Expected Quote event, got {other:?}"),
        }
    }

    #[test]
    fn test_transform_bbo_waits_until_both_sides_seen_before_emitting_quote() {
        use rithmic_rs::rti::BestBidOffer;
        use rithmic_rs::rti::best_bid_offer::PresenceBits;

        let instruments = DashMap::new();
        let mut quote_state = AHashMap::new();

        let ask_only = RithmicMessage::BestBidOffer(BestBidOffer {
            template_id: 150,
            symbol: Some("ESM6".to_string()),
            exchange: Some("CME".to_string()),
            presence_bits: Some(PresenceBits::Ask as u32),
            clear_bits: None,
            is_snapshot: Some(false),
            bid_price: Some(0.0),
            bid_size: Some(0),
            bid_orders: None,
            bid_implicit_size: None,
            bid_time: None,
            ask_price: Some(6617.25),
            ask_size: Some(1),
            ask_orders: None,
            ask_implicit_size: None,
            ask_time: None,
            lean_price: None,
            ssboe: Some(1),
            usecs: Some(0),
        });

        let bid_only = RithmicMessage::BestBidOffer(BestBidOffer {
            template_id: 150,
            symbol: Some("ESM6".to_string()),
            exchange: Some("CME".to_string()),
            presence_bits: Some(PresenceBits::Bid as u32),
            clear_bits: None,
            is_snapshot: Some(false),
            bid_price: Some(6617.00),
            bid_size: Some(3),
            bid_orders: None,
            bid_implicit_size: None,
            bid_time: None,
            ask_price: Some(0.0),
            ask_size: Some(0),
            ask_orders: None,
            ask_implicit_size: None,
            ask_time: None,
            lean_price: None,
            ssboe: Some(2),
            usecs: Some(0),
        });

        let first = transform_market_data_for_test(ask_only, &instruments, &mut quote_state);
        assert!(first.is_none());

        let second = transform_market_data_for_test(bid_only, &instruments, &mut quote_state);
        match second {
            Some(MarketDataEvent::Quote(quote)) => {
                assert_eq!(quote.bid_price, 6617.00);
                assert_eq!(quote.bid_size, 3.0);
                assert_eq!(quote.ask_price, 6617.25);
                assert_eq!(quote.ask_size, 1.0);
            }
            other => panic!("Expected Quote event, got {other:?}"),
        }
    }

    #[test]
    fn test_transform_last_trade_to_trade() {
        use rithmic_rs::rti::LastTrade;

        let instruments = DashMap::new();
        let mut quote_state = AHashMap::new();
        let trade = LastTrade {
            template_id: 151,
            symbol: Some("ESZ4".to_string()),
            exchange: Some("CME".to_string()),
            presence_bits: None,
            clear_bits: None,
            is_snapshot: Some(false),
            trade_price: Some(5000.25),
            trade_size: Some(10),
            aggressor: Some(1), // Buy
            exchange_order_id: Some("12345".to_string()),
            aggressor_exchange_order_id: None,
            net_change: None,
            percent_change: None,
            volume: None,
            vwap: None,
            trade_time: None,
            ssboe: Some(1704067200),
            usecs: Some(750000),
            source_ssboe: None,
            source_usecs: None,
            source_nsecs: None,
            jop_ssboe: None,
            jop_nsecs: None,
        };

        let event = transform_market_data_for_test(
            RithmicMessage::LastTrade(trade),
            &instruments,
            &mut quote_state,
        );
        assert!(event.is_some());

        if let Some(MarketDataEvent::Trade(trade)) = event {
            assert_eq!(trade.symbol, "ESZ4");
            assert_eq!(trade.exchange, "CME");
            assert_eq!(trade.price, 5000.25);
            assert_eq!(trade.size, 10.0);
            assert_eq!(trade.aggressor_side, "BUY");
            assert_eq!(trade.trade_id, "12345");
            assert_eq!(trade.ts_event, 1704067200750000000);
        } else {
            panic!("Expected Trade event");
        }
    }

    #[test]
    fn test_transform_bbo_missing_symbol_returns_none() {
        use rithmic_rs::rti::BestBidOffer;

        let instruments = DashMap::new();
        let mut quote_state = AHashMap::new();
        let bbo = BestBidOffer {
            template_id: 150,
            symbol: None, // Missing symbol
            exchange: Some("CME".to_string()),
            presence_bits: None,
            clear_bits: None,
            is_snapshot: None,
            bid_price: Some(5000.25),
            bid_size: Some(100),
            bid_orders: None,
            bid_implicit_size: None,
            bid_time: None,
            ask_price: Some(5000.50),
            ask_size: Some(150),
            ask_orders: None,
            ask_implicit_size: None,
            ask_time: None,
            lean_price: None,
            ssboe: None,
            usecs: None,
        };

        let event = transform_market_data_for_test(
            RithmicMessage::BestBidOffer(bbo),
            &instruments,
            &mut quote_state,
        );
        assert!(event.is_none());
    }

    #[test]
    fn test_transform_bbo_no_prices_returns_none() {
        use rithmic_rs::rti::BestBidOffer;

        let instruments = DashMap::new();
        let mut quote_state = AHashMap::new();
        let bbo = BestBidOffer {
            template_id: 150,
            symbol: Some("ESZ4".to_string()),
            exchange: Some("CME".to_string()),
            presence_bits: None,
            clear_bits: None,
            is_snapshot: None,
            bid_price: None, // No bid
            bid_size: None,
            bid_orders: None,
            bid_implicit_size: None,
            bid_time: None,
            ask_price: None, // No ask
            ask_size: None,
            ask_orders: None,
            ask_implicit_size: None,
            ask_time: None,
            lean_price: None,
            ssboe: None,
            usecs: None,
        };

        let event = transform_market_data_for_test(
            RithmicMessage::BestBidOffer(bbo),
            &instruments,
            &mut quote_state,
        );
        assert!(event.is_none());
    }

    #[test]
    fn test_transform_trade_zero_size_returns_none() {
        use rithmic_rs::rti::LastTrade;

        let instruments = DashMap::new();
        let mut quote_state = AHashMap::new();
        let trade = LastTrade {
            template_id: 151,
            symbol: Some("ESZ4".to_string()),
            exchange: Some("CME".to_string()),
            presence_bits: None,
            clear_bits: None,
            is_snapshot: None,
            trade_price: Some(5000.25),
            trade_size: Some(0), // Zero size
            aggressor: Some(1),
            exchange_order_id: None,
            aggressor_exchange_order_id: None,
            net_change: None,
            percent_change: None,
            volume: None,
            vwap: None,
            trade_time: None,
            ssboe: None,
            usecs: None,
            source_ssboe: None,
            source_usecs: None,
            source_nsecs: None,
            jop_ssboe: None,
            jop_nsecs: None,
        };

        let event = transform_market_data_for_test(
            RithmicMessage::LastTrade(trade),
            &instruments,
            &mut quote_state,
        );
        assert!(event.is_none());
    }

    #[test]
    fn test_transform_trade_sell_aggressor() {
        use rithmic_rs::rti::LastTrade;

        let instruments = DashMap::new();
        let mut quote_state = AHashMap::new();
        let trade = LastTrade {
            template_id: 151,
            symbol: Some("ESZ4".to_string()),
            exchange: Some("CME".to_string()),
            presence_bits: None,
            clear_bits: None,
            is_snapshot: None,
            trade_price: Some(5000.25),
            trade_size: Some(5),
            aggressor: Some(2), // Sell
            exchange_order_id: None,
            aggressor_exchange_order_id: None,
            net_change: None,
            percent_change: None,
            volume: None,
            vwap: None,
            trade_time: None,
            ssboe: None,
            usecs: None,
            source_ssboe: None,
            source_usecs: None,
            source_nsecs: None,
            jop_ssboe: None,
            jop_nsecs: None,
        };

        let event = transform_market_data_for_test(
            RithmicMessage::LastTrade(trade),
            &instruments,
            &mut quote_state,
        );
        assert!(event.is_some());

        if let Some(MarketDataEvent::Trade(trade)) = event {
            assert_eq!(trade.aggressor_side, "SELL");
        } else {
            panic!("Expected Trade event");
        }
    }

    #[test]
    fn test_transform_account_pnl_update() {
        use rithmic_rs::rti::AccountPnLPositionUpdate;

        let update = AccountPnLPositionUpdate {
            template_id: 450,
            is_snapshot: Some(false),
            fcm_id: Some("FCMID".to_string()),
            ib_id: Some("IBID".to_string()),
            account_id: Some("ACCOUNT123".to_string()),
            fill_buy_qty: Some(10),
            fill_sell_qty: Some(5),
            order_buy_qty: Some(0),
            order_sell_qty: Some(0),
            buy_qty: Some(10),
            sell_qty: Some(5),
            open_long_options_value: None,
            open_short_options_value: None,
            closed_options_value: None,
            option_cash_reserved: None,
            rms_account_commission: None,
            open_position_pnl: Some("1250.50".to_string()),
            open_position_quantity: Some(5),
            closed_position_pnl: Some("500.00".to_string()),
            closed_position_quantity: Some(10),
            net_quantity: Some(5),
            excess_buy_margin: None,
            margin_balance: Some("25000.00".to_string()),
            min_margin_balance: None,
            min_account_balance: None,
            account_balance: Some("100000.00".to_string()),
            cash_on_hand: Some("75000.00".to_string()),
            option_closed_pnl: None,
            percent_maximum_allowable_loss: None,
            option_open_pnl: None,
            mtm_account: None,
            available_buying_power: None,
            used_buying_power: None,
            reserved_buying_power: None,
            excess_sell_margin: None,
            day_open_pnl: None,
            day_closed_pnl: None,
            day_pnl: None,
            day_open_pnl_offset: None,
            day_closed_pnl_offset: None,
            ssboe: Some(1704067200),
            usecs: Some(123456),
        };

        let event = transform_pnl_for_test(RithmicMessage::AccountPnLPositionUpdate(update));
        assert!(event.is_some());

        if let Some(PnlEvent::Account(AccountEvent::BalanceUpdate(balance))) = event {
            assert_eq!(balance.account_id, "ACCOUNT123");
            assert_eq!(balance.total, 100000.0);
            assert_eq!(balance.available, 75000.0);
            assert_eq!(balance.locked, 25000.0);
            assert_eq!(balance.unrealized_pnl, 1250.50);
            assert_eq!(balance.realized_pnl, 500.0);
            assert_eq!(balance.ts_event, 1704067200123456000);
        } else {
            panic!("Expected Account BalanceUpdate event");
        }
    }

    #[test]
    fn test_transform_instrument_pnl_update() {
        use rithmic_rs::rti::InstrumentPnLPositionUpdate;

        let update = InstrumentPnLPositionUpdate {
            template_id: 451,
            is_snapshot: Some(false),
            fcm_id: Some("FCMID".to_string()),
            ib_id: Some("IBID".to_string()),
            account_id: Some("ACCOUNT123".to_string()),
            symbol: Some("ESZ4".to_string()),
            exchange: Some("CME".to_string()),
            product_code: Some("ES".to_string()),
            instrument_type: Some("Future".to_string()),
            fill_buy_qty: Some(10),
            fill_sell_qty: Some(5),
            order_buy_qty: Some(0),
            order_sell_qty: Some(0),
            buy_qty: Some(10),
            sell_qty: Some(5),
            avg_open_fill_price: Some(5025.50),
            day_open_pnl: Some(1500.0),
            day_closed_pnl: Some(750.0),
            day_pnl: Some(2250.0),
            day_open_pnl_offset: None,
            day_closed_pnl_offset: None,
            mtm_security: None,
            open_long_options_value: None,
            open_short_options_value: None,
            closed_options_value: None,
            option_cash_reserved: None,
            open_position_pnl: Some("1500.00".to_string()),
            open_position_quantity: Some(5),
            closed_position_pnl: Some("750.00".to_string()),
            closed_position_quantity: Some(10),
            net_quantity: Some(5),
            ssboe: Some(1704067200),
            usecs: Some(500000),
        };

        let event = transform_pnl_for_test(RithmicMessage::InstrumentPnLPositionUpdate(update));
        assert!(event.is_some());

        if let Some(PnlEvent::Position(PositionEvent::Updated(position))) = event {
            assert_eq!(position.account_id, "ACCOUNT123");
            assert_eq!(position.symbol, "ESZ4");
            assert_eq!(position.exchange, "CME");
            assert_eq!(position.quantity, 5.0); // 10 buy - 5 sell
            assert_eq!(position.avg_price, 5025.50);
            assert_eq!(position.unrealized_pnl, 1500.0); // day_open_pnl
            assert_eq!(position.realized_pnl, 750.0); // day_closed_pnl
            assert_eq!(position.ts_event, 1704067200500000000);
        } else {
            panic!("Expected Position Updated event");
        }
    }

    #[test]
    fn test_transform_pnl_missing_account_id() {
        use rithmic_rs::rti::AccountPnLPositionUpdate;

        let update = AccountPnLPositionUpdate {
            template_id: 450,
            is_snapshot: None,
            fcm_id: None,
            ib_id: None,
            account_id: None, // Missing account_id
            fill_buy_qty: None,
            fill_sell_qty: None,
            order_buy_qty: None,
            order_sell_qty: None,
            buy_qty: None,
            sell_qty: None,
            open_long_options_value: None,
            open_short_options_value: None,
            closed_options_value: None,
            option_cash_reserved: None,
            rms_account_commission: None,
            open_position_pnl: None,
            open_position_quantity: None,
            closed_position_pnl: None,
            closed_position_quantity: None,
            net_quantity: None,
            excess_buy_margin: None,
            margin_balance: None,
            min_margin_balance: None,
            min_account_balance: None,
            account_balance: None,
            cash_on_hand: None,
            option_closed_pnl: None,
            percent_maximum_allowable_loss: None,
            option_open_pnl: None,
            mtm_account: None,
            available_buying_power: None,
            used_buying_power: None,
            reserved_buying_power: None,
            excess_sell_margin: None,
            day_open_pnl: None,
            day_closed_pnl: None,
            day_pnl: None,
            day_open_pnl_offset: None,
            day_closed_pnl_offset: None,
            ssboe: None,
            usecs: None,
        };

        let event = transform_pnl_for_test(RithmicMessage::AccountPnLPositionUpdate(update));
        assert!(event.is_none());
    }

    #[test]
    fn test_parse_optional_f64() {
        // Valid number
        assert_eq!(
            parse_optional_f64(&Some("123.45".to_string())),
            Some(123.45)
        );

        // Integer
        assert_eq!(parse_optional_f64(&Some("100".to_string())), Some(100.0));

        // Negative
        assert_eq!(
            parse_optional_f64(&Some("-50.25".to_string())),
            Some(-50.25)
        );

        // None
        assert_eq!(parse_optional_f64(&None), None);

        // Invalid string
        assert_eq!(parse_optional_f64(&Some("invalid".to_string())), None);

        // Empty string
        assert_eq!(parse_optional_f64(&Some("".to_string())), None);
    }
}
