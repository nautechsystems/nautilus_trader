use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::{BarType, DataType},
    identifiers::{ClientId, InstrumentId, Venue},
};

use super::check_client_id_or_venue;

#[derive(Clone, Debug)]
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
    #[allow(clippy::too_many_arguments)]
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

#[derive(Clone, Debug)]
pub struct UnsubscribeInstrument {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeInstrument {
    /// Creates a new [`UnsubscribeInstrument`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }
}

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
pub struct UnsubscribeBookDeltas {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeBookDeltas {
    /// Creates a new [`UnsubscribeBookDeltas`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UnsubscribeBookDepth10 {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeBookDepth10 {
    /// Creates a new [`UnsubscribeBookDepth10`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UnsubscribeBookSnapshots {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeBookSnapshots {
    /// Creates a new [`UnsubscribeBookSnapshots`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UnsubscribeQuotes {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeQuotes {
    /// Creates a new [`UnsubscribeQuotes`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UnsubscribeTrades {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeTrades {
    /// Creates a new [`UnsubscribeTrades`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UnsubscribeBars {
    pub bar_type: BarType,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeBars {
    /// Creates a new [`UnsubscribeBars`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bar_type: BarType,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            bar_type,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UnsubscribeMarkPrices {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeMarkPrices {
    /// Creates a new [`UnsubscribeMarkPrices`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UnsubscribeIndexPrices {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeIndexPrices {
    /// Creates a new [`UnsubscribeIndexPrices`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UnsubscribeFundingRates {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeFundingRates {
    /// Creates a new [`UnsubscribeFundingRates`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UnsubscribeInstrumentStatus {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeInstrumentStatus {
    /// Creates a new [`UnsubscribeInstrumentStatus`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UnsubscribeInstrumentClose {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
    pub correlation_id: Option<UUID4>,
    pub params: Option<Params>,
}

impl UnsubscribeInstrumentClose {
    /// Creates a new [`UnsubscribeInstrumentClose`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            instrument_id,
            client_id,
            venue,
            command_id,
            ts_init,
            correlation_id,
            params,
        }
    }
}
