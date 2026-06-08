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

//! Per-class market-data extractors for marker sidecar capture.

use std::{
    any::{Any, TypeId},
    fmt::Debug,
};

use ahash::AHashMap;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, BookOrder, OrderBookDeltas, OrderBookDepth10, QuoteTick, TradeTick},
    types::{Price, Quantity, fixed::FIXED_PRECISION},
};

use crate::markers::DataClass;

const QUOTE_FINGERPRINT_DOMAIN: &[u8] = b"nautilus-event-store/marker/fingerprint/quote/v1";
const TRADE_FINGERPRINT_DOMAIN: &[u8] = b"nautilus-event-store/marker/fingerprint/trade/v1";
const BAR_FINGERPRINT_DOMAIN: &[u8] = b"nautilus-event-store/marker/fingerprint/bar/v1";
const DEPTH10_FINGERPRINT_DOMAIN: &[u8] = b"nautilus-event-store/marker/fingerprint/depth10/v1";
const DELTAS_FINGERPRINT_DOMAIN: &[u8] = b"nautilus-event-store/marker/fingerprint/deltas/v1";

/// Extracts marker sidecar fields from a concrete market-data message.
///
/// The bus tap passes messages as `&dyn Any`, so implementations downcast to the type they were
/// registered for and return `None` when the registry is miswired.
pub trait DataMarkerExtractor: Send + Sync {
    /// Returns the data class handled by this extractor.
    fn data_class(&self) -> DataClass;

    /// Returns the stable stream identifier for the message.
    fn identifier(&self, msg: &dyn Any) -> Option<String>;

    /// Returns `(ts_event, ts_init)` for the message.
    fn timestamps(&self, msg: &dyn Any) -> Option<(UnixNanos, UnixNanos)>;

    /// Returns the class-specific canonical content fingerprint for the message.
    fn fingerprint(&self, msg: &dyn Any) -> Option<[u8; 32]>;
}

/// Registry of marker extractors keyed by concrete message [`TypeId`].
///
/// Registration happens before the capture tap is installed. Lookups use the concrete type behind
/// `&dyn Any`, so the hot path avoids trying every extractor.
pub struct DataMarkerExtractorRegistry {
    by_type: AHashMap<TypeId, Box<dyn DataMarkerExtractor>>,
}

impl DataMarkerExtractorRegistry {
    /// Creates an empty extractor registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            by_type: AHashMap::new(),
        }
    }

    /// Registers `ex` as the marker extractor for `T`.
    ///
    /// Replaces any previous extractor for `T`; callers should finish registration before sharing
    /// the registry with the capture path.
    pub fn register<T: 'static>(&mut self, ex: Box<dyn DataMarkerExtractor>) {
        self.by_type.insert(TypeId::of::<T>(), ex);
    }

    /// Creates a registry containing builtin extractors for the enabled `classes`.
    #[must_use]
    pub fn default_registry(classes: &[DataClass]) -> Self {
        let mut registry = Self::new();

        for class in classes {
            match class {
                DataClass::BookDeltas => {
                    registry.register::<OrderBookDeltas>(Box::new(OrderBookDeltasExtractor));
                }
                DataClass::BookDepth10 => {
                    registry.register::<OrderBookDepth10>(Box::new(OrderBookDepth10Extractor));
                }
                DataClass::Quote => {
                    registry.register::<QuoteTick>(Box::new(QuoteTickExtractor));
                }
                DataClass::Trade => {
                    registry.register::<TradeTick>(Box::new(TradeTickExtractor));
                }
                DataClass::Bar => {
                    registry.register::<Bar>(Box::new(BarExtractor));
                }
            }
        }

        registry
    }

    /// Returns the extractor registered for the concrete type behind `msg`.
    #[must_use]
    pub fn lookup(&self, msg: &dyn Any) -> Option<&dyn DataMarkerExtractor> {
        self.by_type.get(&msg.type_id()).map(Box::as_ref)
    }
}

impl Default for DataMarkerExtractorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Debug for DataMarkerExtractorRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DataMarkerExtractorRegistry))
            .field("len", &self.by_type.len())
            .finish()
    }
}

#[derive(Debug)]
struct QuoteTickExtractor;

impl DataMarkerExtractor for QuoteTickExtractor {
    fn data_class(&self) -> DataClass {
        DataClass::Quote
    }

    fn identifier(&self, msg: &dyn Any) -> Option<String> {
        msg.downcast_ref::<QuoteTick>()
            .map(|quote| quote.instrument_id.to_string())
    }

    fn timestamps(&self, msg: &dyn Any) -> Option<(UnixNanos, UnixNanos)> {
        msg.downcast_ref::<QuoteTick>()
            .map(|quote| (quote.ts_event, quote.ts_init))
    }

    fn fingerprint(&self, msg: &dyn Any) -> Option<[u8; 32]> {
        msg.downcast_ref::<QuoteTick>().map(fingerprint_quote)
    }
}

#[derive(Debug)]
struct TradeTickExtractor;

impl DataMarkerExtractor for TradeTickExtractor {
    fn data_class(&self) -> DataClass {
        DataClass::Trade
    }

    fn identifier(&self, msg: &dyn Any) -> Option<String> {
        msg.downcast_ref::<TradeTick>()
            .map(|trade| trade.instrument_id.to_string())
    }

    fn timestamps(&self, msg: &dyn Any) -> Option<(UnixNanos, UnixNanos)> {
        msg.downcast_ref::<TradeTick>()
            .map(|trade| (trade.ts_event, trade.ts_init))
    }

    fn fingerprint(&self, msg: &dyn Any) -> Option<[u8; 32]> {
        msg.downcast_ref::<TradeTick>().map(fingerprint_trade)
    }
}

#[derive(Debug)]
struct BarExtractor;

impl DataMarkerExtractor for BarExtractor {
    fn data_class(&self) -> DataClass {
        DataClass::Bar
    }

    fn identifier(&self, msg: &dyn Any) -> Option<String> {
        msg.downcast_ref::<Bar>()
            .map(|bar| bar.bar_type.to_string())
    }

    fn timestamps(&self, msg: &dyn Any) -> Option<(UnixNanos, UnixNanos)> {
        msg.downcast_ref::<Bar>()
            .map(|bar| (bar.ts_event, bar.ts_init))
    }

    fn fingerprint(&self, msg: &dyn Any) -> Option<[u8; 32]> {
        msg.downcast_ref::<Bar>().map(fingerprint_bar)
    }
}

#[derive(Debug)]
struct OrderBookDepth10Extractor;

impl DataMarkerExtractor for OrderBookDepth10Extractor {
    fn data_class(&self) -> DataClass {
        DataClass::BookDepth10
    }

    fn identifier(&self, msg: &dyn Any) -> Option<String> {
        msg.downcast_ref::<OrderBookDepth10>()
            .map(|depth| depth.instrument_id.to_string())
    }

    fn timestamps(&self, msg: &dyn Any) -> Option<(UnixNanos, UnixNanos)> {
        msg.downcast_ref::<OrderBookDepth10>()
            .map(|depth| (depth.ts_event, depth.ts_init))
    }

    fn fingerprint(&self, msg: &dyn Any) -> Option<[u8; 32]> {
        msg.downcast_ref::<OrderBookDepth10>()
            .map(fingerprint_depth10)
    }
}

#[derive(Debug)]
struct OrderBookDeltasExtractor;

impl DataMarkerExtractor for OrderBookDeltasExtractor {
    fn data_class(&self) -> DataClass {
        DataClass::BookDeltas
    }

    fn identifier(&self, msg: &dyn Any) -> Option<String> {
        msg.downcast_ref::<OrderBookDeltas>()
            .map(|deltas| deltas.instrument_id.to_string())
    }

    fn timestamps(&self, msg: &dyn Any) -> Option<(UnixNanos, UnixNanos)> {
        msg.downcast_ref::<OrderBookDeltas>()
            .map(|deltas| (deltas.ts_event, deltas.ts_init))
    }

    fn fingerprint(&self, msg: &dyn Any) -> Option<[u8; 32]> {
        msg.downcast_ref::<OrderBookDeltas>()
            .map(fingerprint_deltas)
    }
}

fn fingerprint_quote(quote: &QuoteTick) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(QUOTE_FINGERPRINT_DOMAIN);
    write_price_raw(&mut hasher, quote.bid_price);
    write_price_raw(&mut hasher, quote.ask_price);
    write_quantity_raw(&mut hasher, quote.bid_size);
    write_quantity_raw(&mut hasher, quote.ask_size);
    write_unix_nanos(&mut hasher, quote.ts_event);
    *hasher.finalize().as_bytes()
}

fn fingerprint_trade(trade: &TradeTick) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(TRADE_FINGERPRINT_DOMAIN);
    write_price_raw(&mut hasher, trade.price);
    write_quantity_raw(&mut hasher, trade.size);
    hasher.update(&[trade.aggressor_side as u8]);
    write_str(&mut hasher, trade.trade_id.as_str());
    write_unix_nanos(&mut hasher, trade.ts_event);
    *hasher.finalize().as_bytes()
}

fn fingerprint_bar(bar: &Bar) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(BAR_FINGERPRINT_DOMAIN);
    write_str(&mut hasher, &bar.bar_type.to_string());
    write_price_raw(&mut hasher, bar.open);
    write_price_raw(&mut hasher, bar.high);
    write_price_raw(&mut hasher, bar.low);
    write_price_raw(&mut hasher, bar.close);
    write_quantity_raw(&mut hasher, bar.volume);
    write_unix_nanos(&mut hasher, bar.ts_event);
    *hasher.finalize().as_bytes()
}

fn fingerprint_depth10(depth: &OrderBookDepth10) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DEPTH10_FINGERPRINT_DOMAIN);
    for (order, count) in depth.bids.iter().zip(depth.bid_counts) {
        write_depth_level(&mut hasher, order, count);
    }

    for (order, count) in depth.asks.iter().zip(depth.ask_counts) {
        write_depth_level(&mut hasher, order, count);
    }
    write_unix_nanos(&mut hasher, depth.ts_event);
    *hasher.finalize().as_bytes()
}

fn fingerprint_deltas(deltas: &OrderBookDeltas) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DELTAS_FINGERPRINT_DOMAIN);
    hasher.update(&(deltas.deltas.len() as u64).to_be_bytes());
    for delta in &deltas.deltas {
        hasher.update(&[delta.action as u8]);
        hasher.update(&[delta.order.side as u8]);
        write_price_raw(&mut hasher, delta.order.price);
        write_quantity_raw(&mut hasher, delta.order.size);
        hasher.update(&delta.order.order_id.to_be_bytes());
        hasher.update(&[delta.flags]);
    }
    write_unix_nanos(&mut hasher, deltas.ts_event);
    *hasher.finalize().as_bytes()
}

fn write_depth_level(hasher: &mut blake3::Hasher, order: &BookOrder, count: u32) {
    write_price_raw(hasher, order.price);
    write_quantity_raw(hasher, order.size);
    hasher.update(&count.to_be_bytes());
}

fn write_price_raw(hasher: &mut blake3::Hasher, price: Price) {
    hasher.update(&[price.precision]);
    hasher.update(&price_raw_at_precision(price).to_be_bytes());
}

fn write_quantity_raw(hasher: &mut blake3::Hasher, quantity: Quantity) {
    hasher.update(&[quantity.precision]);
    hasher.update(&quantity_raw_at_precision(quantity).to_be_bytes());
}

fn price_raw_at_precision(price: Price) -> i128 {
    let scale_down = FIXED_PRECISION.saturating_sub(price.precision);
    #[cfg(feature = "defi")]
    let raw = price.raw;
    #[cfg(not(feature = "defi"))]
    let raw = i128::from(price.raw);

    raw / 10_i128.pow(u32::from(scale_down))
}

fn quantity_raw_at_precision(quantity: Quantity) -> u128 {
    let scale_down = FIXED_PRECISION.saturating_sub(quantity.precision);
    #[cfg(feature = "defi")]
    let raw = quantity.raw;
    #[cfg(not(feature = "defi"))]
    let raw = u128::from(quantity.raw);

    raw / 10_u128.pow(u32::from(scale_down))
}

fn write_unix_nanos(hasher: &mut blake3::Hasher, ts: UnixNanos) {
    hasher.update(&ts.as_u64().to_be_bytes());
}

fn write_str(hasher: &mut blake3::Hasher, value: &str) {
    let bytes = value.as_bytes();
    hasher.update(&(bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

#[cfg(test)]
mod tests {
    use std::{any::Any, fmt::Write};

    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::{
            Bar, BarType, BookOrder, OrderBookDelta, OrderBookDeltas, OrderBookDepth10, QuoteTick,
            TradeTick, depth::DEPTH10_LEN,
        },
        enums::{AggressorSide, BookAction, OrderSide},
        identifiers::{InstrumentId, TradeId},
        types::{Price, Quantity, price::PriceRaw, quantity::QuantityRaw},
    };
    use rstest::rstest;

    use super::*;
    use crate::markers::DataClass;

    fn hex32(bytes: &[u8; 32]) -> String {
        let mut out = String::with_capacity(64);
        for byte in bytes {
            write!(out, "{byte:02x}").expect("writing to a String is infallible");
        }
        out
    }

    fn quote_tick() -> QuoteTick {
        QuoteTick::new(
            InstrumentId::from("ETHUSDT.BINANCE"),
            Price::from("3000.12"),
            Price::from("3000.25"),
            Quantity::from("1.25"),
            Quantity::from("2.50"),
            UnixNanos::from(1_700_000_000_000_000_100),
            UnixNanos::from(1_700_000_000_000_000_200),
        )
    }

    fn trade_tick() -> TradeTick {
        TradeTick::new(
            InstrumentId::from("ETHUSDT.BINANCE"),
            Price::from("3000.18"),
            Quantity::from("0.75"),
            AggressorSide::Buyer,
            TradeId::new("T-ABC-123"),
            UnixNanos::from(1_700_000_000_000_000_300),
            UnixNanos::from(1_700_000_000_000_000_400),
        )
    }

    fn bar() -> Bar {
        Bar::new(
            BarType::from("ETHUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL"),
            Price::from("3000.00"),
            Price::from("3010.50"),
            Price::from("2995.25"),
            Price::from("3005.75"),
            Quantity::from("42.25"),
            UnixNanos::from(1_700_000_000_000_000_500),
            UnixNanos::from(1_700_000_000_000_000_600),
        )
    }

    fn price_from_cents(cents: i64) -> Price {
        let scale_down = FIXED_PRECISION.saturating_sub(2);
        let scale = PriceRaw::from(10_i64.pow(u32::from(scale_down)));
        Price::from_raw(PriceRaw::from(cents) * scale, 2)
    }

    fn quantity_from_cents(cents: u64) -> Quantity {
        let scale_down = FIXED_PRECISION.saturating_sub(2);
        let scale = QuantityRaw::from(10_u64.pow(u32::from(scale_down)));
        Quantity::from_raw(QuantityRaw::from(cents) * scale, 2)
    }

    fn depth10() -> OrderBookDepth10 {
        let instrument_id = InstrumentId::from("ETHUSDT.BINANCE");
        let bids = std::array::from_fn(|i| {
            let level = i64::try_from(i).expect("depth10 index fits i64");
            let order_offset = u64::try_from(i).expect("depth10 index fits u64");

            BookOrder::new(
                OrderSide::Buy,
                price_from_cents(300_000 - level),
                quantity_from_cents(10_000 + order_offset),
                1_000 + order_offset,
            )
        });
        let asks = std::array::from_fn(|i| {
            let level = i64::try_from(i).expect("depth10 index fits i64");
            let order_offset = u64::try_from(i).expect("depth10 index fits u64");

            BookOrder::new(
                OrderSide::Sell,
                price_from_cents(300_100 + level),
                quantity_from_cents(20_000 + order_offset),
                2_000 + order_offset,
            )
        });
        let bid_counts =
            std::array::from_fn(|i| 10 + u32::try_from(i).expect("depth10 index fits u32"));
        let ask_counts =
            std::array::from_fn(|i| 20 + u32::try_from(i).expect("depth10 index fits u32"));

        OrderBookDepth10::new(
            instrument_id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            0x20,
            42,
            UnixNanos::from(1_700_000_000_000_000_700),
            UnixNanos::from(1_700_000_000_000_000_800),
        )
    }

    fn deltas() -> OrderBookDeltas {
        let instrument_id = InstrumentId::from("ETHUSDT.BINANCE");
        let first = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("3000.00"),
                Quantity::from("1.10"),
                10,
            ),
            0x01,
            41,
            UnixNanos::from(1_700_000_000_000_000_900),
            UnixNanos::from(1_700_000_000_000_001_000),
        );
        let second = OrderBookDelta::new(
            instrument_id,
            BookAction::Update,
            BookOrder::new(
                OrderSide::Sell,
                Price::from("3001.00"),
                Quantity::from("2.20"),
                11,
            ),
            0x20,
            42,
            UnixNanos::from(1_700_000_000_000_000_900),
            UnixNanos::from(1_700_000_000_000_001_000),
        );

        OrderBookDeltas::new(instrument_id, vec![first, second])
    }

    fn extractor_for<'a>(
        registry: &'a DataMarkerExtractorRegistry,
        msg: &dyn Any,
    ) -> &'a dyn DataMarkerExtractor {
        registry
            .lookup(msg)
            .expect("registered data marker extractor")
    }

    #[rstest]
    fn quote_extractor_fields_and_fingerprint() {
        let quote = quote_tick();
        let registry = DataMarkerExtractorRegistry::default_registry(&[DataClass::Quote]);
        let extractor = extractor_for(&registry, &quote);

        assert_eq!(extractor.data_class(), DataClass::Quote);
        assert_eq!(
            extractor.identifier(&quote),
            Some("ETHUSDT.BINANCE".to_string())
        );
        assert_eq!(
            extractor.timestamps(&quote),
            Some((
                UnixNanos::from(1_700_000_000_000_000_100),
                UnixNanos::from(1_700_000_000_000_000_200),
            ))
        );
        assert_eq!(
            hex32(&extractor.fingerprint(&quote).expect("fingerprint")),
            "7c6671e34f01b7b547ac8695c6d2cd19c1a37f6d2e3910d9195ed66fd4c02628"
        );
    }

    #[rstest]
    fn raw_writers_use_declared_precision_scale() {
        assert_eq!(price_raw_at_precision(Price::from("3000.12")), 300_012);
        assert_eq!(quantity_raw_at_precision(Quantity::from("1.25")), 125);
    }

    #[rstest]
    fn trade_extractor_fields_and_fingerprint() {
        let trade = trade_tick();
        let registry = DataMarkerExtractorRegistry::default_registry(&[DataClass::Trade]);
        let extractor = extractor_for(&registry, &trade);

        assert_eq!(extractor.data_class(), DataClass::Trade);
        assert_eq!(
            extractor.identifier(&trade),
            Some("ETHUSDT.BINANCE".to_string())
        );
        assert_eq!(
            extractor.timestamps(&trade),
            Some((
                UnixNanos::from(1_700_000_000_000_000_300),
                UnixNanos::from(1_700_000_000_000_000_400),
            ))
        );
        assert_eq!(
            hex32(&extractor.fingerprint(&trade).expect("fingerprint")),
            "6b32a3187d353451a07d92a0d91051406ce4fe912010202b81616ed315f565cb"
        );
    }

    #[rstest]
    fn bar_extractor_fields_and_fingerprint() {
        let bar = bar();
        let registry = DataMarkerExtractorRegistry::default_registry(&[DataClass::Bar]);
        let extractor = extractor_for(&registry, &bar);

        assert_eq!(extractor.data_class(), DataClass::Bar);
        assert_eq!(
            extractor.identifier(&bar),
            Some("ETHUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL".to_string())
        );
        assert_eq!(
            extractor.timestamps(&bar),
            Some((
                UnixNanos::from(1_700_000_000_000_000_500),
                UnixNanos::from(1_700_000_000_000_000_600),
            ))
        );
        assert_eq!(
            hex32(&extractor.fingerprint(&bar).expect("fingerprint")),
            "f2283ae7ed8d2e6a3874473b11935557b6bc2cbf20446419fbdff8fe51f91e84"
        );
    }

    #[rstest]
    fn depth10_extractor_fields_and_fingerprint() {
        let depth = depth10();
        let registry = DataMarkerExtractorRegistry::default_registry(&[DataClass::BookDepth10]);
        let extractor = extractor_for(&registry, &depth);

        assert_eq!(extractor.data_class(), DataClass::BookDepth10);
        assert_eq!(
            extractor.identifier(&depth),
            Some("ETHUSDT.BINANCE".to_string())
        );
        assert_eq!(depth.bids.len(), DEPTH10_LEN);
        assert_eq!(depth.asks.len(), DEPTH10_LEN);
        assert_eq!(
            extractor.timestamps(&depth),
            Some((
                UnixNanos::from(1_700_000_000_000_000_700),
                UnixNanos::from(1_700_000_000_000_000_800),
            ))
        );
        assert_eq!(
            hex32(&extractor.fingerprint(&depth).expect("fingerprint")),
            "432e1f3951c660acaefb3a44c429b050e5e8d5b6f9c3b89cc969edaad85e8e4d"
        );
    }

    #[rstest]
    fn deltas_extractor_fields_and_fingerprint() {
        let deltas = deltas();
        let registry = DataMarkerExtractorRegistry::default_registry(&[DataClass::BookDeltas]);
        let extractor = extractor_for(&registry, &deltas);

        assert_eq!(extractor.data_class(), DataClass::BookDeltas);
        assert_eq!(
            extractor.identifier(&deltas),
            Some("ETHUSDT.BINANCE".to_string())
        );
        assert_eq!(deltas.deltas.len(), 2);
        assert_eq!(
            extractor.timestamps(&deltas),
            Some((
                UnixNanos::from(1_700_000_000_000_000_900),
                UnixNanos::from(1_700_000_000_000_001_000),
            ))
        );
        assert_eq!(
            hex32(&extractor.fingerprint(&deltas).expect("fingerprint")),
            "4bb7fedc4454d53e08300e1ae7a59e648747152134a921050a2c80edec6d8f9e"
        );
    }

    #[rstest]
    #[case::bid_price(|q: &mut QuoteTick| q.bid_price = Price::from("3000.13"))]
    #[case::ask_price(|q: &mut QuoteTick| q.ask_price = Price::from("3000.26"))]
    #[case::bid_size(|q: &mut QuoteTick| q.bid_size = Quantity::from("1.26"))]
    #[case::ask_size(|q: &mut QuoteTick| q.ask_size = Quantity::from("2.51"))]
    #[case::ts_event(|q: &mut QuoteTick| q.ts_event = UnixNanos::from(1))]
    fn quote_fingerprint_changes_when_hashed_field_changes(#[case] mutate: fn(&mut QuoteTick)) {
        let base = quote_tick();
        let mut changed = base;
        mutate(&mut changed);

        assert_ne!(fingerprint_quote(&base), fingerprint_quote(&changed));
    }

    #[rstest]
    #[case::price(|t: &mut TradeTick| t.price = Price::from("3000.19"))]
    #[case::size(|t: &mut TradeTick| t.size = Quantity::from("0.76"))]
    #[case::aggressor_side(|t: &mut TradeTick| t.aggressor_side = AggressorSide::Seller)]
    #[case::trade_id(|t: &mut TradeTick| t.trade_id = TradeId::new("T-ABC-124"))]
    #[case::ts_event(|t: &mut TradeTick| t.ts_event = UnixNanos::from(1))]
    fn trade_fingerprint_changes_when_hashed_field_changes(#[case] mutate: fn(&mut TradeTick)) {
        let base = trade_tick();
        let mut changed = base;
        mutate(&mut changed);

        assert_ne!(fingerprint_trade(&base), fingerprint_trade(&changed));
    }

    #[rstest]
    #[case::bar_type(|b: &mut Bar| b.bar_type = BarType::from("ETHUSDT.BINANCE-5-MINUTE-LAST-EXTERNAL"))]
    #[case::open(|b: &mut Bar| b.open = Price::from("3000.01"))]
    #[case::high(|b: &mut Bar| b.high = Price::from("3010.51"))]
    #[case::low(|b: &mut Bar| b.low = Price::from("2995.26"))]
    #[case::close(|b: &mut Bar| b.close = Price::from("3005.76"))]
    #[case::volume(|b: &mut Bar| b.volume = Quantity::from("42.26"))]
    #[case::ts_event(|b: &mut Bar| b.ts_event = UnixNanos::from(1))]
    fn bar_fingerprint_changes_when_hashed_field_changes(#[case] mutate: fn(&mut Bar)) {
        let base = bar();
        let mut changed = base;
        mutate(&mut changed);

        assert_ne!(fingerprint_bar(&base), fingerprint_bar(&changed));
    }

    #[rstest]
    #[case::bid_price(|d: &mut OrderBookDepth10| d.bids[0].price = price_from_cents(300_001))]
    #[case::bid_size(|d: &mut OrderBookDepth10| d.bids[0].size = quantity_from_cents(10_001))]
    #[case::bid_count(|d: &mut OrderBookDepth10| d.bid_counts[0] = 99)]
    #[case::ask_price(|d: &mut OrderBookDepth10| d.asks[0].price = price_from_cents(300_101))]
    #[case::ask_size(|d: &mut OrderBookDepth10| d.asks[0].size = quantity_from_cents(20_001))]
    #[case::ask_count(|d: &mut OrderBookDepth10| d.ask_counts[0] = 99)]
    #[case::ts_event(|d: &mut OrderBookDepth10| d.ts_event = UnixNanos::from(1))]
    fn depth10_fingerprint_changes_when_hashed_field_changes(
        #[case] mutate: fn(&mut OrderBookDepth10),
    ) {
        let base = depth10();
        let mut changed = base;
        mutate(&mut changed);

        assert_ne!(fingerprint_depth10(&base), fingerprint_depth10(&changed));
    }

    #[rstest]
    #[case::delta_count(|d: &mut OrderBookDeltas| {
        let instrument_id = d.instrument_id;
        d.deltas.push(OrderBookDelta::new(
            instrument_id,
            BookAction::Delete,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("2999.00"),
                Quantity::from("3.30"),
                12,
            ),
            0x20,
            43,
            UnixNanos::from(1_700_000_000_000_000_900),
            UnixNanos::from(1_700_000_000_000_001_000),
        ));
    })]
    #[case::action(|d: &mut OrderBookDeltas| d.deltas[0].action = BookAction::Delete)]
    #[case::side(|d: &mut OrderBookDeltas| d.deltas[0].order.side = OrderSide::Sell)]
    #[case::price(|d: &mut OrderBookDeltas| d.deltas[0].order.price = Price::from("3000.01"))]
    #[case::size(|d: &mut OrderBookDeltas| d.deltas[0].order.size = Quantity::from("1.11"))]
    #[case::order_id(|d: &mut OrderBookDeltas| d.deltas[0].order.order_id = 99)]
    #[case::flags(|d: &mut OrderBookDeltas| d.deltas[0].flags = 0x02)]
    #[case::order(|d: &mut OrderBookDeltas| d.deltas.reverse())]
    #[case::ts_event(|d: &mut OrderBookDeltas| d.ts_event = UnixNanos::from(1))]
    fn deltas_fingerprint_changes_when_hashed_field_changes(
        #[case] mutate: fn(&mut OrderBookDeltas),
    ) {
        let base = deltas();
        let mut changed = base.clone();
        mutate(&mut changed);

        assert_ne!(fingerprint_deltas(&base), fingerprint_deltas(&changed));
    }

    #[rstest]
    fn fingerprint_domains_are_class_specific() {
        let fingerprints = [
            (DataClass::Quote, fingerprint_quote(&quote_tick())),
            (DataClass::Trade, fingerprint_trade(&trade_tick())),
            (DataClass::Bar, fingerprint_bar(&bar())),
            (DataClass::BookDepth10, fingerprint_depth10(&depth10())),
            (DataClass::BookDeltas, fingerprint_deltas(&deltas())),
        ];

        for (index, (left_class, left_fingerprint)) in fingerprints.iter().enumerate() {
            for (right_class, right_fingerprint) in fingerprints.iter().skip(index + 1) {
                assert_ne!(
                    left_fingerprint, right_fingerprint,
                    "{left_class:?} and {right_class:?} fingerprints should differ"
                );
            }
        }
    }

    #[rstest]
    fn default_registry_installs_only_enabled_builtins() {
        let quote = quote_tick();
        let trade = trade_tick();
        let bar = bar();
        let depth = depth10();
        let deltas = deltas();
        let registry = DataMarkerExtractorRegistry::default_registry(&[
            DataClass::Quote,
            DataClass::BookDepth10,
        ]);

        assert!(registry.lookup(&quote).is_some());
        assert!(registry.lookup(&depth).is_some());
        assert!(registry.lookup(&trade).is_none());
        assert!(registry.lookup(&bar).is_none());
        assert!(registry.lookup(&deltas).is_none());
    }

    #[rstest]
    fn registry_returns_none_for_unregistered_type() {
        let registry = DataMarkerExtractorRegistry::new();
        let quote = quote_tick();
        let value = 1_u8;

        assert!(registry.lookup(&quote).is_none());
        assert!(registry.lookup(&value).is_none());
    }
}
