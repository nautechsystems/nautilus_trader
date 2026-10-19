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

use super::{
    BarsResponse, BookDeltasResponse, BookDepthResponse, BookType, CustomData, CustomDataResponse,
    Data, DataEngine, DataResponse, Display, FromStr, FundingRatesResponse, HasTsInit,
    NANOSECONDS_IN_DAY, OrderBook, OrderBookDelta, OrderBookDeltas, QuotesResponse, RecordFlag,
    RequestCommand, TradesResponse, UUID4, UnixNanos, type_name,
};

impl DataEngine {
    // Replays a day-start snapshot forward to the request's original start: when the first delta
    // is an F_SNAPSHOT on a UTC day boundary, rebuilds the book from the pre-start deltas and
    // replaces them with one snapshot keyed at the original start, then forwards the rest.
    pub(super) fn book_deltas_snapshot_replay(&self, resp: &mut BookDeltasResponse) {
        let Some(original_start_ns) = resp.start else {
            return;
        };

        let Some(first) = resp.data.first().copied() else {
            return;
        };

        if !RecordFlag::F_SNAPSHOT.matches(first.flags) {
            return;
        }

        if first.ts_init.as_u64() % NANOSECONDS_IN_DAY != 0 {
            return;
        }

        // Nothing to fast-forward when the request starts at or before the day-start snapshot
        if original_start_ns <= first.ts_init {
            return;
        }

        if self
            .cache
            .borrow()
            .instrument(&resp.instrument_id)
            .is_none()
        {
            log::warn!(
                "Instrument {} not found in cache, skipping snapshot replay",
                resp.instrument_id,
            );
            return;
        }

        let book_type = resp
            .params
            .as_ref()
            .and_then(|p| p.get_str("book_type"))
            .and_then(|s| BookType::from_str(s).ok())
            .unwrap_or(BookType::L2_MBP);

        let mut book = OrderBook::new(resp.instrument_id, book_type);
        let mut before: Vec<OrderBookDelta> = Vec::new();
        let mut after: Vec<OrderBookDelta> = Vec::new();
        let mut last_applied_ts: Option<UnixNanos> = None;
        let mut crossed = false;

        for delta in &resp.data {
            if crossed {
                after.push(*delta);
            } else {
                before.push(*delta);
                if delta.ts_init >= original_start_ns {
                    crossed = true;
                    last_applied_ts = Some(delta.ts_init);
                }
            }
        }

        if !before.is_empty() {
            if last_applied_ts.is_none() {
                last_applied_ts = before.last().map(|d| d.ts_init);
            }

            let batch = OrderBookDeltas::new(resp.instrument_id, before);
            if let Err(e) = book.apply_deltas(&batch) {
                log::error!(
                    "Failed to rebuild book for snapshot replay on {}: {e}",
                    resp.instrument_id,
                );
                return;
            }
        }

        let Some(last_ts) = last_applied_ts else {
            return;
        };

        let snapshot_ts = last_ts.max(original_start_ns);
        let mut new_data = book.to_deltas(snapshot_ts, snapshot_ts).deltas;
        new_data.extend(after);
        resp.data = new_data;
    }
}

#[inline(always)]
pub(super) fn log_if_empty_response<T, I: Display>(
    data: &[T],
    id: &I,
    correlation_id: &UUID4,
) -> bool {
    if data.is_empty() {
        let name = type_name::<T>();
        let short_name = name.rsplit("::").next().unwrap_or(name);
        log::warn!("Received empty {short_name} response for {id} {correlation_id}");
        return true;
    }

    false
}

/// Concatenates same-variant leg payloads into a single rebuilt response keyed by `parent_id`.
///
/// Returns `None` when legs are mixed-variant or empty; pipelines only group legs of the same
/// variant. `BookDeltas` legs additionally return `None` when their wrapper instruments differ,
/// since a book-delta batch is keyed by one instrument and cannot carry another's children.
/// The rebuilt response inherits `start` and `end` from the parent request when the parent is a
/// `RequestJoin`; otherwise leg bounds are preserved on the first leg.
pub(super) fn rebuild_pipeline_response(
    parent_id: UUID4,
    parent: Option<&RequestCommand>,
    legs: Vec<DataResponse>,
) -> Option<DataResponse> {
    if legs.is_empty() {
        return None;
    }

    let (parent_start, parent_end) = parent_request_window(parent);

    let mut iter = legs.into_iter();
    let first = iter.next()?;

    match first {
        DataResponse::Data(mut acc) => {
            let mut data = custom_response_data(&acc, parent_id)?;

            for leg in iter {
                let DataResponse::Data(other) = leg else {
                    log::error!("Mixed-variant legs in pipeline {parent_id}");
                    return None;
                };

                data.extend(custom_response_data(&other, parent_id)?);
            }

            data.sort_by_key(CustomData::ts_init);
            acc.data = std::sync::Arc::new(data);
            acc.correlation_id = parent_id;

            if parent_start.is_some() {
                acc.start = parent_start;
            }

            if parent_end.is_some() {
                acc.end = parent_end;
            }

            Some(DataResponse::Data(acc))
        }
        DataResponse::Quotes(mut acc) => {
            for leg in iter {
                let DataResponse::Quotes(other) = leg else {
                    log::error!("Mixed-variant legs in pipeline {parent_id}");
                    return None;
                };

                acc.data.extend(other.data);
            }

            acc.data.sort_by_key(|q| q.ts_init);
            acc.correlation_id = parent_id;

            if parent_start.is_some() {
                acc.start = parent_start;
            }

            if parent_end.is_some() {
                acc.end = parent_end;
            }

            Some(DataResponse::Quotes(acc))
        }
        DataResponse::Trades(mut acc) => {
            for leg in iter {
                let DataResponse::Trades(other) = leg else {
                    log::error!("Mixed-variant legs in pipeline {parent_id}");
                    return None;
                };

                acc.data.extend(other.data);
            }

            acc.data.sort_by_key(|t| t.ts_init);
            acc.correlation_id = parent_id;

            if parent_start.is_some() {
                acc.start = parent_start;
            }

            if parent_end.is_some() {
                acc.end = parent_end;
            }

            Some(DataResponse::Trades(acc))
        }
        DataResponse::FundingRates(mut acc) => {
            for leg in iter {
                let DataResponse::FundingRates(other) = leg else {
                    log::error!("Mixed-variant legs in pipeline {parent_id}");
                    return None;
                };

                acc.data.extend(other.data);
            }

            acc.data.sort_by_key(|r| r.ts_init);
            acc.correlation_id = parent_id;

            if parent_start.is_some() {
                acc.start = parent_start;
            }

            if parent_end.is_some() {
                acc.end = parent_end;
            }

            Some(DataResponse::FundingRates(acc))
        }
        DataResponse::Bars(mut acc) => {
            for leg in iter {
                let DataResponse::Bars(other) = leg else {
                    log::error!("Mixed-variant legs in pipeline {parent_id}");
                    return None;
                };

                acc.data.extend(other.data);
            }

            acc.data.sort_by_key(|b| b.ts_init);
            acc.correlation_id = parent_id;

            if parent_start.is_some() {
                acc.start = parent_start;
            }

            if parent_end.is_some() {
                acc.end = parent_end;
            }

            Some(DataResponse::Bars(acc))
        }
        DataResponse::Instruments(mut acc) => {
            for leg in iter {
                let DataResponse::Instruments(other) = leg else {
                    log::error!("Mixed-variant legs in pipeline {parent_id}");
                    return None;
                };

                acc.data.extend(other.data);
            }

            acc.correlation_id = parent_id;
            Some(DataResponse::Instruments(acc))
        }
        DataResponse::BookDeltas(mut acc) => {
            for leg in iter {
                let DataResponse::BookDeltas(other) = leg else {
                    log::error!("Mixed-variant legs in pipeline {parent_id}");
                    return None;
                };

                // A book-delta batch is keyed by one instrument, so legs for different
                // instruments cannot be concatenated into a single response. Matched by
                // value as well as identity, mirroring `OrderBookDeltas::new_checked`,
                // since legs crossing the FFI boundary do not share an intern pool.
                let same_instrument = other.instrument_id == acc.instrument_id
                    || (other.instrument_id.symbol.as_str() == acc.instrument_id.symbol.as_str()
                        && other.instrument_id.venue.as_str() == acc.instrument_id.venue.as_str());

                if !same_instrument {
                    log::error!(
                        "Mixed-instrument BookDeltas legs in pipeline {parent_id}: {} and {}",
                        acc.instrument_id,
                        other.instrument_id,
                    );
                    return None;
                }

                acc.data.extend(other.data);
            }

            acc.data.sort_by_key(|d| d.ts_init);
            acc.correlation_id = parent_id;

            if parent_start.is_some() {
                acc.start = parent_start;
            }

            if parent_end.is_some() {
                acc.end = parent_end;
            }

            Some(DataResponse::BookDeltas(acc))
        }
        DataResponse::BookDepth(mut acc) => {
            for leg in iter {
                let DataResponse::BookDepth(other) = leg else {
                    log::error!("Mixed-variant legs in pipeline {parent_id}");
                    return None;
                };

                acc.data.extend(other.data);
            }

            acc.data.sort_by_key(|d| d.ts_init);
            acc.correlation_id = parent_id;

            if parent_start.is_some() {
                acc.start = parent_start;
            }

            if parent_end.is_some() {
                acc.end = parent_end;
            }

            Some(DataResponse::BookDepth(acc))
        }
        other => {
            // Pipelines today rebuild same-variant time-series legs. Variants
            // without a per-item ts_init payload (singular Book/Instrument,
            // OptionChainReferencePrice) cannot be concatenated and would
            // otherwise leak a leg-keyed response. Drop rather than forward.
            log::error!(
                "Pipeline rebuild not supported for variant {} (parent {parent_id})",
                other.kind(),
            );
            None
        }
    }
}

fn custom_response_data(resp: &CustomDataResponse, parent_id: UUID4) -> Option<Vec<CustomData>> {
    if let Some(data) = resp.data.as_ref().downcast_ref::<Vec<CustomData>>() {
        return Some(data.clone());
    }

    if let Some(data) = resp.data.as_ref().downcast_ref::<CustomData>() {
        return Some(vec![data.clone()]);
    }

    if let Some(data) = resp.data.as_ref().downcast_ref::<Vec<Data>>() {
        let mut custom = Vec::with_capacity(data.len());
        for item in data {
            let Data::Custom(value) = item else {
                log::error!("Custom data pipeline {parent_id} received non-custom data {item:?}");
                return None;
            };

            custom.push(value.clone());
        }

        return Some(custom);
    }

    log::error!(
        "Custom data pipeline {parent_id} received unsupported payload for {}",
        resp.data_type,
    );
    None
}

pub(super) fn parent_request_window(
    parent: Option<&RequestCommand>,
) -> (Option<UnixNanos>, Option<UnixNanos>) {
    let Some(parent) = parent else {
        return (None, None);
    };

    let (start, end) = match parent {
        RequestCommand::Data(cmd) => (cmd.start, cmd.end),
        RequestCommand::Instrument(cmd) => (cmd.start, cmd.end),
        RequestCommand::Instruments(cmd) => (cmd.start, cmd.end),
        RequestCommand::BookDeltas(cmd) => (cmd.start, cmd.end),
        RequestCommand::BookDepth(cmd) => (cmd.start, cmd.end),
        RequestCommand::Quotes(cmd) => (cmd.start, cmd.end),
        RequestCommand::Trades(cmd) => (cmd.start, cmd.end),
        RequestCommand::FundingRates(cmd) => (cmd.start, cmd.end),
        RequestCommand::Bars(cmd) => (cmd.start, cmd.end),
        RequestCommand::Join(cmd) => (cmd.start, cmd.end),
        RequestCommand::BookSnapshot(_) | RequestCommand::OptionChainReferencePrice(_) => {
            return (None, None);
        }
    };

    (
        start.map(datetime_to_unix_nanos_or_zero),
        end.map(datetime_to_unix_nanos_or_zero),
    )
}

pub(super) fn datetime_to_unix_nanos_or_zero(dt: jiff::Timestamp) -> UnixNanos {
    UnixNanos::from(u64::try_from(dt.as_nanosecond().max(0)).unwrap_or(0))
}

pub(super) fn empty_response_like(
    template: &DataResponse,
    correlation_id: UUID4,
    ts_init: UnixNanos,
) -> DataResponse {
    match template {
        DataResponse::Quotes(r) => DataResponse::Quotes(QuotesResponse::new(
            correlation_id,
            r.client_id,
            r.instrument_id,
            Vec::new(),
            r.start,
            r.end,
            ts_init,
            r.params.clone(),
        )),
        DataResponse::Trades(r) => DataResponse::Trades(TradesResponse::new(
            correlation_id,
            r.client_id,
            r.instrument_id,
            Vec::new(),
            r.start,
            r.end,
            ts_init,
            r.params.clone(),
        )),
        DataResponse::FundingRates(r) => DataResponse::FundingRates(FundingRatesResponse::new(
            correlation_id,
            r.client_id,
            r.instrument_id,
            Vec::new(),
            r.start,
            r.end,
            ts_init,
            r.params.clone(),
        )),
        DataResponse::Bars(r) => DataResponse::Bars(BarsResponse::new(
            correlation_id,
            r.client_id,
            r.bar_type,
            Vec::new(),
            r.start,
            r.end,
            ts_init,
            r.params.clone(),
        )),
        DataResponse::BookDeltas(r) => DataResponse::BookDeltas(BookDeltasResponse::new(
            correlation_id,
            r.client_id,
            r.instrument_id,
            Vec::new(),
            r.start,
            r.end,
            ts_init,
            r.params.clone(),
        )),
        DataResponse::BookDepth(r) => DataResponse::BookDepth(BookDepthResponse::new(
            correlation_id,
            r.client_id,
            r.instrument_id,
            Vec::new(),
            r.start,
            r.end,
            ts_init,
            r.params.clone(),
        )),
        other => {
            log::error!(
                "Cannot fabricate empty leg response for variant {}",
                other.kind(),
            );
            other.clone()
        }
    }
}

pub(super) fn rebind_response_correlation(mut resp: DataResponse, new_id: UUID4) -> DataResponse {
    match &mut resp {
        DataResponse::Data(r) => r.correlation_id = new_id,
        DataResponse::Instrument(r) => r.correlation_id = new_id,
        DataResponse::Instruments(r) => r.correlation_id = new_id,
        DataResponse::Book(r) => r.correlation_id = new_id,
        DataResponse::BookDeltas(r) => r.correlation_id = new_id,
        DataResponse::BookDepth(r) => r.correlation_id = new_id,
        DataResponse::Quotes(r) => r.correlation_id = new_id,
        DataResponse::Trades(r) => r.correlation_id = new_id,
        DataResponse::FundingRates(r) => r.correlation_id = new_id,
        DataResponse::OptionChainReferencePrice(r) => r.correlation_id = new_id,
        DataResponse::Bars(r) => r.correlation_id = new_id,
    }

    resp
}
