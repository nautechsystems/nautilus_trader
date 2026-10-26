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

use jiff::Timestamp;
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
pub struct RequestCustomData {
    pub client_id: ClientId,
    pub data_type: DataType,
    pub start: Option<Timestamp>,
    pub end: Option<Timestamp>,
    pub limit: Option<NonZeroUsize>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestCustomData {
    /// Creates a new [`RequestCustomData`] instance.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        client_id: ClientId,
        data_type: DataType,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        limit: Option<NonZeroUsize>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            client_id,
            data_type,
            start,
            end,
            limit,
            request_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestInstrument {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub start: Option<Timestamp>,
    pub end: Option<Timestamp>,
    pub client_id: Option<ClientId>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestInstrument {
    /// Creates a new [`RequestInstrument`] instance.
    ///
    /// Duplicate entries are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        client_id: Option<ClientId>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument ID list");

        Self {
            instrument_ids,
            start,
            end,
            client_id,
            request_id,
            ts_init,
            params,
        }
    }

    /// Returns the instrument ID for a single-member request path.
    ///
    /// # Errors
    ///
    /// Returns an error if the request does not contain exactly one member.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "request")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestInstruments {
    pub start: Option<Timestamp>,
    pub end: Option<Timestamp>,
    pub client_id: Option<ClientId>,
    pub venue: Option<Venue>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestInstruments {
    /// Creates a new [`RequestInstruments`] instance.
    pub fn new(
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        client_id: Option<ClientId>,
        venue: Option<Venue>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        check_client_id_or_venue(&client_id, &venue);
        Self {
            start,
            end,
            client_id,
            venue,
            request_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestBookSnapshot {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub depth: Option<NonZeroUsize>,
    pub client_id: Option<ClientId>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestBookSnapshot {
    /// Creates a new [`RequestBookSnapshot`] instance.
    ///
    /// Duplicate entries are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        depth: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument ID list");

        Self {
            instrument_ids,
            depth,
            client_id,
            request_id,
            ts_init,
            params,
        }
    }

    /// Returns the instrument ID for a single-member request path.
    ///
    /// # Errors
    ///
    /// Returns an error if the request does not contain exactly one member.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "request")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestQuotes {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub start: Option<Timestamp>,
    pub end: Option<Timestamp>,
    pub limit: Option<NonZeroUsize>,
    pub client_id: Option<ClientId>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestQuotes {
    /// Creates a new [`RequestQuotes`] instance.
    ///
    /// Duplicate entries are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        limit: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument ID list");

        Self {
            instrument_ids,
            start,
            end,
            limit,
            client_id,
            request_id,
            ts_init,
            params,
        }
    }

    /// Returns the instrument ID for a single-member request path.
    ///
    /// # Errors
    ///
    /// Returns an error if the request does not contain exactly one member.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "request")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestTrades {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub start: Option<Timestamp>,
    pub end: Option<Timestamp>,
    pub limit: Option<NonZeroUsize>,
    pub client_id: Option<ClientId>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestTrades {
    /// Creates a new [`RequestTrades`] instance.
    ///
    /// Duplicate entries are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        limit: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument ID list");

        Self {
            instrument_ids,
            start,
            end,
            limit,
            client_id,
            request_id,
            ts_init,
            params,
        }
    }

    /// Returns the instrument ID for a single-member request path.
    ///
    /// # Errors
    ///
    /// Returns an error if the request does not contain exactly one member.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "request")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestFundingRates {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub start: Option<Timestamp>,
    pub end: Option<Timestamp>,
    pub limit: Option<NonZeroUsize>,
    pub client_id: Option<ClientId>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestFundingRates {
    /// Creates a new [`RequestFundingRates`] instance.
    ///
    /// Duplicate entries are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        limit: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument ID list");

        Self {
            instrument_ids,
            start,
            end,
            limit,
            client_id,
            request_id,
            ts_init,
            params,
        }
    }

    /// Returns the instrument ID for a single-member request path.
    ///
    /// # Errors
    ///
    /// Returns an error if the request does not contain exactly one member.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "request")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestOptionChainReferencePrice {
    pub series_id: OptionSeriesId,
    pub instrument_id: InstrumentId,
    pub client_id: Option<ClientId>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestOptionChainReferencePrice {
    /// Creates a new [`RequestOptionChainReferencePrice`] instance.
    pub fn new(
        series_id: OptionSeriesId,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        Self {
            series_id,
            instrument_id,
            client_id,
            request_id,
            ts_init,
            params,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestBookDepth {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub start: Option<Timestamp>,
    pub end: Option<Timestamp>,
    pub limit: Option<NonZeroUsize>,
    pub depth: Option<NonZeroUsize>,
    pub client_id: Option<ClientId>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestBookDepth {
    /// Creates a new [`RequestBookDepth`] instance.
    ///
    /// Duplicate entries are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        limit: Option<NonZeroUsize>,
        depth: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument ID list");

        Self {
            instrument_ids,
            start,
            end,
            limit,
            depth,
            client_id,
            request_id,
            ts_init,
            params,
        }
    }

    /// Returns the instrument ID for a single-member request path.
    ///
    /// # Errors
    ///
    /// Returns an error if the request does not contain exactly one member.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "request")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestBookDeltas {
    #[serde(deserialize_with = "deserialize_instrument_ids")]
    pub instrument_ids: Vec<InstrumentId>,
    pub start: Option<Timestamp>,
    pub end: Option<Timestamp>,
    pub limit: Option<NonZeroUsize>,
    pub client_id: Option<ClientId>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestBookDeltas {
    /// Creates a new [`RequestBookDeltas`] instance.
    ///
    /// Duplicate entries are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        instrument_ids: impl IntoInstrumentIds,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        limit: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        let instrument_ids = instrument_ids
            .into_instrument_ids()
            .expect("invalid instrument ID list");

        Self {
            instrument_ids,
            start,
            end,
            limit,
            client_id,
            request_id,
            ts_init,
            params,
        }
    }

    /// Returns the instrument ID for a single-member request path.
    ///
    /// # Errors
    ///
    /// Returns an error if the request does not contain exactly one member.
    pub fn single_instrument_id(&self) -> anyhow::Result<InstrumentId> {
        single_instrument_id(&self.instrument_ids, "request")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestBars {
    #[serde(deserialize_with = "deserialize_bar_types")]
    pub bar_types: Vec<BarType>,
    pub start: Option<Timestamp>,
    pub end: Option<Timestamp>,
    pub limit: Option<NonZeroUsize>,
    pub client_id: Option<ClientId>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
}

impl RequestBars {
    /// Creates a new [`RequestBars`] instance.
    ///
    /// Duplicate entries are removed in first-occurrence order.
    ///
    /// # Panics
    ///
    /// Panics if the identifier list is empty.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        bar_types: impl IntoBarTypes,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        limit: Option<NonZeroUsize>,
        client_id: Option<ClientId>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
    ) -> Self {
        let bar_types = bar_types.into_bar_types().expect("invalid bar type list");

        Self {
            bar_types,
            start,
            end,
            limit,
            client_id,
            request_id,
            ts_init,
            params,
        }
    }

    /// Returns the bar type for a single-member request path.
    ///
    /// # Errors
    ///
    /// Returns an error if the request does not contain exactly one member.
    pub fn single_bar_type(&self) -> anyhow::Result<BarType> {
        single_bar_type(&self.bar_types, "request")
    }
}

/// A request to join multiple in-flight data requests under a single parent response.
///
/// The engine first issues a combined date-range request to bound the join window,
/// then fans out the leg responses through the request-pipeline machinery so the
/// caller receives one consolidated `DataResponse` keyed by the join `request_id`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestJoin {
    pub request_ids: Vec<UUID4>,
    pub start: Option<Timestamp>,
    pub end: Option<Timestamp>,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<Params>,
    pub correlation_id: Option<UUID4>,
}

impl RequestJoin {
    /// Creates a new [`RequestJoin`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `request_ids` is empty.
    pub fn new(
        request_ids: Vec<UUID4>,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<Params>,
        correlation_id: Option<UUID4>,
    ) -> Self {
        assert!(!request_ids.is_empty(), "request_ids must not be empty");
        Self {
            request_ids,
            start,
            end,
            request_id,
            ts_init,
            params,
            correlation_id,
        }
    }

    /// Returns a fresh [`RequestJoin`] for the combined date-range bootstrap leg.
    ///
    /// The returned request inherits `request_ids` and `params`, carries the
    /// supplied dates, and sets `correlation_id` to the original request id so
    /// the response can be matched back to the parent join.
    #[must_use]
    pub fn with_dates(
        &self,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            request_ids: self.request_ids.clone(),
            start,
            end,
            request_id: UUID4::new(),
            ts_init,
            params: self.params.clone(),
            correlation_id: Some(self.request_id),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::messages::data::IdentifierCountError;

    macro_rules! instrument_request_tests {
        ($test:ident, $request:ident, $($field:ident = $value:expr),* $(,)?) => {
            #[rstest]
            fn $test() {
                let first = InstrumentId::from("ESM4.GLBX");
                let second = InstrumentId::from("NQM4.GLBX");
                let client_id = Some(ClientId::from("HISTORY"));
                let request_id = UUID4::new();
                let ts_init = UnixNanos::from(29);
                let params = Some(Params::new([("source".into(), "history".into())].into()));
                let request = $request::new(
                    [second, first, second],
                    $($value,)*
                    client_id,
                    request_id,
                    ts_init,
                    params.clone(),
                );
                let mut serialized = serde_json::to_value(&request).unwrap();
                serialized["instrument_ids"] = serde_json::to_value([first, second, first]).unwrap();
                let decoded: $request = serde_json::from_value(serialized.clone()).unwrap();
                let mut expected = serialized.clone();
                expected["instrument_ids"] = serde_json::to_value([first, second]).unwrap();
                let single = $request::new(
                    first,
                    $($value,)*
                    client_id,
                    request_id,
                    ts_init,
                    params.clone(),
                );
                serialized["instrument_ids"] = serde_json::json!([]);
                let empty = serde_json::from_value::<$request>(serialized);
                let rejection = request.single_instrument_id().unwrap_err();

                assert_eq!(request.instrument_ids, vec![second, first]);
                $(assert_eq!(request.$field, $value);)*
                assert_eq!(request.client_id, client_id);
                assert_eq!(request.request_id, request_id);
                assert_eq!(request.ts_init, ts_init);
                assert_eq!(request.params, params);
                assert!(rejection.is::<IdentifierCountError>());
                assert_eq!(
                    rejection.to_string(),
                    "request requires one instrument ID on this adapter path, received 2",
                );
                assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
                assert_eq!(single.single_instrument_id().unwrap(), first);
                assert!(empty.unwrap_err().to_string().contains("instrument IDs cannot be empty"));
            }
        };
    }

    instrument_request_tests!(
        test_instrument_request_identifier_lists,
        RequestInstrument,
        start = Some(Timestamp::from_second(11).unwrap()),
        end = Some(Timestamp::from_second(19).unwrap()),
    );
    instrument_request_tests!(
        test_book_snapshot_request_identifier_lists,
        RequestBookSnapshot,
        depth = NonZeroUsize::new(7),
    );
    instrument_request_tests!(
        test_quotes_request_identifier_lists,
        RequestQuotes,
        start = Some(Timestamp::from_second(11).unwrap()),
        end = Some(Timestamp::from_second(19).unwrap()),
        limit = NonZeroUsize::new(3),
    );
    instrument_request_tests!(
        test_trades_request_identifier_lists,
        RequestTrades,
        start = Some(Timestamp::from_second(11).unwrap()),
        end = Some(Timestamp::from_second(19).unwrap()),
        limit = NonZeroUsize::new(3),
    );
    instrument_request_tests!(
        test_funding_rates_request_identifier_lists,
        RequestFundingRates,
        start = Some(Timestamp::from_second(11).unwrap()),
        end = Some(Timestamp::from_second(19).unwrap()),
        limit = NonZeroUsize::new(3),
    );
    instrument_request_tests!(
        test_book_depth_request_identifier_lists,
        RequestBookDepth,
        start = Some(Timestamp::from_second(11).unwrap()),
        end = Some(Timestamp::from_second(19).unwrap()),
        limit = NonZeroUsize::new(3),
        depth = NonZeroUsize::new(7),
    );
    instrument_request_tests!(
        test_book_deltas_request_identifier_lists,
        RequestBookDeltas,
        start = Some(Timestamp::from_second(11).unwrap()),
        end = Some(Timestamp::from_second(19).unwrap()),
        limit = NonZeroUsize::new(3),
    );

    #[rstest]
    fn test_bars_request_identifier_lists() {
        let first = BarType::from("ESM4.GLBX-1-MINUTE-LAST-EXTERNAL");
        let second = BarType::from("ESM4.GLBX-5-MINUTE-LAST-EXTERNAL");
        let start = Some(Timestamp::from_second(11).unwrap());
        let end = Some(Timestamp::from_second(19).unwrap());
        let limit = NonZeroUsize::new(3);
        let client_id = Some(ClientId::from("HISTORY"));
        let request_id = UUID4::new();
        let ts_init = UnixNanos::from(29);
        let params = Some(Params::new([("source".into(), "history".into())].into()));

        let request = RequestBars::new(
            [second, first, second],
            start,
            end,
            limit,
            client_id,
            request_id,
            ts_init,
            params.clone(),
        );
        let mut serialized = serde_json::to_value(&request).unwrap();
        serialized["bar_types"] = serde_json::to_value([first, second, first]).unwrap();
        let decoded: RequestBars = serde_json::from_value(serialized.clone()).unwrap();
        let mut expected = serialized.clone();
        expected["bar_types"] = serde_json::to_value([first, second]).unwrap();
        serialized["bar_types"] = serde_json::json!([]);
        let empty = serde_json::from_value::<RequestBars>(serialized);
        let rejection = request.single_bar_type().unwrap_err();

        assert_eq!(request.bar_types, vec![second, first]);
        assert_eq!(request.start, start);
        assert_eq!(request.end, end);
        assert_eq!(request.limit, limit);
        assert_eq!(request.client_id, client_id);
        assert_eq!(request.request_id, request_id);
        assert_eq!(request.ts_init, ts_init);
        assert_eq!(request.params, params);
        assert!(rejection.is::<IdentifierCountError>());
        assert_eq!(
            rejection.to_string(),
            "request requires one bar type on this adapter path, received 2",
        );
        assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
        assert!(
            empty
                .unwrap_err()
                .to_string()
                .contains("bar types cannot be empty")
        );
    }

    fn leg_request_ids() -> Vec<UUID4> {
        vec![
            UUID4::from("00000000-0000-4000-8000-000000000001"),
            UUID4::from("00000000-0000-4000-8000-000000000002"),
        ]
    }

    fn join_request() -> RequestJoin {
        let params = Params::new([("venue".into(), "SIM".into())].into());

        RequestJoin::new(
            leg_request_ids(),
            Some(Timestamp::from_second(1_700_000_000).unwrap()),
            Some(Timestamp::from_second(1_700_003_600).unwrap()),
            UUID4::from("00000000-0000-4000-8000-000000000003"),
            UnixNanos::from(7),
            Some(params),
            None,
        )
    }

    #[rstest]
    fn test_new_assigns_every_field() {
        let request = join_request();

        assert_eq!(request.request_ids, leg_request_ids());
        assert_eq!(
            request.request_id,
            UUID4::from("00000000-0000-4000-8000-000000000003")
        );
        assert_eq!(
            request.start,
            Some(Timestamp::from_second(1_700_000_000).unwrap())
        );
        assert_eq!(
            request.end,
            Some(Timestamp::from_second(1_700_003_600).unwrap())
        );
        assert_eq!(request.ts_init, UnixNanos::from(7));
        assert_eq!(
            request.params.as_ref().unwrap().get("venue"),
            Some(&"SIM".into())
        );
        assert_eq!(request.correlation_id, None);
    }

    #[rstest]
    #[should_panic(expected = "request_ids must not be empty")]
    fn test_new_rejects_empty_request_ids() {
        let _ = RequestJoin::new(
            Vec::new(),
            None,
            None,
            UUID4::new(),
            UnixNanos::from(1),
            None,
            None,
        );
    }

    #[rstest]
    fn test_with_dates_correlates_the_leg_back_to_the_parent() {
        let parent = join_request();
        let start = Timestamp::from_second(1_700_010_000).unwrap();
        let end = Timestamp::from_second(1_700_013_600).unwrap();

        let leg = parent.with_dates(Some(start), Some(end), UnixNanos::from(11));

        assert_eq!(leg.correlation_id, Some(parent.request_id));
        assert_ne!(leg.request_id, parent.request_id);
        assert_eq!(leg.request_ids, parent.request_ids);
        assert_eq!(leg.params, parent.params);
        assert_eq!(leg.start, Some(start));
        assert_eq!(leg.end, Some(end));
        assert_eq!(leg.ts_init, UnixNanos::from(11));
    }

    #[rstest]
    fn test_with_dates_replaces_rather_than_merges_the_parent_window() {
        let parent = join_request();

        let leg = parent.with_dates(None, None, UnixNanos::from(13));

        assert_eq!(leg.start, None);
        assert_eq!(leg.end, None);
        assert_eq!(leg.correlation_id, Some(parent.request_id));
    }

    #[rstest]
    fn test_with_dates_always_mints_a_fresh_request_id() {
        let parent = join_request();

        let first = parent.with_dates(None, None, UnixNanos::from(1));
        let second = parent.with_dates(None, None, UnixNanos::from(1));

        assert_ne!(first.request_id, second.request_id);
        assert_eq!(first.correlation_id, second.correlation_id);
    }
}
