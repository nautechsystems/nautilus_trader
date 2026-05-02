//! StateEncoder: bridges Nautilus events to shared-memory ContextWindow.

use nautilus_model::data::QuoteTick;

use crate::context_window::{ContextWindow, EventToken};
use crate::shared_buffer::SharedStateBuffer;

/// Encodes Nautilus internal events into shared-memory ContextWindows.
///
/// Hooks into MessageBus publish path as a sidecar — does not modify
/// the original event flow.
pub struct StateEncoder {
    buffer: SharedStateBuffer,
    current: ContextWindow,
}

impl StateEncoder {
    pub fn new(instrument_id: &str) -> Self {
        let mut current = ContextWindow::zeroed();
        current.set_instrument_id(instrument_id);

        Self {
            buffer: SharedStateBuffer::new(),
            current,
        }
    }

    /// Handle a quote tick update.
    pub fn on_quote(&mut self, quote: &QuoteTick) {
        self.current.version += 1;
        self.current.timestamp_ns = quote.ts_event.as_u64();

        // Format market state as LLM-readable text
        let state = format!(
            "OrderBook[{}] bid:{} @ {} | ask:{} @ {}",
            self.current.instrument_id_str(),
            quote.bid_size,
            quote.bid_price,
            quote.ask_size,
            quote.ask_price,
        );
        self.current.set_market_state(&state);

        // Push to event trace
        self.current.push_event(EventToken {
            event_type: 0, // Quote
            price: (quote.bid_price.as_f64() + quote.ask_price.as_f64()) / 2.0,
            size: quote.bid_size.as_f64() + quote.ask_size.as_f64(),
            timestamp_ns: quote.ts_event.as_u64(),
        });

        // Write to shared memory
        self.buffer.write(&self.current);
    }

    /// Update position state.
    pub fn update_position(&mut self, size: f64, entry_price: f64, unrealized_pnl: f64) {
        self.current.position_size = size;
        self.current.entry_price = entry_price;
        self.current.unrealized_pnl = unrealized_pnl;
        self.current.version += 1;
        self.buffer.write(&self.current);
    }

    /// Update risk potential values.
    pub fn update_risk(&mut self, total: f64, position: f64, drawdown: f64) {
        self.current.risk_potential = total;
        self.current.position_potential = position;
        self.current.drawdown_potential = drawdown;
        self.current.version += 1;
        self.buffer.write(&self.current);
    }

    /// Get read access to the shared buffer (for agent processes).
    pub fn buffer(&self) -> &SharedStateBuffer {
        &self.buffer
    }

    /// Get the current local context (before write).
    pub fn current_context(&self) -> &ContextWindow {
        &self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nautilus_core::UnixNanos;
    use nautilus_model::data::QuoteTick;
    use nautilus_model::identifiers::InstrumentId;
    use nautilus_model::types::{Price, Quantity};

    #[test]
    fn test_encoder_quote_update() {
        let id = InstrumentId::from("SOL-USDC.OKX");
        let mut encoder = StateEncoder::new("SOL-USDC");

        let quote = QuoteTick::new(
            id,
            Price::from("150.00"),
            Price::from("150.10"),
            Quantity::from("10.0"),
            Quantity::from("5.0"),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(1_000_000_001),
        );

        encoder.on_quote(&quote);

        let ctx = encoder.buffer.read().unwrap();
        assert_eq!(ctx.version, 1);
        assert!(ctx.market_state_str().contains("150.00"));
        assert_eq!(ctx.event_count(), 1);
    }
}
