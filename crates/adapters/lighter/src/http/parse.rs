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

//! Parsers from Lighter REST payloads to Nautilus domain types.

use anyhow::Context;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        Bar, BarType, BookOrder, FundingRateUpdate, OrderBookDelta, OrderBookDeltas, TradeTick,
    },
    enums::{AggressorSide, BookAction, BookType, OrderSide, RecordFlag},
    identifiers::{InstrumentId, Symbol, TradeId},
    instruments::{CryptoPerpetual, CurrencyPair, Instrument, InstrumentAny},
    orderbook::OrderBook,
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::{
        enums::LighterMarketStatus,
        parse::{
            parse_millis_to_nanos, parse_secs_to_nanos, price_from_decimal, quantity_from_decimal,
        },
        symbol::MarketRegistry,
    },
    http::models::{
        LighterCandle, LighterFunding, LighterFundingDirection, LighterOrderBook,
        LighterOrderBookOrders, LighterPerpOrderBookDetail, LighterSimpleOrder,
        LighterSpotOrderBookDetail, LighterTrade,
    },
};

pub fn register_order_books(registry: &MarketRegistry, order_books: &[LighterOrderBook]) {
    for order_book in order_books {
        register_order_book(registry, order_book);
    }
}

pub fn register_perp_order_book_details(
    registry: &MarketRegistry,
    details: &[LighterPerpOrderBookDetail],
) {
    for detail in details {
        register_order_book(registry, &detail.order_book);
    }
}

pub fn register_spot_order_book_details(
    registry: &MarketRegistry,
    details: &[LighterSpotOrderBookDetail],
) {
    for detail in details {
        register_order_book(registry, &detail.order_book);
    }
}

/// Parses Lighter order book metadata into Nautilus instruments and registers their market ids.
///
/// # Errors
///
/// Returns an error if an instrument definition cannot be converted.
pub fn parse_order_book_details_instruments(
    registry: &MarketRegistry,
    perp_details: &[LighterPerpOrderBookDetail],
    spot_details: &[LighterSpotOrderBookDetail],
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<InstrumentAny>> {
    parse_order_book_details_instruments_with_status(registry, perp_details, spot_details, ts_init)
        .map(|instruments| {
            instruments
                .into_iter()
                .map(|(instrument, _)| instrument)
                .collect()
        })
}

/// Parses Lighter order book metadata into Nautilus instruments and market statuses.
///
/// # Errors
///
/// Returns an error if an instrument definition cannot be converted.
pub fn parse_order_book_details_instruments_with_status(
    registry: &MarketRegistry,
    perp_details: &[LighterPerpOrderBookDetail],
    spot_details: &[LighterSpotOrderBookDetail],
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<(InstrumentAny, LighterMarketStatus)>> {
    let mut instruments = Vec::with_capacity(perp_details.len() + spot_details.len());

    for detail in perp_details {
        instruments.push((
            parse_perp_instrument(registry, detail, ts_init)?,
            detail.order_book.status,
        ));
    }

    for detail in spot_details {
        instruments.push((
            parse_spot_instrument(registry, detail, ts_init)?,
            detail.order_book.status,
        ));
    }

    Ok(instruments)
}

/// Parses a Lighter trade into a Nautilus [`TradeTick`].
///
/// # Errors
///
/// Returns an error if the price, size, timestamp, or trade id is invalid.
pub fn parse_trade_tick(
    trade: &LighterTrade,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = price_from_decimal(trade.price, instrument.price_precision())?;
    let size = quantity_from_decimal(trade.size, instrument.size_precision())?;
    let aggressor_side = aggressor_side_from_is_maker_ask(trade.is_maker_ask);
    let trade_id = match trade.trade_id_str.as_deref() {
        Some(s) => TradeId::new_checked(s),
        None => TradeId::new_checked(trade.trade_id.to_string()),
    }
    .context("invalid Lighter trade identifier")?;
    let timestamp_ms =
        u64::try_from(trade.timestamp).context("negative Lighter trade timestamp")?;
    let ts_event = parse_millis_to_nanos(timestamp_ms)?;

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("failed to construct TradeTick from Lighter trade")
}

/// Parses a Lighter candle into a Nautilus [`Bar`].
///
/// # Errors
///
/// Returns an error if any price, volume, or timestamp field cannot be converted.
pub fn parse_candle_bar(
    candle: &LighterCandle,
    bar_type: BarType,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let timestamp_ms =
        u64::try_from(candle.timestamp).context("negative Lighter candle timestamp")?;
    let ts_event = parse_millis_to_nanos(timestamp_ms)?;
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let open = Price::from_decimal_dp(candle.open, price_precision)
        .map_err(|e| anyhow::anyhow!("invalid candle open: {e}"))?;
    let high = Price::from_decimal_dp(candle.high, price_precision)
        .map_err(|e| anyhow::anyhow!("invalid candle high: {e}"))?;
    let low = Price::from_decimal_dp(candle.low, price_precision)
        .map_err(|e| anyhow::anyhow!("invalid candle low: {e}"))?;
    let close = Price::from_decimal_dp(candle.close, price_precision)
        .map_err(|e| anyhow::anyhow!("invalid candle close: {e}"))?;
    anyhow::ensure!(
        candle.volume_base.is_sign_positive(),
        "negative candle volume `{}`",
        candle.volume_base,
    );
    let volume = Quantity::from_decimal_dp(candle.volume_base, size_precision)
        .map_err(|e| anyhow::anyhow!("invalid candle volume: {e}"))?;

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
        .context("failed to construct Bar from Lighter candle")
}

/// Parses a Lighter historical funding row into a Nautilus [`FundingRateUpdate`].
///
/// Lighter returns `rate` as a magnitude and `direction` as the side paying
/// the funding. Nautilus uses the conventional signed rate: positive when
/// longs pay shorts and negative when shorts pay longs.
///
/// # Errors
///
/// Returns an error if the timestamp cannot be converted.
pub fn parse_funding_rate_update(
    funding: &LighterFunding,
    instrument_id: InstrumentId,
    interval: Option<u16>,
    ts_init: UnixNanos,
) -> anyhow::Result<FundingRateUpdate> {
    let timestamp =
        u64::try_from(funding.timestamp).context("negative Lighter funding timestamp")?;
    let ts_event = parse_secs_to_nanos(timestamp)?;
    let rate = match funding.direction {
        LighterFundingDirection::Long => funding.rate,
        LighterFundingDirection::Short => -funding.rate,
    };

    Ok(FundingRateUpdate::new(
        instrument_id,
        rate,
        interval,
        None,
        ts_event,
        ts_init,
    ))
}

/// Parses an HTTP order book snapshot response into Nautilus order book deltas.
///
/// # Errors
///
/// Returns an error if any price or size cannot be converted.
pub fn parse_order_book_snapshot(
    snapshot: &LighterOrderBookOrders,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let total_levels = snapshot.bids.len() + snapshot.asks.len();
    let mut deltas = Vec::with_capacity(total_levels + 1);
    let mut clear = OrderBookDelta::clear(instrument_id, 0, ts_event, ts_init);

    if total_levels == 0 {
        clear.flags |= RecordFlag::F_LAST as u8;
    }
    deltas.push(clear);

    let mut processed = 0_usize;

    for bid in &snapshot.bids {
        let price = price_from_decimal(bid.price, price_precision)?;
        let size = quantity_from_decimal(bid.remaining_base_amount, size_precision)?;
        let order_id =
            u64::try_from(bid.order_index).context("negative Lighter bid order index")?;
        let order = BookOrder::new(OrderSide::Buy, price, size, order_id);
        processed += 1;
        let sequence = processed as u64;
        let delta = OrderBookDelta::new_checked(
            instrument_id,
            BookAction::Add,
            order,
            snapshot_flags(processed, total_levels),
            sequence,
            ts_event,
            ts_init,
        )
        .context("failed to construct Lighter bid snapshot delta")?;
        deltas.push(delta);
    }

    for ask in &snapshot.asks {
        let price = price_from_decimal(ask.price, price_precision)?;
        let size = quantity_from_decimal(ask.remaining_base_amount, size_precision)?;
        let order_id =
            u64::try_from(ask.order_index).context("negative Lighter ask order index")?;
        let order = BookOrder::new(OrderSide::Sell, price, size, order_id);
        processed += 1;
        let sequence = processed as u64;
        let delta = OrderBookDelta::new_checked(
            instrument_id,
            BookAction::Add,
            order,
            snapshot_flags(processed, total_levels),
            sequence,
            ts_event,
            ts_init,
        )
        .context("failed to construct Lighter ask snapshot delta")?;
        deltas.push(delta);
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
        .context("failed to construct OrderBookDeltas from Lighter order book snapshot")
}

/// Parses an HTTP `orderBookOrders` snapshot into an aggregated [`OrderBook`].
///
/// The REST endpoint returns each resting order separately, while WebSocket
/// `order_book` updates carry already-aggregated price levels. To keep
/// snapshot semantics consistent with the live feed, sizes are summed per
/// price on each side and added as a single L2 entry per level.
///
/// The snapshot itself carries no venue timestamp (Lighter's `LighterSimpleOrder`
/// `transaction_time` is `0` for resting orders), so the constructed book's
/// `ts_last` is left at [`UnixNanos::default`]. The first WebSocket delta
/// applied after the snapshot will install a real venue timestamp and avoid
/// spurious "out-of-order" warnings against a wall-clock placeholder.
#[must_use]
pub fn parse_l2_order_book_snapshot(
    snapshot: &LighterOrderBookOrders,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
) -> OrderBook {
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
    let mut sequence: u64 = 0;
    let ts_event = UnixNanos::default();

    let bid_levels = aggregate_order_levels(&snapshot.bids, price_precision, size_precision);
    let ask_levels = aggregate_order_levels(&snapshot.asks, price_precision, size_precision);

    sequence += 1;
    book.clear(sequence, ts_event);

    for (price, size) in bid_levels {
        sequence += 1;
        let order = BookOrder::new(OrderSide::Buy, price, size, 0);
        book.add(order, BookAction::Add as u8, sequence, ts_event);
    }

    for (price, size) in ask_levels {
        sequence += 1;
        let order = BookOrder::new(OrderSide::Sell, price, size, 0);
        book.add(order, BookAction::Add as u8, sequence, ts_event);
    }

    book
}

fn aggregate_order_levels(
    orders: &[LighterSimpleOrder],
    price_precision: u8,
    size_precision: u8,
) -> Vec<(Price, Quantity)> {
    use std::collections::BTreeMap;

    let mut levels: BTreeMap<Decimal, Decimal> = BTreeMap::new();

    for order in orders {
        if !order.remaining_base_amount.is_sign_positive() || order.remaining_base_amount.is_zero()
        {
            continue;
        }
        *levels.entry(order.price).or_insert(Decimal::ZERO) += order.remaining_base_amount;
    }

    levels
        .into_iter()
        .filter_map(|(price, size)| {
            let price = match Price::from_decimal_dp(price, price_precision) {
                Ok(p) => p,
                Err(e) => {
                    log::warn!("Skipping Lighter snapshot price `{price}`: {e}");
                    return None;
                }
            };
            let size = match Quantity::from_decimal_dp(size, size_precision) {
                Ok(q) => q,
                Err(e) => {
                    log::warn!("Skipping Lighter snapshot size `{size}`: {e}");
                    return None;
                }
            };
            Some((price, size))
        })
        .collect()
}

fn aggressor_side_from_is_maker_ask(is_maker_ask: bool) -> AggressorSide {
    if is_maker_ask {
        AggressorSide::Buyer
    } else {
        AggressorSide::Seller
    }
}

fn snapshot_flags(processed: usize, total_levels: usize) -> u8 {
    let mut flags = RecordFlag::F_SNAPSHOT as u8;
    if processed == total_levels {
        flags |= RecordFlag::F_LAST as u8;
    }
    flags
}

fn register_order_book(registry: &MarketRegistry, order_book: &LighterOrderBook) {
    registry.insert(
        order_book.market_id,
        order_book.symbol.as_str(),
        order_book.market_type,
    );
}

fn parse_perp_instrument(
    registry: &MarketRegistry,
    detail: &LighterPerpOrderBookDetail,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let order_book = &detail.order_book;
    let instrument_id = registry.insert(
        order_book.market_id,
        order_book.symbol.as_str(),
        order_book.market_type,
    );
    let raw_symbol = Symbol::from_ustr_unchecked(order_book.symbol);
    let (base_currency, quote_currency) = symbol_currencies(order_book.symbol.as_str(), "USDC");
    let settlement_currency = quote_currency;
    let price_increment = price_increment(detail.price_decimals);
    let size_increment = quantity_increment(detail.size_decimals);

    let instrument = CryptoPerpetual::new_checked(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        settlement_currency,
        false,
        detail.price_decimals,
        detail.size_decimals,
        price_increment,
        size_increment,
        None,
        None,
        None,
        min_quantity(order_book, detail.size_decimals)?,
        max_notional(order_book, quote_currency)?,
        min_notional(order_book, quote_currency)?,
        None,
        None,
        Some(margin_fraction(detail.default_initial_margin_fraction)),
        Some(margin_fraction(detail.maintenance_margin_fraction)),
        Some(order_book.maker_fee),
        Some(order_book.taker_fee),
        None,
        None,
        ts_init,
        ts_init,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    Ok(InstrumentAny::CryptoPerpetual(instrument))
}

fn parse_spot_instrument(
    registry: &MarketRegistry,
    detail: &LighterSpotOrderBookDetail,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let order_book = &detail.order_book;
    let instrument_id = registry.insert(
        order_book.market_id,
        order_book.symbol.as_str(),
        order_book.market_type,
    );
    let raw_symbol = Symbol::from_ustr_unchecked(order_book.symbol);
    let (base_currency, quote_currency) = symbol_currencies(order_book.symbol.as_str(), "USDC");
    let price_increment = price_increment(detail.price_decimals);
    let size_increment = quantity_increment(detail.size_decimals);

    let instrument = CurrencyPair::new_checked(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        detail.price_decimals,
        detail.size_decimals,
        price_increment,
        size_increment,
        None,
        None,
        None,
        min_quantity(order_book, detail.size_decimals)?,
        max_notional(order_book, quote_currency)?,
        min_notional(order_book, quote_currency)?,
        None,
        None,
        None,
        None,
        Some(order_book.maker_fee),
        Some(order_book.taker_fee),
        None,
        None,
        ts_init,
        ts_init,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    Ok(InstrumentAny::CurrencyPair(instrument))
}

fn symbol_currencies(symbol: &str, default_quote: &str) -> (Currency, Currency) {
    let (base, quote) = symbol.split_once('/').unwrap_or((symbol, default_quote));
    (
        Currency::get_or_create_crypto(base),
        Currency::get_or_create_crypto(quote),
    )
}

fn price_increment(decimals: u8) -> Price {
    Price::from(decimal_increment(decimals))
}

fn quantity_increment(decimals: u8) -> Quantity {
    Quantity::from(decimal_increment(decimals))
}

fn decimal_increment(decimals: u8) -> String {
    if decimals == 0 {
        return "1".to_string();
    }

    format!("0.{}1", "0".repeat(usize::from(decimals - 1)))
}

fn min_quantity(
    order_book: &LighterOrderBook,
    size_decimals: u8,
) -> anyhow::Result<Option<Quantity>> {
    quantity_from_decimal(order_book.min_base_amount, size_decimals).map(Some)
}

fn min_notional(
    order_book: &LighterOrderBook,
    currency: Currency,
) -> anyhow::Result<Option<Money>> {
    money_from_decimal(order_book.min_quote_amount, currency).map(Some)
}

fn max_notional(
    order_book: &LighterOrderBook,
    currency: Currency,
) -> anyhow::Result<Option<Money>> {
    money_from_decimal(order_book.order_quote_limit, currency).map(Some)
}

fn money_from_decimal(value: Decimal, currency: Currency) -> anyhow::Result<Money> {
    Money::from_decimal(value, currency).map_err(|e| anyhow::anyhow!("{e}"))
}

fn margin_fraction(value: u16) -> Decimal {
    Decimal::from(value) / Decimal::from(10_000)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_model::{
        data::{BarSpecification, BarType},
        enums::{AggregationSource, BarAggregation, PriceType},
        identifiers::{InstrumentId, Symbol, Venue},
        instruments::CryptoPerpetual,
        types::{Money, Price, Quantity, currency::Currency},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::enums::{
            LighterMarketStatus, LighterPositionMarginMode, LighterProductType, LighterTradeType,
        },
        http::models::{
            LighterCandles, LighterFunding, LighterFundingDirection, LighterMarketConfig,
            LighterSimpleOrder,
        },
    };

    const HTTP_CANDLES: &str = include_str!("../../test_data/http_candles.json");

    fn create_test_instrument() -> InstrumentAny {
        let instrument_id = InstrumentId::new(Symbol::new("ETH-PERP"), Venue::new("LIGHTER"));

        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            instrument_id,
            Symbol::new("ETH-PERP"),
            Currency::from("ETH"),
            Currency::from("USDC"),
            Currency::from("USDC"),
            false,
            2,
            4,
            Price::from("0.01"),
            Quantity::from("0.0001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    fn stub_trade(is_maker_ask: bool) -> LighterTrade {
        LighterTrade {
            trade_id: 19209006902,
            trade_id_str: Some("19209006902".to_string()),
            tx_hash: "000000128b1ee814".to_string(),
            trade_type: LighterTradeType::Trade,
            market_id: 0,
            size: Decimal::from_str("0.1336").unwrap(),
            price: Decimal::from_str("2352.73").unwrap(),
            usd_amount: Decimal::from_str("314.343").unwrap(),
            ask_id: 281476929510102,
            ask_id_str: Some("281476929510102".to_string()),
            bid_id: 562947905631053,
            bid_id_str: Some("562947905631053".to_string()),
            ask_client_id: 0,
            ask_client_id_str: Some("0".to_string()),
            bid_client_id: 7001011966,
            bid_client_id_str: Some("7001011966".to_string()),
            ask_account_id: 91249,
            bid_account_id: 281474976618239,
            is_maker_ask,
            block_height: 227535535,
            timestamp: 1777941384181,
            taker_fee: Some(238),
            taker_position_size_before: Some(Decimal::from_str("-31.5754").unwrap()),
            taker_entry_quote_before: Some(Decimal::from_str("72515.683629").unwrap()),
            taker_initial_margin_fraction_before: Some(200),
            taker_position_sign_changed: None,
            maker_fee: Some(34),
            maker_position_size_before: Some(Decimal::from_str("-1.4442").unwrap()),
            maker_entry_quote_before: Some(Decimal::from_str("3399.343032").unwrap()),
            maker_initial_margin_fraction_before: Some(500),
            maker_position_sign_changed: None,
            transaction_time: 1777941384181586,
            ask_account_pnl: None,
            bid_account_pnl: None,
        }
    }

    fn stub_order_book(
        symbol: &str,
        market_id: i16,
        market_type: LighterProductType,
    ) -> LighterOrderBook {
        LighterOrderBook {
            symbol: Ustr::from(symbol),
            market_id,
            market_type,
            base_asset_id: 0,
            quote_asset_id: 0,
            status: LighterMarketStatus::Active,
            taker_fee: Decimal::ZERO,
            maker_fee: Decimal::ZERO,
            liquidation_fee: Decimal::from_str("1.0000").unwrap(),
            min_base_amount: Decimal::from_str("0.0050").unwrap(),
            min_quote_amount: Decimal::from_str("10.000000").unwrap(),
            order_quote_limit: Decimal::from_str("281474976.710655").unwrap(),
            supported_size_decimals: 4,
            supported_price_decimals: 2,
            supported_quote_decimals: 6,
        }
    }

    #[rstest]
    fn test_parse_trade_tick_maps_aggressor_from_maker_side() {
        let instrument = create_test_instrument();
        let ts_init = UnixNanos::from(1);

        let seller = parse_trade_tick(&stub_trade(false), &instrument, ts_init).unwrap();
        let buyer = parse_trade_tick(&stub_trade(true), &instrument, ts_init).unwrap();

        assert_eq!(seller.aggressor_side, AggressorSide::Seller);
        assert_eq!(buyer.aggressor_side, AggressorSide::Buyer);
        assert_eq!(seller.price, Price::from("2352.73"));
        assert_eq!(seller.size, Quantity::from("0.1336"));
        assert_eq!(seller.trade_id.to_string(), "19209006902");
        assert_eq!(seller.ts_event, UnixNanos::from(1_777_941_384_181_000_000),);
    }

    #[rstest]
    fn test_parse_trade_tick_uses_numeric_trade_id_when_string_missing() {
        let instrument = create_test_instrument();
        let mut trade = stub_trade(false);
        trade.trade_id_str = None;

        let tick = parse_trade_tick(&trade, &instrument, UnixNanos::from(1)).unwrap();

        assert_eq!(tick.trade_id.to_string(), "19209006902");
    }

    #[rstest]
    fn test_parse_trade_tick_rejects_negative_timestamp() {
        let instrument = create_test_instrument();
        let mut trade = stub_trade(false);
        trade.timestamp = -1;

        let err = parse_trade_tick(&trade, &instrument, UnixNanos::from(1)).unwrap_err();

        assert!(err.to_string().contains("negative Lighter trade timestamp"));
    }

    #[rstest]
    fn test_parse_trade_tick_propagates_invalid_price() {
        // With the Decimal model field invalid wire data is rejected at JSON
        // deserialize time, before parse_trade_tick ever runs. This guards
        // that a malformed `price` in the venue payload surfaces a parse error
        // rather than silently constructing a zero-priced tick.
        let payload = serde_json::json!({
            "trade_id": 1,
            "tx_hash": "deadbeef",
            "type": "trade",
            "market_id": 0,
            "size": "1.0",
            "price": "not-a-price",
            "usd_amount": "0",
            "ask_id": 0,
            "bid_id": 0,
            "ask_client_id": 0,
            "bid_client_id": 0,
            "ask_account_id": 0,
            "bid_account_id": 0,
            "is_maker_ask": false,
            "block_height": 0,
            "timestamp": 1,
            "transaction_time": 1,
        });

        let err = serde_json::from_value::<LighterTrade>(payload).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("decimal"));
    }

    #[rstest]
    fn test_parse_funding_rate_update_maps_direction_to_signed_rate() {
        let instrument = create_test_instrument();
        let ts_init = UnixNanos::from(1);
        let long_pays = LighterFunding {
            timestamp: 1_778_702_400,
            value: Decimal::ZERO,
            rate: Decimal::new(12, 4),
            direction: LighterFundingDirection::Long,
        };
        let short_pays = LighterFunding {
            direction: LighterFundingDirection::Short,
            ..long_pays
        };

        let positive =
            parse_funding_rate_update(&long_pays, instrument.id(), Some(60), ts_init).unwrap();
        let negative =
            parse_funding_rate_update(&short_pays, instrument.id(), Some(60), ts_init).unwrap();

        assert_eq!(positive.instrument_id, instrument.id());
        assert_eq!(positive.rate, Decimal::new(12, 4));
        assert_eq!(positive.interval, Some(60));
        assert_eq!(
            positive.ts_event,
            UnixNanos::from(1_778_702_400_000_000_000)
        );
        assert_eq!(negative.rate, Decimal::new(-12, 4));
    }

    #[rstest]
    fn test_parse_funding_rate_update_rejects_negative_timestamp() {
        let instrument = create_test_instrument();
        let funding = LighterFunding {
            timestamp: -1,
            value: Decimal::ZERO,
            rate: Decimal::new(12, 4),
            direction: LighterFundingDirection::Long,
        };

        let err = parse_funding_rate_update(&funding, instrument.id(), None, UnixNanos::from(1))
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("negative Lighter funding timestamp")
        );
    }

    fn test_bar_type(instrument_id: InstrumentId) -> BarType {
        BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        )
    }

    #[rstest]
    fn test_parse_candle_bar_loads_fixture() {
        let instrument = create_test_instrument();
        let bar_type = test_bar_type(instrument.id());
        let candles: LighterCandles = serde_json::from_str(HTTP_CANDLES).unwrap();
        let candle = &candles.candles[0];

        let bar = parse_candle_bar(candle, bar_type, &instrument, UnixNanos::from(42)).unwrap();

        assert_eq!(bar.bar_type, bar_type);
        assert_eq!(bar.open, Price::from("2361.11"));
        assert_eq!(bar.high, Price::from("2362.22"));
        assert_eq!(bar.low, Price::from("2360.00"));
        assert_eq!(bar.close, Price::from("2361.31"));
        assert_eq!(bar.volume, Quantity::from("1.2345"));
        assert_eq!(bar.ts_event, UnixNanos::from(1_700_000_000_000_000_000));
        assert_eq!(bar.ts_init, UnixNanos::from(42));
    }

    #[rstest]
    fn test_parse_candle_bar_rejects_negative_timestamp() {
        let instrument = create_test_instrument();
        let bar_type = test_bar_type(instrument.id());
        let candles: LighterCandles = serde_json::from_str(HTTP_CANDLES).unwrap();
        let mut candle = candles.candles[0].clone();
        candle.timestamp = -1;

        let err = parse_candle_bar(&candle, bar_type, &instrument, UnixNanos::from(1)).unwrap_err();

        assert!(
            err.to_string()
                .contains("negative Lighter candle timestamp")
        );
    }

    // The previous string-roundtrip implementation rejected negative volume
    // implicitly via `parse_quantity`'s decimal sign check. The current
    // direct-Decimal implementation enforces the same constraint with an
    // explicit `is_sign_positive` ensure-clause; without this test a future
    // refactor could drop the guard silently and produce a malformed bar.
    #[rstest]
    fn test_parse_candle_bar_rejects_negative_volume() {
        let instrument = create_test_instrument();
        let bar_type = test_bar_type(instrument.id());
        let candles: LighterCandles = serde_json::from_str(HTTP_CANDLES).unwrap();
        let mut candle = candles.candles[0].clone();
        candle.volume_base = Decimal::new(-1, 0);

        let err = parse_candle_bar(&candle, bar_type, &instrument, UnixNanos::from(1)).unwrap_err();

        assert!(
            err.to_string().contains("negative candle volume"),
            "expected negative-volume error, was: {err}",
        );
    }

    #[rstest]
    fn test_parse_order_book_snapshot_includes_clear_and_last_flag() {
        let instrument = create_test_instrument();
        let snapshot = LighterOrderBookOrders {
            code: 200,
            message: None,
            total_asks: 1,
            asks: vec![LighterSimpleOrder {
                order_index: 281476929510110,
                order_id: "281476929510110".to_string(),
                owner_account_index: 712440,
                initial_base_amount: Decimal::from_str("0.0050").unwrap(),
                remaining_base_amount: Decimal::from_str("0.0050").unwrap(),
                price: Decimal::from_str("2352.74").unwrap(),
                order_expiry: 1780360584479,
                transaction_time: 0,
            }],
            total_bids: 1,
            bids: vec![LighterSimpleOrder {
                order_index: 562947905631047,
                order_id: "562947905631047".to_string(),
                owner_account_index: 281474976619400,
                initial_base_amount: Decimal::from_str("0.2125").unwrap(),
                remaining_base_amount: Decimal::from_str("0.2125").unwrap(),
                price: Decimal::from_str("2352.71").unwrap(),
                order_expiry: 1780360585134,
                transaction_time: 0,
            }],
        };
        let ts_event = UnixNanos::from(10);
        let ts_init = UnixNanos::from(20);

        let deltas = parse_order_book_snapshot(
            &snapshot,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            ts_event,
            ts_init,
        )
        .unwrap();

        assert_eq!(deltas.deltas.len(), 3);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy);
        assert_eq!(deltas.deltas[1].order.price, Price::from("2352.71"));
        assert_eq!(deltas.deltas[1].order.size, Quantity::from("0.2125"));
        assert_eq!(deltas.deltas[2].order.side, OrderSide::Sell);
        assert_eq!(deltas.deltas[2].order.price, Price::from("2352.74"));
        assert_eq!(deltas.deltas[2].order.size, Quantity::from("0.0050"));
        assert_eq!(deltas.deltas[0].sequence, 0);
        assert_eq!(deltas.deltas[1].sequence, 1);
        assert_eq!(deltas.deltas[2].sequence, 2);
        assert_eq!(deltas.sequence, 2);
        assert_eq!(
            deltas.deltas[2].flags & RecordFlag::F_LAST as u8,
            RecordFlag::F_LAST as u8
        );
    }

    #[rstest]
    fn test_parse_order_book_snapshot_marks_empty_clear_as_last() {
        let instrument = create_test_instrument();
        let snapshot = LighterOrderBookOrders {
            code: 200,
            message: None,
            total_asks: 0,
            asks: vec![],
            total_bids: 0,
            bids: vec![],
        };

        let deltas = parse_order_book_snapshot(
            &snapshot,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            UnixNanos::from(10),
            UnixNanos::from(20),
        )
        .unwrap();

        assert_eq!(deltas.deltas.len(), 1);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(
            deltas.deltas[0].flags & RecordFlag::F_LAST as u8,
            RecordFlag::F_LAST as u8,
        );
    }

    #[rstest]
    fn test_parse_order_book_snapshot_rejects_negative_order_index() {
        let instrument = create_test_instrument();
        let snapshot = LighterOrderBookOrders {
            code: 200,
            message: None,
            total_asks: 0,
            asks: vec![],
            total_bids: 1,
            bids: vec![LighterSimpleOrder {
                order_index: -1,
                order_id: "-1".to_string(),
                owner_account_index: 281474976619400,
                initial_base_amount: Decimal::from_str("0.2125").unwrap(),
                remaining_base_amount: Decimal::from_str("0.2125").unwrap(),
                price: Decimal::from_str("2352.71").unwrap(),
                order_expiry: 1780360585134,
                transaction_time: 0,
            }],
        };

        let err = parse_order_book_snapshot(
            &snapshot,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            UnixNanos::from(10),
            UnixNanos::from(20),
        )
        .unwrap_err();

        assert!(err.to_string().contains("negative Lighter bid order index"));
    }

    #[rstest]
    fn test_parse_order_book_snapshot_rejects_zero_size_level() {
        let instrument = create_test_instrument();
        let snapshot = LighterOrderBookOrders {
            code: 200,
            message: None,
            total_asks: 0,
            asks: vec![],
            total_bids: 1,
            bids: vec![LighterSimpleOrder {
                order_index: 562947905631047,
                order_id: "562947905631047".to_string(),
                owner_account_index: 281474976619400,
                initial_base_amount: Decimal::from_str("0.2125").unwrap(),
                remaining_base_amount: Decimal::ZERO,
                price: Decimal::from_str("2352.71").unwrap(),
                order_expiry: 1780360585134,
                transaction_time: 0,
            }],
        };

        let err = parse_order_book_snapshot(
            &snapshot,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            UnixNanos::from(10),
            UnixNanos::from(20),
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("failed to construct Lighter bid snapshot delta")
        );
    }

    fn stub_simple_order(price: &str, remaining_base_amount: &str) -> LighterSimpleOrder {
        let amount = Decimal::from_str(remaining_base_amount).unwrap();
        let price = Decimal::from_str(price).unwrap();
        LighterSimpleOrder {
            order_index: 0,
            order_id: "0".to_string(),
            owner_account_index: 0,
            initial_base_amount: amount,
            remaining_base_amount: amount,
            price,
            order_expiry: 0,
            transaction_time: 0,
        }
    }

    #[rstest]
    fn test_parse_l2_order_book_snapshot_two_sided_aggregates_per_price() {
        let instrument = create_test_instrument();
        let snapshot = LighterOrderBookOrders {
            code: 200,
            message: None,
            total_asks: 3,
            asks: vec![
                stub_simple_order("2352.74", "0.0050"),
                stub_simple_order("2353.00", "1.0000"),
                stub_simple_order("2354.00", "2.0000"),
            ],
            total_bids: 2,
            // Two orders at the same bid price must aggregate into one level.
            bids: vec![
                stub_simple_order("2000.00", "0.0100"),
                stub_simple_order("2000.00", "0.0200"),
            ],
        };

        let book = parse_l2_order_book_snapshot(
            &snapshot,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
        );

        assert_eq!(book.book_type, BookType::L2_MBP);
        assert_eq!(book.instrument_id, instrument.id());
        assert_eq!(book.best_bid_price(), Some(Price::from("2000.00")));
        assert_eq!(book.best_bid_size(), Some(Quantity::from("0.0300")));
        assert_eq!(book.best_ask_price(), Some(Price::from("2352.74")));
        assert_eq!(book.best_ask_size(), Some(Quantity::from("0.0050")));
        // Snapshot must not stamp a wall-clock ts_event so the first WS delta
        // can install a real venue timestamp without tripping the model's
        // out-of-order guard.
        assert_eq!(book.ts_last, UnixNanos::default());
    }

    #[rstest]
    fn test_parse_l2_order_book_snapshot_one_sided_only_populates_asks() {
        let instrument = create_test_instrument();
        let snapshot = LighterOrderBookOrders {
            code: 200,
            message: None,
            total_asks: 1,
            asks: vec![stub_simple_order("2352.74", "0.0050")],
            total_bids: 0,
            bids: vec![],
        };

        let book = parse_l2_order_book_snapshot(
            &snapshot,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
        );

        assert_eq!(book.best_bid_price(), None);
        assert_eq!(book.best_ask_price(), Some(Price::from("2352.74")));
        assert_eq!(book.best_ask_size(), Some(Quantity::from("0.0050")));
    }

    #[rstest]
    fn test_parse_l2_order_book_snapshot_empty_book_yields_empty_book() {
        let instrument = create_test_instrument();
        let snapshot = LighterOrderBookOrders {
            code: 200,
            message: None,
            total_asks: 0,
            asks: vec![],
            total_bids: 0,
            bids: vec![],
        };

        let book = parse_l2_order_book_snapshot(
            &snapshot,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
        );

        assert_eq!(book.best_bid_price(), None);
        assert_eq!(book.best_ask_price(), None);
        assert_eq!(book.ts_last, UnixNanos::default());
    }

    #[rstest]
    #[case::zero_size("0")]
    #[case::negative_size("-1.0")]
    fn test_parse_l2_order_book_snapshot_skips_unusable_orders(#[case] remaining: &str) {
        // The L2 snapshot aggregator drops orders whose size is zero or
        // negative (still constructable as `Decimal`) and keeps the remaining
        // valid orders; contrast with `parse_order_book_snapshot` (the deltas
        // variant) which errs. Non-numeric wire values are now rejected at
        // JSON deserialize time, so the whole `LighterOrderBookOrders`
        // payload errors before the aggregator runs. The Decimal model field
        // chose typed correctness over per-order resilience to garbage from
        // the venue.
        let instrument = create_test_instrument();
        let snapshot = LighterOrderBookOrders {
            code: 200,
            message: None,
            total_asks: 1,
            asks: vec![stub_simple_order("2400.00", "1.0000")],
            total_bids: 2,
            bids: vec![
                stub_simple_order("2000.00", remaining),
                stub_simple_order("1999.50", "0.5000"),
            ],
        };

        let book = parse_l2_order_book_snapshot(
            &snapshot,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
        );

        // Bad bid is dropped; the second valid bid still seeds the bid side.
        assert_eq!(book.best_bid_price(), Some(Price::from("1999.50")));
        assert_eq!(book.best_ask_price(), Some(Price::from("2400.00")));
    }

    #[rstest]
    fn test_parse_l2_order_book_snapshot_loads_fixture() {
        // End-to-end against the venue fixture: 1 bid + 1 ask, cross-checks
        // that the two-sided shape exercised by the rest of the test suite
        // round-trips through the L2 snapshot parser.
        const HTTP_ORDER_BOOK_ORDERS: &str =
            include_str!("../../test_data/http_order_book_orders.json");

        let instrument = create_test_instrument();
        let snapshot: LighterOrderBookOrders =
            serde_json::from_str(HTTP_ORDER_BOOK_ORDERS).expect("fixture deserializes");

        let book = parse_l2_order_book_snapshot(
            &snapshot,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
        );

        assert_eq!(book.best_bid_price(), Some(Price::from("2361.17")));
        assert_eq!(book.best_bid_size(), Some(Quantity::from("3.4125")));
        assert_eq!(book.best_ask_price(), Some(Price::from("2361.32")));
        assert_eq!(book.best_ask_size(), Some(Quantity::from("0.0317")));
    }

    #[rstest]
    fn test_register_order_books_populates_market_registry() {
        let registry = MarketRegistry::new();
        let order_books = vec![stub_order_book("ETH", 0, LighterProductType::Perp)];

        register_order_books(&registry, &order_books);

        assert_eq!(registry.market_index(&instrument_id("ETH-PERP")), Some(0));
    }

    #[rstest]
    fn test_register_perp_order_book_details_populates_market_registry() {
        let registry = MarketRegistry::new();
        let details = vec![LighterPerpOrderBookDetail {
            order_book: stub_order_book("ETH", 0, LighterProductType::Perp),
            size_decimals: 4,
            price_decimals: 2,
            quote_multiplier: 1,
            default_initial_margin_fraction: 500,
            min_initial_margin_fraction: 200,
            maintenance_margin_fraction: 120,
            closeout_margin_fraction: 80,
            last_trade_price: Decimal::new(235_273, 2),
            daily_trades_count: 0,
            daily_base_token_volume: Decimal::ZERO,
            daily_quote_token_volume: Decimal::ZERO,
            daily_price_low: Decimal::ZERO,
            daily_price_high: Decimal::ZERO,
            daily_price_change: Decimal::ZERO,
            open_interest: Decimal::ZERO,
            daily_chart: Default::default(),
            market_config: LighterMarketConfig {
                market_margin_mode: LighterPositionMarginMode::Cross,
                insurance_fund_account_index: 281474976710655,
                liquidation_mode: 0,
                force_reduce_only: false,
                trading_hours: String::new(),
                funding_fee_discounts_enabled: false,
                hidden: false,
            },
            strategy_index: 2,
        }];

        register_perp_order_book_details(&registry, &details);

        assert_eq!(registry.market_index(&instrument_id("ETH-PERP")), Some(0));
    }

    #[rstest]
    fn test_parse_order_book_details_instruments_rejects_invalid_min_quantity() {
        // A negative `min_base_amount` is unreachable from the wire (the venue
        // never sends one) but the parser must still reject it rather than
        // panic inside `Quantity::from_decimal_dp`.
        let registry = MarketRegistry::new();
        let mut order_book = stub_order_book("ETH", 0, LighterProductType::Perp);
        order_book.min_base_amount = Decimal::from_str("-0.0050").unwrap();
        let details = vec![LighterPerpOrderBookDetail {
            order_book,
            size_decimals: 4,
            price_decimals: 2,
            quote_multiplier: 1,
            default_initial_margin_fraction: 500,
            min_initial_margin_fraction: 200,
            maintenance_margin_fraction: 120,
            closeout_margin_fraction: 80,
            last_trade_price: Decimal::new(235_273, 2),
            daily_trades_count: 0,
            daily_base_token_volume: Decimal::ZERO,
            daily_quote_token_volume: Decimal::ZERO,
            daily_price_low: Decimal::ZERO,
            daily_price_high: Decimal::ZERO,
            daily_price_change: Decimal::ZERO,
            open_interest: Decimal::ZERO,
            daily_chart: Default::default(),
            market_config: LighterMarketConfig {
                market_margin_mode: LighterPositionMarginMode::Cross,
                insurance_fund_account_index: 281474976710655,
                liquidation_mode: 0,
                force_reduce_only: false,
                trading_hours: String::new(),
                funding_fee_discounts_enabled: false,
                hidden: false,
            },
            strategy_index: 2,
        }];

        let err =
            parse_order_book_details_instruments(&registry, &details, &[], UnixNanos::from(1))
                .unwrap_err();

        assert!(err.to_string().contains("negative quantity"));
    }

    #[rstest]
    fn test_parse_order_book_details_instruments_parses_spot_pair() {
        let registry = MarketRegistry::new();
        let details = vec![LighterSpotOrderBookDetail {
            order_book: stub_order_book("ETH/USDC", 2048, LighterProductType::Spot),
            size_decimals: 6,
            price_decimals: 6,
            last_trade_price: Decimal::ONE,
            daily_trades_count: 0,
            daily_base_token_volume: Decimal::ZERO,
            daily_quote_token_volume: Decimal::ZERO,
            daily_price_low: Decimal::ZERO,
            daily_price_high: Decimal::ZERO,
            daily_price_change: Decimal::ZERO,
            daily_chart: Default::default(),
        }];
        let instrument_id = instrument_id("ETH/USDC-SPOT");

        let instruments =
            parse_order_book_details_instruments(&registry, &[], &details, UnixNanos::from(1))
                .unwrap();

        assert_eq!(instruments.len(), 1);
        assert_eq!(registry.market_index(&instrument_id), Some(2048));

        match &instruments[0] {
            InstrumentAny::CurrencyPair(pair) => {
                assert_eq!(pair.id, instrument_id);
                assert_eq!(pair.raw_symbol.as_str(), "ETH/USDC");
                assert_eq!(pair.base_currency, Currency::from("ETH"));
                assert_eq!(pair.quote_currency, Currency::from("USDC"));
                assert_eq!(pair.price_precision, 6);
                assert_eq!(pair.size_precision, 6);
                assert_eq!(pair.price_increment, Price::from("0.000001"));
                assert_eq!(pair.size_increment, Quantity::from("0.000001"));
                assert_eq!(pair.min_quantity, Some(Quantity::from("0.005000")));
                assert_eq!(
                    pair.max_notional,
                    Some(Money::from("281474976.710655 USDC"))
                );
                assert_eq!(pair.min_notional, Some(Money::from("10.000000 USDC")));
            }
            other => panic!("expected currency pair, was {other:?}"),
        }
    }

    #[rstest]
    fn test_register_spot_order_book_details_populates_market_registry() {
        let registry = MarketRegistry::new();
        let details = vec![LighterSpotOrderBookDetail {
            order_book: stub_order_book("USDC", 2048, LighterProductType::Spot),
            size_decimals: 6,
            price_decimals: 6,
            last_trade_price: Decimal::ONE,
            daily_trades_count: 0,
            daily_base_token_volume: Decimal::ZERO,
            daily_quote_token_volume: Decimal::ZERO,
            daily_price_low: Decimal::ZERO,
            daily_price_high: Decimal::ZERO,
            daily_price_change: Decimal::ZERO,
            daily_chart: Default::default(),
        }];

        register_spot_order_book_details(&registry, &details);

        assert_eq!(
            registry.market_index(&instrument_id("USDC-SPOT")),
            Some(2048)
        );
    }

    fn instrument_id(symbol: &str) -> InstrumentId {
        InstrumentId::new(Symbol::new(symbol), Venue::new("LIGHTER"))
    }
}
