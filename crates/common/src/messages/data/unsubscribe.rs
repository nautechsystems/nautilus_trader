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

use std::num::NonZeroUsize;

use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::{BarType, DataType},
    identifiers::{ClientId, InstrumentId, OptionSeriesId, Venue},
};
use serde::{Deserialize, Serialize};

use super::{
    IntoBarTypes, IntoInstrumentIds, check_client_id_or_venue,
    identifiers::{
        deserialize_bar_types, deserialize_instrument_ids, single_bar_type, single_instrument_id,
    },
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct UnsubscribeCustomData {
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub data_type: DataType,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeCustomData {
    /// Creates a new [`UnsubscribeCustomData`] instance.
    pub fn new(
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        data_type: DataType,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            client_id,
            venue,
            data_type,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct UnsubscribeInstrument {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeInstrument {
    /// Creates a new [`UnsubscribeInstrument`] instance.
    ///
    /// Duplicate IDs are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - The identifier list is empty.
    /// - Both client and venue are absent.
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_ids,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }

    /// Creates a command for the supplied IDs with the same routing and identity.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[must_use]
    pub fn with_instrument_ids(&self, instrument_ids: impl IntoInstrumentIds) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");

        Self {
            instrument_ids,
            client_id: self.client_id,
            venue: self.venue,
            command_id: self.command_id,
            ts_init: self.ts_init,
            correlation_id: self.correlation_id,
            params: self.params.clone(),
        }
    }

    /// Returns the instrument ID for a single-instrument adapter path.
    ///
    /// # Errors
    ///
    /// Returns an error if the command does not contain exactly one instrument.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "subscription")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct UnsubscribeInstruments {
    pub client_id: Option<ClientId>,
    pub venue: Venue,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeInstruments {
    /// Creates a new [`UnsubscribeInstruments`] instance.
    pub fn new(
        client_id: Option<ClientId>,
        venue: Venue,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        Self {
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct UnsubscribeBookDeltas {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeBookDeltas {
    /// Creates a new [`UnsubscribeBookDeltas`] instance.
    ///
    /// Duplicate IDs are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - The identifier list is empty.
    /// - Both client and venue are absent.
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_ids,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }

    /// Creates a command for the supplied IDs with the same routing and identity.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[must_use]
    pub fn with_instrument_ids(&self, instrument_ids: impl IntoInstrumentIds) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");

        Self {
            instrument_ids,
            client_id: self.client_id,
            venue: self.venue,
            command_id: self.command_id,
            ts_init: self.ts_init,
            correlation_id: self.correlation_id,
            params: self.params.clone(),
        }
    }

    /// Returns the instrument ID for a single-instrument adapter path.
    ///
    /// # Errors
    ///
    /// Returns an error if the command does not contain exactly one instrument.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "subscription")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct UnsubscribeBookDepth {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeBookDepth {
    /// Creates a new [`UnsubscribeBookDepth`] instance.
    ///
    /// Duplicate IDs are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - The identifier list is empty.
    /// - Both client and venue are absent.
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_ids,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }

    /// Creates a command for the supplied IDs with the same routing and identity.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[must_use]
    pub fn with_instrument_ids(&self, instrument_ids: impl IntoInstrumentIds) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");

        Self {
            instrument_ids,
            client_id: self.client_id,
            venue: self.venue,
            command_id: self.command_id,
            ts_init: self.ts_init,
            correlation_id: self.correlation_id,
            params: self.params.clone(),
        }
    }

    /// Returns the instrument ID for a single-instrument adapter path.
    ///
    /// # Errors
    ///
    /// Returns an error if the command does not contain exactly one instrument.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "subscription")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnsubscribeBookSnapshots {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub interval_ms: NonZeroUsize,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeBookSnapshots {
    /// Creates a new [`UnsubscribeBookSnapshots`] instance.
    ///
    /// Duplicate IDs are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - The identifier list is empty.
    /// - Both client and venue are absent.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        interval_ms: NonZeroUsize,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_ids,
            interval_ms,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }

    /// Creates a command for the supplied IDs with the same routing and identity.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[must_use]
    pub fn with_instrument_ids(&self, instrument_ids: impl IntoInstrumentIds) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");

        Self {
            instrument_ids,
            interval_ms: self.interval_ms,
            client_id: self.client_id,
            venue: self.venue,
            command_id: self.command_id,
            ts_init: self.ts_init,
            correlation_id: self.correlation_id,
            params: self.params.clone(),
        }
    }

    /// Returns the instrument ID for a single-instrument adapter path.
    ///
    /// # Errors
    ///
    /// Returns an error if the command does not contain exactly one instrument.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "subscription")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct UnsubscribeQuotes {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeQuotes {
    /// Creates a new [`UnsubscribeQuotes`] instance.
    ///
    /// Duplicate IDs are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - The identifier list is empty.
    /// - Both client and venue are absent.
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_ids,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }

    /// Creates a command for the supplied IDs with the same routing and identity.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[must_use]
    pub fn with_instrument_ids(&self, instrument_ids: impl IntoInstrumentIds) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");

        Self {
            instrument_ids,
            client_id: self.client_id,
            venue: self.venue,
            command_id: self.command_id,
            ts_init: self.ts_init,
            correlation_id: self.correlation_id,
            params: self.params.clone(),
        }
    }

    /// Returns the instrument ID for a single-instrument adapter path.
    ///
    /// # Errors
    ///
    /// Returns an error if the command does not contain exactly one instrument.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "subscription")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct UnsubscribeTrades {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeTrades {
    /// Creates a new [`UnsubscribeTrades`] instance.
    ///
    /// Duplicate IDs are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - The identifier list is empty.
    /// - Both client and venue are absent.
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_ids,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }

    /// Creates a command for the supplied IDs with the same routing and identity.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[must_use]
    pub fn with_instrument_ids(&self, instrument_ids: impl IntoInstrumentIds) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");

        Self {
            instrument_ids,
            client_id: self.client_id,
            venue: self.venue,
            command_id: self.command_id,
            ts_init: self.ts_init,
            correlation_id: self.correlation_id,
            params: self.params.clone(),
        }
    }

    /// Returns the instrument ID for a single-instrument adapter path.
    ///
    /// # Errors
    ///
    /// Returns an error if the command does not contain exactly one instrument.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "subscription")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct UnsubscribeBars {
    #[serde(deserialize_with = "deserialize_bar_types")]
    pub bar_types: Vec<BarType>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeBars {
    /// Creates a new [`UnsubscribeBars`] instance.
    ///
    /// Duplicate bar types are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - The bar type list is empty.
    /// - Both client and venue are absent.
    pub fn new(
        bar_types: impl IntoBarTypes,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        let bar_types = bar_types.into_bar_types().expect("invalid bar type list");
        check_client_id_or_venue(&client_id, &venue);
        Self {
            bar_types,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }

    /// Creates a command for the supplied bar types with the same routing and identity.
    ///
    /// # Panics
    ///
    /// Panics if the bar type list is empty.
    #[must_use]
    pub fn with_bar_types(&self, bar_types: impl IntoBarTypes) -> Self {
        let bar_types = bar_types.into_bar_types().expect("invalid bar type list");

        Self {
            bar_types,
            client_id: self.client_id,
            venue: self.venue,
            command_id: self.command_id,
            ts_init: self.ts_init,
            correlation_id: self.correlation_id,
            params: self.params.clone(),
        }
    }

    /// Returns the bar type for a single-bar-type adapter path.
    ///
    /// # Errors
    ///
    /// Returns an error if the command does not contain exactly one bar type.
    pub fn single_bar_type(&self) -> anyhow::Result<BarType> {
        single_bar_type(&self.bar_types, "subscription")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct UnsubscribeMarkPrices {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeMarkPrices {
    /// Creates a new [`UnsubscribeMarkPrices`] instance.
    ///
    /// Duplicate IDs are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - The identifier list is empty.
    /// - Both client and venue are absent.
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_ids,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }

    /// Creates a command for the supplied IDs with the same routing and identity.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[must_use]
    pub fn with_instrument_ids(&self, instrument_ids: impl IntoInstrumentIds) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");

        Self {
            instrument_ids,
            client_id: self.client_id,
            venue: self.venue,
            command_id: self.command_id,
            ts_init: self.ts_init,
            correlation_id: self.correlation_id,
            params: self.params.clone(),
        }
    }

    /// Returns the instrument ID for a single-instrument adapter path.
    ///
    /// # Errors
    ///
    /// Returns an error if the command does not contain exactly one instrument.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "subscription")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct UnsubscribeIndexPrices {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeIndexPrices {
    /// Creates a new [`UnsubscribeIndexPrices`] instance.
    ///
    /// Duplicate IDs are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - The identifier list is empty.
    /// - Both client and venue are absent.
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_ids,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }

    /// Creates a command for the supplied IDs with the same routing and identity.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[must_use]
    pub fn with_instrument_ids(&self, instrument_ids: impl IntoInstrumentIds) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");

        Self {
            instrument_ids,
            client_id: self.client_id,
            venue: self.venue,
            command_id: self.command_id,
            ts_init: self.ts_init,
            correlation_id: self.correlation_id,
            params: self.params.clone(),
        }
    }

    /// Returns the instrument ID for a single-instrument adapter path.
    ///
    /// # Errors
    ///
    /// Returns an error if the command does not contain exactly one instrument.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "subscription")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct UnsubscribeFundingRates {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeFundingRates {
    /// Creates a new [`UnsubscribeFundingRates`] instance.
    ///
    /// Duplicate IDs are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - The identifier list is empty.
    /// - Both client and venue are absent.
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_ids,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }

    /// Creates a command for the supplied IDs with the same routing and identity.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[must_use]
    pub fn with_instrument_ids(&self, instrument_ids: impl IntoInstrumentIds) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");

        Self {
            instrument_ids,
            client_id: self.client_id,
            venue: self.venue,
            command_id: self.command_id,
            ts_init: self.ts_init,
            correlation_id: self.correlation_id,
            params: self.params.clone(),
        }
    }

    /// Returns the instrument ID for a single-instrument adapter path.
    ///
    /// # Errors
    ///
    /// Returns an error if the command does not contain exactly one instrument.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "subscription")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct UnsubscribeInstrumentStatus {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeInstrumentStatus {
    /// Creates a new [`UnsubscribeInstrumentStatus`] instance.
    ///
    /// Duplicate IDs are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - The identifier list is empty.
    /// - Both client and venue are absent.
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_ids,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }

    /// Creates a command for the supplied IDs with the same routing and identity.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[must_use]
    pub fn with_instrument_ids(&self, instrument_ids: impl IntoInstrumentIds) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");

        Self {
            instrument_ids,
            client_id: self.client_id,
            venue: self.venue,
            command_id: self.command_id,
            ts_init: self.ts_init,
            correlation_id: self.correlation_id,
            params: self.params.clone(),
        }
    }

    /// Returns the instrument ID for a single-instrument adapter path.
    ///
    /// # Errors
    ///
    /// Returns an error if the command does not contain exactly one instrument.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "subscription")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct UnsubscribeOptionGreeks {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeOptionGreeks {
    /// Creates a new [`UnsubscribeOptionGreeks`] instance.
    ///
    /// Duplicate IDs are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - The identifier list is empty.
    /// - Both client and venue are absent.
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_ids,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }

    /// Creates a command for the supplied IDs with the same routing and identity.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[must_use]
    pub fn with_instrument_ids(&self, instrument_ids: impl IntoInstrumentIds) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");

        Self {
            instrument_ids,
            client_id: self.client_id,
            venue: self.venue,
            command_id: self.command_id,
            ts_init: self.ts_init,
            correlation_id: self.correlation_id,
            params: self.params.clone(),
        }
    }

    /// Returns the instrument ID for a single-instrument adapter path.
    ///
    /// # Errors
    ///
    /// Returns an error if the command does not contain exactly one instrument.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "subscription")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", frozen)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct UnsubscribeInstrumentClose {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeInstrumentClose {
    /// Creates a new [`UnsubscribeInstrumentClose`] instance.
    ///
    /// Duplicate IDs are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - The identifier list is empty.
    /// - Both client and venue are absent.
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_ids,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }

    /// Creates a command for the supplied IDs with the same routing and identity.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[must_use]
    pub fn with_instrument_ids(&self, instrument_ids: impl IntoInstrumentIds) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");

        Self {
            instrument_ids,
            client_id: self.client_id,
            venue: self.venue,
            command_id: self.command_id,
            ts_init: self.ts_init,
            correlation_id: self.correlation_id,
            params: self.params.clone(),
        }
    }

    /// Returns the instrument ID for a single-instrument adapter path.
    ///
    /// # Errors
    ///
    /// Returns an error if the command does not contain exactly one instrument.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "subscription")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnsubscribeOptionChain {
    pub series_id: OptionSeriesId,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    #[serde(default)]
    pub params: Option<Params>,
}

impl UnsubscribeOptionChain {
    /// Creates a new [`UnsubscribeOptionChain`] instance.
    pub fn new(
        series_id: OptionSeriesId,
        command_id: UUID4,
        ts_init: UnixNanos,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            series_id,
            command_id,
            ts_init,
            client_id,
            venue,
            params: None,
        }
    }
}
