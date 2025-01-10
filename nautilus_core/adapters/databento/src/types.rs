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

use std::ffi::c_char;

use databento::dbn;
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::OrderSide,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use serde::Deserialize;
use ustr::Ustr;

use super::enums::{DatabentoStatisticType, DatabentoStatisticUpdateAction};

/// Represents a Databento publisher ID.
pub type PublisherId = u16;

/// Represents a Databento dataset code.
pub type Dataset = Ustr;

/// Represents a Databento publisher.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.databento")
)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
pub struct DatabentoPublisher {
    /// The publisher ID assigned by Databento, which denotes the dataset and venue.
    pub publisher_id: PublisherId,
    /// The Databento dataset code for the publisher.
    pub dataset: dbn::Dataset,
    /// The venue for the publisher.
    pub venue: dbn::Venue,
    /// The publisher description.
    pub description: String,
}

/// Represents an auction imbalance.
///
/// This data type includes the populated data fields provided by `Databento`,
/// excluding `publisher_id` and `instrument_id`.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.databento")
)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
pub struct DatabentoImbalance {
    // The instrument ID for the imbalance data.
    pub instrument_id: InstrumentId,
    // The reference price at which the imbalance shares are calculated.
    pub ref_price: Price,
    // The hypothetical auction-clearing price for both cross and continuous orders.
    pub cont_book_clr_price: Price,
    // The hypothetical auction-clearing price for cross orders only.
    pub auct_interest_clr_price: Price,
    // The quantity of shares which are eligible to be matched at `ref_price`.
    pub paired_qty: Quantity,
    // The quantity of shares which are not paired at `ref_price`.
    pub total_imbalance_qty: Quantity,
    // The market side of the `total_imbalance_qty` (can be `NO_ORDER_SIDE`).
    pub side: OrderSide,
    // A venue-specific character code. For Nasdaq, contains the raw Price Variation Indicator.
    pub significant_imbalance: c_char,
    // UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    // UNIX timestamp (nanoseconds) when the data object was received by Databento.
    pub ts_recv: UnixNanos,
    // UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

impl DatabentoImbalance {
    /// Creates a new [`DatabentoImbalance`] instance.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        instrument_id: InstrumentId,
        ref_price: Price,
        cont_book_clr_price: Price,
        auct_interest_clr_price: Price,
        paired_qty: Quantity,
        total_imbalance_qty: Quantity,
        side: OrderSide,
        significant_imbalance: c_char,
        ts_event: UnixNanos,
        ts_recv: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            instrument_id,
            ref_price,
            cont_book_clr_price,
            auct_interest_clr_price,
            paired_qty,
            total_imbalance_qty,
            side,
            significant_imbalance,
            ts_event,
            ts_recv,
            ts_init,
        })
    }
}

/// Represents a market statistics snapshot.
///
/// This data type includes the populated data fields provided by `Databento`,
/// excluding `publisher_id` and `instrument_id`.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.databento")
)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
pub struct DatabentoStatistics {
    // The instrument ID for the statistics message.
    pub instrument_id: InstrumentId,
    // The type of statistic value contained in the message.
    pub stat_type: DatabentoStatisticType,
    // Indicates if the statistic is newly added (1) or deleted (2). (Deleted is only used with some stat_types).
    pub update_action: DatabentoStatisticUpdateAction,
    // The statistics price.
    pub price: Option<Price>,
    // The value for non-price statistics.
    pub quantity: Option<Quantity>,
    // The channel ID within the venue.
    pub channel_id: u16,
    // Additional flags associated with certain stat types.
    pub stat_flags: u8,
    // The message sequence number assigned at the venue.
    pub sequence: u32,
    // UNIX timestamp (nanoseconds) Databento `ts_ref` reference timestamp).
    pub ts_ref: UnixNanos,
    // The matching-engine-sending timestamp expressed as the number of nanoseconds before the Databento `ts_recv`.
    pub ts_in_delta: i32,
    // UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    // UNIX timestamp (nanoseconds) when the data object was received by Databento.
    pub ts_recv: UnixNanos,
    // UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

impl DatabentoStatistics {
    /// Creates a new [`DatabentoStatistics`] instance.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        instrument_id: InstrumentId,
        stat_type: DatabentoStatisticType,
        update_action: DatabentoStatisticUpdateAction,
        price: Option<Price>,
        quantity: Option<Quantity>,
        channel_id: u16,
        stat_flags: u8,
        sequence: u32,
        ts_ref: UnixNanos,
        ts_in_delta: i32,
        ts_event: UnixNanos,
        ts_recv: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            instrument_id,
            stat_type,
            update_action,
            price,
            quantity,
            channel_id,
            stat_flags,
            sequence,
            ts_ref,
            ts_in_delta,
            ts_event,
            ts_recv,
            ts_init,
        })
    }
}
