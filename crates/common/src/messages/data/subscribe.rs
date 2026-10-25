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
    data::{BarType, DataType, option_chain::StrikeRange},
    enums::BookType,
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
pub struct SubscribeCustomData {
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub data_type: DataType,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeCustomData {
    /// Creates a new [`SubscribeCustomData`] instance.
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
pub struct SubscribeInstrument {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeInstrument {
    /// Creates a new [`SubscribeInstrument`] instance.
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
pub struct SubscribeInstruments {
    pub client_id: Option<ClientId>,
    pub venue: Venue,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeInstruments {
    /// Creates a new [`SubscribeInstruments`] instance.
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
pub struct SubscribeBookDeltas {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub book_type: BookType,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub depth: Option<NonZeroUsize>,
    pub managed: bool,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeBookDeltas {
    /// Creates a new [`SubscribeBookDeltas`] instance.
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
        book_type: BookType,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        depth: Option<NonZeroUsize>,
        managed: bool,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_ids,
            book_type,
            client_id,
            venue,
            command_id,
            ts_init,
            depth,
            managed,
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
            book_type: self.book_type,
            client_id: self.client_id,
            venue: self.venue,
            command_id: self.command_id,
            ts_init: self.ts_init,
            depth: self.depth,
            managed: self.managed,
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
pub struct SubscribeBookDepth {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub book_type: BookType,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub depth: Option<NonZeroUsize>,
    pub managed: bool,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeBookDepth {
    /// Creates a new [`SubscribeBookDepth`] instance.
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
        book_type: BookType,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        depth: Option<NonZeroUsize>,
        managed: bool,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_ids,
            book_type,
            client_id,
            venue,
            command_id,
            ts_init,
            depth,
            managed,
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
            book_type: self.book_type,
            client_id: self.client_id,
            venue: self.venue,
            command_id: self.command_id,
            ts_init: self.ts_init,
            depth: self.depth,
            managed: self.managed,
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
pub struct SubscribeBookSnapshots {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub book_type: BookType,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub depth: Option<NonZeroUsize>,
    pub interval_ms: NonZeroUsize,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeBookSnapshots {
    /// Creates a new [`SubscribeBookSnapshots`] instance.
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
        book_type: BookType,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        depth: Option<NonZeroUsize>,
        interval_ms: NonZeroUsize,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument identifier list");
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_ids,
            book_type,
            client_id,
            venue,
            command_id,
            ts_init,
            depth,
            interval_ms,
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
            book_type: self.book_type,
            client_id: self.client_id,
            venue: self.venue,
            command_id: self.command_id,
            ts_init: self.ts_init,
            depth: self.depth,
            interval_ms: self.interval_ms,
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
pub struct SubscribeQuotes {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeQuotes {
    /// Creates a new [`SubscribeQuotes`] instance.
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

/// Reports synchronous acceptance during a failed subscribe or unsubscribe operation.
///
/// `instrument_ids_accepted` and `bar_types_accepted` identify accepted members of the dispatched
/// command. Bar operations use full bar types. Unsubscribe acceptance means successful release;
/// venue acknowledgements arrive asynchronously.
#[derive(Debug, thiserror::Error)]
#[error("{source}")]
#[non_exhaustive]
pub struct SubscriptionError {
    pub instrument_ids_accepted: Vec<InstrumentId>,
    pub bar_types_accepted: Vec<BarType>,
    #[source]
    pub source: anyhow::Error,
}

impl SubscriptionError {
    /// Creates an error with the exact accepted instrument members.
    #[must_use]
    pub fn instruments(instrument_ids_accepted: Vec<InstrumentId>, source: anyhow::Error) -> Self {
        Self {
            instrument_ids_accepted,
            bar_types_accepted: Vec::new(),
            source,
        }
    }

    /// Creates an error with the exact accepted full bar types.
    #[must_use]
    pub fn bars(bar_types_accepted: Vec<BarType>, source: anyhow::Error) -> Self {
        Self {
            instrument_ids_accepted: Vec::new(),
            bar_types_accepted,
            source,
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
pub struct SubscribeTrades {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeTrades {
    /// Creates a new [`SubscribeTrades`] instance.
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
pub struct SubscribeBars {
    #[serde(deserialize_with = "deserialize_bar_types")]
    pub bar_types: Vec<BarType>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeBars {
    /// Creates a new [`SubscribeBars`] instance.
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
pub struct SubscribeMarkPrices {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeMarkPrices {
    /// Creates a new [`SubscribeMarkPrices`] instance.
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
pub struct SubscribeIndexPrices {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeIndexPrices {
    /// Creates a new [`SubscribeIndexPrices`] instance.
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
pub struct SubscribeFundingRates {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeFundingRates {
    /// Creates a new [`SubscribeFundingRates`] instance.
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
pub struct SubscribeInstrumentStatus {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeInstrumentStatus {
    /// Creates a new [`SubscribeInstrumentStatus`] instance.
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
pub struct SubscribeOptionGreeks {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeOptionGreeks {
    /// Creates a new [`SubscribeOptionGreeks`] instance.
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
pub struct SubscribeInstrumentClose {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeInstrumentClose {
    /// Creates a new [`SubscribeInstrumentClose`] instance.
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
pub struct SubscribeOptionChain {
    pub series_id: OptionSeriesId,
    pub strike_range: StrikeRange,
    pub snapshot_interval_ms: Option<u64>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    #[serde(default)]
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl SubscribeOptionChain {
    /// Creates a new [`SubscribeOptionChain`] instance.
    #[expect(
        clippy::too_many_arguments,
        reason = "constructor exposes the option chain subscription fields"
    )]
    pub fn new(
        series_id: OptionSeriesId,
        strike_range: StrikeRange,
        snapshot_interval_ms: Option<u64>,
        command_id: UUID4,
        ts_init: UnixNanos,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            series_id,
            strike_range,
            snapshot_interval_ms,
            command_id,
            ts_init,
            client_id,
            venue,
            correlation_id: None,
            params,
        }
    }
}
