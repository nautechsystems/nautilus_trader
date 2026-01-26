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

//! Shared types for dYdX v4 execution module.
//!
//! This module centralizes type definitions used across order submission,
//! transaction management, and WebSocket handling components.

use chrono::{DateTime, Duration, Utc};
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
    types::{Price, Quantity},
};

use crate::error::DydxError;

/// Default expiration for GTC conditional orders (90 days).
pub const GTC_CONDITIONAL_ORDER_EXPIRATION_DAYS: i64 = 90;

/// Order flag for short-term orders (expire by block height).
pub const ORDER_FLAG_SHORT_TERM: u32 = 0;

/// Order flag for conditional orders (stop-loss, take-profit).
pub const ORDER_FLAG_CONDITIONAL: u32 = 32;

/// Order flag for long-term/stateful orders (expire by timestamp).
pub const ORDER_FLAG_LONG_TERM: u32 = 64;

/// Order lifetime type determined by time_in_force and expire_time.
///
/// dYdX v4 has different execution paths for orders based on their expected lifetime:
/// - **ShortTerm**: Lower latency/fees, expire by block height (max 20 blocks ~30s)
/// - **LongTerm**: Stored on-chain, expire by timestamp, explicit cancel events
/// - **Conditional**: Triggered by price conditions, always stored on-chain
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderLifetime {
    /// Short-term orders expire by block height (max 20 blocks).
    /// Lower latency and fees, but expire silently without cancel events.
    /// Used for IOC, FOK, or orders expiring within 60 seconds.
    ShortTerm,
    /// Long-term orders expire by timestamp.
    /// Stored on-chain with explicit cancel events when they expire or are cancelled.
    /// Used for GTC, GTD orders with expiry > 60 seconds.
    LongTerm,
    /// Conditional orders triggered by price conditions.
    /// Always stored on-chain (stateful), used for stop-loss and take-profit orders.
    Conditional,
}

impl OrderLifetime {
    /// Determines order lifetime based on time_in_force, expire_time, and max short-term duration.
    ///
    /// The `max_short_term_secs` is computed dynamically from `BlockTimeMonitor`:
    /// `max_short_term_secs = SHORT_TERM_ORDER_MAXIMUM_LIFETIME (20 blocks) × seconds_per_block`
    ///
    /// Returns `ShortTerm` when:
    /// - TimeInForce is IOC or FOK (immediate execution orders)
    /// - expire_time is set and within `max_short_term_secs` from now
    ///
    /// Returns `LongTerm` for GTC/GTD orders with expiry beyond short-term window.
    /// Returns `Conditional` for stop/take-profit orders (when `is_conditional` is true).
    #[must_use]
    pub fn from_time_in_force(
        time_in_force: TimeInForce,
        expire_time: Option<i64>,
        is_conditional: bool,
        max_short_term_secs: f64,
    ) -> Self {
        if is_conditional {
            return Self::Conditional;
        }

        // IOC and FOK are always short-term (immediate execution)
        if matches!(time_in_force, TimeInForce::Ioc | TimeInForce::Fok) {
            return Self::ShortTerm;
        }

        // Check if expire_time is within the short-term window
        if let Some(expire_ts) = expire_time {
            let now = Utc::now().timestamp();
            let time_until_expiry = expire_ts - now;
            if time_until_expiry > 0 && (time_until_expiry as f64) <= max_short_term_secs {
                return Self::ShortTerm;
            }
        }

        Self::LongTerm
    }

    /// Returns the dYdX order_flags value for this lifetime.
    ///
    /// These flags are used in both `MsgPlaceOrder` and `MsgCancelOrder` to identify
    /// the order type on-chain.
    #[must_use]
    pub const fn order_flags(&self) -> u32 {
        match self {
            Self::ShortTerm => ORDER_FLAG_SHORT_TERM,
            Self::LongTerm => ORDER_FLAG_LONG_TERM,
            Self::Conditional => ORDER_FLAG_CONDITIONAL,
        }
    }

    /// Returns true if this is a short-term order.
    #[must_use]
    pub const fn is_short_term(&self) -> bool {
        matches!(self, Self::ShortTerm)
    }

    /// Returns true if this is a conditional order (stop/take-profit).
    #[must_use]
    pub const fn is_conditional(&self) -> bool {
        matches!(self, Self::Conditional)
    }
}

/// Conditional order types supported by dYdX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionalOrderType {
    /// Triggers at trigger price, executes as market order.
    StopMarket,
    /// Triggers at trigger price, places limit order at limit price.
    StopLimit,
    /// Triggers at trigger price for profit taking, executes as market order.
    TakeProfitMarket,
    /// Triggers at trigger price for profit taking, places limit order at limit price.
    TakeProfitLimit,
}

/// Parameters for a limit order in batch submission.
#[derive(Debug, Clone)]
pub struct LimitOrderParams {
    /// Instrument to trade.
    pub instrument_id: InstrumentId,
    /// Client-assigned order ID (u32 for dYdX protocol).
    pub client_order_id: u32,
    /// Order side (Buy or Sell).
    pub side: OrderSide,
    /// Limit price.
    pub price: Price,
    /// Order quantity.
    pub quantity: Quantity,
    /// Time in force.
    pub time_in_force: TimeInForce,
    /// Whether this is a post-only order.
    pub post_only: bool,
    /// Whether this is a reduce-only order.
    pub reduce_only: bool,
    /// Optional expiration timestamp (nanoseconds since epoch).
    /// The builder will convert this to seconds and apply default_short_term_expiry if configured.
    pub expire_time_ns: Option<UnixNanos>,
}

/// Contains the raw bytes and metadata for retry handling.
#[derive(Debug, Clone)]
pub struct PreparedTransaction {
    /// Serialized transaction bytes.
    pub tx_bytes: Vec<u8>,
    /// Sequence number used for this transaction.
    pub sequence: u64,
    /// Human-readable operation name for logging.
    pub operation: String,
}

/// Order context passed from submission to WebSocket confirmation handler.
///
/// This context is registered before transaction submission and used by the
/// WebSocket handler to correlate incoming order updates with the original
/// submission request, similar to Deribit's `order_contexts` pattern.
#[derive(Debug, Clone)]
pub struct OrderContext {
    /// Nautilus client order ID.
    pub client_order_id: ClientOrderId,
    /// Trader ID from the order.
    pub trader_id: TraderId,
    /// Strategy ID that submitted the order.
    pub strategy_id: StrategyId,
    /// Instrument being traded.
    pub instrument_id: InstrumentId,
    /// Timestamp when the order was submitted.
    pub submitted_at: UnixNanos,
    /// dYdX order flags (0=short-term, 32=conditional, 64=long-term).
    /// Stored at submission time to ensure cancellation uses correct flags.
    pub order_flags: u32,
}

/// Calculates the expiration time for conditional orders based on TimeInForce.
///
/// - `GTD` with explicit `expire_time`: uses the provided timestamp.
/// - `GTC` or no `expire_time`: defaults to 90 days from now.
/// - `IOC`/`FOK`: uses 1 hour (these are unusual for conditional orders).
///
/// # Errors
///
/// Returns `DydxError::Parse` if the provided `expire_time` timestamp is invalid.
pub fn calculate_conditional_order_expiration(
    time_in_force: TimeInForce,
    expire_time: Option<i64>,
) -> Result<DateTime<Utc>, DydxError> {
    if let Some(expire_ts) = expire_time {
        DateTime::from_timestamp(expire_ts, 0)
            .ok_or_else(|| DydxError::Parse(format!("Invalid expire timestamp: {expire_ts}")))
    } else {
        let expiration = match time_in_force {
            TimeInForce::Gtc => Utc::now() + Duration::days(GTC_CONDITIONAL_ORDER_EXPIRATION_DAYS),
            TimeInForce::Ioc | TimeInForce::Fok => {
                // IOC/FOK don't typically apply to conditional orders, use short expiration
                Utc::now() + Duration::hours(1)
            }
            // GTD without expire_time, or any other TIF - use long default
            _ => Utc::now() + Duration::days(GTC_CONDITIONAL_ORDER_EXPIRATION_DAYS),
        };
        Ok(expiration)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    // Default max short-term seconds for testing (20 blocks × 3 sec/block = 60 sec)
    const TEST_MAX_SHORT_TERM_SECS: f64 = 60.0;

    #[rstest]
    fn test_order_lifetime_ioc_is_short_term() {
        let lifetime = OrderLifetime::from_time_in_force(
            TimeInForce::Ioc,
            None,
            false,
            TEST_MAX_SHORT_TERM_SECS,
        );
        assert_eq!(lifetime, OrderLifetime::ShortTerm);
        assert!(lifetime.is_short_term());
    }

    #[rstest]
    fn test_order_lifetime_fok_is_short_term() {
        let lifetime = OrderLifetime::from_time_in_force(
            TimeInForce::Fok,
            None,
            false,
            TEST_MAX_SHORT_TERM_SECS,
        );
        assert_eq!(lifetime, OrderLifetime::ShortTerm);
    }

    #[rstest]
    fn test_order_lifetime_gtc_is_long_term() {
        let lifetime = OrderLifetime::from_time_in_force(
            TimeInForce::Gtc,
            None,
            false,
            TEST_MAX_SHORT_TERM_SECS,
        );
        assert_eq!(lifetime, OrderLifetime::LongTerm);
        assert!(!lifetime.is_short_term());
    }

    #[rstest]
    fn test_order_lifetime_conditional_flag() {
        let lifetime = OrderLifetime::from_time_in_force(
            TimeInForce::Gtc,
            None,
            true,
            TEST_MAX_SHORT_TERM_SECS,
        );
        assert_eq!(lifetime, OrderLifetime::Conditional);
        assert!(lifetime.is_conditional());
    }

    #[rstest]
    fn test_order_lifetime_short_expire_time() {
        // Expire time 30 seconds from now should be short-term (within 60s max)
        let expire_time = Some(Utc::now().timestamp() + 30);
        let lifetime = OrderLifetime::from_time_in_force(
            TimeInForce::Gtc,
            expire_time,
            false,
            TEST_MAX_SHORT_TERM_SECS,
        );
        assert_eq!(lifetime, OrderLifetime::ShortTerm);
    }

    #[rstest]
    fn test_order_lifetime_long_expire_time() {
        // Expire time 5 minutes from now should be long-term (beyond 60s max)
        let expire_time = Some(Utc::now().timestamp() + 300);
        let lifetime = OrderLifetime::from_time_in_force(
            TimeInForce::Gtd,
            expire_time,
            false,
            TEST_MAX_SHORT_TERM_SECS,
        );
        assert_eq!(lifetime, OrderLifetime::LongTerm);
    }

    #[rstest]
    fn test_order_lifetime_expire_at_boundary() {
        // Expire time exactly at max_short_term_secs should be short-term
        let expire_time = Some(Utc::now().timestamp() + 60);
        let lifetime = OrderLifetime::from_time_in_force(
            TimeInForce::Gtd,
            expire_time,
            false,
            TEST_MAX_SHORT_TERM_SECS,
        );
        assert_eq!(lifetime, OrderLifetime::ShortTerm);
    }

    #[rstest]
    fn test_order_lifetime_expire_just_beyond_boundary() {
        // Expire time 1 second beyond max should be long-term
        let expire_time = Some(Utc::now().timestamp() + 61);
        let lifetime = OrderLifetime::from_time_in_force(
            TimeInForce::Gtd,
            expire_time,
            false,
            TEST_MAX_SHORT_TERM_SECS,
        );
        assert_eq!(lifetime, OrderLifetime::LongTerm);
    }

    #[rstest]
    fn test_order_flags() {
        assert_eq!(
            OrderLifetime::ShortTerm.order_flags(),
            ORDER_FLAG_SHORT_TERM
        );
        assert_eq!(OrderLifetime::LongTerm.order_flags(), ORDER_FLAG_LONG_TERM);
        assert_eq!(
            OrderLifetime::Conditional.order_flags(),
            ORDER_FLAG_CONDITIONAL
        );
    }
}
