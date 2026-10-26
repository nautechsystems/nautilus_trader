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

use std::sync::Arc;

use nautilus_common::messages::data::InstrumentsResponse;
use nautilus_model::data::BatchView;

use super::{
    BookDeltasResponse, BookType, CustomData, CustomDataResponse, Data, DataEngine, DataResponse,
    Display, FromStr, HasTsInit, InstrumentAny, InstrumentId, NANOSECONDS_IN_DAY, OrderBook,
    OrderBookDelta, OrderBookDeltas, RecordFlag, RequestCommand, UUID4, UnixNanos, type_name,
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
        for delta in resp.data.iter() {
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
        resp.data = new_data.into();
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

// A book-delta batch carries one instrument ID, so different instruments must stay separate
pub(super) fn rebuild_pipeline_response(
    parent_id: UUID4,
    parent: Option<&RequestCommand>,
    legs: Vec<DataResponse>,
) -> Option<DataResponse> {
    let (start, end) = parent_request_window(parent);
    let mut iter = legs.into_iter();
    let first = iter.next()?;

    let response = match first {
        DataResponse::Data(mut acc) => {
            let mut data = custom_response_data(&acc, parent_id)?;

            for leg in iter {
                let DataResponse::Data(other) = leg else {
                    return None;
                };

                data.extend(custom_response_data(&other, parent_id)?);
            }

            data.sort_by_key(CustomData::ts_init);
            acc.data = Arc::new(data);
            acc.start = start.or(acc.start);
            acc.end = end.or(acc.end);
            DataResponse::Data(acc)
        }
        DataResponse::Instrument(acc) => {
            let data = merge_instrument_data(vec![acc.data], iter)?;
            DataResponse::Instruments(InstrumentsResponse::new(
                parent_id,
                acc.client_id,
                acc.instrument_id.venue,
                data,
                start.or(acc.start),
                end.or(acc.end),
                acc.ts_init,
                acc.params,
            ))
        }
        DataResponse::Instruments(mut acc) => {
            acc.data = merge_instrument_data(acc.data, iter)?;
            acc.start = start.or(acc.start);
            acc.end = end.or(acc.end);
            DataResponse::Instruments(acc)
        }
        DataResponse::Quotes(mut acc) => {
            acc.data = merge_series(
                acc.data,
                iter.map(|r| {
                    if let DataResponse::Quotes(r) = r {
                        Some(r.data)
                    } else {
                        None
                    }
                }),
                |row| (row.ts_init, row.instrument_id),
            )?;

            acc.start = start.or(acc.start);
            acc.end = end.or(acc.end);
            if let Some(RequestCommand::Quotes(parent)) = parent {
                acc.instrument_ids.clone_from(&parent.instrument_ids);
            }

            DataResponse::Quotes(acc)
        }
        DataResponse::Trades(mut acc) => {
            acc.data = merge_series(
                acc.data,
                iter.map(|r| {
                    if let DataResponse::Trades(r) = r {
                        Some(r.data)
                    } else {
                        None
                    }
                }),
                |row| (row.ts_init, row.instrument_id),
            )?;

            acc.start = start.or(acc.start);
            acc.end = end.or(acc.end);
            if let Some(RequestCommand::Trades(parent)) = parent {
                acc.instrument_ids.clone_from(&parent.instrument_ids);
            }

            DataResponse::Trades(acc)
        }
        DataResponse::FundingRates(mut acc) => {
            acc.data = merge_series(
                acc.data,
                iter.map(|r| {
                    if let DataResponse::FundingRates(r) = r {
                        Some(r.data)
                    } else {
                        None
                    }
                }),
                |row| row.ts_init,
            )?;

            acc.start = start.or(acc.start);
            acc.end = end.or(acc.end);
            DataResponse::FundingRates(acc)
        }
        DataResponse::Bars(mut acc) => {
            acc.data = merge_series(
                acc.data,
                iter.map(|r| {
                    if let DataResponse::Bars(r) = r {
                        Some(r.data)
                    } else {
                        None
                    }
                }),
                |row| row.ts_init,
            )?;

            acc.start = start.or(acc.start);
            acc.end = end.or(acc.end);
            DataResponse::Bars(acc)
        }
        DataResponse::BookDeltas(mut acc) => {
            let instrument_id = acc.instrument_id;
            acc.data = merge_series(
                acc.data,
                iter.map(|r| {
                    let DataResponse::BookDeltas(r) = r else {
                        return None;
                    };

                    same_instrument(r.instrument_id, instrument_id).then_some(r.data)
                }),
                |row| row.ts_init,
            )?;

            acc.start = start.or(acc.start);
            acc.end = end.or(acc.end);
            DataResponse::BookDeltas(acc)
        }
        DataResponse::BookDepth(mut acc) => {
            acc.data = merge_series(
                acc.data,
                iter.map(|r| {
                    if let DataResponse::BookDepth(r) = r {
                        Some(r.data)
                    } else {
                        None
                    }
                }),
                |row| row.ts_init,
            )?;

            acc.start = start.or(acc.start);
            acc.end = end.or(acc.end);
            DataResponse::BookDepth(acc)
        }
        _ => return None,
    };

    Some(rebind_response_correlation(response, parent_id))
}

pub(super) fn same_instrument(left: InstrumentId, right: InstrumentId) -> bool {
    left == right
        || (left.symbol.as_str() == right.symbol.as_str()
            && left.venue.as_str() == right.venue.as_str())
}

fn merge_series<T: Clone, K: Ord>(
    mut data: BatchView<T>,
    legs: impl Iterator<Item = Option<BatchView<T>>>,
    key: impl Fn(&T) -> K,
) -> Option<BatchView<T>> {
    let mut batches = Vec::new();

    for leg in legs {
        let leg = leg?;

        if leg.is_empty() {
            continue;
        }

        if data.is_empty() {
            data = leg;
        } else {
            batches.push(leg);
        }
    }

    if !batches.is_empty() {
        let len = data.len() + batches.iter().map(|batch| batch.len()).sum::<usize>();
        let mut rows = Vec::with_capacity(len);
        rows.extend_from_slice(&data);
        for batch in batches {
            rows.extend_from_slice(&batch);
        }

        data = rows.into();
    }

    if !data.is_sorted_by_key(&key) {
        data.make_mut().sort_by_key(key);
    }

    Some(data)
}

fn merge_instrument_data(
    mut data: Vec<InstrumentAny>,
    legs: impl Iterator<Item = DataResponse>,
) -> Option<Vec<InstrumentAny>> {
    for leg in legs {
        match leg {
            DataResponse::Instrument(r) => data.push(r.data),
            DataResponse::Instruments(r) => data.extend(r.data),
            _ => return None,
        }
    }

    Some(data)
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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::sorted(vec![9, 1, 2, 8], true)]
    #[case::unsorted(vec![9, 2, 1, 8], false)]
    fn test_merge_series_preserves_single_visible_leg(
        #[case] rows: Vec<u64>,
        #[case] shared: bool,
    ) {
        let backing = Arc::new(rows.clone());
        let view = BatchView::new(Arc::clone(&backing), 1..3);
        let empty: BatchView<u64> = Vec::new().into();

        let merged = merge_series(
            empty.clone(),
            [Some(view), Some(empty)].into_iter(),
            |row| *row,
        )
        .unwrap();

        assert_eq!(merged.as_ref(), &[1, 2]);
        assert_eq!(Arc::ptr_eq(merged.arc(), &backing), shared);
        assert_eq!(merged.range(), if shared { 1..3 } else { 0..2 });
        assert_eq!(backing.as_slice(), rows.as_slice());
    }

    #[rstest]
    fn test_merge_series_materializes_visible_rows_in_stable_leg_order() {
        let first = Arc::new(vec![(0, 90), (2, 11), (1, 12), (9, 91)]);
        let second = Arc::new(vec![(0, 92), (1, 21), (2, 22), (9, 93)]);

        let merged = merge_series(
            BatchView::new(Arc::clone(&first), 1..3),
            [Some(BatchView::new(Arc::clone(&second), 1..3))].into_iter(),
            |row| row.0,
        )
        .unwrap();

        assert_eq!(merged.as_ref(), &[(1, 12), (1, 21), (2, 11), (2, 22)]);
        assert_eq!(merged.range(), 0..4);
        assert_eq!(first.as_slice(), &[(0, 90), (2, 11), (1, 12), (9, 91)]);
        assert_eq!(second.as_slice(), &[(0, 92), (1, 21), (2, 22), (9, 93)]);
    }

    #[rstest]
    fn test_merge_series_rejects_missing_variant_after_empty_legs() {
        let data: BatchView<u64> = vec![1, 2].into();
        let empty: BatchView<u64> = Vec::new().into();

        let merged = merge_series(data, [Some(empty), None].into_iter(), |row| *row);

        assert!(merged.is_none());
    }
}
