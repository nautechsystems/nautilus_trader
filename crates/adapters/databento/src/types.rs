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

use std::{collections::HashMap, ffi::c_char, sync::Arc};

use databento::dbn;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{HasTsInit, custom::CustomDataTrait},
    enums::OrderSide,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::enums::{DatabentoStatisticType, DatabentoStatisticUpdateAction};

/// Subscription acknowledgement event from the Databento gateway.
#[derive(Debug, Clone)]
pub struct SubscriptionAckEvent {
    /// The schema that was acknowledged.
    pub schema: String,
    /// The raw message from the gateway.
    pub message: String,
    /// Timestamp when the ack was received.
    pub ts_received: UnixNanos,
}

/// Represents a Databento publisher ID.
pub type PublisherId = u16;

/// Represents a Databento dataset ID.
pub type Dataset = Ustr;

/// Represents a Databento publisher.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.databento",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.databento")
)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
pub struct DatabentoPublisher {
    /// The publisher ID assigned by Databento, which denotes the dataset and venue.
    pub publisher_id: PublisherId,
    /// The Databento dataset ID for the publisher.
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
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.databento",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.databento")
)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// Returns the metadata for the type, for use with serialization formats.
    #[must_use]
    pub fn get_metadata(
        instrument_id: &InstrumentId,
        price_precision: u8,
        size_precision: u8,
    ) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert("instrument_id".to_string(), instrument_id.to_string());
        metadata.insert("price_precision".to_string(), price_precision.to_string());
        metadata.insert("size_precision".to_string(), size_precision.to_string());
        metadata
    }

    /// Creates a new [`DatabentoImbalance`] instance.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
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
    ) -> Self {
        Self {
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
        }
    }
}

impl HasTsInit for DatabentoImbalance {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for DatabentoImbalance {
    fn type_name(&self) -> &'static str {
        "DatabentoImbalance"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(self.clone())
    }

    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        if let Some(o) = other.as_any().downcast_ref::<Self>() {
            self == o
        } else {
            false
        }
    }

    #[cfg(feature = "python")]
    fn to_pyobject(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
        nautilus_model::data::custom::clone_pyclass_to_pyobject(self, py)
    }

    fn type_name_static() -> &'static str {
        "DatabentoImbalance"
    }

    fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
        let parsed: Self = serde_json::from_value(value)?;
        Ok(Arc::new(parsed))
    }
}

/// Represents a market statistics snapshot.
///
/// This data type includes the populated data fields provided by `Databento`,
/// excluding `publisher_id` and `instrument_id`.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.databento",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.databento")
)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// Returns the metadata for the type, for use with serialization formats.
    #[must_use]
    pub fn get_metadata(
        instrument_id: &InstrumentId,
        price_precision: u8,
        size_precision: u8,
    ) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert("instrument_id".to_string(), instrument_id.to_string());
        metadata.insert("price_precision".to_string(), price_precision.to_string());
        metadata.insert("size_precision".to_string(), size_precision.to_string());
        metadata
    }

    /// Creates a new [`DatabentoStatistics`] instance.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
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
    ) -> Self {
        Self {
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
        }
    }
}

impl HasTsInit for DatabentoStatistics {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for DatabentoStatistics {
    fn type_name(&self) -> &'static str {
        "DatabentoStatistics"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(self.clone())
    }

    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        if let Some(o) = other.as_any().downcast_ref::<Self>() {
            self == o
        } else {
            false
        }
    }

    #[cfg(feature = "python")]
    fn to_pyobject(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
        nautilus_model::data::custom::clone_pyclass_to_pyobject(self, py)
    }

    fn type_name_static() -> &'static str {
        "DatabentoStatistics"
    }

    fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
        let parsed: Self = serde_json::from_value(value)?;
        Ok(Arc::new(parsed))
    }
}
