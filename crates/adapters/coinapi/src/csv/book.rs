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

//! `T-LIMITBOOK_FULL` rows.
//!
//! A file opens with a `SNAPSHOT` block, then lists updates in file order. Rows without an
//! `order_id` change price levels (L2): `SET` gives a level's absolute size and zero removes it.
//! Rows with an `order_id` change resting orders (L3): `ADD` adds one, `SET` gives its absolute
//! size, `MATCH` and `SUB` subtract a filled size, and `DELETE` removes it. A fill to zero removes
//! the order without a `DELETE`, and a venue can reuse an identifier after its `DELETE`.

use ahash::AHashMap;
use csv::StringRecord;
use jiff::civil::Date;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta},
    enums::{BookAction, OrderSide, RecordFlag},
    identifiers::InstrumentId,
};
use rust_decimal::Decimal;

use super::{
    Decode,
    rows::{Precision, Rows, number, text, time_on},
};
use crate::websocket::parse::{price_at, quantity_at};

const DAY_NANOS: u64 = 86_400_000_000_000;
const HALF_DAY_NANOS: u64 = DAY_NANOS / 2;

/// Converts `T-LIMITBOOK_FULL` rows to [`OrderBookDelta`]s.
///
/// Each event ends with an `F_LAST` delta: the opening snapshot, which starts with a clear, is one
/// event, and so is each run of updates sharing a `time_exchange`.
#[derive(Debug)]
pub(crate) struct BookDecoder {
    instrument_id: InstrumentId,
    date: Date,
    time_exchange: usize,
    time_coinapi: usize,
    update_type: usize,
    is_buy: usize,
    entry_px: usize,
    entry_sx: usize,
    order_id: usize,
    // Resting L3 orders by venue identifier, as last emitted
    orders: AHashMap<String, BookOrder>,
    order_id_last: u64,
    // Deltas of the event in progress, and whether it is a snapshot with its first timestamp
    event: Vec<OrderBookDelta>,
    event_key: Option<(bool, UnixNanos)>,
}

impl Decode for BookDecoder {
    type Output = OrderBookDelta;

    fn fit(
        &self,
        record: &StringRecord,
        price: &mut Precision,
        size: &mut Precision,
    ) -> anyhow::Result<()> {
        price.fit(number(record, self.entry_px, "entry_px")?);
        size.fit(number(record, self.entry_sx, "entry_sx")?);
        Ok(())
    }

    fn decode(
        &mut self,
        record: &StringRecord,
        price: &mut Precision,
        size: &mut Precision,
        out: &mut Vec<OrderBookDelta>,
    ) -> anyhow::Result<()> {
        let update_type = text(record, self.update_type)?;
        let is_snapshot = update_type == "SNAPSHOT";
        let ts_event = time_on(
            self.date,
            text(record, self.time_exchange)?,
            "time_exchange",
        )?;
        let mut ts_init = time_on(self.date, text(record, self.time_coinapi)?, "time_coinapi")?;

        // A receive time of day far below the exchange time falls after midnight
        if ts_init.as_u64() + HALF_DAY_NANOS < ts_event.as_u64() {
            ts_init = UnixNanos::from(ts_init.as_u64() + DAY_NANOS);
        }

        let side = match text(record, self.is_buy)? {
            "1" => OrderSide::Buy,
            "0" => OrderSide::Sell,
            other => anyhow::bail!("invalid is_buy '{other}'"),
        };

        let entry_price = number(record, self.entry_px, "entry_px")?;
        let entry_size = number(record, self.entry_sx, "entry_sx")?;
        price.fit(entry_price);
        size.fit(entry_size);

        // Every row must hold a valid price and size, including rows that only remove an order
        price_at(entry_price, price.value, "entry_px")?;
        quantity_at(entry_size, size.value, "entry_sx")?;

        self.start_event(is_snapshot, ts_event, ts_init, out);

        let flags = if is_snapshot {
            RecordFlag::F_SNAPSHOT as u8
        } else {
            0
        };

        let order_id = text(record, self.order_id)?;

        let row = Row {
            update_type,
            side,
            price: entry_price,
            size: entry_size,
            flags,
            ts_event,
            ts_init,
        };

        if order_id.is_empty() {
            self.decode_level(&row, *price, *size)
        } else {
            self.decode_order(order_id, &row, *price, *size)
        }
    }

    fn pending(&self) -> usize {
        self.event.len()
    }

    fn finish(&mut self, rows: &mut Rows, out: &mut Vec<OrderBookDelta>) -> anyhow::Result<()> {
        // A limit can stop inside the snapshot, so read ahead to the first update to date it
        if matches!(self.event_key, Some((true, _))) {
            while let Some(record) = rows.next()? {
                if text(record, self.update_type)? != "SNAPSHOT" {
                    let first_update = time_on(
                        self.date,
                        text(record, self.time_exchange)?,
                        "time_exchange",
                    )?;
                    self.date_snapshot(first_update);
                    break;
                }
            }
        }

        self.complete_event(out);
        Ok(())
    }

    fn truncate(out: &mut Vec<OrderBookDelta>, len: usize) {
        out.truncate(len);

        if let Some(last) = out.last_mut() {
            last.flags |= RecordFlag::F_LAST as u8;
        }
    }

    fn set_precision(value: &mut OrderBookDelta, price: Option<u8>, size: Option<u8>) {
        if let Some(precision) = price {
            value.order.price.precision = precision;
        }

        if let Some(precision) = size {
            value.order.size.precision = precision;
        }
    }
}

impl BookDecoder {
    pub(crate) fn new(
        rows: &mut Rows,
        instrument_id: InstrumentId,
        date: Date,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            instrument_id,
            date,
            time_exchange: rows.column("time_exchange")?,
            time_coinapi: rows.column("time_coinapi")?,
            update_type: rows.column("update_type")?,
            is_buy: rows.column("is_buy")?,
            entry_px: rows.column("entry_px")?,
            entry_sx: rows.column("entry_sx")?,
            order_id: rows.column("order_id")?,
            orders: AHashMap::new(),
            order_id_last: 0,
            event: Vec::new(),
            event_key: None,
        })
    }

    // Starts the row's event, completing the previous one when the row does not continue it
    fn start_event(
        &mut self,
        is_snapshot: bool,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        out: &mut Vec<OrderBookDelta>,
    ) {
        match self.event_key {
            Some((true, _)) if is_snapshot => return,
            Some((false, key)) if !is_snapshot && key == ts_event => return,
            Some((true, _)) => {
                self.date_snapshot(ts_event);
                self.complete_event(out);
            }
            Some(_) => self.complete_event(out),
            None => {}
        }

        self.event_key = Some((is_snapshot, ts_event));

        if is_snapshot {
            self.orders.clear();
            self.event.push(OrderBookDelta::clear(
                self.instrument_id,
                0,
                ts_event,
                ts_init,
            ));
        }
    }

    // Moves the held snapshot back a day when it is stamped after the first update, since it then
    // carries the previous day's closing time of day
    fn date_snapshot(&mut self, first_update: UnixNanos) {
        let Some((true, snapshot_ts)) = self.event_key else {
            return;
        };

        if first_update < snapshot_ts {
            for delta in &mut self.event {
                delta.ts_event = UnixNanos::from(delta.ts_event.as_u64().saturating_sub(DAY_NANOS));
                delta.ts_init = UnixNanos::from(delta.ts_init.as_u64().saturating_sub(DAY_NANOS));
            }
        }
    }

    fn complete_event(&mut self, out: &mut Vec<OrderBookDelta>) {
        if let Some(last) = self.event.last_mut() {
            last.flags |= RecordFlag::F_LAST as u8;
        }

        out.append(&mut self.event);
    }

    fn decode_level(
        &mut self,
        row: &Row<'_>,
        price: Precision,
        size: Precision,
    ) -> anyhow::Result<()> {
        let action = match row.update_type {
            "SNAPSHOT" if row.size.is_zero() => return Ok(()),
            "SNAPSHOT" => BookAction::Add,
            "DELETE" => BookAction::Delete,
            _ if row.size.is_zero() => BookAction::Delete,
            _ => BookAction::Update,
        };

        let order = BookOrder::new(
            row.side,
            price_at(row.price, price.value, "entry_px")?,
            quantity_at(row.size, size.value, "entry_sx")?,
            0,
        );
        self.push(action, order, row.flags, row.ts_event, row.ts_init)
    }

    fn decode_order(
        &mut self,
        order_id: &str,
        row: &Row<'_>,
        price: Precision,
        size: Precision,
    ) -> anyhow::Result<()> {
        let (resting, remaining) = match (row.update_type, self.orders.get(order_id).copied()) {
            ("SNAPSHOT" | "ADD", None) => return self.add_order(order_id, row, price, size),
            ("SNAPSHOT" | "ADD" | "SET", Some(resting)) => (resting, row.size),
            ("MATCH" | "SUB", Some(resting)) => (resting, resting.size.as_decimal() - row.size),
            ("DELETE", Some(resting)) => (resting, Decimal::ZERO),
            ("DELETE", None) => return Ok(()),
            (update_type, _) => {
                log::warn!("Skipping CoinAPI {update_type} for unknown order {order_id}");
                return Ok(());
            }
        };

        if remaining <= Decimal::ZERO {
            if remaining < Decimal::ZERO {
                log::warn!(
                    "CoinAPI {} of {} exceeds order {order_id} size {}; removing the order",
                    row.update_type,
                    row.size,
                    resting.size
                );
            }

            self.orders.remove(order_id);
            return self.push(
                BookAction::Delete,
                resting,
                row.flags,
                row.ts_event,
                row.ts_init,
            );
        }

        let order = BookOrder::new(
            row.side,
            price_at(row.price, price.value, "entry_px")?,
            quantity_at(remaining, size.value, "entry_sx")?,
            resting.order_id,
        );
        self.orders.insert(order_id.to_owned(), order);
        self.push(
            BookAction::Update,
            order,
            row.flags,
            row.ts_event,
            row.ts_init,
        )
    }

    fn add_order(
        &mut self,
        order_id: &str,
        row: &Row<'_>,
        price: Precision,
        size: Precision,
    ) -> anyhow::Result<()> {
        if row.size <= Decimal::ZERO {
            log::warn!(
                "Skipping CoinAPI {} of non-positive size for {order_id}",
                row.update_type
            );
            return Ok(());
        }

        self.order_id_last += 1;

        let order = BookOrder::new(
            row.side,
            price_at(row.price, price.value, "entry_px")?,
            quantity_at(row.size, size.value, "entry_sx")?,
            self.order_id_last,
        );
        self.orders.insert(order_id.to_owned(), order);
        self.push(BookAction::Add, order, row.flags, row.ts_event, row.ts_init)
    }

    fn push(
        &mut self,
        action: BookAction,
        order: BookOrder,
        flags: u8,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<()> {
        self.event.push(OrderBookDelta::new_checked(
            self.instrument_id,
            action,
            order,
            flags,
            0,
            ts_event,
            ts_init,
        )?);
        Ok(())
    }
}

// The fields of one limit book row
struct Row<'a> {
    update_type: &'a str,
    side: OrderSide,
    price: Decimal,
    size: Decimal,
    flags: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
}
