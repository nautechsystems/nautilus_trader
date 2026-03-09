//! Stream subscription management for Binance Spot WebSocket.
//!
//! ## Stream Names
//!
//! - `<symbol>@trade` - Trade stream
//! - `<symbol>@bestBidAsk` - Best bid/ask stream (with auto-culling)
//! - `<symbol>@depth` - Diff depth stream (50ms updates)
//! - `<symbol>@depth20` - Partial book depth (top 20 levels, 50ms updates)
//!
//! ## Connection URL Patterns
//!
//! Single stream: `/ws/<streamName>`
//! Multiple streams: `/stream?streams=<stream1>/<stream2>/...`

/// Maximum number of streams per connection.
pub const MAX_STREAMS_PER_CONNECTION: usize = 1024;

/// Stream type for subscription management.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamType {
    /// Trade stream (`<symbol>@trade`).
    Trade,
    /// Best bid/ask stream (`<symbol>@bestBidAsk`).
    BestBidAsk,
    /// Diff depth stream (`<symbol>@depth`).
    DepthDiff,
    /// Partial book depth stream (`<symbol>@depth<N>`).
    DepthSnapshot { levels: u8 },
}

impl StreamType {
    /// Build stream name for a symbol.
    #[must_use]
    pub fn stream_name(&self, symbol: &str) -> String {
        let symbol_lower = symbol.to_lowercase();
        match self {
            Self::Trade => format!("{symbol_lower}@trade"),
            Self::BestBidAsk => format!("{symbol_lower}@bestBidAsk"),
            Self::DepthDiff => format!("{symbol_lower}@depth"),
            Self::DepthSnapshot { levels } => format!("{symbol_lower}@depth{levels}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_stream_names() {
        assert_eq!(StreamType::Trade.stream_name("BTCUSDT"), "btcusdt@trade");
        assert_eq!(
            StreamType::BestBidAsk.stream_name("ETHUSDT"),
            "ethusdt@bestBidAsk"
        );
        assert_eq!(
            StreamType::DepthDiff.stream_name("BTCUSDT"),
            "btcusdt@depth"
        );
        assert_eq!(
            StreamType::DepthSnapshot { levels: 20 }.stream_name("BTCUSDT"),
            "btcusdt@depth20"
        );
    }
}
