// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    data::{
        Bar, BarType, BookOrder, IndexPriceUpdate, MarkPriceUpdate, OrderBookDelta,
        OrderBookDeltas, QuoteTick, TradeTick,
    },
    enums::{AggregationSource, AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::{InstrumentId, Symbol, TradeId},
    instruments::{CryptoPerpetual, CurrencyPair, any::InstrumentAny},
    types::{Price, Quantity},
};
use rust_decimal::Decimal;

use super::messages::{
    CoinbaseIntxWsCandleSnapshotMsg, CoinbaseIntxWsInstrumentMsg,
    CoinbaseIntxWsOrderBookSnapshotMsg, CoinbaseIntxWsOrderBookUpdateMsg, CoinbaseIntxWsQuoteMsg,
    CoinbaseIntxWsRiskMsg, CoinbaseIntxWsTradeMsg,
};
use crate::common::{
    enums::CoinbaseIntxInstrumentType,
    parse::{coinbase_channel_as_bar_spec, get_currency, parse_instrument_id},
};

/// Parses a Coinbase spot instrument into an InstrumentAny::CurrencyPair.
pub fn parse_spot_instrument(
    definition: &CoinbaseIntxWsInstrumentMsg,
    margin_init: Option<Decimal>,
    margin_maint: Option<Decimal>,
    maker_fee: Option<Decimal>,
    taker_fee: Option<Decimal>,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = parse_instrument_id(definition.product_id);
    let raw_symbol = Symbol::from_ustr_unchecked(definition.product_id);

    let base_currency = get_currency(&definition.base_asset_name);
    let quote_currency = get_currency(&definition.quote_asset_name);

    let price_increment = Price::from(&definition.quote_increment);
    let size_increment = Quantity::from(&definition.base_increment);

    let lot_size = None;
    let max_quantity = None;
    let min_quantity = None;
    let max_notional = None;
    let min_notional = None;
    let max_price = None;
    let min_price = None;

    let instrument = CurrencyPair::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        lot_size,
        max_quantity,
        min_quantity,
        max_notional,
        min_notional,
        max_price,
        min_price,
        margin_init,
        margin_maint,
        maker_fee,
        taker_fee,
        definition.time.into(),
        ts_init,
    );

    Ok(InstrumentAny::CurrencyPair(instrument))
}

/// Parses a Coinbase perpetual instrument into an InstrumentAny::CryptoPerpetual.
pub fn parse_perp_instrument(
    definition: &CoinbaseIntxWsInstrumentMsg,
    margin_init: Option<Decimal>,
    margin_maint: Option<Decimal>,
    maker_fee: Option<Decimal>,
    taker_fee: Option<Decimal>,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = parse_instrument_id(definition.product_id);
    let raw_symbol = Symbol::from_ustr_unchecked(definition.product_id);

    let base_currency = get_currency(&definition.base_asset_name);
    let quote_currency = get_currency(&definition.quote_asset_name);
    let settlement_currency = quote_currency;

    let price_increment = Price::from(&definition.quote_increment);
    let size_increment = Quantity::from(&definition.base_increment);

    let multiplier = Some(Quantity::from(&definition.base_asset_multiplier));

    let lot_size = None;
    let max_quantity = None;
    let min_quantity = None;
    let max_notional = None;
    let min_notional = None;
    let max_price = None;
    let min_price = None;

    let is_inverse = false;

    let instrument = CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        settlement_currency,
        is_inverse,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        multiplier,
        lot_size,
        max_quantity,
        min_quantity,
        max_notional,
        min_notional,
        max_price,
        min_price,
        margin_init,
        margin_maint,
        maker_fee,
        taker_fee,
        definition.time.into(),
        ts_init,
    );

    Ok(InstrumentAny::CryptoPerpetual(instrument))
}

#[must_use]
pub fn parse_instrument_any(
    instrument: &CoinbaseIntxWsInstrumentMsg,
    ts_init: UnixNanos,
) -> Option<InstrumentAny> {
    let result = match instrument.instrument_type {
        CoinbaseIntxInstrumentType::Spot => {
            parse_spot_instrument(instrument, None, None, None, None, ts_init).map(Some)
        }
        CoinbaseIntxInstrumentType::Perp => {
            parse_perp_instrument(instrument, None, None, None, None, ts_init).map(Some)
        }
        CoinbaseIntxInstrumentType::Index => {
            tracing::warn!(
                "Index instrument parsing not implemented {}",
                instrument.product_id,
            );
            Ok(None)
        }
    };

    match result {
        Ok(instrument) => instrument,
        Err(e) => {
            tracing::warn!("Failed to parse instrument {}: {e}", instrument.product_id,);
            None
        }
    }
}

pub fn parse_orderbook_snapshot_msg(
    msg: &CoinbaseIntxWsOrderBookSnapshotMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let ts_event = UnixNanos::from(msg.time);

    // Set the snapshot flag
    let flags = RecordFlag::F_SNAPSHOT.value();

    // Allocate capacity for all bids and asks
    let mut deltas = Vec::with_capacity(msg.bids.len() + msg.asks.len());

    // Process bids - in Coinbase, bids are buy orders
    for bid in &msg.bids {
        let price_str = &bid[0];
        let size_str = &bid[1];

        let price = Price::new(
            price_str
                .parse::<f64>()
                .map_err(|e| anyhow::anyhow!("Failed to parse bid price: {e}"))?,
            price_precision,
        );

        let size = Quantity::new(
            size_str
                .parse::<f64>()
                .map_err(|e| anyhow::anyhow!("Failed to parse bid size: {e}"))?,
            size_precision,
        );

        // For bids (buy orders), we use OrderSide::Buy
        let order_id = 0; // Not provided by Coinbase
        let order = BookOrder::new(OrderSide::Buy, price, size, order_id);

        let delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Add, // For snapshots, always use Add
            order,
            flags,
            msg.sequence,
            ts_event,
            ts_init,
        );

        deltas.push(delta);
    }

    // Process asks - in Coinbase, asks are sell orders
    for ask in &msg.asks {
        let price_str = &ask[0];
        let size_str = &ask[1];

        let price = Price::new(
            price_str
                .parse::<f64>()
                .map_err(|e| anyhow::anyhow!("Failed to parse ask price: {e}"))?,
            price_precision,
        );

        let size = Quantity::new(
            size_str
                .parse::<f64>()
                .map_err(|e| anyhow::anyhow!("Failed to parse ask size: {e}"))?,
            size_precision,
        );

        // For asks (sell orders), we use OrderSide::Sell
        let order_id = 0; // Not provided by Coinbase
        let order = BookOrder::new(OrderSide::Sell, price, size, order_id);

        let delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Add, // For snapshots, always use Add
            order,
            flags,
            msg.sequence,
            ts_event,
            ts_init,
        );

        deltas.push(delta);
    }

    Ok(OrderBookDeltas::new(instrument_id, deltas))
}

pub fn parse_orderbook_update_msg(
    msg: &CoinbaseIntxWsOrderBookUpdateMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let ts_event = UnixNanos::from(msg.time);

    // No snapshot flag for updates
    let flags = 0;

    // Allocate capacity for all changes
    let mut deltas = Vec::with_capacity(msg.changes.len());

    // Process changes
    for change in &msg.changes {
        let side_str = &change[0];
        let price_str = &change[1];
        let size_str = &change[2];

        let price = Price::new(
            price_str
                .parse::<f64>()
                .map_err(|e| anyhow::anyhow!("Failed to parse price: {e}"))?,
            price_precision,
        );

        let size = Quantity::new(
            size_str
                .parse::<f64>()
                .map_err(|e| anyhow::anyhow!("Failed to parse size: {e}"))?,
            size_precision,
        );

        // Determine order side
        let side = match side_str.as_str() {
            "BUY" => OrderSide::Buy,
            "SELL" => OrderSide::Sell,
            _ => return Err(anyhow::anyhow!("Unknown order side: {side_str}")),
        };

        // Determine book action based on size
        let book_action = if size.is_zero() {
            BookAction::Delete
        } else {
            BookAction::Update
        };

        let order_id = 0; // Not provided by Coinbase
        let order = BookOrder::new(side, price, size, order_id);

        let delta = OrderBookDelta::new(
            instrument_id,
            book_action,
            order,
            flags,
            msg.sequence,
            ts_event,
            ts_init,
        );

        deltas.push(delta);
    }

    Ok(OrderBookDeltas::new(instrument_id, deltas))
}

pub fn parse_quote_msg(
    msg: &CoinbaseIntxWsQuoteMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let bid_price = Price::new(msg.bid_price.parse::<f64>()?, price_precision);
    let ask_price = Price::new(msg.ask_price.parse::<f64>()?, price_precision);
    let bid_size = Quantity::new(msg.bid_qty.parse::<f64>()?, size_precision);
    let ask_size = Quantity::new(msg.ask_qty.parse::<f64>()?, size_precision);
    let ts_event = UnixNanos::from(msg.time);

    Ok(QuoteTick::new(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    ))
}

pub fn parse_trade_msg(
    msg: &CoinbaseIntxWsTradeMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = Price::new(msg.trade_price.parse::<f64>()?, price_precision);
    let size = Quantity::new(msg.trade_qty.parse::<f64>()?, size_precision);
    let aggressor_side: AggressorSide = msg.aggressor_side.clone().into();
    let trade_id = TradeId::new(&msg.match_id);
    let ts_event = UnixNanos::from(msg.time);

    Ok(TradeTick::new(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    ))
}

pub fn parse_mark_price_msg(
    msg: &CoinbaseIntxWsRiskMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<MarkPriceUpdate> {
    let value = Price::new(msg.mark_price.parse::<f64>()?, price_precision);
    let ts_event = UnixNanos::from(msg.time);

    Ok(MarkPriceUpdate::new(
        instrument_id,
        value,
        ts_event,
        ts_init,
    ))
}

pub fn parse_index_price_msg(
    msg: &CoinbaseIntxWsRiskMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<IndexPriceUpdate> {
    let value = Price::new(msg.index_price.parse::<f64>()?, price_precision);
    let ts_event = UnixNanos::from(msg.time);

    Ok(IndexPriceUpdate::new(
        instrument_id,
        value,
        ts_event,
        ts_init,
    ))
}

pub fn parse_candle_msg(
    msg: &CoinbaseIntxWsCandleSnapshotMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let bar_spec = coinbase_channel_as_bar_spec(&msg.channel)?;
    let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::External);
    let candle = msg.candles.last().unwrap();
    let ts_event = UnixNanos::from(candle.start); // TODO: Convert to close

    let open_price = Price::new(candle.open.parse::<f64>()?, price_precision);
    let high_price = Price::new(candle.high.parse::<f64>()?, price_precision);
    let low_price = Price::new(candle.low.parse::<f64>()?, price_precision);
    let close_price = Price::new(candle.close.parse::<f64>()?, price_precision);
    let volume = Quantity::new(candle.volume.parse::<f64>()?, size_precision);

    // Create a new bar
    Ok(Bar::new(
        bar_type,
        open_price,
        high_price,
        low_price,
        close_price,
        volume,
        ts_event,
        ts_init,
    ))
}
