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

//! Display-mode Arrow encoder for [`InstrumentAny`].
//!
//! Emits a single schema built from the common [`Instrument`] trait surface,
//! plus the dated/option accessors (`strike_price`, `activation_ns`,
//! `expiration_ns`, `option_kind`) that are uniformly reachable across
//! variants. Variants that do not expose a given accessor emit a null for
//! that row, so mixed-type instrument batches (spot, perp, future, option,
//! equity, etc.) flow through one Perspective table.
//!
//! Variant-only metadata that is not reachable through the trait (e.g.
//! `BettingInstrument::market_id`, `BinaryOption::outcome`,
//! `FuturesSpread::strategy_type`) is intentionally not emitted. Consumers
//! that need those fields should encode the concrete variant through the
//! FixedSizeBinary encoders in the parent [`crate::arrow`] module.

use std::sync::Arc;

use arrow::{
    array::{
        BooleanBuilder, Float64Builder, StringBuilder, TimestampNanosecondBuilder, UInt8Builder,
    },
    datatypes::Schema,
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::instruments::{Instrument, InstrumentAny};
use rust_decimal::prelude::ToPrimitive;

use super::{
    bool_field, float64_field, money_to_f64, price_to_f64, quantity_to_f64, timestamp_field,
    uint8_field, unix_nanos_to_i64, utf8_field,
};

/// Returns the display-mode Arrow schema for [`InstrumentAny`].
#[must_use]
pub fn instrument_schema() -> Schema {
    Schema::new(vec![
        utf8_field("instrument_id", false),
        utf8_field("symbol", false),
        utf8_field("venue", false),
        utf8_field("instrument_type", false),
        utf8_field("raw_symbol", false),
        utf8_field("asset_class", false),
        utf8_field("instrument_class", false),
        utf8_field("underlying", true),
        utf8_field("base_currency", true),
        utf8_field("quote_currency", false),
        utf8_field("settlement_currency", false),
        utf8_field("isin", true),
        utf8_field("option_kind", true),
        utf8_field("exchange", true),
        float64_field("strike_price", true),
        timestamp_field("activation_ns", true),
        timestamp_field("expiration_ns", true),
        bool_field("is_inverse", false),
        bool_field("is_quanto", false),
        uint8_field("price_precision", false),
        uint8_field("size_precision", false),
        float64_field("price_increment", false),
        float64_field("size_increment", false),
        float64_field("multiplier", false),
        float64_field("lot_size", true),
        float64_field("max_quantity", true),
        float64_field("min_quantity", true),
        float64_field("max_notional_amount", true),
        utf8_field("max_notional_currency", true),
        float64_field("min_notional_amount", true),
        utf8_field("min_notional_currency", true),
        float64_field("max_price", true),
        float64_field("min_price", true),
        float64_field("margin_init", false),
        float64_field("margin_maint", false),
        float64_field("maker_fee", false),
        float64_field("taker_fee", false),
        timestamp_field("ts_event", false),
        timestamp_field("ts_init", false),
    ])
}

/// Returns a stable name for the [`InstrumentAny`] variant.
fn instrument_type_name(instrument: &InstrumentAny) -> &'static str {
    match instrument {
        InstrumentAny::Betting(_) => "BettingInstrument",
        InstrumentAny::BinaryOption(_) => "BinaryOption",
        InstrumentAny::Cfd(_) => "Cfd",
        InstrumentAny::Commodity(_) => "Commodity",
        InstrumentAny::CryptoFuture(_) => "CryptoFuture",
        InstrumentAny::CryptoOption(_) => "CryptoOption",
        InstrumentAny::CryptoPerpetual(_) => "CryptoPerpetual",
        InstrumentAny::CurrencyPair(_) => "CurrencyPair",
        InstrumentAny::Equity(_) => "Equity",
        InstrumentAny::FuturesContract(_) => "FuturesContract",
        InstrumentAny::FuturesSpread(_) => "FuturesSpread",
        InstrumentAny::IndexInstrument(_) => "IndexInstrument",
        InstrumentAny::OptionContract(_) => "OptionContract",
        InstrumentAny::OptionSpread(_) => "OptionSpread",
        InstrumentAny::PerpetualContract(_) => "PerpetualContract",
        InstrumentAny::TokenizedAsset(_) => "TokenizedAsset",
    }
}

/// Encodes instruments as a display-friendly Arrow [`RecordBatch`].
///
/// Emits a single schema built from the common [`Instrument`] trait surface.
/// `Utf8` columns carry identifiers and enum names, `Float64` columns carry
/// prices/quantities/fees, `Timestamp(Nanosecond)` columns carry activation,
/// expiration, and bookkeeping timestamps, and `Boolean` columns carry
/// `is_inverse`/`is_quanto`. Trait accessors that are not applicable to a
/// row (e.g. `strike_price` on a spot pair) emit as nulls, so mixed-type
/// batches round-trip cleanly. Variant-only metadata not reachable through
/// the trait is intentionally omitted; see the module-level comment.
///
/// Returns an empty [`RecordBatch`] with the correct schema when `data` is empty.
///
/// # Errors
///
/// Returns an [`ArrowError`] if the Arrow `RecordBatch` cannot be constructed.
pub fn encode_instruments(data: &[InstrumentAny]) -> Result<RecordBatch, ArrowError> {
    let mut instrument_id = StringBuilder::new();
    let mut symbol = StringBuilder::new();
    let mut venue = StringBuilder::new();
    let mut instrument_type = StringBuilder::new();
    let mut raw_symbol = StringBuilder::new();
    let mut asset_class = StringBuilder::new();
    let mut instrument_class = StringBuilder::new();
    let mut underlying = StringBuilder::new();
    let mut base_currency = StringBuilder::new();
    let mut quote_currency = StringBuilder::new();
    let mut settlement_currency = StringBuilder::new();
    let mut isin = StringBuilder::new();
    let mut option_kind = StringBuilder::new();
    let mut exchange = StringBuilder::new();
    let mut strike_price = Float64Builder::with_capacity(data.len());
    let mut activation_ns = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut expiration_ns = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut is_inverse = BooleanBuilder::with_capacity(data.len());
    let mut is_quanto = BooleanBuilder::with_capacity(data.len());
    let mut price_precision = UInt8Builder::with_capacity(data.len());
    let mut size_precision = UInt8Builder::with_capacity(data.len());
    let mut price_increment = Float64Builder::with_capacity(data.len());
    let mut size_increment = Float64Builder::with_capacity(data.len());
    let mut multiplier = Float64Builder::with_capacity(data.len());
    let mut lot_size = Float64Builder::with_capacity(data.len());
    let mut max_quantity = Float64Builder::with_capacity(data.len());
    let mut min_quantity = Float64Builder::with_capacity(data.len());
    let mut max_notional_amount = Float64Builder::with_capacity(data.len());
    let mut max_notional_currency = StringBuilder::new();
    let mut min_notional_amount = Float64Builder::with_capacity(data.len());
    let mut min_notional_currency = StringBuilder::new();
    let mut max_price = Float64Builder::with_capacity(data.len());
    let mut min_price = Float64Builder::with_capacity(data.len());
    let mut margin_init = Float64Builder::with_capacity(data.len());
    let mut margin_maint = Float64Builder::with_capacity(data.len());
    let mut maker_fee = Float64Builder::with_capacity(data.len());
    let mut taker_fee = Float64Builder::with_capacity(data.len());
    let mut ts_event = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut ts_init = TimestampNanosecondBuilder::with_capacity(data.len());

    for instrument in data {
        instrument_id.append_value(instrument.id().to_string());
        symbol.append_value(instrument.symbol());
        venue.append_value(instrument.venue());
        instrument_type.append_value(instrument_type_name(instrument));
        raw_symbol.append_value(instrument.raw_symbol());
        asset_class.append_value(format!("{}", instrument.asset_class()));
        instrument_class.append_value(format!("{}", instrument.instrument_class()));
        underlying.append_option(instrument.underlying().map(|v| v.to_string()));
        base_currency.append_option(instrument.base_currency().map(|v| v.to_string()));
        quote_currency.append_value(instrument.quote_currency().to_string());
        settlement_currency.append_value(instrument.settlement_currency().to_string());
        isin.append_option(instrument.isin().map(|v| v.to_string()));
        option_kind.append_option(instrument.option_kind().map(|v| format!("{v}")));
        exchange.append_option(instrument.exchange().map(|v| v.to_string()));
        strike_price.append_option(instrument.strike_price().map(|v| price_to_f64(&v)));
        activation_ns.append_option(
            instrument
                .activation_ns()
                .map(|v| unix_nanos_to_i64(v.as_u64())),
        );
        expiration_ns.append_option(
            instrument
                .expiration_ns()
                .map(|v| unix_nanos_to_i64(v.as_u64())),
        );
        is_inverse.append_value(instrument.is_inverse());
        is_quanto.append_value(instrument.is_quanto());
        price_precision.append_value(instrument.price_precision());
        size_precision.append_value(instrument.size_precision());
        price_increment.append_value(price_to_f64(&instrument.price_increment()));
        size_increment.append_value(quantity_to_f64(&instrument.size_increment()));
        multiplier.append_value(quantity_to_f64(&instrument.multiplier()));
        lot_size.append_option(instrument.lot_size().map(|v| quantity_to_f64(&v)));
        max_quantity.append_option(instrument.max_quantity().map(|v| quantity_to_f64(&v)));
        min_quantity.append_option(instrument.min_quantity().map(|v| quantity_to_f64(&v)));
        max_notional_amount.append_option(instrument.max_notional().map(|v| money_to_f64(&v)));
        max_notional_currency
            .append_option(instrument.max_notional().map(|v| v.currency.to_string()));
        min_notional_amount.append_option(instrument.min_notional().map(|v| money_to_f64(&v)));
        min_notional_currency
            .append_option(instrument.min_notional().map(|v| v.currency.to_string()));
        max_price.append_option(instrument.max_price().map(|v| price_to_f64(&v)));
        min_price.append_option(instrument.min_price().map(|v| price_to_f64(&v)));
        margin_init.append_value(instrument.margin_init().to_f64().unwrap_or(f64::NAN));
        margin_maint.append_value(instrument.margin_maint().to_f64().unwrap_or(f64::NAN));
        maker_fee.append_value(instrument.maker_fee().to_f64().unwrap_or(f64::NAN));
        taker_fee.append_value(instrument.taker_fee().to_f64().unwrap_or(f64::NAN));
        ts_event.append_value(unix_nanos_to_i64(instrument.ts_event().as_u64()));
        ts_init.append_value(unix_nanos_to_i64(instrument.ts_init().as_u64()));
    }

    RecordBatch::try_new(
        Arc::new(instrument_schema()),
        vec![
            Arc::new(instrument_id.finish()),
            Arc::new(symbol.finish()),
            Arc::new(venue.finish()),
            Arc::new(instrument_type.finish()),
            Arc::new(raw_symbol.finish()),
            Arc::new(asset_class.finish()),
            Arc::new(instrument_class.finish()),
            Arc::new(underlying.finish()),
            Arc::new(base_currency.finish()),
            Arc::new(quote_currency.finish()),
            Arc::new(settlement_currency.finish()),
            Arc::new(isin.finish()),
            Arc::new(option_kind.finish()),
            Arc::new(exchange.finish()),
            Arc::new(strike_price.finish()),
            Arc::new(activation_ns.finish()),
            Arc::new(expiration_ns.finish()),
            Arc::new(is_inverse.finish()),
            Arc::new(is_quanto.finish()),
            Arc::new(price_precision.finish()),
            Arc::new(size_precision.finish()),
            Arc::new(price_increment.finish()),
            Arc::new(size_increment.finish()),
            Arc::new(multiplier.finish()),
            Arc::new(lot_size.finish()),
            Arc::new(max_quantity.finish()),
            Arc::new(min_quantity.finish()),
            Arc::new(max_notional_amount.finish()),
            Arc::new(max_notional_currency.finish()),
            Arc::new(min_notional_amount.finish()),
            Arc::new(min_notional_currency.finish()),
            Arc::new(max_price.finish()),
            Arc::new(min_price.finish()),
            Arc::new(margin_init.finish()),
            Arc::new(margin_maint.finish()),
            Arc::new(maker_fee.finish()),
            Arc::new(taker_fee.finish()),
            Arc::new(ts_event.finish()),
            Arc::new(ts_init.finish()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{Array, BooleanArray, Float64Array, StringArray, TimestampNanosecondArray},
        datatypes::{DataType, TimeUnit},
    };
    use nautilus_model::{
        instruments::{
            InstrumentAny,
            stubs::{
                betting, binary_option, cfd_gold, commodity_gold, crypto_future_btcusdt,
                crypto_option_btc_deribit, crypto_perpetual_ethusdt, currency_pair_btcusdt,
                equity_aapl, futures_contract_es, futures_spread_es, index_instrument_spx,
                option_contract_appl, option_spread, perpetual_contract_eurusd,
                tokenized_asset_aaplx, xbtusd_bitmex,
            },
        },
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    fn spot() -> InstrumentAny {
        InstrumentAny::CurrencyPair(currency_pair_btcusdt())
    }

    fn all_variants() -> Vec<(InstrumentAny, &'static str)> {
        vec![
            (InstrumentAny::Betting(betting()), "BettingInstrument"),
            (InstrumentAny::BinaryOption(binary_option()), "BinaryOption"),
            (InstrumentAny::Cfd(cfd_gold()), "Cfd"),
            (InstrumentAny::Commodity(commodity_gold()), "Commodity"),
            (
                InstrumentAny::CryptoFuture(crypto_future_btcusdt(
                    2,
                    6,
                    Price::from("0.01"),
                    Quantity::from("0.000001"),
                )),
                "CryptoFuture",
            ),
            (
                InstrumentAny::CryptoOption(crypto_option_btc_deribit(
                    3,
                    1,
                    Price::from("0.001"),
                    Quantity::from("0.1"),
                )),
                "CryptoOption",
            ),
            (
                InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()),
                "CryptoPerpetual",
            ),
            (
                InstrumentAny::CurrencyPair(currency_pair_btcusdt()),
                "CurrencyPair",
            ),
            (InstrumentAny::Equity(equity_aapl()), "Equity"),
            (
                InstrumentAny::FuturesContract(futures_contract_es(None, None)),
                "FuturesContract",
            ),
            (
                InstrumentAny::FuturesSpread(futures_spread_es()),
                "FuturesSpread",
            ),
            (
                InstrumentAny::IndexInstrument(index_instrument_spx()),
                "IndexInstrument",
            ),
            (
                InstrumentAny::OptionContract(option_contract_appl()),
                "OptionContract",
            ),
            (InstrumentAny::OptionSpread(option_spread()), "OptionSpread"),
            (
                InstrumentAny::PerpetualContract(perpetual_contract_eurusd()),
                "PerpetualContract",
            ),
            (
                InstrumentAny::TokenizedAsset(tokenized_asset_aaplx()),
                "TokenizedAsset",
            ),
        ]
    }

    #[rstest]
    fn test_encode_instruments_schema() {
        let batch = encode_instruments(&[]).unwrap();
        let schema = batch.schema();
        let fields = schema.fields();
        assert_eq!(fields.len(), 39);
        assert_eq!(fields[0].name(), "instrument_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);
        assert_eq!(fields[14].name(), "strike_price");
        assert_eq!(fields[14].data_type(), &DataType::Float64);
        assert_eq!(fields[17].name(), "is_inverse");
        assert_eq!(fields[17].data_type(), &DataType::Boolean);
        assert_eq!(fields[19].name(), "price_precision");
        assert_eq!(fields[19].data_type(), &DataType::UInt8);
        assert_eq!(fields[33].name(), "margin_init");
        assert_eq!(fields[33].data_type(), &DataType::Float64);
        assert_eq!(fields[36].name(), "taker_fee");
        assert_eq!(fields[37].name(), "ts_event");
        assert_eq!(
            fields[37].data_type(),
            &DataType::Timestamp(TimeUnit::Nanosecond, None)
        );
    }

    #[rstest]
    fn test_encode_instruments_empty() {
        let batch = encode_instruments(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.schema().fields().len(), 39);
    }

    #[rstest]
    fn test_encode_instruments_spot_values() {
        let instruments = vec![spot()];
        let batch = encode_instruments(&instruments).unwrap();

        assert_eq!(batch.num_rows(), 1);

        let instrument_type_col = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let strike_price_col = batch
            .column(14)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let activation_col = batch
            .column(15)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let expiration_col = batch
            .column(16)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let is_inverse_col = batch
            .column(17)
            .as_any()
            .downcast_ref::<BooleanArray>()
            .unwrap();
        let price_increment_col = batch
            .column(21)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();

        assert_eq!(instrument_type_col.value(0), "CurrencyPair");
        assert!(strike_price_col.is_null(0));
        assert!(activation_col.is_null(0));
        assert!(expiration_col.is_null(0));
        assert!(!is_inverse_col.value(0));
        assert!(price_increment_col.value(0) > 0.0);
    }

    #[rstest]
    fn test_encode_instruments_mixed_variants_preserves_per_row_nulls() {
        let instruments = vec![
            spot(),
            InstrumentAny::Equity(equity_aapl()),
            InstrumentAny::OptionContract(option_contract_appl()),
        ];
        let batch = encode_instruments(&instruments).unwrap();

        assert_eq!(batch.num_rows(), 3);

        let instrument_type_col = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let strike_price_col = batch
            .column(14)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let expiration_col = batch
            .column(16)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let base_currency_col = batch
            .column(8)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();

        assert_eq!(instrument_type_col.value(0), "CurrencyPair");
        assert_eq!(instrument_type_col.value(1), "Equity");
        assert_eq!(instrument_type_col.value(2), "OptionContract");

        // Only the option carries a strike + expiration
        assert!(strike_price_col.is_null(0));
        assert!(strike_price_col.is_null(1));
        assert!(!strike_price_col.is_null(2));
        assert!(expiration_col.is_null(0));
        assert!(expiration_col.is_null(1));
        assert!(!expiration_col.is_null(2));

        // Only the spot pair carries a base currency
        assert!(!base_currency_col.is_null(0));
        assert!(base_currency_col.is_null(1));
    }

    #[rstest]
    fn test_encode_instruments_shared_schema_across_batches() {
        let a = encode_instruments(&[spot()]).unwrap();
        let b = encode_instruments(&[InstrumentAny::Equity(equity_aapl())]).unwrap();
        assert_eq!(a.schema(), b.schema());
    }

    #[rstest]
    fn test_encode_instruments_all_variant_names() {
        let variants = all_variants();
        assert_eq!(variants.len(), 16, "all InstrumentAny variants covered");

        let instruments: Vec<InstrumentAny> = variants.iter().map(|(v, _)| v.clone()).collect();
        let batch = encode_instruments(&instruments).unwrap();
        let instrument_type_col = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();

        for (row, (_, expected)) in variants.iter().enumerate() {
            assert_eq!(instrument_type_col.value(row), *expected);
        }
    }

    #[rstest]
    fn test_encode_instruments_inverse_perpetual() {
        let instruments = vec![InstrumentAny::CryptoPerpetual(xbtusd_bitmex())];
        let batch = encode_instruments(&instruments).unwrap();

        let instrument_type_col = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let settlement_currency_col = batch
            .column(10)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let is_inverse_col = batch
            .column(17)
            .as_any()
            .downcast_ref::<BooleanArray>()
            .unwrap();
        let max_notional_amount_col = batch
            .column(27)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let max_notional_currency_col = batch
            .column(28)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let min_notional_amount_col = batch
            .column(29)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let min_notional_currency_col = batch
            .column(30)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();

        assert_eq!(instrument_type_col.value(0), "CryptoPerpetual");
        assert_eq!(settlement_currency_col.value(0), "BTC");
        assert!(is_inverse_col.value(0));
        assert!((max_notional_amount_col.value(0) - 10_000_000.0).abs() < 1e-9);
        assert_eq!(max_notional_currency_col.value(0), "USD");
        assert!((min_notional_amount_col.value(0) - 1.0).abs() < 1e-9);
        assert_eq!(min_notional_currency_col.value(0), "USD");

        let margin_init_col = batch
            .column(33)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let margin_maint_col = batch
            .column(34)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let maker_fee_col = batch
            .column(35)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let taker_fee_col = batch
            .column(36)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();

        assert!((margin_init_col.value(0) - 0.01).abs() < 1e-9);
        assert!((margin_maint_col.value(0) - 0.0035).abs() < 1e-9);
        assert!((maker_fee_col.value(0) - (-0.00025)).abs() < 1e-9);
        assert!((taker_fee_col.value(0) - 0.00075).abs() < 1e-9);
    }
}
