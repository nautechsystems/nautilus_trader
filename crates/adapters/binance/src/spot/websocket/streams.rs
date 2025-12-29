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
