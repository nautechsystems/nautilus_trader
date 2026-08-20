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

//! Shared types for the Polymarket execution module.

use nautilus_core::UnixNanos;
use nautilus_live::execution::failure::CommandFailure;
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    identifiers::VenueOrderId,
    orders::OrderAny,
    types::{Price, Quantity},
};

use crate::{
    common::{consts::CANCEL_ALREADY_DONE, enums::PolymarketOrderType},
    http::{error::Error, models::PolymarketOrder},
};

/// Classifies cancel rejection reasons to eliminate duplicate if/else blocks.
pub(crate) enum CancelOutcome {
    AlreadyDone,
    Rejected(String),
}

impl CancelOutcome {
    pub(crate) fn classify(reason: &str) -> Self {
        if reason.contains(CANCEL_ALREADY_DONE) {
            Self::AlreadyDone
        } else {
            Self::Rejected(reason.to_string())
        }
    }
}

/// Classifies a state-changing order command HTTP failure at the execution boundary.
pub(crate) fn classify_http_command_failure(error: &Error) -> CommandFailure {
    if error.is_submit_outcome_unknown() {
        CommandFailure::ambiguous(error.strategy_reason())
    } else if matches!(error, Error::BurstExceeded { .. } | Error::UrlParse(_)) {
        CommandFailure::not_sent(error.strategy_reason())
    } else {
        CommandFailure::venue_rejected(error.strategy_reason())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LimitOrderSubmitRequest {
    pub(crate) token_id: String,
    pub(crate) side: OrderSide,
    pub(crate) price: Price,
    pub(crate) quantity: Quantity,
    pub(crate) time_in_force: TimeInForce,
    pub(crate) post_only: bool,
    pub(crate) neg_risk: bool,
    pub(crate) expire_time: Option<UnixNanos>,
    pub(crate) tick_decimals: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct SignedLimitOrderSubmission {
    pub(crate) order: PolymarketOrder,
    pub(crate) order_type: PolymarketOrderType,
    pub(crate) post_only: bool,
    pub(crate) expected_venue_order_id: VenueOrderId,
}

#[derive(Clone, Debug)]
pub(crate) struct BatchLimitOrderContext {
    pub(crate) order: OrderAny,
    pub(crate) request: LimitOrderSubmitRequest,
    pub(crate) size_precision: u8,
    pub(crate) price_precision: u8,
}

#[cfg(test)]
mod tests {
    use nautilus_live::execution::failure::CommandFailure;
    use rstest::rstest;

    use super::classify_http_command_failure;
    use crate::http::error::Error;

    #[rstest]
    fn test_classify_425_and_headerless_429_as_ambiguous() {
        assert_eq!(
            classify_http_command_failure(&Error::http(425, "too early")),
            CommandFailure::ambiguous("too early")
        );
        assert_eq!(
            classify_http_command_failure(&Error::rate_limit("/orders", 10, Some(1_000))),
            CommandFailure::ambiguous("rate limit exceeded")
        );
        assert_eq!(
            classify_http_command_failure(&Error::Timeout),
            CommandFailure::ambiguous("timeout")
        );
    }

    #[rstest]
    fn test_classify_definitive_venue_errors_as_rejected() {
        assert_eq!(
            classify_http_command_failure(&Error::bad_request("invalid signature")),
            CommandFailure::venue_rejected("bad request: invalid signature")
        );
        assert_eq!(
            classify_http_command_failure(&Error::rate_limit_response(
                "/orders",
                1,
                None,
                "max rate exceeded",
                true,
            )),
            CommandFailure::venue_rejected("max rate exceeded")
        );
    }

    #[rstest]
    fn test_classify_burst_exceeded_is_not_sent() {
        let error = Error::BurstExceeded {
            endpoint: "/orders",
            token_cost: 100,
            tier: "legacy".to_string(),
            bucket: "order".to_string(),
            burst: 10,
        };

        assert!(matches!(
            classify_http_command_failure(&error),
            CommandFailure::NotSent(_)
        ));
    }
}
