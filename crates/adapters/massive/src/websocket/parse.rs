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

//! Parsing from Massive WebSocket events into Nautilus domain types.

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick, bar::BarType},
    enums::AggressorSide,
    identifiers::TradeId,
    types::Quantity,
};
use rust_decimal::Decimal;

use crate::{
    common::parse::{
        instrument_id_from_ticker, parse_price, parse_quantity, price_precision,
        shared_price_precision, unix_nanos_from_millis,
    },
    websocket::messages::{MassiveWsAggregate, MassiveWsQuote, MassiveWsTrade},
};

/// Parses a Massive WebSocket trade into a [`TradeTick`].
///
/// Returns `Ok(None)` for events without a positive size.
///
/// # Errors
///
/// Returns an error if a value cannot be represented exactly.
pub fn parse_ws_trade(
    trade: &MassiveWsTrade,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<TradeTick>> {
    let size_decimal = trade.s.unwrap_or(Decimal::ZERO);
    if size_decimal <= Decimal::ZERO {
        return Ok(None);
    }

    let instrument_id = instrument_id_from_ticker(trade.sym.as_str());
    let price = parse_price(trade.p, price_precision(trade.p)?)?;
    let size = parse_quantity(size_decimal)?;
    let trade_id = ws_trade_id(trade)?;
    let ts_event = unix_nanos_from_millis(trade.t)?;
    let ts_init = ts_init.max(ts_event);

    let tick = TradeTick::new_checked(
        instrument_id,
        price,
        size,
        AggressorSide::NoAggressor, // SIP feeds do not identify the aggressor
        trade_id,
        ts_event,
        ts_init,
    )?;
    Ok(Some(tick))
}

fn ws_trade_id(trade: &MassiveWsTrade) -> anyhow::Result<TradeId> {
    if let Some(id) = trade.i.as_deref().filter(|s| !s.is_empty()) {
        return TradeId::new_checked(id).map_err(Into::into);
    }
    let seq = trade
        .q
        .ok_or_else(|| anyhow::anyhow!("Trade has neither `i` nor `q` identifier"))?;
    TradeId::new_checked(format!("{}-{seq}", trade.t)).map_err(Into::into)
}

/// Parses a Massive WebSocket NBBO quote into a [`QuoteTick`].
///
/// Returns `Ok(None)` for one-sided quotes.
///
/// # Errors
///
/// Returns an error if a value cannot be represented exactly.
pub fn parse_ws_quote(
    quote: &MassiveWsQuote,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<QuoteTick>> {
    let (Some(bid_price), Some(ask_price)) = (quote.bp, quote.ap) else {
        return Ok(None);
    };

    if bid_price <= Decimal::ZERO || ask_price <= Decimal::ZERO {
        return Ok(None);
    }

    let instrument_id = instrument_id_from_ticker(quote.sym.as_str());

    let price_prec = shared_price_precision(bid_price, ask_price)?;
    let bid = parse_price(bid_price, price_prec)?;
    let ask = parse_price(ask_price, price_prec)?;

    let bid_size_dec = quote.bs.unwrap_or(Decimal::ZERO);
    let ask_size_dec = quote.ask_size.unwrap_or(Decimal::ZERO);
    let size_scale = (bid_size_dec.scale().max(ask_size_dec.scale())) as u8;
    let bid_size = Quantity::from_decimal_dp(bid_size_dec, size_scale)
        .map_err(|e| anyhow::anyhow!("Failed to parse bid size {bid_size_dec}: {e}"))?;
    let ask_size = Quantity::from_decimal_dp(ask_size_dec, size_scale)
        .map_err(|e| anyhow::anyhow!("Failed to parse ask size {ask_size_dec}: {e}"))?;

    let ts_event = unix_nanos_from_millis(quote.t)?;
    let ts_init = ts_init.max(ts_event);

    let tick = QuoteTick::new_checked(
        instrument_id,
        bid,
        ask,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )?;
    Ok(Some(tick))
}

/// Parses a Massive WebSocket aggregate window into a [`Bar`].
///
/// The wire carries explicit window boundaries; the bar is stamped on the
/// window end when `timestamp_on_close` is true, otherwise on the start.
///
/// # Errors
///
/// Returns an error if prices are inconsistent or timestamps are invalid.
pub fn parse_ws_aggregate(
    agg: &MassiveWsAggregate,
    bar_type: BarType,
    timestamp_on_close: bool,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let precision =
        shared_price_precision(agg.o, agg.h)?.max(shared_price_precision(agg.l, agg.c)?);

    let open = parse_price(agg.o, precision)?;
    let high = parse_price(agg.h, precision)?;
    let low = parse_price(agg.l, precision)?;
    let close = parse_price(agg.c, precision)?;
    let volume = parse_quantity(agg.v)?;

    let ts_millis = if timestamp_on_close { agg.e } else { agg.s };
    let ts_event = unix_nanos_from_millis(ts_millis)?;
    let ts_init = ts_init.max(ts_event);

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::bar::BarSpecification,
        enums::{AggregationSource, BarAggregation, PriceType},
        identifiers::InstrumentId,
        types::Price,
    };
    use rstest::rstest;

    use super::*;
    use crate::{common::testing::load_test_fixture, websocket::messages::MassiveWsEvent};

    fn events_from_fixture(name: &str) -> Vec<MassiveWsEvent> {
        serde_json::from_str(&load_test_fixture(name)).unwrap()
    }

    #[rstest]
    fn test_parse_ws_trade() {
        let events = events_from_fixture("ws_trade.json");
        let MassiveWsEvent::Trade(trade) = &events[0] else {
            panic!("expected Trade");
        };

        let ts_init = UnixNanos::from(1_600_000_000_000_000_000);
        let tick = parse_ws_trade(trade, ts_init).unwrap().unwrap();

        assert_eq!(tick.instrument_id.to_string(), "MSFT.MASSIVE");
        assert_eq!(tick.price, Price::from("114.125"));
        assert_eq!(tick.size, Quantity::from(100));
        assert_eq!(tick.trade_id.to_string(), "12345");
        assert_eq!(tick.aggressor_side, AggressorSide::NoAggressor);
        assert_eq!(tick.ts_event.as_u64(), 1_536_036_818_784_000_000);
    }

    #[rstest]
    fn test_parse_ws_trade_zero_size_skipped() {
        let events = events_from_fixture("ws_trade.json");
        let MassiveWsEvent::Trade(trade) = &events[0] else {
            panic!("expected Trade");
        };
        let mut trade = trade.clone();
        trade.s = Some(Decimal::ZERO);

        assert!(
            parse_ws_trade(&trade, UnixNanos::default())
                .unwrap()
                .is_none()
        );
    }

    #[rstest]
    fn test_parse_ws_trade_synthesizes_id_from_sequence() {
        let events = events_from_fixture("ws_trade.json");
        let MassiveWsEvent::Trade(trade) = &events[0] else {
            panic!("expected Trade");
        };
        let mut trade = trade.clone();
        trade.i = None;

        let tick = parse_ws_trade(&trade, UnixNanos::default())
            .unwrap()
            .unwrap();
        assert_eq!(tick.trade_id.to_string(), "1536036818784-3681328");
    }

    #[rstest]
    fn test_parse_ws_quote() {
        let events = events_from_fixture("ws_quote.json");
        let MassiveWsEvent::Quote(quote) = &events[0] else {
            panic!("expected Quote");
        };

        let ts_init = UnixNanos::from(1_600_000_000_000_000_000);
        let tick = parse_ws_quote(quote, ts_init).unwrap().unwrap();

        assert_eq!(tick.instrument_id.to_string(), "MSFT.MASSIVE");
        assert_eq!(tick.bid_price, Price::from("114.125"));
        assert_eq!(tick.ask_price, Price::from("114.128"));
        assert_eq!(tick.bid_size, Quantity::from(100));
        assert_eq!(tick.ask_size, Quantity::from(160));
        assert_eq!(tick.ts_event.as_u64(), 1_536_036_818_784_000_000);
    }

    #[rstest]
    fn test_parse_ws_quote_one_sided_skipped() {
        let events = events_from_fixture("ws_quote.json");
        let MassiveWsEvent::Quote(quote) = &events[0] else {
            panic!("expected Quote");
        };
        let mut quote = quote.clone();
        quote.ap = None;

        assert!(
            parse_ws_quote(&quote, UnixNanos::default())
                .unwrap()
                .is_none()
        );
    }

    #[rstest]
    fn test_parse_ws_aggregate_second_on_close() {
        let events = events_from_fixture("ws_aggregates.json");
        let MassiveWsEvent::AggregateSecond(agg) = &events[0] else {
            panic!("expected AggregateSecond");
        };

        let bar_type = BarType::new(
            InstrumentId::from("SPCE.MASSIVE"),
            BarSpecification::new(1, BarAggregation::Second, PriceType::Last),
            AggregationSource::External,
        );
        let ts_init = UnixNanos::from(1_700_000_000_000_000_000);

        let bar = parse_ws_aggregate(agg, bar_type, true, ts_init).unwrap();

        assert_eq!(bar.open, Price::from("25.39"));
        assert_eq!(bar.close, Price::from("25.39"));
        assert_eq!(bar.volume, Quantity::from(200));
        // Stamped on window end
        assert_eq!(bar.ts_event.as_u64(), 1_610_144_869_000_000_000);
    }

    #[rstest]
    fn test_parse_ws_aggregate_minute_sub_dollar_precision() {
        let events = events_from_fixture("ws_aggregates.json");
        let MassiveWsEvent::AggregateMinute(agg) = &events[1] else {
            panic!("expected AggregateMinute");
        };

        let bar_type = BarType::new(
            InstrumentId::from("GTE.MASSIVE"),
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );

        let bar = parse_ws_aggregate(agg, bar_type, false, UnixNanos::default()).unwrap();

        assert_eq!(bar.open, Price::from("0.4488"));
        assert_eq!(bar.low, Price::from("0.4486"));
        assert_eq!(bar.open.precision, 4);
        // Stamped on window start
        assert_eq!(bar.ts_event.as_u64(), 1_610_144_640_000_000_000);
    }
}
