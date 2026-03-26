//! Message handler for Rithmic market data.

use ahash::AHashMap;
use rithmic_rs::rti::messages::RithmicMessage;
use tracing::debug;

use crate::error::Result;

use super::client::{MarketDataEvent, QuoteTick, TradeTick};

/// Handles incoming market data messages from Rithmic.
#[allow(dead_code)] // Scaffolding for future implementation
pub struct MarketDataHandler {
    quotes: parking_lot::Mutex<AHashMap<String, QuoteTick>>,
}

#[allow(dead_code)] // Scaffolding for future implementation
impl MarketDataHandler {
    /// Creates a new message handler.
    pub fn new() -> Self {
        Self {
            quotes: parking_lot::Mutex::new(AHashMap::new()),
        }
    }

    /// Clears cached quote state, typically after a disconnect or stream reset.
    pub fn clear_quotes(&self) {
        self.quotes.lock().clear();
    }

    /// Handles a best bid/offer update message.
    pub fn handle_bbo_update(
        &self,
        symbol: &str,
        exchange: &str,
        bid_price: Option<f64>,
        bid_size: Option<f64>,
        ask_price: Option<f64>,
        ask_size: Option<f64>,
        ts_nanos: u64,
    ) -> Result<QuoteTick> {
        let key = format!("{exchange}:{symbol}");
        let mut quotes = self.quotes.lock();
        let prior = quotes.get(&key).cloned();

        let quote = QuoteTick {
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            bid_price: bid_price
                .or_else(|| prior.as_ref().map(|quote| quote.bid_price))
                .unwrap_or(0.0),
            ask_price: ask_price
                .or_else(|| prior.as_ref().map(|quote| quote.ask_price))
                .unwrap_or(0.0),
            bid_size: bid_size
                .or_else(|| prior.as_ref().map(|quote| quote.bid_size))
                .unwrap_or(0.0),
            ask_size: ask_size
                .or_else(|| prior.as_ref().map(|quote| quote.ask_size))
                .unwrap_or(0.0),
            ts_event: ts_nanos,
            ts_init: ts_nanos,
        };

        quotes.insert(key, quote.clone());

        Ok(quote)
    }

    /// Handles a last trade update message.
    pub fn handle_trade_update(
        &self,
        symbol: &str,
        exchange: &str,
        price: f64,
        size: f64,
        aggressor_side: &str,
        trade_id: &str,
        ts_nanos: u64,
    ) -> Result<TradeTick> {
        Ok(TradeTick {
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            price,
            size,
            aggressor_side: aggressor_side.to_string(),
            trade_id: trade_id.to_string(),
            ts_event: ts_nanos,
            ts_init: ts_nanos,
        })
    }

    /// Processes a Rithmic message and returns the appropriate market data event.
    ///
    /// Matches on `BestBidOffer` and `LastTrade` variants, extracting fields
    /// and delegating to the appropriate handler method. Returns `None` for
    /// unrecognized or incomplete messages.
    pub fn process_message(&self, message: &RithmicMessage) -> Result<Option<MarketDataEvent>> {
        match message {
            RithmicMessage::ConnectionError | RithmicMessage::HeartbeatTimeout => {
                self.clear_quotes();
                Ok(None)
            }
            RithmicMessage::BestBidOffer(bbo) => {
                use rithmic_rs::rti::best_bid_offer::PresenceBits;

                let symbol = match bbo.symbol.as_ref() {
                    Some(s) => s,
                    None => return Ok(None),
                };
                let exchange = match bbo.exchange.as_ref() {
                    Some(e) => e,
                    None => return Ok(None),
                };

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
                    return Ok(None);
                }

                let ts_nanos = ssboe_usecs_to_nanos(bbo.ssboe, bbo.usecs);

                let quote = self.handle_bbo_update(
                    symbol,
                    exchange,
                    if bid_updated { bbo.bid_price } else { None },
                    if bid_updated {
                        bbo.bid_size.map(|value| value as f64)
                    } else {
                        None
                    },
                    if ask_updated { bbo.ask_price } else { None },
                    if ask_updated {
                        bbo.ask_size.map(|value| value as f64)
                    } else {
                        None
                    },
                    ts_nanos,
                )?;

                if quote_is_complete(&quote) {
                    Ok(Some(MarketDataEvent::Quote(quote)))
                } else {
                    Ok(None)
                }
            }
            RithmicMessage::LastTrade(trade) => {
                let symbol = match trade.symbol.as_ref() {
                    Some(s) => s,
                    None => return Ok(None),
                };
                let exchange = match trade.exchange.as_ref() {
                    Some(e) => e,
                    None => return Ok(None),
                };
                let price = match trade.trade_price {
                    Some(p) => p,
                    None => return Ok(None),
                };

                let size = trade.trade_size.unwrap_or(0);
                if size == 0 {
                    return Ok(None);
                }

                // Rithmic TransactionType: 1 = Buy (aggressor bought), 2 = Sell (aggressor sold)
                let aggressor_side = match trade.aggressor {
                    Some(1) => "BUY",
                    Some(2) => "SELL",
                    _ => "UNKNOWN",
                };

                let trade_id = trade.exchange_order_id.as_deref().unwrap_or("");
                let ts_nanos = ssboe_usecs_to_nanos(trade.ssboe, trade.usecs);

                let tick = self.handle_trade_update(
                    symbol,
                    exchange,
                    price,
                    size as f64,
                    aggressor_side,
                    trade_id,
                    ts_nanos,
                )?;

                Ok(Some(MarketDataEvent::Trade(tick)))
            }
            _ => {
                debug!("Unhandled market data message type");
                Ok(None)
            }
        }
    }
}

/// Converts Rithmic's ssboe (seconds since beginning of epoch) and usecs to nanoseconds.
///
/// Uses integer arithmetic to avoid floating-point precision loss.
#[inline]
fn ssboe_usecs_to_nanos(ssboe: Option<i32>, usecs: Option<i32>) -> u64 {
    let secs = ssboe.unwrap_or(0).max(0) as u64;
    let micros = usecs.unwrap_or(0).max(0) as u64;
    secs * 1_000_000_000 + micros * 1_000
}

#[inline]
fn bbo_side_updated(bits: Option<u32>, bit: u32, price: Option<f64>, size: Option<i32>) -> bool {
    match bits {
        Some(bits) => bits & bit != 0,
        None => price.is_some_and(|value| value > 0.0) || size.is_some_and(|value| value > 0),
    }
}

#[inline]
fn quote_is_complete(quote: &QuoteTick) -> bool {
    quote.bid_price > 0.0 && quote.ask_price > 0.0 && quote.bid_size > 0.0 && quote.ask_size > 0.0
}

impl Default for MarketDataHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rithmic_rs::rti::{BestBidOffer, LastTrade};

    fn make_bbo(
        symbol: Option<&str>,
        exchange: Option<&str>,
        bid_price: Option<f64>,
        bid_size: Option<i32>,
        ask_price: Option<f64>,
        ask_size: Option<i32>,
        ssboe: Option<i32>,
        usecs: Option<i32>,
    ) -> RithmicMessage {
        RithmicMessage::BestBidOffer(BestBidOffer {
            template_id: 150,
            symbol: symbol.map(String::from),
            exchange: exchange.map(String::from),
            presence_bits: None,
            clear_bits: None,
            is_snapshot: Some(false),
            bid_price,
            bid_size,
            bid_orders: None,
            bid_implicit_size: None,
            bid_time: None,
            ask_price,
            ask_size,
            ask_orders: None,
            ask_implicit_size: None,
            ask_time: None,
            lean_price: None,
            ssboe,
            usecs,
        })
    }

    fn make_last_trade(
        symbol: Option<&str>,
        exchange: Option<&str>,
        trade_price: Option<f64>,
        trade_size: Option<i32>,
        aggressor: Option<i32>,
        exchange_order_id: Option<&str>,
        ssboe: Option<i32>,
        usecs: Option<i32>,
    ) -> RithmicMessage {
        RithmicMessage::LastTrade(LastTrade {
            template_id: 151,
            symbol: symbol.map(String::from),
            exchange: exchange.map(String::from),
            presence_bits: None,
            clear_bits: None,
            is_snapshot: Some(false),
            trade_price,
            trade_size,
            aggressor,
            exchange_order_id: exchange_order_id.map(String::from),
            aggressor_exchange_order_id: None,
            net_change: None,
            percent_change: None,
            volume: None,
            vwap: None,
            trade_time: None,
            ssboe,
            usecs,
            source_ssboe: None,
            source_usecs: None,
            source_nsecs: None,
            jop_ssboe: None,
            jop_nsecs: None,
        })
    }

    #[test]
    fn test_process_bbo_message() {
        let handler = MarketDataHandler::new();
        let msg = make_bbo(
            Some("ESZ4"),
            Some("CME"),
            Some(5000.25),
            Some(100),
            Some(5000.50),
            Some(150),
            Some(1704067200),
            Some(500000),
        );

        let result = handler.process_message(&msg).unwrap();
        let event = result.expect("expected Some event");

        if let MarketDataEvent::Quote(quote) = event {
            assert_eq!(quote.symbol, "ESZ4");
            assert_eq!(quote.exchange, "CME");
            assert_eq!(quote.bid_price, 5000.25);
            assert_eq!(quote.ask_price, 5000.50);
            assert_eq!(quote.bid_size, 100.0);
            assert_eq!(quote.ask_size, 150.0);
            // ssboe=1704067200, usecs=500000 → 1704067200.5s → 1704067200500000000ns
            assert_eq!(quote.ts_event, 1704067200500000000);
        } else {
            panic!("Expected Quote event");
        }
    }

    #[test]
    fn test_process_last_trade_message() {
        let handler = MarketDataHandler::new();
        let msg = make_last_trade(
            Some("ESZ4"),
            Some("CME"),
            Some(5000.25),
            Some(10),
            Some(1), // Buy
            Some("12345"),
            Some(1704067200),
            Some(500000),
        );

        let result = handler.process_message(&msg).unwrap();
        let event = result.expect("expected Some event");

        if let MarketDataEvent::Trade(trade) = event {
            assert_eq!(trade.symbol, "ESZ4");
            assert_eq!(trade.exchange, "CME");
            assert_eq!(trade.price, 5000.25);
            assert_eq!(trade.size, 10.0);
            assert_eq!(trade.aggressor_side, "BUY");
            assert_eq!(trade.trade_id, "12345");
            // ssboe=1704067200, usecs=500000 → 1704067200.5s → 1704067200500000000ns
            assert_eq!(trade.ts_event, 1704067200500000000);
        } else {
            panic!("Expected Trade event");
        }
    }

    #[test]
    fn test_process_bbo_missing_symbol() {
        let handler = MarketDataHandler::new();
        let msg = make_bbo(
            None, // no symbol
            Some("CME"),
            Some(5000.25),
            Some(100),
            Some(5000.50),
            Some(150),
            Some(1704067200),
            Some(500000),
        );

        let result = handler.process_message(&msg).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_process_bbo_no_prices() {
        let handler = MarketDataHandler::new();
        let msg = make_bbo(
            Some("ESZ4"),
            Some("CME"),
            None, // no bid
            None,
            None, // no ask
            None,
            Some(1704067200),
            Some(500000),
        );

        let result = handler.process_message(&msg).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_process_trade_missing_price() {
        let handler = MarketDataHandler::new();
        let msg = make_last_trade(
            Some("ESZ4"),
            Some("CME"),
            None, // no price
            Some(10),
            Some(1),
            Some("12345"),
            Some(1704067200),
            Some(500000),
        );

        let result = handler.process_message(&msg).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_process_trade_zero_size() {
        let handler = MarketDataHandler::new();
        let msg = make_last_trade(
            Some("ESZ4"),
            Some("CME"),
            Some(5000.25),
            Some(0), // zero size
            Some(1),
            Some("12345"),
            Some(1704067200),
            Some(500000),
        );

        let result = handler.process_message(&msg).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_process_bbo_partial_prices() {
        use rithmic_rs::rti::best_bid_offer::PresenceBits;

        let handler = MarketDataHandler::new();
        let initial = make_bbo(
            Some("ESZ4"),
            Some("CME"),
            Some(5000.25),
            Some(100),
            Some(5000.50),
            Some(150),
            Some(1704067200),
            Some(400000),
        );

        let partial = make_bbo(
            Some("ESZ4"),
            Some("CME"),
            Some(5000.25),
            Some(99),
            None, // no ask price
            None,
            Some(1704067200),
            Some(500000),
        );

        let partial = match partial {
            RithmicMessage::BestBidOffer(mut bbo) => {
                bbo.presence_bits = Some(PresenceBits::Bid as u32);
                bbo.ask_price = Some(0.0);
                bbo.ask_size = Some(0);
                RithmicMessage::BestBidOffer(bbo)
            }
            other => other,
        };

        let _ = handler.process_message(&initial).unwrap();
        let result = handler.process_message(&partial).unwrap();
        let event = result.expect("expected Some event for one-sided quote");

        if let MarketDataEvent::Quote(quote) = event {
            assert_eq!(quote.bid_price, 5000.25);
            assert_eq!(quote.bid_size, 99.0);
            assert_eq!(quote.ask_price, 5000.50);
            assert_eq!(quote.ask_size, 150.0);
        } else {
            panic!("Expected Quote event");
        }
    }

    #[test]
    fn test_process_bbo_clears_cached_quote_on_connection_issue() {
        use rithmic_rs::rti::best_bid_offer::PresenceBits;

        let handler = MarketDataHandler::new();
        let initial = make_bbo(
            Some("ESM6"),
            Some("CME"),
            Some(6619.00),
            Some(9),
            Some(6619.50),
            Some(6),
            Some(1704067200),
            Some(400000),
        );

        let partial_after_disconnect = make_bbo(
            Some("ESM6"),
            Some("CME"),
            None,
            None,
            Some(6619.50),
            Some(5),
            Some(1704067201),
            Some(0),
        );

        let partial_after_disconnect = match partial_after_disconnect {
            RithmicMessage::BestBidOffer(mut bbo) => {
                bbo.presence_bits = Some(PresenceBits::Ask as u32);
                bbo.bid_price = Some(0.0);
                bbo.bid_size = Some(0);
                RithmicMessage::BestBidOffer(bbo)
            }
            other => other,
        };

        let bid_after_disconnect = make_bbo(
            Some("ESM6"),
            Some("CME"),
            Some(6619.00),
            Some(8),
            Some(0.0),
            Some(0),
            Some(1704067201),
            Some(1000),
        );
        let bid_after_disconnect = match bid_after_disconnect {
            RithmicMessage::BestBidOffer(mut bbo) => {
                bbo.presence_bits = Some(PresenceBits::Bid as u32);
                RithmicMessage::BestBidOffer(bbo)
            }
            other => other,
        };

        let _ = handler.process_message(&initial).unwrap();
        let cleared = handler
            .process_message(&RithmicMessage::ConnectionError)
            .unwrap();
        assert!(cleared.is_none());

        let partial = handler.process_message(&partial_after_disconnect).unwrap();
        assert!(partial.is_none());

        let result = handler.process_message(&bid_after_disconnect).unwrap();
        let event = result.expect("expected full quote after both sides observed");

        if let MarketDataEvent::Quote(quote) = event {
            assert_eq!(quote.bid_price, 6619.00);
            assert_eq!(quote.bid_size, 8.0);
            assert_eq!(quote.ask_price, 6619.50);
            assert_eq!(quote.ask_size, 5.0);
        } else {
            panic!("Expected Quote event");
        }
    }

    #[test]
    fn test_process_bbo_preserves_last_seen_ask_when_bid_only_update_zeroes_ask_fields() {
        use rithmic_rs::rti::best_bid_offer::PresenceBits;

        let handler = MarketDataHandler::new();
        let initial = make_bbo(
            Some("ESM6"),
            Some("CME"),
            Some(6617.00),
            Some(3),
            Some(6617.25),
            Some(1),
            Some(1704067200),
            Some(0),
        );
        let initial = match initial {
            RithmicMessage::BestBidOffer(mut bbo) => {
                bbo.presence_bits = Some((PresenceBits::Bid as u32) | (PresenceBits::Ask as u32));
                RithmicMessage::BestBidOffer(bbo)
            }
            other => other,
        };

        let bid_only = make_bbo(
            Some("ESM6"),
            Some("CME"),
            Some(6616.75),
            Some(10),
            Some(0.0),
            Some(0),
            Some(1704067201),
            Some(0),
        );
        let bid_only = match bid_only {
            RithmicMessage::BestBidOffer(mut bbo) => {
                bbo.presence_bits = Some(PresenceBits::Bid as u32);
                RithmicMessage::BestBidOffer(bbo)
            }
            other => other,
        };

        let _ = handler.process_message(&initial).unwrap();
        let result = handler.process_message(&bid_only).unwrap();
        let event = result.expect("expected Some event for one-sided quote");

        if let MarketDataEvent::Quote(quote) = event {
            assert_eq!(quote.bid_price, 6616.75);
            assert_eq!(quote.bid_size, 10.0);
            assert_eq!(quote.ask_price, 6617.25);
            assert_eq!(quote.ask_size, 1.0);
        } else {
            panic!("Expected Quote event");
        }
    }

    #[test]
    fn test_process_bbo_waits_until_both_sides_seen_before_emitting_quote() {
        use rithmic_rs::rti::best_bid_offer::PresenceBits;

        let handler = MarketDataHandler::new();

        let ask_only = make_bbo(
            Some("ESM6"),
            Some("CME"),
            Some(0.0),
            Some(0),
            Some(6617.25),
            Some(1),
            Some(1704067200),
            Some(0),
        );
        let ask_only = match ask_only {
            RithmicMessage::BestBidOffer(mut bbo) => {
                bbo.presence_bits = Some(PresenceBits::Ask as u32);
                RithmicMessage::BestBidOffer(bbo)
            }
            other => other,
        };

        let bid_only = make_bbo(
            Some("ESM6"),
            Some("CME"),
            Some(6617.00),
            Some(3),
            Some(0.0),
            Some(0),
            Some(1704067201),
            Some(0),
        );
        let bid_only = match bid_only {
            RithmicMessage::BestBidOffer(mut bbo) => {
                bbo.presence_bits = Some(PresenceBits::Bid as u32);
                RithmicMessage::BestBidOffer(bbo)
            }
            other => other,
        };

        let first = handler.process_message(&ask_only).unwrap();
        assert!(first.is_none());

        let second = handler.process_message(&bid_only).unwrap();
        let event = second.expect("expected full quote after both sides observed");

        if let MarketDataEvent::Quote(quote) = event {
            assert_eq!(quote.bid_price, 6617.00);
            assert_eq!(quote.bid_size, 3.0);
            assert_eq!(quote.ask_price, 6617.25);
            assert_eq!(quote.ask_size, 1.0);
        } else {
            panic!("Expected Quote event");
        }
    }
}
