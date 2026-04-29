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
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    orders::OrderAny,
    types::{Price, Quantity},
};

use crate::{
    common::{consts::CANCEL_ALREADY_DONE, enums::PolymarketOrderType},
    http::models::PolymarketOrder,
};

/// Classifies cancel rejection reasons to eliminate duplicate if/else blocks.
pub(crate) enum CancelOutcome {
    AlreadyDone,
    Rejected(String),
}

impl CancelOutcome {
    pub fn classify(reason: &str) -> Self {
        if reason.contains(CANCEL_ALREADY_DONE) {
            Self::AlreadyDone
        } else {
            Self::Rejected(reason.to_string())
        }
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
}

#[derive(Clone, Debug)]
pub(crate) struct BatchLimitOrderContext {
    pub(crate) order: OrderAny,
    pub(crate) request: LimitOrderSubmitRequest,
    pub(crate) size_precision: u8,
    pub(crate) price_precision: u8,
}
