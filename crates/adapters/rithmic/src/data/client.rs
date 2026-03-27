//! Rithmic data client implementation.

use dashmap::DashMap;
use std::sync::Arc;

use rithmic_rs::rti::request_time_bar_replay::BarType as TimeBarType;

use crate::{
    common::{
        enums::ConnectionState,
        types::{ExchangeId, RithmicSymbol, UnixNanos},
    },
    error::{Result, RithmicError},
    gateway::RithmicGateway,
};

/// Quote tick data.
#[derive(Debug, Clone)]
pub struct QuoteTick {
    /// Instrument symbol.
    pub symbol: RithmicSymbol,
    /// Exchange.
    pub exchange: ExchangeId,
    /// Best bid price.
    pub bid_price: f64,
    /// Best ask price.
    pub ask_price: f64,
    /// Bid size.
    pub bid_size: f64,
    /// Ask size.
    pub ask_size: f64,
    /// Timestamp in nanoseconds.
    pub ts_event: UnixNanos,
    /// Initialization timestamp.
    pub ts_init: UnixNanos,
}

/// Trade tick data.
#[derive(Debug, Clone)]
pub struct TradeTick {
    /// Instrument symbol.
    pub symbol: RithmicSymbol,
    /// Exchange.
    pub exchange: ExchangeId,
    /// Trade price.
    pub price: f64,
    /// Trade size.
    pub size: f64,
    /// Aggressor side ("BUY" or "SELL").
    pub aggressor_side: String,
    /// Trade ID.
    pub trade_id: String,
    /// Timestamp in nanoseconds.
    pub ts_event: UnixNanos,
    /// Initialization timestamp.
    pub ts_init: UnixNanos,
}

/// Live time bar data.
#[derive(Debug, Clone)]
pub struct TimeBar {
    /// Instrument symbol.
    pub symbol: RithmicSymbol,
    /// Exchange.
    pub exchange: ExchangeId,
    /// Rithmic time bar type.
    pub bar_type: TimeBarType,
    /// The bar period/step (for example `1` for a 1-minute bar).
    pub bar_period: i32,
    /// Open price.
    pub open_price: f64,
    /// High price.
    pub high_price: f64,
    /// Low price.
    pub low_price: f64,
    /// Close price.
    pub close_price: f64,
    /// Trade volume.
    pub volume: f64,
    /// Rithmic bar marker, typically epoch seconds for the bar close.
    pub marker: Option<i64>,
    /// Event timestamp in nanoseconds.
    pub ts_event: UnixNanos,
    /// Initialization timestamp in nanoseconds.
    pub ts_init: UnixNanos,
}

/// Market data event emitted by the data client.
#[derive(Debug, Clone)]
pub enum MarketDataEvent {
    /// Quote tick (best bid/offer update).
    Quote(QuoteTick),
    /// Trade tick (last trade).
    Trade(TradeTick),
    /// Time bar update.
    Bar(TimeBar),
    /// Connection state change.
    ConnectionState(ConnectionState),
    /// Successfully reconnected after disconnect.
    Reconnected,
    /// Successfully authenticated with venue.
    Authenticated,
    /// Error event.
    Error(String),
}

/// Subscription tracking for a single instrument.
#[derive(Debug, Clone, Default)]
struct InstrumentSubscription {
    quotes: bool,
    trades: bool,
}

/// Rithmic market data client.
///
/// Provides a high-level interface for subscribing to market data through
/// the `RithmicGateway`. The gateway handles the actual connection and message
/// processing; this client manages subscription state and provides a clean API.
///
/// # Example
///
/// ```rust,ignore
/// use nautilus_rithmic::{RithmicGateway, RithmicDataClient, GatewayConfig};
/// use std::sync::Arc;
///
/// let config = GatewayConfig::from_env()?;
/// let mut gateway = RithmicGateway::new(config);
///
/// // Take the receiver BEFORE wrapping in Arc (requires &mut self)
/// let mut rx = gateway.take_market_data_receiver().unwrap();
///
/// gateway.connect().await?;
///
/// let gateway = Arc::new(gateway);
/// let client = RithmicDataClient::new(Arc::clone(&gateway));
///
/// client.subscribe_quotes("ESZ4", "CME").await?;
///
/// while let Some(event) = rx.recv().await {
///     match event {
///         MarketDataEvent::Quote(q) => println!("Quote: {:?}", q),
///         MarketDataEvent::Trade(t) => println!("Trade: {:?}", t),
///         _ => {}
///     }
/// }
/// ```
pub struct RithmicDataClient {
    gateway: Arc<RithmicGateway>,
    /// Tracks which instruments have active subscriptions.
    /// Key: "EXCHANGE:SYMBOL" (e.g., "CME:ESZ4")
    subscriptions: DashMap<String, InstrumentSubscription>,
    /// Tracks live bar subscriptions.
    /// Key: "EXCHANGE:SYMBOL:BarType:Period" (e.g., "CME:ESZ4:MinuteBar:1")
    bar_subscriptions: DashMap<String, ()>,
}

impl RithmicDataClient {
    /// Creates a new data client backed by the given gateway.
    ///
    /// The gateway should already be connected before subscribing to data.
    pub fn new(gateway: Arc<RithmicGateway>) -> Self {
        Self {
            gateway,
            subscriptions: DashMap::new(),
            bar_subscriptions: DashMap::new(),
        }
    }

    /// Returns the current connection state from the gateway.
    pub fn connection_state(&self) -> ConnectionState {
        self.gateway.connection_state()
    }

    /// Returns true if the gateway is connected.
    pub fn is_connected(&self) -> bool {
        self.gateway.is_connected()
    }

    /// Returns a reference to the underlying gateway.
    pub fn gateway(&self) -> &Arc<RithmicGateway> {
        &self.gateway
    }

    /// Subscribes to quotes (best bid/offer) for an instrument.
    ///
    /// After subscribing, `MarketDataEvent::Quote` events will be emitted
    /// on the gateway's market data receiver.
    ///
    /// # Note
    /// Rithmic's subscription returns both BBO and LastTrade messages.
    /// Calling `subscribe_quotes` will also enable receiving trades.
    pub async fn subscribe_quotes(&self, symbol: &str, exchange: &str) -> Result<()> {
        if !self.is_connected() {
            return Err(RithmicError::Connection("Not connected".to_string()));
        }

        let key = format!("{exchange}:{symbol}");

        // Check if we already have an active subscription for this instrument.
        // Returns true if we already have ANY subscription (quotes or trades),
        // meaning we don't need to send another subscribe request to Rithmic.
        let already_subscribed = {
            let entry = self.subscriptions.entry(key.clone());
            match entry {
                dashmap::mapref::entry::Entry::Occupied(mut e) => {
                    let sub = e.get_mut();
                    if sub.quotes {
                        return Ok(()); // Already subscribed to quotes
                    }
                    sub.quotes = true;
                    // If we already have trades subscription, we're already subscribed
                    // to this instrument at the Rithmic level
                    sub.trades
                }
                dashmap::mapref::entry::Entry::Vacant(e) => {
                    e.insert(InstrumentSubscription {
                        quotes: true,
                        trades: false,
                    });
                    false // No existing subscription
                }
            }
        };

        // Only send subscription request if we don't already have one for this instrument
        if !already_subscribed {
            self.gateway.subscribe_market_data(symbol, exchange).await?;
        }

        Ok(())
    }

    /// Subscribes to trades (last trade) for an instrument.
    ///
    /// After subscribing, `MarketDataEvent::Trade` events will be emitted
    /// on the gateway's market data receiver.
    ///
    /// # Note
    /// Rithmic's subscription returns both BBO and LastTrade messages.
    /// Calling `subscribe_trades` will also enable receiving quotes.
    pub async fn subscribe_trades(&self, symbol: &str, exchange: &str) -> Result<()> {
        if !self.is_connected() {
            return Err(RithmicError::Connection("Not connected".to_string()));
        }

        let key = format!("{exchange}:{symbol}");

        // Check if we already have an active subscription for this instrument.
        // Returns true if we already have ANY subscription (quotes or trades).
        let already_subscribed = {
            let entry = self.subscriptions.entry(key.clone());
            match entry {
                dashmap::mapref::entry::Entry::Occupied(mut e) => {
                    let sub = e.get_mut();
                    if sub.trades {
                        return Ok(()); // Already subscribed to trades
                    }
                    sub.trades = true;
                    // If we already have quotes subscription, we're already subscribed
                    sub.quotes
                }
                dashmap::mapref::entry::Entry::Vacant(e) => {
                    e.insert(InstrumentSubscription {
                        quotes: false,
                        trades: true,
                    });
                    false // No existing subscription
                }
            }
        };

        // Only send subscription request if we don't already have one for this instrument
        if !already_subscribed {
            self.gateway.subscribe_market_data(symbol, exchange).await?;
        }

        Ok(())
    }

    /// Subscribes to both quotes and trades for an instrument.
    ///
    /// This is equivalent to calling both `subscribe_quotes` and `subscribe_trades`.
    pub async fn subscribe(&self, symbol: &str, exchange: &str) -> Result<()> {
        if !self.is_connected() {
            return Err(RithmicError::Connection("Not connected".to_string()));
        }

        let key = format!("{exchange}:{symbol}");

        // Check if we already have any subscription for this instrument.
        let already_subscribed = {
            let entry = self.subscriptions.entry(key.clone());
            match entry {
                dashmap::mapref::entry::Entry::Occupied(mut e) => {
                    let sub = e.get_mut();
                    let was_subscribed = sub.quotes || sub.trades;
                    sub.quotes = true;
                    sub.trades = true;
                    was_subscribed
                }
                dashmap::mapref::entry::Entry::Vacant(e) => {
                    e.insert(InstrumentSubscription {
                        quotes: true,
                        trades: true,
                    });
                    false
                }
            }
        };

        if !already_subscribed {
            self.gateway.subscribe_market_data(symbol, exchange).await?;
        }

        Ok(())
    }

    /// Subscribes to live time bars for an instrument through the history plant.
    pub async fn subscribe_bars(
        &self,
        symbol: &str,
        exchange: &str,
        bar_type: TimeBarType,
        bar_period: i32,
    ) -> Result<()> {
        if !self.is_connected() {
            return Err(RithmicError::Connection("Not connected".to_string()));
        }

        let key = bar_subscription_key(symbol, exchange, bar_type, bar_period);
        if self.bar_subscriptions.contains_key(&key) {
            return Ok(());
        }

        self.gateway
            .subscribe_time_bars(symbol, exchange, bar_type, bar_period)
            .await?;
        self.bar_subscriptions.insert(key, ());

        Ok(())
    }

    /// Unsubscribes from quotes for an instrument (local tracking only).
    ///
    /// This method only updates local subscription tracking. It does **not**
    /// send an unsubscribe request to the venue. The venue will continue
    /// sending data until the connection is closed.
    ///
    /// Use [`unsubscribe_market_data_async`] if you need to stop data from
    /// the venue (e.g., to reduce bandwidth or message volume).
    ///
    /// [`unsubscribe_market_data_async`]: Self::unsubscribe_market_data_async
    pub fn unsubscribe_quotes(&self, symbol: &str, exchange: &str) {
        let key = format!("{exchange}:{symbol}");
        if let Some(mut sub) = self.subscriptions.get_mut(&key) {
            sub.quotes = false;
            if !sub.quotes && !sub.trades {
                drop(sub);
                self.subscriptions.remove(&key);
            }
        }
    }

    /// Unsubscribes from trades for an instrument (local tracking only).
    ///
    /// This method only updates local subscription tracking. It does **not**
    /// send an unsubscribe request to the venue. The venue will continue
    /// sending data until the connection is closed.
    ///
    /// Use [`unsubscribe_market_data_async`] if you need to stop data from
    /// the venue (e.g., to reduce bandwidth or message volume).
    ///
    /// [`unsubscribe_market_data_async`]: Self::unsubscribe_market_data_async
    pub fn unsubscribe_trades(&self, symbol: &str, exchange: &str) {
        let key = format!("{exchange}:{symbol}");
        if let Some(mut sub) = self.subscriptions.get_mut(&key) {
            sub.trades = false;
            if !sub.quotes && !sub.trades {
                drop(sub);
                self.subscriptions.remove(&key);
            }
        }
    }

    /// Unsubscribes from all market data (local tracking only).
    ///
    /// Clears local subscription tracking. Does **not** send unsubscribe
    /// requests to the venue.
    pub fn unsubscribe_all(&self) {
        self.subscriptions.clear();
        self.bar_subscriptions.clear();
    }

    /// Unsubscribes from market data and notifies the venue.
    ///
    /// Unlike the sync `unsubscribe_*` methods, this sends an actual
    /// unsubscribe request to the Rithmic ticker plant. Use this when
    /// you need to stop receiving data from the venue.
    pub async fn unsubscribe_market_data_async(&self, symbol: &str, exchange: &str) -> Result<()> {
        let key = format!("{exchange}:{symbol}");
        self.subscriptions.remove(&key);
        self.gateway.unsubscribe_market_data(symbol, exchange).await
    }

    /// Unsubscribes from live time bars and notifies the venue.
    pub async fn unsubscribe_bars(
        &self,
        symbol: &str,
        exchange: &str,
        bar_type: TimeBarType,
        bar_period: i32,
    ) -> Result<()> {
        let key = bar_subscription_key(symbol, exchange, bar_type, bar_period);
        self.bar_subscriptions.remove(&key);
        self.gateway
            .unsubscribe_time_bars(symbol, exchange, bar_type, bar_period)
            .await
    }

    /// Returns the number of active subscriptions.
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Returns all active subscription keys in "EXCHANGE:SYMBOL" format.
    pub fn subscriptions(&self) -> Vec<String> {
        self.subscriptions.iter().map(|r| r.key().clone()).collect()
    }

    /// Returns the number of active live bar subscriptions.
    pub fn bar_subscription_count(&self) -> usize {
        self.bar_subscriptions.len()
    }

    /// Returns all active live bar subscription keys.
    pub fn bar_subscriptions(&self) -> Vec<String> {
        self.bar_subscriptions
            .iter()
            .map(|r| r.key().clone())
            .collect()
    }

    /// Returns true if subscribed to quotes for the given instrument.
    pub fn is_subscribed_quotes(&self, symbol: &str, exchange: &str) -> bool {
        let key = format!("{exchange}:{symbol}");
        self.subscriptions
            .get(&key)
            .map(|s| s.quotes)
            .unwrap_or(false)
    }

    /// Returns true if subscribed to trades for the given instrument.
    pub fn is_subscribed_trades(&self, symbol: &str, exchange: &str) -> bool {
        let key = format!("{exchange}:{symbol}");
        self.subscriptions
            .get(&key)
            .map(|s| s.trades)
            .unwrap_or(false)
    }

    /// Returns true if subscribed to live bars for the given symbol/bar shape.
    pub fn is_subscribed_bars(
        &self,
        symbol: &str,
        exchange: &str,
        bar_type: TimeBarType,
        bar_period: i32,
    ) -> bool {
        let key = bar_subscription_key(symbol, exchange, bar_type, bar_period);
        self.bar_subscriptions.contains_key(&key)
    }
}

impl std::fmt::Debug for RithmicDataClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RithmicDataClient")
            .field("connection_state", &self.connection_state())
            .field("subscriptions", &self.subscription_count())
            .field("bar_subscriptions", &self.bar_subscription_count())
            .finish()
    }
}

fn bar_subscription_key(
    symbol: &str,
    exchange: &str,
    bar_type: TimeBarType,
    bar_period: i32,
) -> String {
    let bar_type = match bar_type {
        TimeBarType::SecondBar => "SecondBar",
        TimeBarType::MinuteBar => "MinuteBar",
        TimeBarType::DailyBar => "DailyBar",
        TimeBarType::WeeklyBar => "WeeklyBar",
    };

    format!("{exchange}:{symbol}:{bar_type}:{bar_period}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RithmicEnv;
    use crate::gateway::GatewayConfig;

    fn create_test_gateway() -> Arc<RithmicGateway> {
        let config = GatewayConfig::new(
            RithmicEnv::Demo,
            "user",
            "pass",
            "system",
            "fcm",
            "ib",
            "account",
        );
        Arc::new(RithmicGateway::new(config))
    }

    #[test]
    fn test_data_client_creation() {
        let gateway = create_test_gateway();
        let client = RithmicDataClient::new(gateway);
        assert_eq!(client.connection_state(), ConnectionState::Disconnected);
        assert_eq!(client.subscription_count(), 0);
        assert_eq!(client.bar_subscription_count(), 0);
    }

    #[test]
    fn test_subscription_tracking() {
        let gateway = create_test_gateway();
        let client = RithmicDataClient::new(gateway);

        // Manually insert subscription for testing (without actually subscribing)
        client.subscriptions.insert(
            "CME:ESZ4".to_string(),
            InstrumentSubscription {
                quotes: true,
                trades: false,
            },
        );

        assert!(client.is_subscribed_quotes("ESZ4", "CME"));
        assert!(!client.is_subscribed_trades("ESZ4", "CME"));
        assert_eq!(client.subscription_count(), 1);

        // Update to include trades
        if let Some(mut sub) = client.subscriptions.get_mut("CME:ESZ4") {
            sub.trades = true;
        }
        assert!(client.is_subscribed_trades("ESZ4", "CME"));

        // Unsubscribe from quotes
        client.unsubscribe_quotes("ESZ4", "CME");
        assert!(!client.is_subscribed_quotes("ESZ4", "CME"));
        assert!(client.is_subscribed_trades("ESZ4", "CME"));
        assert_eq!(client.subscription_count(), 1);

        // Unsubscribe from trades - should remove entry
        client.unsubscribe_trades("ESZ4", "CME");
        assert_eq!(client.subscription_count(), 0);
    }

    #[test]
    fn test_unsubscribe_all() {
        let gateway = create_test_gateway();
        let client = RithmicDataClient::new(gateway);

        // Add some subscriptions
        client.subscriptions.insert(
            "CME:ESZ4".to_string(),
            InstrumentSubscription {
                quotes: true,
                trades: true,
            },
        );
        client.subscriptions.insert(
            "CME:NQZ4".to_string(),
            InstrumentSubscription {
                quotes: true,
                trades: false,
            },
        );
        client
            .bar_subscriptions
            .insert("CME:ESZ4:MinuteBar:1".to_string(), ());

        assert_eq!(client.subscription_count(), 2);
        assert_eq!(client.bar_subscription_count(), 1);

        client.unsubscribe_all();
        assert_eq!(client.subscription_count(), 0);
        assert_eq!(client.bar_subscription_count(), 0);
    }

    #[test]
    fn test_subscriptions_list() {
        let gateway = create_test_gateway();
        let client = RithmicDataClient::new(gateway);

        client.subscriptions.insert(
            "CME:ESZ4".to_string(),
            InstrumentSubscription {
                quotes: true,
                trades: true,
            },
        );
        client.subscriptions.insert(
            "NYMEX:CLZ4".to_string(),
            InstrumentSubscription {
                quotes: true,
                trades: false,
            },
        );

        let subs = client.subscriptions();
        assert_eq!(subs.len(), 2);
        assert!(subs.contains(&"CME:ESZ4".to_string()));
        assert!(subs.contains(&"NYMEX:CLZ4".to_string()));
    }

    #[test]
    fn test_bar_subscriptions_list() {
        let gateway = create_test_gateway();
        let client = RithmicDataClient::new(gateway);

        client
            .bar_subscriptions
            .insert("CME:ESZ4:MinuteBar:1".to_string(), ());

        assert!(client.is_subscribed_bars("ESZ4", "CME", TimeBarType::MinuteBar, 1));
        assert_eq!(client.bar_subscription_count(), 1);
        assert_eq!(
            client.bar_subscriptions(),
            vec!["CME:ESZ4:MinuteBar:1".to_string()],
        );
    }
}
