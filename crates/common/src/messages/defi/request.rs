use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::identifiers::{ClientId, InstrumentId};

/// Represents a request for a pool snapshot from a specific AMM pool.
#[derive(Clone, Debug)]
pub struct RequestPoolSnapshot {
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestPoolSnapshot {
    /// Creates a new [`RequestPoolSnapshot`] instance.
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            instrument_id,
            client_id,
            request_id,
            ts_init,
            params,
        }
    }
}
