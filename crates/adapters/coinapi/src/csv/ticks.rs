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

//! `T-TRADES` and `T-QUOTES` rows.

use csv::StringRecord;
use nautilus_model::{
    data::{QuoteTick, TradeTick},
    identifiers::{InstrumentId, TradeId},
};
use ustr::Ustr;

use super::{
    Decode,
    rows::{Precision, Rows, date_time, number, text},
};
use crate::websocket::parse::{parse_aggressor_side, price_at, quantity_at};

/// Converts `T-TRADES` rows to [`TradeTick`]s, skipping zero-size trades.
#[derive(Debug)]
pub(crate) struct TradeDecoder {
    instrument_id: InstrumentId,
    time_exchange: usize,
    time_coinapi: usize,
    guid: usize,
    price: usize,
    base_amount: usize,
    taker_side: usize,
}

impl TradeDecoder {
    pub(crate) fn new(rows: &mut Rows, instrument_id: InstrumentId) -> anyhow::Result<Self> {
        Ok(Self {
            instrument_id,
            time_exchange: rows.column("time_exchange")?,
            time_coinapi: rows.column("time_coinapi")?,
            guid: rows.column("guid")?,
            price: rows.column("price")?,
            base_amount: rows.column("base_amount")?,
            taker_side: rows.column("taker_side")?,
        })
    }
}

impl Decode for TradeDecoder {
    type Output = TradeTick;

    fn fit(
        &self,
        record: &StringRecord,
        price: &mut Precision,
        size: &mut Precision,
    ) -> anyhow::Result<()> {
        price.fit(number(record, self.price, "price")?);
        size.fit(number(record, self.base_amount, "base_amount")?);
        Ok(())
    }

    fn decode(
        &mut self,
        record: &StringRecord,
        price: &mut Precision,
        size: &mut Precision,
        out: &mut Vec<TradeTick>,
    ) -> anyhow::Result<()> {
        let trade_price = number(record, self.price, "price")?;
        let trade_size = number(record, self.base_amount, "base_amount")?;

        if trade_size.is_zero() {
            log::warn!(
                "Skipping zero-size CoinAPI trade {}",
                text(record, self.guid)?
            );
            return Ok(());
        }

        price.fit(trade_price);
        size.fit(trade_size);

        out.push(TradeTick::new_checked(
            self.instrument_id,
            price_at(trade_price, price.value, "price")?,
            quantity_at(trade_size, size.value, "base_amount")?,
            parse_aggressor_side(Ustr::from(text(record, self.taker_side)?)),
            TradeId::new_checked(text(record, self.guid)?)?,
            date_time(text(record, self.time_exchange)?, "time_exchange")?,
            date_time(text(record, self.time_coinapi)?, "time_coinapi")?,
        )?);
        Ok(())
    }

    fn set_precision(value: &mut TradeTick, price: Option<u8>, size: Option<u8>) {
        if let Some(precision) = price {
            value.price.precision = precision;
        }

        if let Some(precision) = size {
            value.size.precision = precision;
        }
    }
}

/// Converts `T-QUOTES` rows to [`QuoteTick`]s.
#[derive(Debug)]
pub(crate) struct QuoteDecoder {
    instrument_id: InstrumentId,
    time_exchange: usize,
    time_coinapi: usize,
    ask_px: usize,
    ask_sx: usize,
    bid_px: usize,
    bid_sx: usize,
}

impl QuoteDecoder {
    pub(crate) fn new(rows: &mut Rows, instrument_id: InstrumentId) -> anyhow::Result<Self> {
        Ok(Self {
            instrument_id,
            time_exchange: rows.column("time_exchange")?,
            time_coinapi: rows.column("time_coinapi")?,
            ask_px: rows.column("ask_px")?,
            ask_sx: rows.column("ask_sx")?,
            bid_px: rows.column("bid_px")?,
            bid_sx: rows.column("bid_sx")?,
        })
    }
}

impl Decode for QuoteDecoder {
    type Output = QuoteTick;

    fn fit(
        &self,
        record: &StringRecord,
        price: &mut Precision,
        size: &mut Precision,
    ) -> anyhow::Result<()> {
        price.fit(number(record, self.bid_px, "bid_px")?);
        price.fit(number(record, self.ask_px, "ask_px")?);
        size.fit(number(record, self.bid_sx, "bid_sx")?);
        size.fit(number(record, self.ask_sx, "ask_sx")?);
        Ok(())
    }

    fn decode(
        &mut self,
        record: &StringRecord,
        price: &mut Precision,
        size: &mut Precision,
        out: &mut Vec<QuoteTick>,
    ) -> anyhow::Result<()> {
        let bid_price = number(record, self.bid_px, "bid_px")?;
        let ask_price = number(record, self.ask_px, "ask_px")?;
        let bid_size = number(record, self.bid_sx, "bid_sx")?;
        let ask_size = number(record, self.ask_sx, "ask_sx")?;

        price.fit(bid_price);
        price.fit(ask_price);
        size.fit(bid_size);
        size.fit(ask_size);

        out.push(QuoteTick::new_checked(
            self.instrument_id,
            price_at(bid_price, price.value, "bid_px")?,
            price_at(ask_price, price.value, "ask_px")?,
            quantity_at(bid_size, size.value, "bid_sx")?,
            quantity_at(ask_size, size.value, "ask_sx")?,
            date_time(text(record, self.time_exchange)?, "time_exchange")?,
            date_time(text(record, self.time_coinapi)?, "time_coinapi")?,
        )?);
        Ok(())
    }

    fn set_precision(value: &mut QuoteTick, price: Option<u8>, size: Option<u8>) {
        if let Some(precision) = price {
            value.bid_price.precision = precision;
            value.ask_price.precision = precision;
        }

        if let Some(precision) = size {
            value.bid_size.precision = precision;
            value.ask_size.precision = precision;
        }
    }
}
