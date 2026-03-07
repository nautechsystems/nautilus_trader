// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use std::{cmp::Ordering, collections::HashMap};

use anyhow::{Context, bail};
use nautilus_core::{datetime::NANOSECONDS_IN_MILLISECOND, nanos::UnixNanos};
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, BookOrder, FundingRateUpdate, IndexPriceUpdate,
        MarkPriceUpdate, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick,
    },
    enums::{
        AggregationSource, AggressorSide, BookAction, OrderSide, RecordFlag,
    },
    identifiers::TradeId,
    instruments::{Instrument, any::InstrumentAny},
    types::{Price, Quantity},
};
use nautilus_model::data::bar::{
    BAR_SPEC_1_DAY_LAST, BAR_SPEC_1_HOUR_LAST, BAR_SPEC_1_MINUTE_LAST, BAR_SPEC_1_MONTH_LAST,
    BAR_SPEC_1_SECOND_LAST, BAR_SPEC_1_WEEK_LAST, BAR_SPEC_12_HOUR_LAST, BAR_SPEC_12_MONTH_LAST,
    BAR_SPEC_15_MINUTE_LAST, BAR_SPEC_2_DAY_LAST, BAR_SPEC_2_HOUR_LAST, BAR_SPEC_30_MINUTE_LAST,
    BAR_SPEC_3_DAY_LAST, BAR_SPEC_3_MINUTE_LAST, BAR_SPEC_3_MONTH_LAST, BAR_SPEC_4_HOUR_LAST,
    BAR_SPEC_5_DAY_LAST, BAR_SPEC_5_MINUTE_LAST, BAR_SPEC_6_HOUR_LAST, BAR_SPEC_6_MONTH_LAST,
};
use rust_decimal::Decimal;
#[cfg(feature = "python")]
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
#[cfg(feature = "python")]
use nautilus_model::{
    data::{Data, OrderBookDeltas_API},
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
};
#[cfg(feature = "python")]
use pyo3::prelude::*;

use crate::websocket::messages::{
    BitgetWsArg, BitgetWsBookData, BitgetWsBookMessage, BitgetWsCandle, BitgetWsCandleMessage,
    BitgetWsEvent, BitgetWsTickerData, BitgetWsTickerMessage, BitgetWsTrade, BitgetWsTradeMessage,
};

#[derive(Debug, Default, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bitget")
)]
pub struct BitgetBookState {
    bids: HashMap<String, String>,
    asks: HashMap<String, String>,
    last_seq: Option<i64>,
}

impl BitgetBookState {
    #[must_use]
    pub fn last_seq(&self) -> Option<i64> {
        self.last_seq
    }

    pub fn apply_snapshot(&mut self, book: &BitgetWsBookData) -> anyhow::Result<()> {
        self.bids.clear();
        self.asks.clear();
        apply_levels(&mut self.bids, &book.bids);
        apply_levels(&mut self.asks, &book.asks);
        self.validate_checksum(book)?;
        self.last_seq = Some(book.seq);
        Ok(())
    }

    pub fn apply_update(&mut self, book: &BitgetWsBookData) -> anyhow::Result<()> {
        let Some(last_seq) = self.last_seq else {
            bail!("Bitget book update received before initial snapshot");
        };

        if book.seq <= last_seq {
            bail!(
                "Bitget book sequence out of order: previous seq={last_seq}, current seq={}",
                book.seq
            );
        }

        apply_levels(&mut self.bids, &book.bids);
        apply_levels(&mut self.asks, &book.asks);
        self.validate_checksum(book)?;
        self.last_seq = Some(book.seq);
        Ok(())
    }

    pub fn reset(&mut self) {
        self.bids.clear();
        self.asks.clear();
        self.last_seq = None;
    }

    #[must_use]
    pub fn checksum_string(&self) -> String {
        build_checksum_string(&sorted_levels(&self.bids, true), &sorted_levels(&self.asks, false))
    }

    #[must_use]
    pub fn checksum(&self) -> i32 {
        crc32fast::hash(self.checksum_string().as_bytes()) as i32
    }

    #[must_use]
    pub fn best_bid(&self) -> Option<(String, String)> {
        sorted_levels(&self.bids, true)
            .into_iter()
            .next()
    }

    #[must_use]
    pub fn best_ask(&self) -> Option<(String, String)> {
        sorted_levels(&self.asks, false)
            .into_iter()
            .next()
    }

    fn validate_checksum(&self, book: &BitgetWsBookData) -> anyhow::Result<()> {
        let expected = self.checksum();
        if expected != book.checksum {
            bail!(
                "Bitget checksum mismatch: expected {expected}, received {}",
                book.checksum
            );
        }

        Ok(())
    }
}

fn apply_levels(levels: &mut HashMap<String, String>, updates: &[[String; 2]]) {
    for [price, size] in updates {
        if is_zero(size) {
            levels.remove(price);
        } else {
            levels.insert(price.clone(), size.clone());
        }
    }
}

fn is_zero(value: &str) -> bool {
    value
        .trim()
        .parse::<f64>()
        .map_or(false, |parsed| parsed == 0.0)
}

fn sort_key(price: &str) -> f64 {
    price.parse::<f64>().unwrap_or(0.0)
}

fn sorted_levels(levels: &HashMap<String, String>, descending: bool) -> Vec<(String, String)> {
    let mut values: Vec<_> = levels
        .iter()
        .map(|(price, size)| (price.clone(), size.clone()))
        .collect();
    values.sort_by(|lhs, rhs| {
        let lhs = sort_key(&lhs.0);
        let rhs = sort_key(&rhs.0);
        if descending {
            rhs.partial_cmp(&lhs).unwrap_or(Ordering::Equal)
        } else {
            lhs.partial_cmp(&rhs).unwrap_or(Ordering::Equal)
        }
    });
    values.truncate(25);
    values
}

fn build_checksum_string(bids: &[(String, String)], asks: &[(String, String)]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let max_len = bids.len().max(asks.len());

    for idx in 0..max_len {
        if let Some((price, size)) = bids.get(idx) {
            parts.push(format!("{price}:{size}"));
        }
        if let Some((price, size)) = asks.get(idx) {
            parts.push(format!("{price}:{size}"));
        }
    }

    parts.join(":")
}

fn parse_ts_millis(value: &str, field: &str) -> anyhow::Result<UnixNanos> {
    let millis = value
        .parse::<u64>()
        .with_context(|| format!("invalid Bitget timestamp in {field}"))?;
    let nanos = millis
        .checked_mul(NANOSECONDS_IN_MILLISECOND)
        .with_context(|| format!("timestamp overflow in {field}"))?;
    Ok(UnixNanos::from(nanos))
}

pub fn parse_ws_event(input: &str) -> serde_json::Result<BitgetWsEvent> {
    serde_json::from_str::<BitgetWsEvent>(input)
}

pub fn parse_public_book(input: &str) -> serde_json::Result<BitgetWsBookMessage> {
    serde_json::from_str::<BitgetWsBookMessage>(input)
}

pub fn parse_public_trades(input: &str) -> serde_json::Result<BitgetWsTradeMessage> {
    serde_json::from_str::<BitgetWsTradeMessage>(input)
}

pub fn parse_public_ticker(input: &str) -> serde_json::Result<BitgetWsTickerMessage> {
    serde_json::from_str::<BitgetWsTickerMessage>(input)
}

pub fn parse_public_candle(input: &str) -> serde_json::Result<BitgetWsCandleMessage> {
    serde_json::from_str::<BitgetWsCandleMessage>(input)
}

pub fn parse_public_quote_tick(
    instrument: &InstrumentAny,
    _arg: &BitgetWsArg,
    ticker: &BitgetWsTickerData,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let bid_price = ticker
        .bid_price
        .as_deref()
        .context("Bitget ticker missing bid price")?;
    let ask_price = ticker
        .ask_price
        .as_deref()
        .context("Bitget ticker missing ask price")?;

    let bid_size = ticker.bid_size.as_deref().unwrap_or("0");
    let ask_size = ticker.ask_size.as_deref().unwrap_or("0");
    let ts_event = parse_public_ticker_ts(&ticker.ts, ts_init).context("invalid ticker timestamp")?;
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };

    QuoteTick::new_checked(
        instrument.id(),
        Price::from(bid_price),
        Price::from(ask_price),
        Quantity::from(bid_size),
        Quantity::from(ask_size),
        ts_event,
        ts_init,
    )
    .context("failed to construct Bitget QuoteTick")
}

pub fn parse_public_mark_price(
    instrument: &InstrumentAny,
    _arg: &BitgetWsArg,
    ticker: &BitgetWsTickerData,
    ts_init: UnixNanos,
) -> anyhow::Result<MarkPriceUpdate> {
    let mark_price = ticker
        .mark_price
        .as_deref()
        .context("Bitget ticker missing mark price")?;
    let ts_event = parse_public_ticker_ts(&ticker.ts, ts_init).context("invalid ticker timestamp")?;
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };

    Ok(MarkPriceUpdate::new(instrument.id(), Price::from(mark_price), ts_event, ts_init))
}

pub fn parse_public_index_price(
    instrument: &InstrumentAny,
    _arg: &BitgetWsArg,
    ticker: &BitgetWsTickerData,
    ts_init: UnixNanos,
) -> anyhow::Result<IndexPriceUpdate> {
    let index_price = ticker
        .index_price
        .as_deref()
        .context("Bitget ticker missing index price")?;
    let ts_event = parse_public_ticker_ts(&ticker.ts, ts_init).context("invalid ticker timestamp")?;
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };

    Ok(IndexPriceUpdate::new(instrument.id(), Price::from(index_price), ts_event, ts_init))
}

pub fn parse_public_funding_rate(
    instrument: &InstrumentAny,
    _arg: &BitgetWsArg,
    ticker: &BitgetWsTickerData,
    ts_init: UnixNanos,
) -> anyhow::Result<FundingRateUpdate> {
    let funding_rate = ticker
        .funding_rate
        .as_deref()
        .context("Bitget ticker missing funding rate")?;
    let ts_event = parse_public_ticker_ts(&ticker.ts, ts_init).context("invalid ticker timestamp")?;
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };

    let rate = funding_rate
        .parse::<Decimal>()
        .with_context(|| format!("invalid funding rate '{funding_rate}'"))?;

    let next_funding_ns = match ticker.next_funding_time.as_deref() {
        Some(v) => Some(parse_ts_millis(v, "ticker.nextFundingTime").context("invalid next funding time")?),
        None => None,
    };

    Ok(FundingRateUpdate::new(
        instrument.id(),
        rate,
        next_funding_ns,
        ts_event,
        ts_init,
    ))
}

pub fn parse_public_bars(
    msg: &BitgetWsCandleMessage,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<Bar>> {
    let bar_spec = bitget_channel_to_bar_spec(&msg.arg.channel)
        .with_context(|| format!("unsupported candle interval '{}':", msg.arg.channel))?;
    let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::External);

    let ts_init = ts_init;
    let mut bars = Vec::with_capacity(msg.data.len());

    for candle in &msg.data {
        bars.push(parse_public_bar(candle, bar_type, ts_init)?);
    }

    Ok(bars)
}

fn parse_public_bar(
    candle: &BitgetWsCandle,
    bar_type: BarType,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let open = candle
        .get(1)
        .context("Bitget candle missing open")?
        .as_str();
    let high = candle
        .get(2)
        .context("Bitget candle missing high")?
        .as_str();
    let low = candle
        .get(3)
        .context("Bitget candle missing low")?
        .as_str();
    let close = candle
        .get(4)
        .context("Bitget candle missing close")?
        .as_str();
    let volume = candle
        .get(5)
        .context("Bitget candle missing volume")?
        .as_str();

    let ts_event = parse_ts_millis(
        candle
            .first()
            .context("Bitget candle missing timestamp")?
            .as_str(),
        "candle.ts",
    )?;
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };

    Bar::new_checked(
        bar_type,
        Price::from(open),
        Price::from(high),
        Price::from(low),
        Price::from(close),
        Quantity::from(volume),
        ts_event,
        ts_init,
    )
    .context("failed to construct Bitget Bar")
}

fn parse_public_ticker_ts(
    value: &Option<String>,
    ts_init: UnixNanos,
) -> anyhow::Result<UnixNanos> {
    match value.as_deref() {
        Some(v) => parse_ts_millis(v, "ticker.ts"),
        None if ts_init.is_zero() => bail!("Bitget ticker missing ts"),
        None => Ok(ts_init),
    }
}

fn bitget_channel_to_bar_spec(channel: &str) -> Option<BarSpecification> {
    let channel = channel.strip_prefix("candle").unwrap_or(channel);

    match channel {
        "1s" => Some(BAR_SPEC_1_SECOND_LAST),
        "1m" => Some(BAR_SPEC_1_MINUTE_LAST),
        "3m" => Some(BAR_SPEC_3_MINUTE_LAST),
        "5m" => Some(BAR_SPEC_5_MINUTE_LAST),
        "15m" => Some(BAR_SPEC_15_MINUTE_LAST),
        "30m" => Some(BAR_SPEC_30_MINUTE_LAST),
        "1H" => Some(BAR_SPEC_1_HOUR_LAST),
        "2H" => Some(BAR_SPEC_2_HOUR_LAST),
        "4H" => Some(BAR_SPEC_4_HOUR_LAST),
        "6H" => Some(BAR_SPEC_6_HOUR_LAST),
        "12H" => Some(BAR_SPEC_12_HOUR_LAST),
        "1D" => Some(BAR_SPEC_1_DAY_LAST),
        "2D" => Some(BAR_SPEC_2_DAY_LAST),
        "3D" => Some(BAR_SPEC_3_DAY_LAST),
        "5D" => Some(BAR_SPEC_5_DAY_LAST),
        "1W" => Some(BAR_SPEC_1_WEEK_LAST),
        "1M" => Some(BAR_SPEC_1_MONTH_LAST),
        "3M" => Some(BAR_SPEC_3_MONTH_LAST),
        "6M" => Some(BAR_SPEC_6_MONTH_LAST),
        "1h" => Some(BAR_SPEC_1_HOUR_LAST),
        "2h" => Some(BAR_SPEC_2_HOUR_LAST),
        "4h" => Some(BAR_SPEC_4_HOUR_LAST),
        "6h" => Some(BAR_SPEC_6_HOUR_LAST),
        "12h" => Some(BAR_SPEC_12_HOUR_LAST),
        "1d" => Some(BAR_SPEC_1_DAY_LAST),
        "2d" => Some(BAR_SPEC_2_DAY_LAST),
        "3d" => Some(BAR_SPEC_3_DAY_LAST),
        "5d" => Some(BAR_SPEC_5_DAY_LAST),
        "1w" => Some(BAR_SPEC_1_WEEK_LAST),
        "1y" => Some(BAR_SPEC_12_MONTH_LAST),
        _ => None,
    }
}

pub fn parse_public_trade_tick(
    trade: &BitgetWsTrade,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = Price::from(trade.price.as_str());
    let size = Quantity::from(trade.size.as_str());
    let aggressor = match trade.side.to_ascii_lowercase().as_str() {
        "buy" => AggressorSide::Buyer,
        "sell" => AggressorSide::Seller,
        _ => AggressorSide::NoAggressor,
    };
    let trade_id = TradeId::new_checked(trade.trade_id.as_str())
        .context("invalid Bitget trade identifier")?;
    let ts_event = parse_ts_millis(&trade.ts, "trade.ts")?;
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("failed to construct Bitget TradeTick")
}

pub fn parse_public_book_deltas(
    msg: &BitgetWsBookMessage,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let book = msg
        .data
        .first()
        .context("Bitget public book message missing data payload")?;
    let is_snapshot = msg.action.eq_ignore_ascii_case("snapshot");
    let ts_event = parse_ts_millis(&book.ts, "book.ts")?;
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };
    let update_id = u64::try_from(book.seq).context("negative Bitget book seq")?;
    let sequence = update_id;

    let total_levels = book.bids.len() + book.asks.len();
    let mut deltas = Vec::with_capacity(if is_snapshot {
        total_levels + 1
    } else {
        total_levels
    });

    if is_snapshot {
        deltas.push(OrderBookDelta::clear(
            instrument.id(),
            sequence,
            ts_event,
            ts_init,
        ));
    }

    let mut processed = 0_usize;
    let mut push_level = |level: &[String; 2], side: OrderSide| -> anyhow::Result<()> {
        let price = Price::from(level[0].as_str());
        let size = Quantity::from(level[1].as_str());
        let action = if size.is_zero() {
            BookAction::Delete
        } else if is_snapshot {
            BookAction::Add
        } else {
            BookAction::Update
        };

        processed += 1;
        let mut flags = RecordFlag::F_MBP as u8;
        if processed == total_levels {
            flags |= RecordFlag::F_LAST as u8;
        }

        let order = BookOrder::new(side, price, size, update_id);
        let delta = OrderBookDelta::new_checked(
            instrument.id(),
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        )
        .context("failed to construct Bitget OrderBookDelta")?;
        deltas.push(delta);
        Ok(())
    };

    for level in &book.bids {
        push_level(level, OrderSide::Buy)?;
    }
    for level in &book.asks {
        push_level(level, OrderSide::Sell)?;
    }

    if total_levels == 0
        && let Some(last) = deltas.last_mut()
    {
        last.flags |= RecordFlag::F_LAST as u8;
    }

    OrderBookDeltas::new_checked(instrument.id(), deltas)
        .context("failed to assemble Bitget OrderBookDeltas")
}

#[cfg(feature = "python")]
#[pyo3::pymethods]
impl BitgetBookState {
    #[new]
    fn py_new() -> Self {
        Self::default()
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }

    #[pyo3(name = "apply_message")]
    #[pyo3(signature = (input, instrument, ts_init = None))]
    fn py_apply_message(
        &mut self,
        py: Python<'_>,
        input: &str,
        instrument: Py<PyAny>,
        ts_init: Option<u64>,
    ) -> PyResult<Py<PyAny>> {
        let msg = parse_public_book(input).map_err(to_pyvalue_err)?;
        let book = msg
            .data
            .first()
            .ok_or_else(|| to_pyruntime_err("Bitget public book message missing data payload"))?;

        if msg.action.eq_ignore_ascii_case("snapshot") {
            self.apply_snapshot(book).map_err(to_pyruntime_err)?;
        } else if msg.action.eq_ignore_ascii_case("update") {
            self.apply_update(book).map_err(to_pyruntime_err)?;
        } else {
            return Err(to_pyvalue_err(format!(
                "Unsupported Bitget public book action: {}",
                msg.action
            )));
        }

        let instrument = pyobject_to_instrument_any(py, instrument)?;
        let deltas = parse_public_book_deltas(
            &msg,
            &instrument,
            UnixNanos::from(ts_init.unwrap_or_default()),
        )
        .map_err(to_pyruntime_err)?;

        Ok(data_to_pycapsule(
            py,
            Data::Deltas(OrderBookDeltas_API::new(deltas)),
        ))
    }
}
