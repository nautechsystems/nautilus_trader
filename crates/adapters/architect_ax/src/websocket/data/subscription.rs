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

use ustr::Ustr;

use crate::common::enums::AxMarketDataLevel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxMdSubscriptionSpec {
    pub(crate) level: AxMarketDataLevel,
    pub(crate) trades: Option<bool>,
    pub(crate) ticker: Option<bool>,
}

impl AxMdSubscriptionSpec {
    pub(crate) const fn new(
        level: AxMarketDataLevel,
        trades: Option<bool>,
        ticker: Option<bool>,
    ) -> Self {
        Self {
            level,
            trades,
            ticker,
        }
    }

    pub(crate) fn topic(self, symbol: &str) -> String {
        format!(
            "{symbol}:{:?}:{}:{}",
            self.level,
            Self::encode_bool(self.trades),
            Self::encode_bool(self.ticker)
        )
    }

    pub(crate) fn parse_topic(topic: &str) -> Option<(Ustr, Self)> {
        let mut parts = topic.rsplitn(4, ':');
        let ticker = Self::decode_bool(parts.next()?).ok()?;
        let trades = Self::decode_bool(parts.next()?).ok()?;
        let level = Self::parse_level(parts.next()?)?;
        let symbol = Ustr::from(parts.next()?);

        Some((
            symbol,
            Self {
                level,
                trades,
                ticker,
            },
        ))
    }

    fn encode_bool(value: Option<bool>) -> &'static str {
        match value {
            None => "default",
            Some(true) => "true",
            Some(false) => "false",
        }
    }

    fn decode_bool(value: &str) -> Result<Option<bool>, ()> {
        match value {
            "default" => Ok(None),
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            _ => Err(()),
        }
    }

    fn parse_level(value: &str) -> Option<AxMarketDataLevel> {
        match value {
            "Level1" => Some(AxMarketDataLevel::Level1),
            "Level2" => Some(AxMarketDataLevel::Level2),
            "Level3" => Some(AxMarketDataLevel::Level3),
            "Trades" => Some(AxMarketDataLevel::Trades),
            _ => None,
        }
    }
}

/// Orders subscription changes that run as separate tasks.
///
/// Each change waits until every earlier change completes, so the WebSocket client applies them in
/// command order. A change completes when its [`SubscriptionTurn`] drops, including when its task is
/// cancelled, so a lost change never blocks later ones.
#[derive(Debug, Default)]
pub(crate) struct SubscriptionOrder {
    last: Option<tokio::sync::oneshot::Receiver<()>>,
}

impl SubscriptionOrder {
    /// Returns the turn of the next change in command order.
    pub(crate) fn next(&mut self) -> SubscriptionTurn {
        let (done, last) = tokio::sync::oneshot::channel();

        SubscriptionTurn {
            previous: self.last.replace(last),
            _done: done,
        }
    }
}

/// One subscription change's place in command order; dropping it lets the next change run.
#[derive(Debug)]
pub(crate) struct SubscriptionTurn {
    previous: Option<tokio::sync::oneshot::Receiver<()>>,
    _done: tokio::sync::oneshot::Sender<()>,
}

impl SubscriptionTurn {
    /// Waits until every earlier change has completed.
    pub(crate) async fn wait(&mut self) {
        if let Some(previous) = self.previous.as_mut() {
            // The sender only drops, so an error marks the earlier change complete
            let _ = previous.await;
            self.previous = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_topic_encodes_full_spec() {
        let spec = AxMdSubscriptionSpec::new(AxMarketDataLevel::Level2, Some(false), Some(true));

        assert_eq!(spec.topic("EURUSD-PERP"), "EURUSD-PERP:Level2:false:true");
    }

    #[rstest]
    fn test_parse_topic_new_format() {
        let (symbol, spec) =
            AxMdSubscriptionSpec::parse_topic("EURUSD-PERP:Level1:false:default").unwrap();

        assert_eq!(symbol, Ustr::from("EURUSD-PERP"));
        assert_eq!(
            spec,
            AxMdSubscriptionSpec::new(AxMarketDataLevel::Level1, Some(false), None)
        );
    }

    #[rstest]
    fn test_parse_topic_rejects_invalid_flags() {
        assert!(AxMdSubscriptionSpec::parse_topic("EURUSD-PERP:Level1:false:nope").is_none());
    }

    #[rstest]
    #[tokio::test]
    async fn test_subscription_turn_waits_for_earlier_change() {
        let mut order = SubscriptionOrder::default();
        let mut first = order.next();
        let mut second = order.next();
        first.wait().await;

        let while_first_runs = tokio::time::timeout(Duration::from_millis(50), second.wait()).await;
        drop(first);
        let after_first = tokio::time::timeout(Duration::from_secs(1), second.wait()).await;

        assert!(while_first_runs.is_err());
        assert!(after_first.is_ok());
    }

    #[rstest]
    #[tokio::test]
    async fn test_subscription_turn_skips_cancelled_change() {
        let mut order = SubscriptionOrder::default();
        let first = order.next();
        let second = order.next();
        let mut third = order.next();

        // The earlier changes end without running, as an aborted task drops its turn
        drop(first);
        drop(second);
        let waited = tokio::time::timeout(Duration::from_secs(1), third.wait()).await;

        assert!(waited.is_ok());
    }
}
