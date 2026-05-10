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

//! Error types and classification for the Interactive Brokers adapter.

use thiserror::Error;

/// Errors that can occur in the Interactive Brokers adapter.
#[derive(Error, Debug)]
pub enum InteractiveBrokersError {
    /// Connection error.
    #[error("Connection error: {0}")]
    Connection(String),

    /// Authentication error.
    #[error("Authentication error: {0}")]
    Authentication(String),

    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    Configuration(String),

    /// API request error.
    #[error("API request error: {0}")]
    Request(String),

    /// Response parsing error.
    #[error("Response parsing error: {0}")]
    Parse(String),

    /// Instrument error.
    #[error("Instrument error: {0}")]
    Instrument(String),

    /// Order error.
    #[error("Order error: {0}")]
    Order(String),

    /// Market data error.
    #[error("Market data error: {0}")]
    MarketData(String),

    /// Generic error from rust-ibapi.
    #[error("IB API error: {0}")]
    IbApi(String),

    /// Internal error.
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Result type for Interactive Brokers operations.
pub type InteractiveBrokersResult<T> = Result<T, InteractiveBrokersError>;

/// IB API error code classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Client/application error (should not retry).
    ClientError,
    /// Connectivity error (should retry with backoff).
    ConnectivityError,
    /// Subscription error (may need resubscription).
    SubscriptionError,
    /// Order error (may need special handling).
    OrderError,
    /// Market data error (may need resubscription).
    MarketDataError,
    /// Unknown/unclassified error.
    Unknown,
}

/// Classify an IB error code into a category.
///
/// # Arguments
///
/// * `error_code` - The IB API error code
///
/// # Returns
///
/// Returns the error category for the given code.
pub fn classify_error_code(error_code: i32) -> ErrorCategory {
    match error_code {
        // Client errors - should not retry
        200..=299 => ErrorCategory::ClientError,

        // Connectivity errors - should retry
        326 | 502 | 503 | 504 | 1100 | 1101 | 1102 | 1300 | 1301 | 1302 => {
            ErrorCategory::ConnectivityError
        }

        // Subscription errors - may need resubscription
        10189 | 366 | 102 | 10182 => ErrorCategory::SubscriptionError,

        // Market data errors
        100..=199 if error_code != 10182 => ErrorCategory::MarketDataError,

        // Note: Order errors overlap with client errors range
        // We handle order errors separately in the match

        // Unknown
        _ => ErrorCategory::Unknown,
    }
}

/// Determine if an error is recoverable.
///
/// # Arguments
///
/// * `error_code` - The IB API error code
///
/// # Returns
///
/// Returns `true` if the error is recoverable (should retry).
pub fn is_recoverable_error(error_code: i32) -> bool {
    matches!(
        classify_error_code(error_code),
        ErrorCategory::ConnectivityError | ErrorCategory::SubscriptionError
    )
}

/// Determine if an error requires subscription resubscription.
///
/// # Arguments
///
/// * `error_code` - The IB API error code
///
/// # Returns
///
/// Returns `true` if subscriptions should be resubscribed.
pub fn requires_resubscription(error_code: i32) -> bool {
    matches!(error_code, 10189 | 366 | 102 | 10182)
}

/// Get a human-readable error description.
///
/// # Arguments
///
/// * `error_code` - The IB API error code
/// * `error_string` - The error message from IB
///
/// # Returns
///
/// Returns a formatted error description.
pub fn format_error_message(error_code: i32, error_string: &str) -> String {
    let category = classify_error_code(error_code);
    let category_str = match category {
        ErrorCategory::ClientError => "Client Error",
        ErrorCategory::ConnectivityError => "Connectivity Error",
        ErrorCategory::SubscriptionError => "Subscription Error",
        ErrorCategory::OrderError => "Order Error",
        ErrorCategory::MarketDataError => "Market Data Error",
        ErrorCategory::Unknown => "Unknown Error",
    };

    format!(
        "[{}] {} (Code: {}): {}",
        category_str, error_string, error_code, error_string
    )
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{ErrorCategory, classify_error_code, is_recoverable_error};

    #[rstest]
    #[case(326, ErrorCategory::ConnectivityError)]
    #[case(502, ErrorCategory::ConnectivityError)]
    #[case(10182, ErrorCategory::SubscriptionError)]
    #[case(200, ErrorCategory::ClientError)]
    fn test_classify_error_code(#[case] error_code: i32, #[case] expected: ErrorCategory) {
        assert_eq!(classify_error_code(error_code), expected);
    }

    #[rstest]
    #[case(326, true)]
    #[case(10182, true)]
    #[case(200, false)]
    fn test_is_recoverable_error(#[case] error_code: i32, #[case] expected: bool) {
        assert_eq!(is_recoverable_error(error_code), expected);
    }
}
