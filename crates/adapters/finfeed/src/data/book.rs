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

use std::{collections::HashSet, num::NonZeroUsize};

use anyhow::Context;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, OrderBookDepth},
    enums::{BookAction, BookType, OrderSide, RecordFlag},
    instruments::BinaryOption,
    orderbook::{OrderBook, analysis::book_check_integrity},
};
use rust_decimal::Decimal;

use super::parse;
use crate::http::models::BookEvent;

pub(super) fn replay(
    source: &[BookEvent],
    instrument: &BinaryOption,
    start: UnixNanos,
    end: UnixNanos,
) -> anyhow::Result<Vec<OrderBookDeltas>> {
    let mut book = OrderBook::new(instrument.id, BookType::L2_MBP);
    let mut batches = Vec::new();
    let mut seeded = false;
    let mut previous = None;
    let mut offset = 0;
    while offset < source.len() {
        let time = source[offset].time_exchange;
        let ts_event = parse::timestamp(time)?;
        anyhow::ensure!(
            previous.is_none_or(|prior| prior < time),
            "FinFeed book timestamps are out of order"
        );
        let count = source[offset..]
            .iter()
            .take_while(|row| row.time_exchange == time)
            .count();
        let rows = &source[offset..offset + count];
        if offset > 0 {
            anyhow::ensure!(
                source[offset - 1].update_type != 3 || rows[0].update_type != 3,
                "FinFeed snapshot spans timestamps without a completion marker"
            );
        }

        if ts_event > start && batches.is_empty() {
            anyhow::ensure!(
                seeded,
                "FinFeed book has no snapshot seed at or before start"
            );
            batches.push(book.to_deltas(start, start));
        }

        if let Some(batch) = apply(rows, instrument, &mut book, &mut seeded)?
            && ts_event > start
            && ts_event <= end
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

    // Downstream trimming and pipeline ordering use ts_init
    let mut ts_init = start;

    for delta in batches.iter().flat_map(|batch| &batch.deltas) {
        anyhow::ensure!(
            delta.ts_init >= ts_init && delta.ts_init <= end,
            "FinFeed book receive clocks cannot preserve request bounds and order"
        );
        ts_init = delta.ts_init;
    }

    Ok(batches)
}

pub(super) fn deltas(
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

pub(super) fn depths(
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

pub(super) fn snapshot(
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
) -> anyhow::Result<Option<OrderBookDeltas>> {
    let mut deltas = Vec::new();
    let mut snapshot = false;
    let mut prices = HashSet::new();

    for row in rows {
        let delta = parse_delta(row, instrument, book)?;
        if !*seeded && row.update_type != 3 {
            continue;
        }

        if row.update_type == 3 {
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

        snapshot = row.update_type == 3;
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
    anyhow::ensure!(
        matches!(row.update_type, 0 | 1 | 3),
        "unsupported FinFeed book action {}",
        row.update_type
    );
    let ts_event = parse::timestamp(row.time_exchange)?;
    let ts_init = parse::timestamp(row.time_coinapi.unwrap_or(row.time_exchange))?;
    let price = parse::price(row.price, instrument)?;
    let mut size = parse::qty(row.size, instrument)?;

    let side = if row.is_sell_ask {
        OrderSide::Sell
    } else {
        OrderSide::Buy
    };

    if row.update_type == 0 {
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

    let snapshot = row.update_type == 3;
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

#[cfg(test)]
mod tests {
    use nautilus_model::orderbook::BookIntegrityError;
    use rstest::rstest;

    use super::*;
    use crate::http::parse::parse_instrument;

    fn instrument() -> BinaryOption {
        let market =
            serde_json::from_str(include_str!("../../test_data/polymarket-market.json")).unwrap();
        parse_instrument(&market, UnixNanos::default()).unwrap()
    }

    fn events() -> Vec<BookEvent> {
        serde_json::from_str(include_str!("../../test_data/book-replay.json")).unwrap()
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
    #[case("decreasing-time")]
    #[case("split-snapshot")]
    #[case("zero-snapshot")]
    fn invalid_replay_fails_atomically_before_depth(#[case] invalid: &str) {
        let mut source = events();
        match invalid {
            "identity" => source[1].symbol_id.push_str("_YES"),
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
            "decreasing-time" => source.swap(3, 4),
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
            "decreasing-time" => "FinFeed book timestamps are out of order",
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
    #[case("after-end")]
    #[case("before-start")]
    #[case("reversed")]
    fn incompatible_receive_clocks_reject_the_complete_response(#[case] invalid: &str) {
        let mut source = events();
        let start = parse::timestamp(source[0].time_exchange).unwrap();
        let end = parse::timestamp("2026-10-11T00:00:02.0000005Z".parse().unwrap()).unwrap();

        match invalid {
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

        assert_eq!(
            replay(&source, &instrument(), start, end)
                .unwrap_err()
                .to_string(),
            "FinFeed book receive clocks cannot preserve request bounds and order"
        );
    }
}
