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

//! Deterministic L2 replay of bounded returned records, without a source-coverage guarantee.
//! Groups preserve source order and exchange clocks. Their derived ordering time is the cumulative
//! maximum of available processing clocks or exchange-clock fallbacks, used for seed and bounds.

use std::{collections::HashSet, num::NonZeroUsize};

use anyhow::Context;
use jiff::Timestamp;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, OrderBookDepth},
    enums::{BookAction, BookType, OrderSide, RecordFlag},
    instruments::BinaryOption,
    orderbook::{OrderBook, analysis::book_check_integrity},
};
use rust_decimal::Decimal;
use strum::FromRepr;

use super::parse;
use crate::http::models::BookEvent;

pub(crate) fn replay(
    source: &[BookEvent],
    instrument: &BinaryOption,
    start: UnixNanos,
    end: UnixNanos,
) -> anyhow::Result<Vec<OrderBookDeltas>> {
    let mut book = OrderBook::new(instrument.id, BookType::L2_MBP);
    let mut batches = Vec::new();
    let mut seeded = false;
    let mut previous = None;
    let mut ordering = UnixNanos::default();
    let mut offset = 0;
    while offset < source.len() {
        let time = source[offset].time_exchange;
        let count = source[offset..]
            .iter()
            .take_while(|row| {
                row.time_exchange == time
                    && (row.update_type == UpdateType::Snapshot as i64)
                        == (source[offset].update_type == UpdateType::Snapshot as i64)
            })
            .count();

        let rows = &source[offset..offset + count];
        for row in rows {
            ordering = ordering.max(parse::timestamp(time_ordering(row))?);
        }

        if offset > 0 && previous != Some(time) {
            anyhow::ensure!(
                source[offset - 1].update_type != UpdateType::Snapshot as i64
                    || rows[0].update_type != UpdateType::Snapshot as i64,
                "FinFeed snapshot spans timestamps without a completion marker"
            );
        }

        if ordering > end {
            break;
        }

        if ordering > start && batches.is_empty() {
            anyhow::ensure!(
                seeded,
                "FinFeed book has no snapshot seed at or before start"
            );
            batches.push(book.to_deltas(start, start));
        }

        if let Some(batch) = apply(rows, instrument, &mut book, &mut seeded, ordering)?
            && ordering > start
        {
            batches.push(batch);
        }

        previous = Some(time);
        offset += count;
    }

    if batches.is_empty() {
        anyhow::ensure!(
            seeded,
            "FinFeed book has no snapshot seed at or before start"
        );
        batches.push(book.to_deltas(start, start));
    }

    Ok(batches)
}

pub(crate) fn deltas(
    batches: Vec<OrderBookDeltas>,
    limit: Option<NonZeroUsize>,
) -> anyhow::Result<Vec<OrderBookDelta>> {
    let mut deltas = Vec::new();
    for batch in batches {
        if limit.is_some_and(|limit| deltas.len() + batch.deltas.len() > limit.get()) {
            anyhow::ensure!(
                !deltas.is_empty(),
                "FinFeed book limit cannot contain the snapshot seed"
            );
            break;
        }

        deltas.extend(batch.deltas);
    }

    Ok(deltas)
}

pub(crate) fn depths(
    batches: &[OrderBookDeltas],
    instrument: &BinaryOption,
    depth: usize,
    limit: Option<NonZeroUsize>,
) -> anyhow::Result<Vec<OrderBookDepth>> {
    let mut book = OrderBook::new(instrument.id, BookType::L2_MBP);
    let mut result = Vec::new();

    for batch in batches
        .iter()
        .take(limit.map_or(usize::MAX, NonZeroUsize::get))
    {
        book.apply_deltas(batch)?;
        let bids: Vec<_> = book
            .bids(Some(depth))
            .flat_map(|level| level.iter().copied())
            .collect();
        let asks: Vec<_> = book
            .asks(Some(depth))
            .flat_map(|level| level.iter().copied())
            .collect();
        let bid_counts = vec![0; bids.len()];
        let ask_counts = vec![0; asks.len()];
        result.push(OrderBookDepth::new_checked(
            instrument.id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8,
            0,
            batch.ts_event,
            batch.ts_init,
        )?);
    }

    Ok(result)
}

pub(crate) fn snapshot(
    batches: &[OrderBookDeltas],
    instrument: &BinaryOption,
    depth: Option<usize>,
) -> anyhow::Result<OrderBook> {
    let mut book = OrderBook::new(instrument.id, BookType::L2_MBP);
    for batch in batches {
        book.apply_deltas(batch)?;
    }

    if let Some(depth) = depth {
        book = parse::limit_depth(&book, depth);
    }

    Ok(book)
}

fn apply(
    rows: &[BookEvent],
    instrument: &BinaryOption,
    book: &mut OrderBook,
    seeded: &mut bool,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<OrderBookDeltas>> {
    let mut deltas = Vec::new();
    let mut snapshot = false;
    let mut prices = HashSet::new();

    for row in rows {
        let mut delta = parse_delta(row, instrument, book)?;
        delta.ts_init = ts_init;

        if !*seeded && row.update_type != UpdateType::Snapshot as i64 {
            continue;
        }

        if row.update_type == UpdateType::Snapshot as i64 {
            if !snapshot {
                let clear = OrderBookDelta::clear(instrument.id, 0, delta.ts_event, delta.ts_init);
                book.apply_delta(&clear)?;
                deltas.push(clear);
                prices.clear();
                *seeded = true;
            }

            anyhow::ensure!(
                prices.insert((delta.order.side, delta.order.price)),
                "duplicate FinFeed snapshot price level"
            );
        }

        snapshot = row.update_type == UpdateType::Snapshot as i64;
        book.apply_delta(&delta)?;
        deltas.push(delta);
    }

    if deltas.is_empty() {
        return Ok(None);
    }

    book_check_integrity(book)?;
    deltas.last_mut().context("empty FinFeed book batch")?.flags |= RecordFlag::F_LAST as u8;
    OrderBookDeltas::new_checked(instrument.id, deltas).map(Some)
}

fn parse_delta(
    row: &BookEvent,
    instrument: &BinaryOption,
    book: &OrderBook,
) -> anyhow::Result<OrderBookDelta> {
    anyhow::ensure!(
        row.symbol_id
            .eq_ignore_ascii_case(instrument.id.symbol.as_str()),
        "FinFeed book event identity mismatch"
    );
    anyhow::ensure!(
        row.id.is_empty(),
        "FinFeed historical replay requires L2 events without order IDs"
    );
    let update_type = UpdateType::from_repr(row.update_type)
        .with_context(|| format!("unsupported FinFeed book action {}", row.update_type))?;
    let ts_event = parse::timestamp(row.time_exchange)?;
    let ts_init = parse::timestamp(time_ordering(row))?;
    let price = parse::price(row.price, instrument)?;
    let mut size = parse::qty(row.size, instrument)?;

    let side = if row.is_sell_ask {
        OrderSide::Sell
    } else {
        OrderSide::Buy
    };

    if update_type == UpdateType::Add {
        let prior = book
            .bids(None)
            .chain(book.asks(None))
            .find(|level| level.side() == side && level.price.value == price)
            .and_then(|level| level.first())
            .map_or(Decimal::ZERO, |order| order.size.as_decimal());
        let total = prior
            .checked_add(row.size)
            .context("FinFeed book size overflow")?;
        size = parse::qty(total, instrument)?;
    }

    let snapshot = update_type == UpdateType::Snapshot;
    anyhow::ensure!(
        !snapshot || row.size > Decimal::ZERO,
        "FinFeed snapshot level size must be positive"
    );

    let action = if snapshot {
        BookAction::Add
    } else if size.is_zero() {
        BookAction::Delete
    } else {
        BookAction::Update
    };

    let flags = if snapshot {
        RecordFlag::F_SNAPSHOT as u8
    } else {
        0
    };

    let order = BookOrder::new(side, price, size, 0);
    OrderBookDelta::new_checked(instrument.id, action, order, flags, 0, ts_event, ts_init)
}

pub(crate) fn time_ordering(row: &BookEvent) -> Timestamp {
    row.time_coinapi.unwrap_or(row.time_exchange)
}

#[repr(i64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromRepr)]
enum UpdateType {
    Add = 0,
    Set = 1,
    Snapshot = 3,
}

#[cfg(test)]
mod tests {
    use nautilus_live::book::conformance::BookStreamChecker;
    use nautilus_model::orderbook::BookIntegrityError;
    use rstest::rstest;

    use super::*;
    use crate::http::parse::parse_instrument;

    fn instrument() -> BinaryOption {
        let market =
            serde_json::from_str(include_str!("../test_data/polymarket-market.json")).unwrap();
        parse_instrument(&market, UnixNanos::default()).unwrap()
    }

    fn events() -> Vec<BookEvent> {
        serde_json::from_str(include_str!("../test_data/book-replay.json")).unwrap()
    }

    #[rstest]
    fn every_completed_batch_reconstructs_exact_full_ladders() {
        let instrument = instrument();
        let source = events();
        let start = parse::timestamp(source[0].time_exchange).unwrap();
        let end = parse::timestamp(source[11].time_exchange).unwrap();
        let batches = replay(&source, &instrument, start, end).unwrap();

        let expected = [
            (
                vec![
                    (
                        "0.4".parse::<Decimal>().unwrap(),
                        "2".parse::<Decimal>().unwrap(),
                    ),
                    (
                        "0.3".parse::<Decimal>().unwrap(),
                        "5".parse::<Decimal>().unwrap(),
                    ),
                ],
                vec![
                    (
                        "0.6".parse::<Decimal>().unwrap(),
                        "3".parse::<Decimal>().unwrap(),
                    ),
                    (
                        "0.7".parse::<Decimal>().unwrap(),
                        "7".parse::<Decimal>().unwrap(),
                    ),
                ],
            ),
            (
                vec![
                    (
                        "0.4".parse::<Decimal>().unwrap(),
                        "5.25".parse::<Decimal>().unwrap(),
                    ),
                    (
                        "0.3".parse::<Decimal>().unwrap(),
                        "5".parse::<Decimal>().unwrap(),
                    ),
                ],
                vec![
                    (
                        "0.65".parse::<Decimal>().unwrap(),
                        "8".parse::<Decimal>().unwrap(),
                    ),
                    (
                        "0.7".parse::<Decimal>().unwrap(),
                        "7".parse::<Decimal>().unwrap(),
                    ),
                ],
            ),
            (
                vec![],
                vec![(
                    "0.8".parse::<Decimal>().unwrap(),
                    "9".parse::<Decimal>().unwrap(),
                )],
            ),
            (vec![], vec![]),
            (
                vec![(
                    "0.2".parse::<Decimal>().unwrap(),
                    "6".parse::<Decimal>().unwrap(),
                )],
                vec![(
                    "0.9".parse::<Decimal>().unwrap(),
                    "10".parse::<Decimal>().unwrap(),
                )],
            ),
        ];

        let mut consumer = OrderBook::new(instrument.id, BookType::L2_MBP);
        assert_eq!(batches.len(), expected.len());

        for (batch, (bids, asks)) in batches.iter().zip(expected) {
            consumer.apply_deltas(batch).unwrap();
            assert_eq!(
                consumer.bids_as_map(None).into_iter().collect::<Vec<_>>(),
                bids
            );
            assert_eq!(
                consumer.asks_as_map(None).into_iter().collect::<Vec<_>>(),
                asks
            );
            assert_eq!(
                batch
                    .deltas
                    .iter()
                    .filter(|delta| delta.flags & RecordFlag::F_LAST as u8 != 0)
                    .count(),
                1
            );
            assert_ne!(
                batch.deltas.last().unwrap().flags & RecordFlag::F_LAST as u8,
                0
            );

            for delta in &batch.deltas {
                assert_eq!(delta.instrument_id, instrument.id);
                assert_eq!(delta.sequence, 0);
                assert_eq!(delta.ts_init, delta.ts_event);
            }
        }

        assert_eq!(
            batches[1]
                .deltas
                .iter()
                .map(|delta| delta.action)
                .collect::<Vec<_>>(),
            vec![
                BookAction::Update,
                BookAction::Update,
                BookAction::Delete,
                BookAction::Update
            ]
        );
        assert_eq!(batches[2].deltas[0].action, BookAction::Clear);
        assert_eq!(batches[4].deltas[0].action, BookAction::Clear);
    }

    #[rstest]
    #[case::snapshot_update(9..10, 8, 0, 0)]
    #[case::update_snapshot(8..9, 7, 0, 0)]
    #[case::multiple(8..10, 7, 0, 0)]
    #[case::seed(4..10, 0, 0, 3)]
    #[case::leading_update(4..10, 0, 4, 3)]
    #[case::only_seed(0..12, 0, 0, 4)]
    fn same_time_transitions_preserve_contract_ladders_and_limits(
        #[case] tied: std::ops::Range<usize>,
        #[case] clock: usize,
        #[case] discard: usize,
        #[case] first_ladder: usize,
    ) {
        let mut source = events();
        let instrument = instrument();
        let time = source[clock].time_exchange;
        for row in &mut source[tied] {
            row.time_exchange = time;
        }

        source.drain(..discard);
        let start = parse::timestamp(source[0].time_exchange).unwrap();
        let end = parse::timestamp(source.last().unwrap().time_exchange).unwrap();
        let batches = replay(&source, &instrument, start, end).unwrap();

        let expected = [
            (
                vec![("0.4", "2"), ("0.3", "5")],
                vec![("0.6", "3"), ("0.7", "7")],
            ),
            (
                vec![("0.4", "5.25"), ("0.3", "5")],
                vec![("0.65", "8"), ("0.7", "7")],
            ),
            (vec![], vec![("0.8", "9")]),
            (vec![], vec![]),
            (vec![("0.2", "6")], vec![("0.9", "10")]),
        ];

        let expected = &expected[first_ladder..];

        let mut checker = BookStreamChecker::new(BookType::L2_MBP, false);
        checker.open(instrument.id);
        assert_eq!(batches.len(), expected.len());
        let mut count = 0;

        for (batch, (bids, asks)) in batches.iter().zip(expected) {
            checker.apply(batch).unwrap();
            let book = checker.book(instrument.id).unwrap();

            for (actual, expected) in [
                (book.bids_as_map(None), bids),
                (book.asks_as_map(None), asks),
            ] {
                assert_eq!(
                    actual.into_iter().collect::<Vec<_>>(),
                    expected
                        .iter()
                        .map(|(price, size)| (
                            price.parse::<Decimal>().unwrap(),
                            size.parse::<Decimal>().unwrap()
                        ))
                        .collect::<Vec<_>>()
                );
            }

            for delta in &batch.deltas {
                assert_eq!(delta.instrument_id, instrument.id);
                assert_eq!(delta.sequence, 0);
                assert_eq!(delta.ts_event, batch.ts_event);
                assert_eq!(delta.ts_init, delta.ts_event);
            }

            count += batch.deltas.len();
            let limited = deltas(batches.clone(), NonZeroUsize::new(count)).unwrap();
            assert_eq!(
                limited,
                batches
                    .iter()
                    .flat_map(|batch| batch.deltas.iter())
                    .take(count)
                    .copied()
                    .collect::<Vec<_>>()
            );
            let partial = NonZeroUsize::new(count - 1);
            if count == batches[0].deltas.len() {
                if partial.is_some() {
                    assert!(deltas(batches.clone(), partial).is_err());
                }
            } else {
                assert_eq!(
                    deltas(batches.clone(), partial).unwrap().len(),
                    count - batch.deltas.len()
                );
            }
        }

        if first_ladder == 0 {
            assert_eq!(batches[2].deltas[0].action, BookAction::Clear);
            assert_eq!(batches[2].deltas[1].action, BookAction::Add);
            assert_eq!(batches[3].deltas[0].action, BookAction::Delete);
        }
    }

    #[rstest]
    #[case(0, 4)]
    #[case(4, 1)]
    fn tied_end_bounds_keep_every_group_and_seed_the_final_state(
        #[case] start_row: usize,
        #[case] count: usize,
    ) {
        let mut source = events();
        source[8].time_exchange = source[7].time_exchange;
        source[9].time_exchange = source[7].time_exchange;
        let instrument = instrument();
        let start = parse::timestamp(source[start_row].time_exchange).unwrap();
        let end = parse::timestamp(source[9].time_exchange).unwrap();
        let batches = replay(&source, &instrument, start, end).unwrap();
        let mut checker = BookStreamChecker::new(BookType::L2_MBP, false);
        checker.open(instrument.id);
        for batch in &batches {
            checker.apply(batch).unwrap();
        }

        let book = checker.book(instrument.id).unwrap();
        assert_eq!(batches.len(), count);
        assert_eq!(batches.last().unwrap().ts_event, end);
        assert_eq!(book.bids_as_map(None).len(), 0);
        assert_eq!(book.asks_as_map(None).len(), 0);
    }

    #[rstest]
    fn crossed_same_time_group_rejects_replay_before_a_repairing_snapshot() {
        let mut source = events();
        source[7].price = "0.35".parse().unwrap();
        source[8].time_exchange = source[7].time_exchange;
        let start = parse::timestamp(source[0].time_exchange).unwrap();
        let end = parse::timestamp(source[11].time_exchange).unwrap();
        let error = replay(&source, &instrument(), start, end).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<BookIntegrityError>(),
            Some(BookIntegrityError::OrdersCrossed(..))
        ));
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn groups_past_end_do_not_invalidate_the_requested_prefix(#[case] received: bool) {
        let mut source = events();
        source.truncate(8);
        let instrument = instrument();
        let start = parse::timestamp(source[0].time_exchange).unwrap();
        let end = parse::timestamp(source[4].time_exchange).unwrap();
        let expected = replay(&source, &instrument, start, end).unwrap();
        let mut tail = source[4].clone();
        tail.price = "0.7".parse().unwrap();
        let after = "2026-10-11T00:00:03Z".parse().unwrap();

        if received {
            tail.time_exchange = source[0].time_exchange;
            tail.time_coinapi = Some(after);
        } else {
            tail.time_exchange = after;
        }

        source.push(tail);
        let actual = replay(&source, &instrument, start, end).unwrap();
        let at_end = replay(
            &source,
            &instrument,
            start,
            parse::timestamp(after).unwrap(),
        )
        .unwrap_err();

        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 2);
        assert!(matches!(
            at_end.downcast_ref::<BookIntegrityError>(),
            Some(BookIntegrityError::OrdersCrossed(..))
        ));
    }

    #[rstest]
    #[case(-1)]
    #[case(2)]
    #[case(4)]
    #[case(5)]
    #[case(99)]
    #[case(i64::MIN)]
    #[case(i64::MAX)]
    fn raw_unsupported_actions_are_rejected_only_inside_the_replay_window(#[case] code: i64) {
        let mut source = events();
        source.truncate(8);
        let instrument = instrument();
        let start = parse::timestamp(source[0].time_exchange).unwrap();
        let end = parse::timestamp(source[4].time_exchange).unwrap();
        let expected = replay(&source, &instrument, start, end).unwrap();
        let mut tail = source[4].clone();
        tail.time_exchange = "2026-10-11T00:00:03Z".parse().unwrap();
        tail.update_type = code;
        let after = parse::timestamp(tail.time_exchange).unwrap();
        source.push(tail);
        let actual = replay(&source, &instrument, start, end).unwrap();
        let error = replay(&source, &instrument, start, after).unwrap_err();

        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 2);
        assert_eq!(
            error.to_string(),
            format!("unsupported FinFeed book action {code}")
        );
    }

    #[rstest]
    #[case("2026-10-11T00:00:01.0000001Z")]
    #[case("2026-10-11T00:00:01.5000000Z")]
    fn snapshot_continuation_past_end_rejects_an_incomplete_seed(#[case] end: Timestamp) {
        let mut source = events();
        source[1].time_exchange = source[4].time_exchange;
        let start = parse::timestamp(source[0].time_exchange).unwrap();
        let error = replay(
            &source,
            &instrument(),
            start,
            parse::timestamp(end).unwrap(),
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "FinFeed snapshot spans timestamps without a completion marker"
        );
    }

    #[rstest]
    fn bounds_seed_state_and_limits_preserve_whole_batches() {
        let source = events();
        let instrument = instrument();
        let start = parse::timestamp(source[4].time_exchange).unwrap();
        let end = parse::timestamp(source[9].time_exchange).unwrap();
        let batches = replay(&source, &instrument, start, end).unwrap();
        assert_eq!(batches.len(), 3);
        assert!(
            batches[0]
                .deltas
                .iter()
                .all(|delta| delta.ts_event == start)
        );
        assert_eq!(
            batches[0].deltas[1].order.size.as_decimal(),
            "5.25".parse::<Decimal>().unwrap()
        );
        assert_eq!(
            deltas(batches.clone(), NonZeroUsize::new(5)).unwrap().len(),
            5
        );
        let limited = deltas(batches.clone(), NonZeroUsize::new(6)).unwrap();
        assert_eq!(limited.len(), 5);
        assert_ne!(limited.last().unwrap().flags & RecordFlag::F_LAST as u8, 0);
        assert_eq!(
            deltas(batches.clone(), NonZeroUsize::new(4))
                .unwrap_err()
                .to_string(),
            "FinFeed book limit cannot contain the snapshot seed"
        );
        let depths = depths(&batches, &instrument, 1, NonZeroUsize::new(2)).unwrap();
        assert_eq!(depths.len(), 2);
        assert_eq!(
            depths[0].bids[0].size.as_decimal(),
            "5.25".parse::<Decimal>().unwrap()
        );
        assert_eq!(
            depths[0].asks[0].price.as_decimal(),
            "0.65".parse::<Decimal>().unwrap()
        );
        let point = snapshot(&batches, &instrument, Some(1)).unwrap();
        assert_eq!(point.bids_as_map(None).len(), 0);
        assert_eq!(point.asks_as_map(None).len(), 0);
    }

    #[rstest]
    #[case("identity")]
    #[case("order-id")]
    #[case("unknown-action")]
    #[case("sub")]
    #[case("delete")]
    #[case("match")]
    #[case("round-price")]
    #[case("round-size")]
    #[case("negative-size")]
    #[case("crossed")]
    #[case("duplicate")]
    #[case("split-snapshot")]
    #[case("zero-snapshot")]
    fn invalid_replay_fails_atomically_before_depth(#[case] invalid: &str) {
        let mut source = events();
        match invalid {
            "identity" => source[1].symbol_id = format!("{}_YES", source[1].symbol_id).into(),
            "order-id" => source[1].id = "order-1".into(),
            "unknown-action" => source[1].update_type = 99,
            "sub" => source[1].update_type = 2,
            "delete" => source[1].update_type = 4,
            "match" => source[1].update_type = 5,
            "round-price" => source[1].price = "0.30001".parse::<Decimal>().unwrap(),
            "round-size" => source[1].size = "5.0000001".parse::<Decimal>().unwrap(),
            "negative-size" => source[1].size = "-1".parse::<Decimal>().unwrap(),
            "crossed" => source[1].price = "0.8".parse::<Decimal>().unwrap(),
            "duplicate" => source[1].price = source[0].price,
            "split-snapshot" => source[1].time_exchange = source[4].time_exchange,
            "zero-snapshot" => source[1].size = Decimal::ZERO,
            _ => unreachable!(),
        }

        let start = parse::timestamp(source[0].time_exchange).unwrap();
        let error = replay(&source, &instrument(), start, start).unwrap_err();

        if invalid == "crossed" {
            assert!(matches!(
                error.downcast_ref::<BookIntegrityError>(),
                Some(BookIntegrityError::OrdersCrossed(..))
            ));
            return;
        }

        let expected = match invalid {
            "identity" => "FinFeed book event identity mismatch",
            "order-id" => "FinFeed historical replay requires L2 events without order IDs",
            "unknown-action" => "unsupported FinFeed book action 99",
            "sub" => "unsupported FinFeed book action 2",
            "delete" => "unsupported FinFeed book action 4",
            "match" => "unsupported FinFeed book action 5",
            "round-price" => "FinFeed price requires rounding",
            "round-size" => "FinFeed size requires rounding",
            "negative-size" => "Decimal value '-1' is negative, Quantity must be non-negative",
            "duplicate" => "duplicate FinFeed snapshot price level",
            "split-snapshot" => "FinFeed snapshot spans timestamps without a completion marker",
            "zero-snapshot" => "FinFeed snapshot level size must be positive",
            _ => unreachable!(),
        };

        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    fn missing_or_late_seed_is_an_error() {
        let source = events();
        let start = parse::timestamp(source[0].time_exchange).unwrap();
        let before = UnixNanos::from(start.as_u64() - 1);

        for (rows, first) in [
            (&[][..], start),
            (&source[4..], start),
            (&source[..], before),
        ] {
            assert_eq!(
                replay(rows, &instrument(), first, start)
                    .unwrap_err()
                    .to_string(),
                "FinFeed book has no snapshot seed at or before start"
            );
        }
    }

    #[rstest]
    #[case(0)]
    #[case(1)]
    fn seed_without_later_events_is_the_complete_series(#[case] offset: u64) {
        let source = events();
        let instrument = instrument();
        let last = parse::timestamp(source[11].time_exchange).unwrap();
        let start = UnixNanos::from(last.as_u64() + offset);
        let batches = replay(&source, &instrument, start, start).unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].deltas[0].action, BookAction::Clear);
        assert!(
            batches[0]
                .deltas
                .iter()
                .all(|delta| delta.ts_event == start && delta.ts_init == start)
        );
        assert_ne!(
            batches[0].deltas.last().unwrap().flags & RecordFlag::F_LAST as u8,
            0
        );
        let book = snapshot(&batches, &instrument, None).unwrap();
        assert_eq!(
            book.bids_as_map(None).into_iter().collect::<Vec<_>>(),
            vec![("0.2".parse::<Decimal>().unwrap(), Decimal::from(6))]
        );
        assert_eq!(
            book.asks_as_map(None).into_iter().collect::<Vec<_>>(),
            vec![("0.9".parse::<Decimal>().unwrap(), Decimal::from(10))]
        );
        assert_eq!(book.ts_last, start);
    }

    #[rstest]
    fn receive_clocks_and_nanosecond_bounds_preserve_source_order() {
        let mut source = events();
        let instrument = instrument();
        let start = parse::timestamp(source[0].time_exchange).unwrap();
        let ts_event = parse::timestamp(source[4].time_exchange).unwrap();
        let received = "2026-10-11T00:00:02.0000009Z".parse().unwrap();
        for row in &mut source[4..8] {
            row.time_coinapi = Some(received);
        }

        let end = parse::timestamp(received).unwrap();
        let batches = replay(&source, &instrument, start, end).unwrap();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[1].deltas[0].ts_event, ts_event);
        assert_eq!(
            batches[1].deltas[0].ts_init,
            parse::timestamp(source[4].time_coinapi.unwrap()).unwrap()
        );
        assert_eq!(batches[0].ts_init, start);
        let before_end = UnixNanos::from(ts_event.as_u64() - 1);
        let earlier = replay(&source, &instrument, start, before_end).unwrap();
        assert_eq!(earlier.len(), 1);
        assert_eq!(
            earlier[0].deltas[1].order.size.as_decimal(),
            Decimal::from(2)
        );
    }

    #[rstest]
    fn absent_level_add_and_locked_completed_batch_remain_valid() {
        let instrument = instrument();
        let mut source = events();
        source[5].price = "0.35".parse().unwrap();
        let start = parse::timestamp(source[0].time_exchange).unwrap();
        let end = parse::timestamp(source[4].time_exchange).unwrap();
        let batches = replay(&source, &instrument, start, end).unwrap();
        assert_eq!(
            batches[1].deltas[1].order.size.as_decimal(),
            "1.25".parse::<Decimal>().unwrap()
        );
        source[4].price = "0.68".parse().unwrap();
        source[7].price = "0.68".parse().unwrap();
        let batches = replay(&source, &instrument, start, end).unwrap();
        let book = snapshot(&batches, &instrument, None).unwrap();
        assert_eq!(book.best_bid_price(), book.best_ask_price());
        assert_eq!(
            book.best_bid_price().unwrap().as_decimal(),
            "0.68".parse::<Decimal>().unwrap()
        );
    }

    #[rstest]
    fn leading_updates_are_validated_before_a_seed_at_start() {
        let mut source = events()[4..].to_vec();
        let instrument = instrument();
        let start = parse::timestamp(source[4].time_exchange).unwrap();
        let end = parse::timestamp(source[7].time_exchange).unwrap();
        let batches = replay(&source, &instrument, start, end).unwrap();
        assert_eq!(batches.len(), 3);
        let initial = snapshot(&batches[..1], &instrument, None).unwrap();
        assert_eq!(
            initial.bids_as_map(None).into_iter().collect::<Vec<_>>(),
            vec![]
        );
        assert_eq!(
            initial.asks_as_map(None).into_iter().collect::<Vec<_>>(),
            vec![("0.8".parse::<Decimal>().unwrap(), Decimal::from(9))]
        );
        assert_eq!(initial.ts_last, start);
        source[0].price = "0.40001".parse().unwrap();
        assert_eq!(
            replay(&source, &instrument, start, end)
                .unwrap_err()
                .to_string(),
            "FinFeed price requires rounding"
        );
    }

    #[rstest]
    #[case("after-end", 1, "2026-10-11T00:00:01.0000001Z")]
    #[case("before-start", 2, "2026-10-11T00:00:02.0000001Z")]
    #[case("reversed", 2, "2026-10-11T00:00:02.0000003Z")]
    fn mixed_source_clocks_use_one_derived_time_per_group(
        #[case] clocks: &str,
        #[case] count: usize,
        #[case] expected: Timestamp,
    ) {
        let mut source = events();
        let start = parse::timestamp(source[0].time_exchange).unwrap();
        let end = parse::timestamp("2026-10-11T00:00:02.0000005Z".parse().unwrap()).unwrap();

        match clocks {
            "after-end" => {
                source[7].time_coinapi = Some("2026-10-11T00:00:02.0000009Z".parse().unwrap());
            }
            "before-start" => {
                source[4].time_coinapi = Some("2026-10-11T00:00:01.0000000Z".parse().unwrap());
            }
            "reversed" => {
                source[4].time_coinapi = Some("2026-10-11T00:00:02.0000003Z".parse().unwrap());
            }
            _ => unreachable!(),
        }

        let batches = replay(&source, &instrument(), start, end).unwrap();
        let last = batches.last().unwrap();

        assert_eq!(batches.len(), count);
        assert_eq!(last.ts_init, parse::timestamp(expected).unwrap());

        for delta in &last.deltas {
            assert_eq!(delta.ts_init, last.ts_init);
        }

        if count == 2 {
            for delta in &last.deltas {
                assert_eq!(
                    delta.ts_event,
                    parse::timestamp(source[4].time_exchange).unwrap()
                );
            }

            assert_eq!(
                last.deltas[1].order.size.as_decimal(),
                "5.25".parse::<Decimal>().unwrap()
            );
        }
    }

    #[rstest]
    #[case(
        "2026-10-11T00:00:01.0000001Z",
        "2026-10-11T00:00:02.0000005Z",
        3,
        "2",
        "5.25"
    )]
    #[case(
        "2026-10-11T00:00:02.0000003Z",
        "2026-10-11T00:00:02.0000005Z",
        3,
        "2",
        "5.25"
    )]
    #[case(
        "2026-10-11T00:00:01.0000001Z",
        "2026-10-11T00:00:02.0000003Z",
        1,
        "2",
        "2"
    )]
    #[case(
        "2026-10-11T00:00:02.0000005Z",
        "2026-10-11T00:00:02.0000005Z",
        1,
        "5.25",
        "5.25"
    )]
    fn late_events_preserve_source_order_and_seed_on_the_derived_clock(
        #[case] start: Timestamp,
        #[case] end: Timestamp,
        #[case] count: usize,
        #[case] seed_qty: Decimal,
        #[case] final_qty: Decimal,
    ) {
        let mut source = events();
        source[4].time_exchange = "2026-10-11T00:00:02.0000005Z".parse().unwrap();
        let instrument = instrument();
        let start = parse::timestamp(start).unwrap();
        let end = parse::timestamp(end).unwrap();
        let batches = replay(&source, &instrument, start, end).unwrap();
        let seed = snapshot(&batches[..1], &instrument, None).unwrap();
        let book = snapshot(&batches, &instrument, None).unwrap();
        let price = "0.4".parse::<Decimal>().unwrap();

        assert_eq!(batches.len(), count);
        assert_eq!(seed.bids_as_map(None)[&price], seed_qty);
        assert_eq!(book.bids_as_map(None)[&price], final_qty);

        for delta in &batches[0].deltas {
            assert_eq!(delta.ts_event, start);
            assert_eq!(delta.ts_init, start);
        }

        if count == 3 {
            assert_eq!(
                batches[1].ts_event,
                parse::timestamp(source[4].time_exchange).unwrap()
            );
            assert_eq!(
                batches[2].ts_event,
                parse::timestamp(source[7].time_exchange).unwrap()
            );

            for batch in &batches[1..] {
                assert_eq!(batch.ts_init, end);

                for delta in &batch.deltas {
                    assert_eq!(delta.ts_init, end);
                }
            }
        }
    }
}
