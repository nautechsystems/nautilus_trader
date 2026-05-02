//! ContextWindow: the shared-memory state representation for agents.

use std::fmt;

/// Greeks representation for options positions.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Greeks {
    pub delta: f64,
    pub gamma: f64,
    pub theta: f64,
    pub vega: f64,
}

/// A compact event token for the rolling event trace.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EventToken {
    /// 0=Quote, 1=Trade, 2=Fill, 3=Alert, 4=RiskAlert
    pub event_type: u8,
    pub price: f64,
    pub size: f64,
    pub timestamp_ns: u64,
}

/// The shared-memory state window that agents read via mmap.
///
/// Layout is `#[repr(C)]` for cross-process compatibility.
/// Agents map this read-only and check `version` for consistency.
#[repr(C)]
pub struct ContextWindow {
    /// Sequence number for consistency checks (even = stable, odd = writing).
    pub version: u64,
    /// Nanosecond timestamp of last update.
    pub timestamp_ns: u64,
    /// Instrument identifier (UTF-8, null-padded).
    pub instrument_id: [u8; 32],
    pub instrument_id_len: u8,

    // Market state (text, LLM-readable)
    pub market_state_len: u32,
    pub market_state: [u8; 2048],

    // Position state
    pub position_size: f64,
    pub entry_price: f64,
    pub unrealized_pnl: f64,
    pub greeks: Greeks,

    // Risk potential field (agent senses gradient)
    pub risk_potential: f64,       // Total risk potential [0, +inf)
    pub position_potential: f64,   // Position potential
    pub drawdown_potential: f64,   // Drawdown potential

    // Rolling event trace (last N events)
    pub event_trace_len: u32,
    pub event_trace: [EventToken; 64],
}

impl ContextWindow {
    pub fn zeroed() -> Self {
        Self {
            version: 0,
            timestamp_ns: 0,
            instrument_id: [0u8; 32],
            instrument_id_len: 0,
            market_state_len: 0,
            market_state: [0u8; 2048],
            position_size: 0.0,
            entry_price: 0.0,
            unrealized_pnl: 0.0,
            greeks: Greeks::default(),
            risk_potential: 0.0,
            position_potential: 0.0,
            drawdown_potential: 0.0,
            event_trace_len: 0,
            event_trace: [EventToken::default(); 64],
        }
    }

    /// Set instrument ID from a string, truncating to 32 bytes.
    pub fn set_instrument_id(&mut self, id: &str) {
        let bytes = id.as_bytes();
        let len = bytes.len().min(32);
        self.instrument_id[..len].copy_from_slice(&bytes[..len]);
        self.instrument_id_len = len as u8;
    }

    /// Get instrument ID as a string slice.
    pub fn instrument_id_str(&self) -> &str {
        let len = self.instrument_id_len as usize;
        std::str::from_utf8(&self.instrument_id[..len]).unwrap_or("")
    }

    /// Set market state from a string, truncating to 2048 bytes.
    pub fn set_market_state(&mut self, state: &str) {
        let bytes = state.as_bytes();
        let len = bytes.len().min(2048);
        self.market_state[..len].copy_from_slice(&bytes[..len]);
        self.market_state_len = len as u32;
    }

    /// Get market state as a string slice.
    pub fn market_state_str(&self) -> &str {
        let len = self.market_state_len as usize;
        std::str::from_utf8(&self.market_state[..len]).unwrap_or("")
    }

    /// Push an event token into the rolling trace (circular buffer).
    /// Uses a write counter in event_trace_len that always increments,
    /// wrapping index via modulo.
    pub fn push_event(&mut self, event: EventToken) {
        let idx = (self.event_trace_len as usize) % 64;
        self.event_trace[idx] = event;
        self.event_trace_len = self.event_trace_len.wrapping_add(1);
    }

    /// Number of events stored (capped at 64).
    pub fn event_count(&self) -> u32 {
        self.event_trace_len.min(64)
    }
}

impl fmt::Debug for ContextWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContextWindow")
            .field("version", &self.version)
            .field("instrument", &self.instrument_id_str())
            .field("position", &self.position_size)
            .field("risk_potential", &self.risk_potential)
            .field("events", &self.event_trace_len)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_window_basic() {
        let mut ctx = ContextWindow::zeroed();
        assert_eq!(ctx.version, 0);

        ctx.set_instrument_id("SOL-USDC");
        assert_eq!(ctx.instrument_id_str(), "SOL-USDC");

        ctx.set_market_state("OrderBook[SOL-USDC] best_bid:150.0 best_ask:150.1");
        assert!(ctx.market_state_str().starts_with("OrderBook"));

        ctx.push_event(EventToken {
            event_type: 0,
            price: 150.05,
            size: 10.0,
            timestamp_ns: 1_000_000_000,
        });
        assert_eq!(ctx.event_trace_len, 1);
    }

    #[test]
    fn test_circular_buffer() {
        let mut ctx = ContextWindow::zeroed();

        for i in 0..70u64 {
            ctx.push_event(EventToken {
                event_type: 0,
                price: i as f64,
                size: 1.0,
                timestamp_ns: i,
            });
        }
        assert_eq!(ctx.event_trace_len, 70);
        assert_eq!(ctx.event_count(), 64);

        // Check all indices that should have been overwritten
        for i in 64..70u64 {
            let idx = (i as usize) % 64;
            assert_eq!(
                ctx.event_trace[idx].price,
                i as f64,
                "Index {} should have price {} but has {}",
                idx,
                i,
                ctx.event_trace[idx].price
            );
        }
    }
}
