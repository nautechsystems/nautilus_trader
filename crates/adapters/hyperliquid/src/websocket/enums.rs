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

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

/// WebSocket channel names for Hyperliquid.
#[derive(
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
pub enum HyperliquidWsChannel {
    #[serde(rename = "subscriptionResponse")]
    SubscriptionResponse,
    #[serde(rename = "trades")]
    Trades,
    #[serde(rename = "l2Book")]
    L2Book,
    #[serde(rename = "bbo")]
    Bbo,
    #[serde(rename = "orderUpdates")]
    OrderUpdates,
    #[serde(rename = "userEvents")]
    UserEvents,
    #[serde(rename = "userFills")]
    UserFills,
    #[serde(rename = "userFundings")]
    UserFundings,
    #[serde(rename = "userNonFundingLedgerUpdates")]
    UserNonFundingLedgerUpdates,
    #[serde(rename = "post")]
    Post,
    #[serde(rename = "pong")]
    Pong,
}

impl HyperliquidWsChannel {
    /// Returns the string representation of the channel.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SubscriptionResponse => "subscriptionResponse",
            Self::Trades => "trades",
            Self::L2Book => "l2Book",
            Self::Bbo => "bbo",
            Self::OrderUpdates => "orderUpdates",
            Self::UserEvents => "userEvents",
            Self::UserFills => "userFills",
            Self::UserFundings => "userFundings",
            Self::UserNonFundingLedgerUpdates => "userNonFundingLedgerUpdates",
            Self::Post => "post",
            Self::Pong => "pong",
        }
    }

    /// Returns true if this is a public channel (does not require authentication).
    pub fn is_public(&self) -> bool {
        matches!(
            self,
            Self::SubscriptionResponse | Self::Trades | Self::L2Book | Self::Bbo | Self::Pong
        )
    }

    /// Returns true if this is a private channel (requires authentication).
    pub fn is_private(&self) -> bool {
        !self.is_public()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json;

    use super::*;

    #[rstest]
    #[case(HyperliquidWsChannel::Trades, r#""trades""#)]
    #[case(HyperliquidWsChannel::L2Book, r#""l2Book""#)]
    #[case(HyperliquidWsChannel::UserFills, r#""userFills""#)]
    #[case(HyperliquidWsChannel::Bbo, r#""bbo""#)]
    #[case(
        HyperliquidWsChannel::SubscriptionResponse,
        r#""subscriptionResponse""#
    )]
    fn test_channel_serialization(#[case] channel: HyperliquidWsChannel, #[case] expected: &str) {
        assert_eq!(serde_json::to_string(&channel).unwrap(), expected);
    }

    #[rstest]
    #[case(r#""trades""#, HyperliquidWsChannel::Trades)]
    #[case(r#""l2Book""#, HyperliquidWsChannel::L2Book)]
    #[case(r#""userEvents""#, HyperliquidWsChannel::UserEvents)]
    #[case(r#""bbo""#, HyperliquidWsChannel::Bbo)]
    #[case(r#""pong""#, HyperliquidWsChannel::Pong)]
    fn test_channel_deserialization(#[case] json: &str, #[case] expected: HyperliquidWsChannel) {
        assert_eq!(
            serde_json::from_str::<HyperliquidWsChannel>(json).unwrap(),
            expected
        );
    }

    #[rstest]
    #[case(HyperliquidWsChannel::Trades, "trades")]
    #[case(HyperliquidWsChannel::L2Book, "l2Book")]
    #[case(HyperliquidWsChannel::UserFills, "userFills")]
    #[case(
        HyperliquidWsChannel::UserNonFundingLedgerUpdates,
        "userNonFundingLedgerUpdates"
    )]
    #[case(HyperliquidWsChannel::Bbo, "bbo")]
    fn test_as_str_method(#[case] channel: HyperliquidWsChannel, #[case] expected: &str) {
        assert_eq!(channel.as_str(), expected);
    }

    #[rstest]
    fn test_display_trait() {
        assert_eq!(format!("{}", HyperliquidWsChannel::Trades), "Trades");
        assert_eq!(format!("{}", HyperliquidWsChannel::L2Book), "L2Book");
        assert_eq!(format!("{}", HyperliquidWsChannel::UserFills), "UserFills");
    }

    #[rstest]
    fn test_is_public_channel() {
        assert!(HyperliquidWsChannel::Trades.is_public());
        assert!(HyperliquidWsChannel::L2Book.is_public());
        assert!(HyperliquidWsChannel::Bbo.is_public());
        assert!(HyperliquidWsChannel::SubscriptionResponse.is_public());
        assert!(HyperliquidWsChannel::Pong.is_public());

        assert!(!HyperliquidWsChannel::OrderUpdates.is_public());
        assert!(!HyperliquidWsChannel::UserEvents.is_public());
        assert!(!HyperliquidWsChannel::UserFills.is_public());
        assert!(!HyperliquidWsChannel::UserFundings.is_public());
        assert!(!HyperliquidWsChannel::UserNonFundingLedgerUpdates.is_public());
        assert!(!HyperliquidWsChannel::Post.is_public());
    }

    #[rstest]
    fn test_is_private_channel() {
        assert!(!HyperliquidWsChannel::Trades.is_private());
        assert!(!HyperliquidWsChannel::L2Book.is_private());
        assert!(!HyperliquidWsChannel::Bbo.is_private());

        assert!(HyperliquidWsChannel::OrderUpdates.is_private());
        assert!(HyperliquidWsChannel::UserEvents.is_private());
        assert!(HyperliquidWsChannel::UserFills.is_private());
        assert!(HyperliquidWsChannel::UserFundings.is_private());
        assert!(HyperliquidWsChannel::UserNonFundingLedgerUpdates.is_private());
        assert!(HyperliquidWsChannel::Post.is_private());
    }

    #[rstest]
    fn test_enum_iter() {
        use strum::IntoEnumIterator;

        let channels: Vec<HyperliquidWsChannel> = HyperliquidWsChannel::iter().collect();
        assert_eq!(channels.len(), 11);
        assert!(channels.contains(&HyperliquidWsChannel::Trades));
        assert!(channels.contains(&HyperliquidWsChannel::L2Book));
        assert!(channels.contains(&HyperliquidWsChannel::UserFills));
    }

    #[rstest]
    fn test_from_str() {
        use std::str::FromStr;

        assert_eq!(
            HyperliquidWsChannel::from_str("Trades").unwrap(),
            HyperliquidWsChannel::Trades
        );
        assert_eq!(
            HyperliquidWsChannel::from_str("L2Book").unwrap(),
            HyperliquidWsChannel::L2Book
        );
        assert_eq!(
            HyperliquidWsChannel::from_str("UserFills").unwrap(),
            HyperliquidWsChannel::UserFills
        );

        assert!(HyperliquidWsChannel::from_str("InvalidChannel").is_err());
    }
}
