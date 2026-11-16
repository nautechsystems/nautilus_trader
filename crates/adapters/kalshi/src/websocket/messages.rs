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

//! Typed control messages and channel identities for the Predictions WebSocket protocol.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::EnumIter;

/// Predictions WebSocket channels; subscription support is validated separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalshiChannel {
    OrderbookDelta,
    Ticker,
    Trade,
    Fill,
    MarketPositions,
    MarketLifecycleV2,
    UserOrders,
    MultivariateMarketLifecycle,
    Communications,
    OrderGroupUpdates,
    CfbenchmarksValue,
    #[serde(rename = "cfbenchmarks_value_5hz")]
    CfbenchmarksValue5hz,
    PythValue,
}

/// A shared subscription's original channel and market filter.
///
/// An empty market list selects the channel's global stream where supported.
/// Order books always request YES-leg pricing. Ticker subscriptions request initial snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KalshiSubscription {
    pub channel: KalshiChannel,
    pub market_tickers: Vec<String>,
}

impl KalshiSubscription {
    /// Validates and sorts the market filter without changing its meaning.
    ///
    /// # Errors
    ///
    /// Returns an error if tickers are empty or duplicated, order-book markets are absent,
    /// or the channel subscription or market filter is unsupported.
    pub(crate) fn normalize(mut self) -> anyhow::Result<Self> {
        anyhow::ensure!(
            self.market_tickers
                .iter()
                .all(|ticker| !ticker.trim().is_empty()),
            "market tickers must not be empty"
        );
        self.market_tickers.sort();
        anyhow::ensure!(
            !self
                .market_tickers
                .windows(2)
                .any(|pair| pair[0] == pair[1]),
            "duplicate market ticker"
        );

        match self.channel {
            KalshiChannel::OrderbookDelta => anyhow::ensure!(
                !self.market_tickers.is_empty(),
                "order books require market tickers"
            ),
            KalshiChannel::MarketLifecycleV2 => anyhow::ensure!(
                self.market_tickers.is_empty(),
                "channel does not support market filters"
            ),
            KalshiChannel::Ticker
            | KalshiChannel::Trade
            | KalshiChannel::Fill
            | KalshiChannel::MarketPositions
            | KalshiChannel::UserOrders => {}
            _ => anyhow::bail!("channel subscriptions are not supported"),
        }

        Ok(self)
    }

    pub(crate) fn topic(&self) -> String {
        serde_json::to_string(self).expect("subscription serialization is infallible")
    }
}

/// Supported updates to an existing Predictions subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalshiUpdateAction {
    AddMarkets,
    DeleteMarkets,
    GetSnapshot,
}

/// A client command with a nonzero session-unique correlation ID.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct KalshiCommand {
    pub id: u64,
    #[serde(flatten)]
    pub operation: KalshiOperation,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "cmd", content = "params", rename_all = "snake_case")]
pub(crate) enum KalshiOperation {
    Subscribe {
        channels: [KalshiChannel; 1],
        #[serde(skip_serializing_if = "Vec::is_empty")]
        market_tickers: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        use_yes_price: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        send_initial_snapshot: Option<bool>,
    },
    Unsubscribe {
        sids: [u64; 1],
    },
    UpdateSubscription {
        sid: u64,
        market_tickers: Vec<String>,
        action: KalshiUpdateAction,
        #[serde(skip_serializing_if = "Option::is_none")]
        send_initial_snapshot: Option<bool>,
    },
}

/// A single decoded server frame, before domain-specific payload conversion.
///
/// Data payloads retain the provider's decimal strings and optional fields unchanged.
/// The handler validates channel and subscription ownership before emitting them.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum KalshiWsMessage {
    Subscribed {
        id: u64,
        msg: KalshiSubscribed,
    },
    Unsubscribed {
        id: Option<u64>,
        sid: u64,
        seq: u64,
    },
    Ok {
        id: Option<u64>,
        sid: Option<u64>,
        seq: Option<u64>,
        sending_ts_ms: Option<u64>,
        msg: Option<KalshiUpdated>,
    },
    Error {
        id: Option<u64>,
        sid: Option<u64>,
        msg: KalshiServerError,
    },
    OrderbookSnapshot(KalshiDataMessage),
    OrderbookDelta(KalshiDataMessage),
    Ticker(KalshiDataMessage),
    Trade(KalshiDataMessage),
    Fill(KalshiDataMessage),
    MarketPosition(KalshiDataMessage),
    MarketLifecycleV2(KalshiDataMessage),
    EventLifecycle(KalshiDataMessage),
    EventFeeUpdate(KalshiDataMessage),
    UserOrder(KalshiDataMessage),
    MultivariateMarketLifecycle(KalshiDataMessage),
    OrderGroupUpdates(KalshiDataMessage),
    CfbenchmarksValue(KalshiDataMessage),
    CfbenchmarksValueIndexlist(KalshiDataMessage),
    #[serde(rename = "cfbenchmarks_value_5hz")]
    CfbenchmarksValue5hz(KalshiDataMessage),
    #[serde(rename = "cfbenchmarks_value_5hz_indexlist")]
    CfbenchmarksValue5hzIndexlist(KalshiDataMessage),
    PythValue(KalshiDataMessage),
    PythValueUnderlyingList(KalshiDataMessage),
    RfqCreated(KalshiDataMessage),
    RfqDeleted(KalshiDataMessage),
    QuoteCreated(KalshiDataMessage),
    QuoteAccepted(KalshiDataMessage),
    QuoteExecuted(KalshiDataMessage),
    RfqBatchCreated(KalshiDataMessage),
    RfqBatchDeleted(KalshiDataMessage),
    BatchQuoteCreated(KalshiDataMessage),
    RfqBatchBestQuotes(KalshiDataMessage),
    BatchQuoteAccepted(KalshiDataMessage),
    RfqBatchExecuted(KalshiDataMessage),
    BatchQuoteExecuted(KalshiDataMessage),
    #[serde(other)]
    Unknown,
}

/// A confirmed channel and its server subscription ID.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KalshiSubscribed {
    pub channel: KalshiChannel,
    pub sid: u64,
}

/// The authoritative market filter after a subscription update.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KalshiUpdated {
    pub market_tickers: Vec<String>,
}

/// A protocol error; codes 10 and 25 terminate the affected channel.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KalshiServerError {
    pub code: u32,
    pub msg: String,
}

/// A channel payload with the identity needed for downstream sequence validation.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct KalshiDataMessage {
    pub id: Option<u64>,
    pub sid: u64,
    pub seq: Option<u64>,
    pub sending_ts_ms: Option<u64>,
    pub msg: Value,
}

impl KalshiWsMessage {
    /// Returns the subscription channel and payload of a data frame.
    ///
    /// Order-book snapshots and deltas both report [`KalshiChannel::OrderbookDelta`]. Market and
    /// event lifecycle frames report [`KalshiChannel::MarketLifecycleV2`]; multivariate lifecycle
    /// frames report [`KalshiChannel::MultivariateMarketLifecycle`].
    #[must_use]
    pub fn data(&self) -> Option<(KalshiChannel, &KalshiDataMessage)> {
        match self {
            Self::OrderbookSnapshot(data) | Self::OrderbookDelta(data) => {
                Some((KalshiChannel::OrderbookDelta, data))
            }
            Self::Ticker(data) => Some((KalshiChannel::Ticker, data)),
            Self::Trade(data) => Some((KalshiChannel::Trade, data)),
            Self::Fill(data) => Some((KalshiChannel::Fill, data)),
            Self::MarketPosition(data) => Some((KalshiChannel::MarketPositions, data)),
            Self::MarketLifecycleV2(data)
            | Self::EventLifecycle(data)
            | Self::EventFeeUpdate(data) => Some((KalshiChannel::MarketLifecycleV2, data)),
            Self::UserOrder(data) => Some((KalshiChannel::UserOrders, data)),
            Self::MultivariateMarketLifecycle(data) => {
                Some((KalshiChannel::MultivariateMarketLifecycle, data))
            }
            Self::OrderGroupUpdates(data) => Some((KalshiChannel::OrderGroupUpdates, data)),
            Self::CfbenchmarksValue(data) | Self::CfbenchmarksValueIndexlist(data) => {
                Some((KalshiChannel::CfbenchmarksValue, data))
            }
            Self::CfbenchmarksValue5hz(data) | Self::CfbenchmarksValue5hzIndexlist(data) => {
                Some((KalshiChannel::CfbenchmarksValue5hz, data))
            }
            Self::PythValue(data) | Self::PythValueUnderlyingList(data) => {
                Some((KalshiChannel::PythValue, data))
            }
            Self::RfqCreated(data)
            | Self::RfqDeleted(data)
            | Self::QuoteCreated(data)
            | Self::QuoteAccepted(data)
            | Self::QuoteExecuted(data)
            | Self::RfqBatchCreated(data)
            | Self::RfqBatchDeleted(data)
            | Self::BatchQuoteCreated(data)
            | Self::RfqBatchBestQuotes(data)
            | Self::BatchQuoteAccepted(data)
            | Self::RfqBatchExecuted(data)
            | Self::BatchQuoteExecuted(data) => Some((KalshiChannel::Communications, data)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(KalshiChannel::MultivariateMarketLifecycle)]
    #[case(KalshiChannel::Communications)]
    #[case(KalshiChannel::OrderGroupUpdates)]
    #[case(KalshiChannel::CfbenchmarksValue)]
    #[case(KalshiChannel::CfbenchmarksValue5hz)]
    #[case(KalshiChannel::PythValue)]
    fn documented_channels_require_subscription_support(#[case] channel: KalshiChannel) {
        let subscription = KalshiSubscription {
            channel,
            market_tickers: Vec::new(),
        };

        assert_eq!(
            subscription.normalize().unwrap_err().to_string(),
            "channel subscriptions are not supported"
        );
    }
}
