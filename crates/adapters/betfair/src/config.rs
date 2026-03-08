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

//! Configuration structures for the Betfair adapter.

/// Configuration for the Betfair live data client.
#[derive(Clone, Debug)]
pub struct BetfairDataConfig {
    /// Stream conflation setting in milliseconds. When set, Betfair batches
    /// stream updates for this interval. `None` uses Betfair defaults.
    pub stream_conflate_ms: Option<u64>,
    /// Delay in seconds before sending the initial subscription message
    /// after connecting to the stream (default: 3).
    pub subscription_delay_secs: Option<u64>,
}

impl Default for BetfairDataConfig {
    fn default() -> Self {
        Self {
            stream_conflate_ms: None,
            subscription_delay_secs: Some(3),
        }
    }
}

/// Configuration for the Betfair live execution client.
#[derive(Clone, Debug)]
pub struct BetfairExecConfig {
    /// Market IDs to filter on the order stream. When set, OCM updates for
    /// markets not in this list are skipped. `None` processes all markets.
    pub stream_market_ids_filter: Option<Vec<String>>,
    /// When true, silently ignore orders from OCM that are not tracked
    /// in the local cache (default: false). Useful for multi-node setups.
    pub ignore_external_orders: bool,
    /// Whether to poll account state periodically (default: true).
    pub calculate_account_state: bool,
    /// Interval in seconds between account state polls (default: 300).
    /// Set to 0 to disable polling. Only applies when
    /// `calculate_account_state` is true.
    pub request_account_state_secs: u64,
}

impl Default for BetfairExecConfig {
    fn default() -> Self {
        Self {
            stream_market_ids_filter: None,
            ignore_external_orders: false,
            calculate_account_state: true,
            request_account_state_secs: 300,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_data_config_default() {
        let config = BetfairDataConfig::default();

        assert!(config.stream_conflate_ms.is_none());
        assert_eq!(config.subscription_delay_secs, Some(3));
    }

    #[rstest]
    fn test_exec_config_default() {
        let config = BetfairExecConfig::default();

        assert!(config.stream_market_ids_filter.is_none());
        assert!(!config.ignore_external_orders);
        assert!(config.calculate_account_state);
        assert_eq!(config.request_account_state_secs, 300);
    }

    #[rstest]
    fn test_exec_config_with_market_filter() {
        let config = BetfairExecConfig {
            stream_market_ids_filter: Some(vec!["1.234567".to_string(), "1.890123".to_string()]),
            ..Default::default()
        };

        let filter = config.stream_market_ids_filter.as_ref().unwrap();
        assert_eq!(filter.len(), 2);
        assert!(filter.contains(&"1.234567".to_string()));
    }

    #[rstest]
    fn test_exec_config_external_orders_ignored() {
        let config = BetfairExecConfig {
            ignore_external_orders: true,
            ..Default::default()
        };

        assert!(config.ignore_external_orders);
    }

    #[rstest]
    fn test_exec_config_account_state_disabled() {
        let config = BetfairExecConfig {
            calculate_account_state: false,
            ..Default::default()
        };

        assert!(!config.calculate_account_state);
    }
}
