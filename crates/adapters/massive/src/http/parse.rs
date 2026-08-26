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

//! Parsing from Massive REST wire models into Nautilus domain types.

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        Bar, QuoteTick, TradeTick,
        bar::{BarSpecification, BarType},
    },
    enums::{AggressorSide, BarAggregation, PriceType},
    identifiers::TradeId,
    instruments::{Equity, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::parse::{
        instrument_id_from_ticker, parse_price, parse_quantity, price_precision,
        shared_price_precision, unix_nanos_from_millis, unix_nanos_from_nanos,
        unix_nanos_from_rfc3339,
    },
    http::models::{MassiveAggBar, MassiveQuote, MassiveTickerInfo, MassiveTrade},
};

/// Default lot size for US equities when the venue does not report one.
const DEFAULT_LOT_SIZE: u64 = 100;

/// Parses a Massive reference ticker into an [`Equity`] instrument.
///
/// # Errors
///
/// Returns an error if instrument construction fails.
pub fn parse_instrument(
    info: &MassiveTickerInfo,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = instrument_id_from_ticker(info.ticker.as_str());

    let currency = info
        .currency_name
        .as_deref()
        .and_then(|c| Currency::try_from_str(&c.to_uppercase()))
        .unwrap_or(Currency::USD());

    // US equities display in cents; sub-penny executions are preserved by
    // trade/quote parsing which uses the natural scale of each wire value.
    let price_increment = Price::from("0.01");

    let lot_size = info
        .round_lot
        .map_or_else(|| Ok(Quantity::from(DEFAULT_LOT_SIZE)), parse_quantity)?;

    let ts_event = info
        .last_updated_utc
        .as_deref()
        .and_then(|s| unix_nanos_from_rfc3339(s).ok())
        .unwrap_or(ts_init);

    let equity = Equity::new_checked(
        instrument_id,
        instrument_id.symbol,
        None, // ISIN not provided
        currency,
        price_increment.precision,
        price_increment,
        Some(lot_size),
        None, // max_quantity
        None, // min_quantity
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        None, // maker_fee
        None, // taker_fee
        None, // tick_scheme
        None, // info
        ts_event,
        ts_init,
    )?;

    Ok(InstrumentAny::Equity(equity))
}

/// Returns the Massive aggregate path segments `(multiplier, timespan)` for a
/// bar specification.
///
/// # Errors
///
/// Returns an error if the aggregation or price type is unsupported.
pub fn bar_spec_to_aggs_params(spec: &BarSpecification) -> anyhow::Result<(usize, &'static str)> {
    anyhow::ensure!(
        spec.price_type == PriceType::Last,
        "Massive only provides LAST price bars, was {}",
        spec.price_type
    );
    let timespan = match spec.aggregation {
        BarAggregation::Second => "second",
        BarAggregation::Minute => "minute",
        BarAggregation::Hour => "hour",
        BarAggregation::Day => "day",
        BarAggregation::Week => "week",
        BarAggregation::Month => "month",
        aggregation => anyhow::bail!("Massive does not support {aggregation} aggregation"),
    };
    Ok((spec.step.get(), timespan))
}

/// Returns the nanosecond duration of one aggregate window for a bar
/// specification (months use a 30-day proxy, matching the core convention).
///
/// # Errors
///
/// Returns an error if the aggregation is unsupported.
pub fn bar_window_nanos(spec: &BarSpecification) -> anyhow::Result<u64> {
    let unit_secs: u64 = match spec.aggregation {
        BarAggregation::Second => 1,
        BarAggregation::Minute => 60,
        BarAggregation::Hour => 3_600,
        BarAggregation::Day => 86_400,
        BarAggregation::Week => 604_800,
        BarAggregation::Month => 2_592_000,
        aggregation => anyhow::bail!("Massive does not support {aggregation} aggregation"),
    };
    Ok(unit_secs * 1_000_000_000 * spec.step.get() as u64)
}

/// Parses a Massive aggregate window into a [`Bar`].
///
/// The wire timestamp marks the window open; when `timestamp_on_close` is
/// true the bar is stamped on the window close (Nautilus convention).
///
/// # Errors
///
/// Returns an error if prices are inconsistent or timestamps are invalid.
pub fn parse_agg_bar(
    bar_type: BarType,
    agg: &MassiveAggBar,
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

    let mut ts_event = unix_nanos_from_millis(agg.t)?;

    if timestamp_on_close {
        let window = bar_window_nanos(&bar_type.spec())?;
        ts_event = UnixNanos::from(
            ts_event
                .as_u64()
                .checked_add(window)
                .ok_or_else(|| anyhow::anyhow!("Bar close timestamp overflow"))?,
        );
    }
    let ts_init = ts_init.max(ts_event);

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
}

/// Parses a Massive historical trade into a [`TradeTick`].
///
/// Returns `Ok(None)` for records without a positive size (e.g. corrections
/// or average-price notations that carry no tradable quantity).
///
/// # Errors
///
/// Returns an error if a value cannot be represented exactly.
pub fn parse_http_trade(
    ticker: &str,
    trade: &MassiveTrade,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<TradeTick>> {
    // Fractional executions report the exact size in `decimal_size`.
    let size_decimal = trade.decimal_size.or(trade.size).unwrap_or(Decimal::ZERO);
    if size_decimal <= Decimal::ZERO {
        return Ok(None);
    }

    let instrument_id = instrument_id_from_ticker(ticker);
    let price = parse_price(trade.price, price_precision(trade.price)?)?;
    let size = parse_quantity(size_decimal)?;
    let trade_id = trade_id_for(trade)?;
    let ts_event = unix_nanos_from_nanos(trade.sip_timestamp)?;
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

fn trade_id_for(trade: &MassiveTrade) -> anyhow::Result<TradeId> {
    if let Some(id) = trade.id.as_deref().filter(|s| !s.is_empty()) {
        return TradeId::new_checked(id).map_err(Into::into);
    }
    // Synthesize a stable ID from the tape sequence when the venue omits one
    let seq = trade
        .sequence_number
        .ok_or_else(|| anyhow::anyhow!("Trade has neither `id` nor `sequence_number`"))?;
    TradeId::new_checked(format!("{}-{seq}", trade.sip_timestamp)).map_err(Into::into)
}

/// Parses a Massive historical NBBO record into a [`QuoteTick`].
///
/// Returns `Ok(None)` for one-sided records, which cannot be represented as
/// a Nautilus quote tick.
///
/// # Errors
///
/// Returns an error if a value cannot be represented exactly.
pub fn parse_http_quote(
    ticker: &str,
    quote: &MassiveQuote,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<QuoteTick>> {
    let (Some(bid_price), Some(ask_price)) = (quote.bid_price, quote.ask_price) else {
        return Ok(None);
    };

    if bid_price <= Decimal::ZERO || ask_price <= Decimal::ZERO {
        return Ok(None);
    }

    let instrument_id = instrument_id_from_ticker(ticker);

    let price_prec = shared_price_precision(bid_price, ask_price)?;
    let bid = parse_price(bid_price, price_prec)?;
    let ask = parse_price(ask_price, price_prec)?;

    let bid_size_dec = quote.bid_size.unwrap_or(Decimal::ZERO);
    let ask_size_dec = quote.ask_size.unwrap_or(Decimal::ZERO);
    let size_scale = (bid_size_dec.scale().max(ask_size_dec.scale())) as u8;
    let bid_size = Quantity::from_decimal_dp(bid_size_dec, size_scale)
        .map_err(|e| anyhow::anyhow!("Failed to parse bid size {bid_size_dec}: {e}"))?;
    let ask_size = Quantity::from_decimal_dp(ask_size_dec, size_scale)
        .map_err(|e| anyhow::anyhow!("Failed to parse ask size {ask_size_dec}: {e}"))?;

    let ts_event = unix_nanos_from_nanos(quote.sip_timestamp)?;
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

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::AggregationSource, identifiers::InstrumentId, instruments::Instrument,
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{common::testing::load_test_fixture, http::models::MassiveTickersResponse};

    fn aapl_bar_type(step: usize, aggregation: BarAggregation) -> BarType {
        BarType::new(
            InstrumentId::from("AAPL.MASSIVE"),
            BarSpecification::new(step, aggregation, PriceType::Last),
            AggregationSource::External,
        )
    }

    #[rstest]
    fn test_parse_instrument() {
        let json = load_test_fixture("http_tickers.json");
        let response: MassiveTickersResponse = serde_json::from_str(&json).unwrap();
        let results = response.results.unwrap();

        let ts_init = UnixNanos::from(2_000_000_000_000_000_000);
        let instrument = parse_instrument(&results[0], ts_init).unwrap();

        assert_eq!(instrument.id().to_string(), "AAPL.MASSIVE");
        assert_eq!(instrument.quote_currency(), Currency::USD());
        assert_eq!(instrument.price_precision(), 2);
        assert_eq!(instrument.price_increment(), Price::from("0.01"));
        assert_eq!(instrument.ts_init(), ts_init);
        // ts_event from last_updated_utc
        assert!(instrument.ts_event() < ts_init);

        let brk = parse_instrument(&results[1], ts_init).unwrap();
        assert_eq!(brk.id().to_string(), "BRK.A.MASSIVE");
    }

    #[rstest]
    #[case(1, BarAggregation::Second, 1, "second")]
    #[case(5, BarAggregation::Minute, 5, "minute")]
    #[case(1, BarAggregation::Hour, 1, "hour")]
    #[case(1, BarAggregation::Day, 1, "day")]
    fn test_bar_spec_to_aggs_params(
        #[case] step: usize,
        #[case] aggregation: BarAggregation,
        #[case] expected_multiplier: usize,
        #[case] expected_timespan: &str,
    ) {
        let spec = BarSpecification::new(step, aggregation, PriceType::Last);
        let (multiplier, timespan) = bar_spec_to_aggs_params(&spec).unwrap();
        assert_eq!(multiplier, expected_multiplier);
        assert_eq!(timespan, expected_timespan);
    }

    #[rstest]
    fn test_bar_spec_rejects_non_last_price_type() {
        let spec = BarSpecification::new(1, BarAggregation::Minute, PriceType::Mid);
        assert!(bar_spec_to_aggs_params(&spec).is_err());
    }

    #[rstest]
    fn test_bar_spec_rejects_tick_aggregation() {
        let spec = BarSpecification::new(100, BarAggregation::Tick, PriceType::Last);
        assert!(bar_spec_to_aggs_params(&spec).is_err());
    }

    #[rstest]
    fn test_parse_agg_bar_timestamp_on_close() {
        let agg = MassiveAggBar {
            o: dec!(74.06),
            h: dec!(75.15),
            l: dec!(73.7975),
            c: dec!(75.0875),
            v: dec!(135647456),
            vw: Some(dec!(74.6099)),
            t: 1_577_941_200_000,
            n: Some(1),
            otc: None,
        };
        let bar_type = aapl_bar_type(1, BarAggregation::Day);
        let ts_init = UnixNanos::from(1_600_000_000_000_000_000);

        let bar = parse_agg_bar(bar_type, &agg, true, ts_init).unwrap();

        assert_eq!(bar.open, Price::from("74.0600"));
        assert_eq!(bar.high, Price::from("75.1500"));
        assert_eq!(bar.low, Price::from("73.7975"));
        assert_eq!(bar.close, Price::from("75.0875"));
        assert_eq!(bar.volume, Quantity::from(135_647_456));
        // 2020-01-02 05:00 UTC open + 1 day
        assert_eq!(bar.ts_event.as_u64(), 1_578_027_600_000_000_000);
    }

    #[rstest]
    fn test_parse_agg_bar_timestamp_on_open() {
        let agg = MassiveAggBar {
            o: dec!(74.06),
            h: dec!(75.15),
            l: dec!(73.79),
            c: dec!(75.08),
            v: dec!(1000),
            vw: None,
            t: 1_577_941_200_000,
            n: None,
            otc: None,
        };
        let bar_type = aapl_bar_type(1, BarAggregation::Day);
        let ts_init = UnixNanos::from(1_600_000_000_000_000_000);

        let bar = parse_agg_bar(bar_type, &agg, false, ts_init).unwrap();
        assert_eq!(bar.ts_event.as_u64(), 1_577_941_200_000_000_000);
        assert_eq!(bar.open, Price::from("74.06"));
    }

    #[rstest]
    fn test_parse_http_trade_whole_shares() {
        let trade = MassiveTrade {
            id: Some("1".to_string()),
            price: dec!(171.55),
            size: Some(dec!(100)),
            decimal_size: None,
            sip_timestamp: 1_517_562_000_016_036_600,
            participant_timestamp: None,
            exchange: Some(11),
            conditions: None,
            sequence_number: Some(1063),
            tape: Some(3),
            correction: None,
        };
        let ts_init = UnixNanos::from(1_600_000_000_000_000_000);

        let tick = parse_http_trade("AAPL", &trade, ts_init).unwrap().unwrap();
        assert_eq!(tick.instrument_id.to_string(), "AAPL.MASSIVE");
        assert_eq!(tick.price, Price::from("171.55"));
        assert_eq!(tick.size, Quantity::from(100));
        assert_eq!(tick.aggressor_side, AggressorSide::NoAggressor);
        assert_eq!(tick.trade_id.to_string(), "1");
        assert_eq!(tick.ts_event.as_u64(), 1_517_562_000_016_036_600);
    }

    #[rstest]
    fn test_parse_http_trade_fractional() {
        let trade = MassiveTrade {
            id: Some("52983575627601".to_string()),
            price: dec!(171.5501),
            size: Some(dec!(0)),
            decimal_size: Some(dec!(0.0406)),
            sip_timestamp: 1_517_562_000_016_038_000,
            participant_timestamp: None,
            exchange: Some(4),
            conditions: None,
            sequence_number: Some(1064),
            tape: Some(3),
            correction: None,
        };
        let ts_init = UnixNanos::from(1_600_000_000_000_000_000);

        let tick = parse_http_trade("AAPL", &trade, ts_init).unwrap().unwrap();
        assert_eq!(tick.price, Price::from("171.5501"));
        assert_eq!(tick.size, Quantity::from("0.0406"));
    }

    #[rstest]
    fn test_parse_http_trade_zero_size_skipped() {
        let trade = MassiveTrade {
            id: Some("x".to_string()),
            price: dec!(171.55),
            size: Some(dec!(0)),
            decimal_size: None,
            sip_timestamp: 1_517_562_000_016_036_600,
            participant_timestamp: None,
            exchange: None,
            conditions: None,
            sequence_number: None,
            tape: None,
            correction: None,
        };
        let ts_init = UnixNanos::default();
        assert!(parse_http_trade("AAPL", &trade, ts_init).unwrap().is_none());
    }

    #[rstest]
    fn test_parse_http_quote() {
        let quote = MassiveQuote {
            bid_price: Some(dec!(102.7)),
            bid_size: Some(dec!(60)),
            bid_exchange: Some(11),
            ask_price: Some(dec!(102.71)),
            ask_size: Some(dec!(60)),
            ask_exchange: Some(12),
            sip_timestamp: 1_517_562_000_065_700_400,
            participant_timestamp: None,
            conditions: None,
            indicators: None,
            sequence_number: Some(2060),
            tape: Some(3),
        };
        let ts_init = UnixNanos::from(1_600_000_000_000_000_000);

        let tick = parse_http_quote("AAPL", &quote, ts_init).unwrap().unwrap();
        assert_eq!(tick.instrument_id.to_string(), "AAPL.MASSIVE");
        assert_eq!(tick.bid_price, Price::from("102.70"));
        assert_eq!(tick.ask_price, Price::from("102.71"));
        assert_eq!(tick.bid_size, Quantity::from(60));
        assert_eq!(tick.ask_size, Quantity::from(60));
        assert_eq!(tick.ts_event.as_u64(), 1_517_562_000_065_700_400);
    }

    #[rstest]
    fn test_parse_http_quote_sub_penny_shared_precision() {
        let quote = MassiveQuote {
            bid_price: Some(dec!(119.99)),
            bid_size: Some(dec!(8)),
            bid_exchange: Some(12),
            ask_price: Some(dec!(120.0048)),
            ask_size: Some(dec!(3)),
            ask_exchange: Some(12),
            sip_timestamp: 1_517_562_000_065_791_500,
            participant_timestamp: None,
            conditions: None,
            indicators: None,
            sequence_number: Some(2061),
            tape: Some(3),
        };
        let ts_init = UnixNanos::default();

        let tick = parse_http_quote("AAPL", &quote, ts_init).unwrap().unwrap();
        assert_eq!(tick.bid_price, Price::from("119.9900"));
        assert_eq!(tick.ask_price, Price::from("120.0048"));
        assert_eq!(tick.bid_price.precision, 4);
    }

    #[rstest]
    fn test_parse_http_quote_one_sided_skipped() {
        let quote = MassiveQuote {
            bid_price: Some(dec!(102.7)),
            bid_size: Some(dec!(60)),
            bid_exchange: Some(11),
            ask_price: None,
            ask_size: None,
            ask_exchange: None,
            sip_timestamp: 1_517_562_000_065_700_400,
            participant_timestamp: None,
            conditions: None,
            indicators: None,
            sequence_number: None,
            tape: None,
        };
        assert!(
            parse_http_quote("AAPL", &quote, UnixNanos::default())
                .unwrap()
                .is_none()
        );
    }
}
