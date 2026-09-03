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

//! Arrow serialization for instruments.
//!
//! `InstrumentAny` acts as a dispatcher that routes to the appropriate concrete instrument type's
//! Arrow serialization implementation. Each concrete instrument type implements its own schema
//! with all fields as columns (wide schema approach), matching the Python implementation.

use std::{any::type_name, collections::HashMap, fmt, str::FromStr};

use arrow::{
    array::{Array, StringArray},
    datatypes::Schema,
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    enums::{AssetClass, OptionKind},
    instruments::{
        Instrument, InstrumentAny, betting::BettingInstrument, binary_option::BinaryOption,
        cfd::Cfd, commodity::Commodity, crypto_future::CryptoFuture,
        crypto_futures_spread::CryptoFuturesSpread, crypto_option::CryptoOption,
        crypto_option_spread::CryptoOptionSpread, crypto_perpetual::CryptoPerpetual,
        currency_pair::CurrencyPair, equity::Equity, futures_contract::FuturesContract,
        futures_spread::FuturesSpread, index_instrument::IndexInstrument,
        option_contract::OptionContract, option_spread::OptionSpread,
        perpetual_contract::PerpetualContract, tokenized_asset::TokenizedAsset,
    },
    types::{Currency, Price, Quantity},
};

use crate::arrow::{ArrowSchemaProvider, EncodeToRecordBatch, EncodingError, KEY_INSTRUMENT_ID};

pub mod betting;
pub mod binary_option;
pub mod cfd;
pub mod commodity;
pub mod crypto_future;
pub mod crypto_futures_spread;
pub mod crypto_option;
pub mod crypto_option_spread;
pub mod crypto_perpetual;
pub mod currency_pair;
pub mod equity;
pub mod futures_contract;
pub mod futures_spread;
pub mod index_instrument;
pub mod option_contract;
pub mod option_spread;
pub mod perpetual_contract;
pub mod tokenized_asset;

// Columns added after the original schemas are read by name and yield `None` when absent,
// so fragments written before the column existed decode exactly as they did previously.
pub(crate) fn optional_quantity_value(
    values: Option<&StringArray>,
    field: &'static str,
    row: usize,
) -> Result<Option<Quantity>, EncodingError> {
    let Some(column) = values else {
        return Ok(None);
    };

    if column.is_null(row) {
        return Ok(None);
    }

    Quantity::from_str(column.value(row))
        .map(Some)
        .map_err(|e| EncodingError::ParseError(field, format!("row {row}: {e}")))
}

pub(crate) fn optional_price_value(
    values: Option<&StringArray>,
    field: &'static str,
    row: usize,
) -> Result<Option<Price>, EncodingError> {
    let Some(column) = values else {
        return Ok(None);
    };

    if column.is_null(row) {
        return Ok(None);
    }

    Price::from_str(column.value(row))
        .map(Some)
        .map_err(|e| EncodingError::ParseError(field, format!("row {row}: {e}")))
}

// Errors on empty/whitespace codes so corrupted rows surface as ParseError,
// instead of silently registering as a fallback currency. Known codes resolve
// from CURRENCY_MAP with original metadata; unknown non-empty codes fall back
// to a new crypto currency to support newly listed exchange assets.
pub(crate) fn decode_currency(
    value: &str,
    field: &'static str,
    context: &'static str,
    row: usize,
) -> Result<Currency, EncodingError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(EncodingError::ParseError(
            field,
            format!("row {row}: empty currency code"),
        ));
    }

    Ok(Currency::get_or_create_crypto_with_context(
        trimmed,
        Some(context),
    ))
}

// The instrument modules that call these write the enums as PascalCase, which differs from the
// `Display`/`FromStr` impls on the enums themselves (SCREAMING_SNAKE_CASE), so the mapping is
// written out rather than derived. Other modules encode `asset_class` through the enum's own
// impls instead, so a column's format follows the module that writes it.
pub(crate) fn asset_class_to_string(value: AssetClass) -> String {
    match value {
        AssetClass::FX => "FX".to_string(),
        AssetClass::Equity => "Equity".to_string(),
        AssetClass::Commodity => "Commodity".to_string(),
        AssetClass::Debt => "Debt".to_string(),
        AssetClass::Index => "Index".to_string(),
        AssetClass::Cryptocurrency => "Cryptocurrency".to_string(),
        AssetClass::Alternative => "Alternative".to_string(),
    }
}

pub(crate) fn asset_class_from_str(value: &str) -> Result<AssetClass, EncodingError> {
    match value {
        "FX" => Ok(AssetClass::FX),
        "Equity" => Ok(AssetClass::Equity),
        "Commodity" => Ok(AssetClass::Commodity),
        "Debt" => Ok(AssetClass::Debt),
        "Index" => Ok(AssetClass::Index),
        "Cryptocurrency" => Ok(AssetClass::Cryptocurrency),
        "Alternative" => Ok(AssetClass::Alternative),
        _ => Err(EncodingError::ParseError(
            "asset_class",
            format!("Unknown asset class: {value}"),
        )),
    }
}

pub(crate) fn option_kind_to_string(value: OptionKind) -> String {
    match value {
        OptionKind::Call => "Call".to_string(),
        OptionKind::Put => "Put".to_string(),
    }
}

pub(crate) fn option_kind_from_str(value: &str) -> Result<OptionKind, EncodingError> {
    match value {
        "Call" => Ok(OptionKind::Call),
        "Put" => Ok(OptionKind::Put),
        _ => Err(EncodingError::ParseError(
            "option_kind",
            format!("Unknown option kind: {value}"),
        )),
    }
}

pub(crate) const KEY_CLASS: &str = "class";

const INSTRUMENT_VALIDATION_FIELD: &str = "instrument";

pub(crate) fn instrument_validation_error<T>(
    row: usize,
    error: impl fmt::Display,
) -> EncodingError {
    let type_name = type_name::<T>();
    let instrument_type = type_name.rsplit("::").next().unwrap_or(type_name);

    EncodingError::ParseError(
        INSTRUMENT_VALIDATION_FIELD,
        format!("row {row}: invalid {instrument_type}: {error}"),
    )
}

/// Wires every [`InstrumentAny`] variant into the Arrow schema, encode, and decode paths from a
/// single table of `(variant, instrument type, class name, decode function)` rows.
macro_rules! impl_instrument_any_arrow {
    ($(($variant:ident, $instrument:ty, $class:literal, $decode:path)),+ $(,)?) => {
        impl ArrowSchemaProvider for InstrumentAny {
            fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
                let class = metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get(KEY_CLASS))
                    .map(String::as_str);

                match class {
                    $(Some($class) => <$instrument>::get_schema(metadata),)+
                    // Batches written without a class, or with one this build does not know,
                    // decode against the `CurrencyPair` schema; that was the only instrument
                    // schema when the column was introduced.
                    _ => CurrencyPair::get_schema(metadata),
                }
            }
        }

        impl EncodeToRecordBatch for InstrumentAny {
            fn encode_batch(
                metadata: &HashMap<String, String>,
                data: &[Self],
            ) -> Result<RecordBatch, ArrowError> {
                let Some(first) = data.first() else {
                    return Err(ArrowError::InvalidArgumentError(
                        "Cannot encode empty instrument batch".to_string(),
                    ));
                };

                match first {
                    $(Self::$variant(_) => {
                        let mut instruments = Vec::with_capacity(data.len());

                        for instrument in data {
                            let Self::$variant(instrument) = instrument else {
                                return Err(mixed_instrument_types());
                            };

                            instruments.push(instrument.clone());
                        }

                        <$instrument>::encode_batch(metadata, &instruments)
                    })+
                }
            }

            fn metadata(&self) -> HashMap<String, String> {
                let class = match self {
                    $(Self::$variant(_) => $class,)+
                };

                HashMap::from([
                    (KEY_INSTRUMENT_ID.to_string(), Instrument::id(self).to_string()),
                    (KEY_CLASS.to_string(), class.to_string()),
                ])
            }
        }

        fn decode_batch_for_class(
            class: &str,
            metadata: &HashMap<String, String>,
            record_batch: &RecordBatch,
        ) -> Result<Vec<InstrumentAny>, EncodingError> {
            match class {
                $($class => Ok($decode(metadata, record_batch)?
                    .into_iter()
                    .map(InstrumentAny::$variant)
                    .collect()),)+
                _ => Err(EncodingError::ParseError(
                    KEY_CLASS,
                    format!("Unknown instrument type: {class}"),
                )),
            }
        }
    };
}

fn mixed_instrument_types() -> ArrowError {
    ArrowError::InvalidArgumentError(
        "Cannot encode mixed instrument types in a single batch. Use separate batches for each type."
            .to_string(),
    )
}

impl_instrument_any_arrow!(
    (
        Betting,
        BettingInstrument,
        "BettingInstrument",
        betting::decode_betting_instrument_batch
    ),
    (
        BinaryOption,
        BinaryOption,
        "BinaryOption",
        binary_option::decode_binary_option_batch
    ),
    (Cfd, Cfd, "Cfd", cfd::decode_cfd_batch),
    (
        Commodity,
        Commodity,
        "Commodity",
        commodity::decode_commodity_batch
    ),
    (
        CryptoFuture,
        CryptoFuture,
        "CryptoFuture",
        crypto_future::decode_crypto_future_batch
    ),
    (
        CryptoFuturesSpread,
        CryptoFuturesSpread,
        "CryptoFuturesSpread",
        crypto_futures_spread::decode_crypto_futures_spread_batch
    ),
    (
        CryptoOption,
        CryptoOption,
        "CryptoOption",
        crypto_option::decode_crypto_option_batch
    ),
    (
        CryptoOptionSpread,
        CryptoOptionSpread,
        "CryptoOptionSpread",
        crypto_option_spread::decode_crypto_option_spread_batch
    ),
    (
        CryptoPerpetual,
        CryptoPerpetual,
        "CryptoPerpetual",
        crypto_perpetual::decode_crypto_perpetual_batch
    ),
    (
        CurrencyPair,
        CurrencyPair,
        "CurrencyPair",
        currency_pair::decode_currency_pair_batch
    ),
    (Equity, Equity, "Equity", equity::decode_equity_batch),
    (
        FuturesContract,
        FuturesContract,
        "FuturesContract",
        futures_contract::decode_futures_contract_batch
    ),
    (
        FuturesSpread,
        FuturesSpread,
        "FuturesSpread",
        futures_spread::decode_futures_spread_batch
    ),
    (
        IndexInstrument,
        IndexInstrument,
        "IndexInstrument",
        index_instrument::decode_index_instrument_batch
    ),
    (
        OptionContract,
        OptionContract,
        "OptionContract",
        option_contract::decode_option_contract_batch
    ),
    (
        OptionSpread,
        OptionSpread,
        "OptionSpread",
        option_spread::decode_option_spread_batch
    ),
    (
        PerpetualContract,
        PerpetualContract,
        "PerpetualContract",
        perpetual_contract::decode_perpetual_contract_batch
    ),
    (
        TokenizedAsset,
        TokenizedAsset,
        "TokenizedAsset",
        tokenized_asset::decode_tokenized_asset_batch
    ),
);

/// Decodes `InstrumentAny` values from a record batch.
///
/// Not a [`DecodeFromRecordBatch`] implementation because that trait requires `Into<Data>`.
///
/// # Errors
///
/// Returns an `EncodingError` if the record batch cannot be decoded.
///
/// [`DecodeFromRecordBatch`]: crate::arrow::DecodeFromRecordBatch
pub fn decode_instrument_any_batch(
    metadata: &HashMap<String, String>,
    record_batch: &RecordBatch,
) -> Result<Vec<InstrumentAny>, EncodingError> {
    let class = metadata
        .get(KEY_CLASS)
        .map(String::as_str)
        .ok_or(EncodingError::MissingMetadata(KEY_CLASS))?;

    decode_batch_for_class(class, metadata, record_batch)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::array::{ArrayRef, StringArray, UInt8Array};
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        enums::{AssetClass, CurrencyType, OptionKind},
        identifiers::{InstrumentId, Symbol},
        instruments::{
            Instrument, InstrumentAny,
            currency_pair::CurrencyPair,
            stubs::{betting, currency_pair_btcusdt, equity_aapl},
        },
        types::{Currency, Money, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;

    #[rstest]
    fn test_get_schema() {
        let mut metadata = HashMap::new();
        metadata.insert(KEY_CLASS.to_string(), "CurrencyPair".to_string());
        let schema = InstrumentAny::get_schema(Some(metadata));
        assert!(schema.fields().len() >= 20);
        assert_eq!(schema.field(0).name(), "id");
    }

    #[rstest]
    fn test_encode_batch_rejects_mixed_instrument_types() {
        let instruments = [
            InstrumentAny::CurrencyPair(currency_pair_btcusdt()),
            InstrumentAny::Equity(equity_aapl()),
        ];

        let error = InstrumentAny::encode_batch(&HashMap::new(), &instruments).unwrap_err();

        let ArrowError::InvalidArgumentError(message) = error else {
            panic!("unexpected error variant: {error:?}");
        };
        assert_eq!(
            message,
            "Cannot encode mixed instrument types in a single batch. Use separate batches for each type."
        );
    }

    #[rstest]
    #[case("")]
    #[case("   ")]
    #[case("\t\n")]
    fn test_decode_currency_empty_or_whitespace_errors(#[case] value: &str) {
        let result = decode_currency(value, "currency", "test.currency", 7);
        let err = result.expect_err("empty code must surface EncodingError");
        match err {
            EncodingError::ParseError(field, msg) => {
                assert_eq!(field, "currency");
                assert!(
                    msg.contains("row 7"),
                    "message should include row index, found: {msg}",
                );
                assert!(
                    msg.contains("empty currency code"),
                    "message should describe empty code, found: {msg}",
                );
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
        // Ensure the fallback did not register a phantom currency under the empty key.
        assert!(Currency::try_from_str(value.trim()).is_none());
    }

    #[rstest]
    #[case("USD", CurrencyType::Fiat, 2)]
    #[case("BTC", CurrencyType::Crypto, 8)]
    #[case("XAU", CurrencyType::CommodityBacked, 2)]
    fn test_decode_currency_known_code_preserves_metadata(
        #[case] code: &str,
        #[case] expected_type: CurrencyType,
        #[case] expected_precision: u8,
    ) {
        let currency = decode_currency(code, "currency", "test.currency", 0).unwrap();
        assert_eq!(currency.code.as_str(), code);
        assert_eq!(currency.currency_type, expected_type);
        assert_eq!(currency.precision, expected_precision);
    }

    #[rstest]
    fn test_decode_currency_unknown_code_registers_as_crypto() {
        let code = "XDECTEST";
        assert!(
            Currency::try_from_str(code).is_none(),
            "test precondition: '{code}' must not be pre-registered",
        );

        let currency = decode_currency(code, "base_currency", "test.base_currency", 0).unwrap();
        assert_eq!(currency.code.as_str(), code);
        assert_eq!(currency.currency_type, CurrencyType::Crypto);
        assert_eq!(currency.precision, 8);
        assert_eq!(currency.iso4217, 0);

        let registered = Currency::try_from_str(code).expect("unknown code must be registered");
        assert_eq!(registered, currency);
    }

    #[rstest]
    fn test_encode_decode_round_trip() {
        let instrument_id = InstrumentId::from("EUR/USD.SIM");
        let currency_pair = CurrencyPair::builder()
            .instrument_id(instrument_id)
            .raw_symbol(Symbol::from("EUR/USD"))
            .base_currency(Currency::from("EUR"))
            .quote_currency(Currency::from("USD"))
            .price_precision(5)
            // size_precision must match size_increment precision (0)
            .size_precision(0)
            .price_increment(Price::new(0.00001, 5))
            // precision 0
            .size_increment(Quantity::new(1.0, 0))
            .tick_scheme(Ustr::from("FOREX_5DECIMAL"))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap();
        let instrument = InstrumentAny::CurrencyPair(currency_pair);

        let metadata = instrument.metadata();
        let record_batch =
            InstrumentAny::encode_batch(&metadata, std::slice::from_ref(&instrument)).unwrap();
        let decoded = decode_instrument_any_batch(&metadata, &record_batch).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(Instrument::id(&decoded[0]), Instrument::id(&instrument));
        assert_eq!(
            Instrument::raw_symbol(&decoded[0]),
            Instrument::raw_symbol(&instrument)
        );
        assert_eq!(
            Instrument::asset_class(&decoded[0]),
            Instrument::asset_class(&instrument)
        );

        match (&decoded[0], &instrument) {
            (InstrumentAny::CurrencyPair(decoded_cp), InstrumentAny::CurrencyPair(original_cp)) => {
                assert_eq!(decoded_cp.id, original_cp.id);
                assert_eq!(decoded_cp.base_currency, original_cp.base_currency);
                assert_eq!(decoded_cp.quote_currency, original_cp.quote_currency);
                assert_eq!(decoded_cp.price_precision, original_cp.price_precision);
                assert_eq!(decoded_cp.size_precision, original_cp.size_precision);
                assert_eq!(decoded_cp.tick_scheme, original_cp.tick_scheme);
            }
            _ => panic!("Decoded instrument type mismatch"),
        }
    }

    #[rstest]
    fn test_decode_currency_pair_without_tick_scheme_column_defaults_none() {
        let instrument_id = InstrumentId::from("EUR/USD.SIM");
        let currency_pair = CurrencyPair::builder()
            .instrument_id(instrument_id)
            .raw_symbol(Symbol::from("EUR/USD"))
            .base_currency(Currency::from("EUR"))
            .quote_currency(Currency::from("USD"))
            .price_precision(5)
            .size_precision(0)
            .price_increment(Price::new(0.00001, 5))
            .size_increment(Quantity::new(1.0, 0))
            .tick_scheme(Ustr::from("FOREX_5DECIMAL"))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap();
        let instrument = InstrumentAny::CurrencyPair(currency_pair);

        let metadata = instrument.metadata();
        let record_batch =
            InstrumentAny::encode_batch(&metadata, std::slice::from_ref(&instrument)).unwrap();
        let record_batch = batch_without_column(&record_batch, "tick_scheme");
        let decoded = decode_instrument_any_batch(&metadata, &record_batch).unwrap();

        assert_eq!(decoded.len(), 1);
        match &decoded[0] {
            InstrumentAny::CurrencyPair(decoded_cp) => {
                assert_eq!(decoded_cp.id, instrument.id());
                assert_eq!(decoded_cp.tick_scheme, None);
            }
            _ => panic!("Decoded instrument type mismatch"),
        }
    }

    #[rstest]
    fn test_encode_decode_round_trip_equity() {
        use nautilus_model::instruments::{Instrument, equity::Equity};

        let instrument_id = InstrumentId::from("AAPL.NASDAQ");
        let equity = Equity::builder()
            .instrument_id(instrument_id)
            .raw_symbol(Symbol::from("AAPL"))
            .currency(Currency::from("USD"))
            .price_precision(2)
            .price_increment(Price::new(0.01, 2))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap();
        let instrument = InstrumentAny::Equity(equity);

        let metadata = instrument.metadata();
        let record_batch =
            InstrumentAny::encode_batch(&metadata, std::slice::from_ref(&instrument)).unwrap();
        let decoded = decode_instrument_any_batch(&metadata, &record_batch).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(Instrument::id(&decoded[0]), Instrument::id(&instrument));
        assert_eq!(
            Instrument::raw_symbol(&decoded[0]),
            Instrument::raw_symbol(&instrument)
        );
        assert_eq!(
            Instrument::asset_class(&decoded[0]),
            Instrument::asset_class(&instrument)
        );

        match (&decoded[0], &instrument) {
            (InstrumentAny::Equity(decoded_eq), InstrumentAny::Equity(original_eq)) => {
                assert_eq!(decoded_eq.id, original_eq.id);
                assert_eq!(decoded_eq.currency, original_eq.currency);
                assert_eq!(decoded_eq.price_precision, original_eq.price_precision);
            }
            _ => panic!("Decoded instrument type mismatch"),
        }
    }

    #[rstest]
    fn test_encode_decode_round_trip_equity_all_fields() {
        use nautilus_core::Params;

        let mut info = Params::new();
        info.insert("sector".to_string(), serde_json::json!("technology"));

        let equity = Equity::builder()
            .instrument_id(InstrumentId::from("AAPL.NASDAQ"))
            .raw_symbol(Symbol::from("AAPL"))
            .isin(Ustr::from("US0378331005"))
            .currency(Currency::from("USD"))
            .price_precision(2)
            .price_increment(Price::from("0.01"))
            .lot_size(Quantity::from("100"))
            .max_quantity(Quantity::from("10000"))
            .min_quantity(Quantity::from("1"))
            .max_price(Price::from("9999.99"))
            .min_price(Price::from("0.01"))
            .margin_init(dec!(0.01))
            .margin_maint(dec!(0.02))
            .maker_fee(dec!(0.0002))
            .taker_fee(dec!(0.0004))
            .tick_scheme(Ustr::from("TOPIX100"))
            .info(info)
            .ts_event(1.into())
            .ts_init(2.into())
            .build()
            .unwrap();
        let instrument = InstrumentAny::Equity(equity.clone());

        let metadata = instrument.metadata();
        let record_batch =
            InstrumentAny::encode_batch(&metadata, std::slice::from_ref(&instrument)).unwrap();
        let decoded = decode_instrument_any_batch(&metadata, &record_batch).unwrap();

        assert_eq!(decoded.len(), 1);
        let InstrumentAny::Equity(decoded_equity) = &decoded[0] else {
            panic!("Decoded instrument type mismatch");
        };

        // The v1 `from_dict` dropped these quantity constraints (#4461), so check them here
        assert_eq!(decoded_equity.max_quantity, equity.max_quantity);
        assert_eq!(decoded_equity.min_quantity, equity.min_quantity);

        // `PartialEq` compares only `id`, so compare every field via its serialized form
        assert_eq!(
            serde_json::to_value(decoded_equity).unwrap(),
            serde_json::to_value(&equity).unwrap(),
        );
    }

    #[rstest]
    fn test_encode_decode_round_trip_futures_contract_all_fields() {
        let contract = FuturesContract::builder()
            .instrument_id(InstrumentId::from("ESZ4.XCME"))
            .raw_symbol(Symbol::from("ESZ4"))
            .asset_class(AssetClass::Index)
            .exchange(Ustr::from("XCME"))
            .underlying(Ustr::from("ES"))
            .activation_ns(1.into())
            .expiration_ns(2.into())
            .currency(Currency::from("USD"))
            .price_precision(2)
            .price_increment(Price::from("0.01"))
            .multiplier(Quantity::from("1"))
            .lot_size(Quantity::from("1"))
            .max_quantity(Quantity::from("10000"))
            .min_quantity(Quantity::from("5"))
            .max_price(Price::from("9999.99"))
            .min_price(Price::from("0.01"))
            .margin_init(dec!(0.01))
            .margin_maint(dec!(0.02))
            .maker_fee(dec!(0.0002))
            .taker_fee(dec!(0.0004))
            .ts_event(1.into())
            .ts_init(2.into())
            .build()
            .unwrap();

        let decoded = encode_decode_instrument(&InstrumentAny::FuturesContract(contract.clone()));
        let InstrumentAny::FuturesContract(decoded) = decoded else {
            panic!("Decoded instrument type mismatch");
        };

        assert_eq!(
            serde_json::to_value(&decoded).unwrap(),
            serde_json::to_value(&contract).unwrap(),
        );
    }

    #[rstest]
    fn test_encode_decode_round_trip_option_contract_all_fields() {
        let contract = OptionContract::builder()
            .instrument_id(InstrumentId::from("AAPL_C100.OPRA"))
            .raw_symbol(Symbol::from("AAPL_C100"))
            .asset_class(AssetClass::Equity)
            .exchange(Ustr::from("OPRA"))
            .underlying(Ustr::from("AAPL"))
            .option_kind(OptionKind::Call)
            .strike_price(Price::from("100.00"))
            .currency(Currency::from("USD"))
            .activation_ns(1.into())
            .expiration_ns(2.into())
            .price_precision(2)
            .price_increment(Price::from("0.01"))
            .multiplier(Quantity::from("100"))
            .lot_size(Quantity::from("1"))
            .max_quantity(Quantity::from("10000"))
            .min_quantity(Quantity::from("5"))
            .max_price(Price::from("9999.99"))
            .min_price(Price::from("0.01"))
            .margin_init(dec!(0.01))
            .margin_maint(dec!(0.02))
            .maker_fee(dec!(0.0002))
            .taker_fee(dec!(0.0004))
            .ts_event(1.into())
            .ts_init(2.into())
            .build()
            .unwrap();

        let decoded = encode_decode_instrument(&InstrumentAny::OptionContract(contract.clone()));
        let InstrumentAny::OptionContract(decoded) = decoded else {
            panic!("Decoded instrument type mismatch");
        };

        assert_eq!(
            serde_json::to_value(&decoded).unwrap(),
            serde_json::to_value(&contract).unwrap(),
        );
    }

    #[rstest]
    fn test_encode_decode_round_trip_binary_option_all_fields() {
        let option = BinaryOption::builder()
            .instrument_id(InstrumentId::from("ELECTION.POLYMARKET"))
            .raw_symbol(Symbol::from("ELECTION"))
            .asset_class(AssetClass::Alternative)
            .currency(Currency::from("USDC"))
            .activation_ns(1.into())
            .expiration_ns(2.into())
            .price_precision(2)
            .size_precision(0)
            .price_increment(Price::from("0.01"))
            .size_increment(Quantity::from("1"))
            .outcome(Ustr::from("YES"))
            .description(Ustr::from("Election outcome"))
            .max_quantity(Quantity::from("10000"))
            .min_quantity(Quantity::from("5"))
            .max_notional(Money::from("50000 USDC"))
            .min_notional(Money::from("5 USDC"))
            .max_price(Price::from("0.99"))
            .min_price(Price::from("0.01"))
            .margin_init(dec!(0.01))
            .margin_maint(dec!(0.02))
            .maker_fee(dec!(0.0002))
            .taker_fee(dec!(0.0004))
            .ts_event(1.into())
            .ts_init(2.into())
            .build()
            .unwrap();

        let decoded = encode_decode_instrument(&InstrumentAny::BinaryOption(option.clone()));
        let InstrumentAny::BinaryOption(decoded) = decoded else {
            panic!("Decoded instrument type mismatch");
        };

        assert_eq!(
            serde_json::to_value(&decoded).unwrap(),
            serde_json::to_value(&option).unwrap(),
        );
    }

    // The `betting` stub populates every bound, margin, and fee, so this covers the whole struct
    #[rstest]
    fn test_encode_decode_round_trip_betting_all_fields() {
        let instrument = betting();

        let decoded = encode_decode_instrument(&InstrumentAny::Betting(instrument.clone()));
        let InstrumentAny::Betting(decoded) = decoded else {
            panic!("Decoded instrument type mismatch");
        };

        assert_eq!(
            serde_json::to_value(&decoded).unwrap(),
            serde_json::to_value(&instrument).unwrap(),
        );
    }

    fn encode_decode_instrument(instrument: &InstrumentAny) -> InstrumentAny {
        let metadata = instrument.metadata();
        let record_batch =
            InstrumentAny::encode_batch(&metadata, std::slice::from_ref(instrument)).unwrap();
        let mut decoded = decode_instrument_any_batch(&metadata, &record_batch).unwrap();

        assert_eq!(decoded.len(), 1);
        decoded.remove(0)
    }

    fn roundtrip_case(instrument: &InstrumentAny) {
        let metadata = instrument.metadata();
        let record_batch =
            InstrumentAny::encode_batch(&metadata, std::slice::from_ref(instrument)).unwrap();
        let decoded = decode_instrument_any_batch(&metadata, &record_batch).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(Instrument::id(&decoded[0]), Instrument::id(instrument));
        assert_eq!(
            Instrument::raw_symbol(&decoded[0]),
            Instrument::raw_symbol(instrument)
        );
        assert_eq!(
            Instrument::asset_class(&decoded[0]),
            Instrument::asset_class(instrument)
        );
        assert_eq!(
            Instrument::instrument_class(&decoded[0]),
            Instrument::instrument_class(instrument)
        );
        assert_eq!(
            Instrument::price_precision(&decoded[0]),
            Instrument::price_precision(instrument)
        );
        assert_eq!(
            Instrument::size_precision(&decoded[0]),
            Instrument::size_precision(instrument)
        );
        assert_eq!(
            Instrument::quote_currency(&decoded[0]),
            Instrument::quote_currency(instrument)
        );
        assert_eq!(
            std::mem::discriminant(&decoded[0]),
            std::mem::discriminant(instrument),
            "decoded variant must match encoded variant"
        );
    }

    fn batch_without_column(record_batch: &RecordBatch, column_name: &str) -> RecordBatch {
        let schema = record_batch.schema();
        let column_index = schema.index_of(column_name).unwrap();
        let fields: Vec<_> = schema
            .fields()
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != column_index)
            .map(|(_, field)| field.as_ref().clone())
            .collect();
        let columns = record_batch
            .columns()
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != column_index)
            .map(|(_, column)| Arc::clone(column))
            .collect();
        let new_schema = Schema::new_with_metadata(fields, schema.metadata().clone());

        RecordBatch::try_new(Arc::new(new_schema), columns).unwrap()
    }

    fn batch_with_null_string_column(record_batch: &RecordBatch, column_name: &str) -> RecordBatch {
        let schema = record_batch.schema();
        let column_index = schema.index_of(column_name).unwrap();
        let mut columns = record_batch.columns().to_vec();
        let null_column: ArrayRef = Arc::new(StringArray::from(vec![None::<&str>]));
        columns[column_index] = null_column;

        RecordBatch::try_new(schema, columns).unwrap()
    }

    fn batch_with_uint8_column(
        record_batch: &RecordBatch,
        column_name: &str,
        values: Vec<u8>,
    ) -> RecordBatch {
        let schema = record_batch.schema();
        let column_index = schema.index_of(column_name).unwrap();
        let mut columns = record_batch.columns().to_vec();
        columns[column_index] = Arc::new(UInt8Array::from(values));

        RecordBatch::try_new(schema, columns).unwrap()
    }

    #[rstest]
    #[case::binary_option(InstrumentAny::BinaryOption(
        nautilus_model::instruments::stubs::binary_option()
    ))]
    #[case::cfd(InstrumentAny::Cfd(nautilus_model::instruments::stubs::cfd_gold()))]
    #[case::commodity(InstrumentAny::Commodity(
        nautilus_model::instruments::stubs::commodity_gold()
    ))]
    #[case::crypto_future(InstrumentAny::CryptoFuture(
        nautilus_model::instruments::stubs::crypto_future_btcusdt(
            2,
            6,
            Price::from("0.01"),
            Quantity::from("0.000001"),
        )
    ))]
    #[case::crypto_futures_spread(InstrumentAny::CryptoFuturesSpread(
        nautilus_model::instruments::stubs::crypto_futures_spread_btc_deribit()
    ))]
    #[case::crypto_option(InstrumentAny::CryptoOption(
        nautilus_model::instruments::stubs::crypto_option_btc_deribit(
            3,
            1,
            Price::from("0.001"),
            Quantity::from("0.1"),
        )
    ))]
    #[case::crypto_option_spread(InstrumentAny::CryptoOptionSpread(
        nautilus_model::instruments::stubs::crypto_option_spread_btc_deribit()
    ))]
    #[case::crypto_perpetual(InstrumentAny::CryptoPerpetual(
        nautilus_model::instruments::stubs::crypto_perpetual_ethusdt()
    ))]
    #[case::currency_pair(InstrumentAny::CurrencyPair(
        nautilus_model::instruments::stubs::currency_pair_btcusdt()
    ))]
    #[case::equity(InstrumentAny::Equity(nautilus_model::instruments::stubs::equity_aapl()))]
    #[case::futures_contract(InstrumentAny::FuturesContract(
        nautilus_model::instruments::stubs::futures_contract_es(None, None,)
    ))]
    #[case::futures_spread(InstrumentAny::FuturesSpread(
        nautilus_model::instruments::stubs::futures_spread_es()
    ))]
    #[case::index_instrument(InstrumentAny::IndexInstrument(
        nautilus_model::instruments::stubs::index_instrument_spx()
    ))]
    #[case::option_contract(InstrumentAny::OptionContract(
        nautilus_model::instruments::stubs::option_contract_appl()
    ))]
    #[case::option_spread(InstrumentAny::OptionSpread(
        nautilus_model::instruments::stubs::option_spread()
    ))]
    #[case::perpetual_contract(InstrumentAny::PerpetualContract(
        nautilus_model::instruments::stubs::perpetual_contract_eurusd()
    ))]
    #[case::tokenized_asset(InstrumentAny::TokenizedAsset(
        nautilus_model::instruments::stubs::tokenized_asset_aaplx()
    ))]
    fn test_decode_instrument_checked_constructor_error(#[case] instrument: InstrumentAny) {
        let metadata = instrument.metadata();
        let class = metadata.get(KEY_CLASS).unwrap();
        let first_row_price_precision = Instrument::price_precision(&instrument);
        let instruments = vec![instrument.clone(), instrument];
        let record_batch = InstrumentAny::encode_batch(&metadata, &instruments).unwrap();
        let record_batch = batch_with_uint8_column(
            &record_batch,
            "price_precision",
            vec![first_row_price_precision, u8::MAX],
        );

        let error = decode_instrument_any_batch(&metadata, &record_batch)
            .expect_err("invalid precision must return EncodingError");

        match error {
            EncodingError::ParseError(field, message) => {
                assert_eq!(field, INSTRUMENT_VALIDATION_FIELD);
                assert!(
                    message.contains(class),
                    "message should include instrument class, found: {message}",
                );
                assert!(
                    message.starts_with("row 1:"),
                    "message should include row index, found: {message}",
                );
                assert!(
                    message.contains("price_precision"),
                    "message should include failed precision, found: {message}",
                );
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[rstest]
    fn test_roundtrip_betting() {
        use nautilus_model::instruments::stubs::betting;
        roundtrip_case(&InstrumentAny::Betting(betting()));
    }

    #[rstest]
    fn test_roundtrip_binary_option() {
        use nautilus_model::instruments::stubs::binary_option;
        roundtrip_case(&InstrumentAny::BinaryOption(binary_option()));
    }

    #[rstest]
    fn test_roundtrip_cfd() {
        use nautilus_model::instruments::stubs::cfd_gold;
        roundtrip_case(&InstrumentAny::Cfd(cfd_gold()));
    }

    #[rstest]
    fn test_roundtrip_commodity() {
        use nautilus_model::instruments::stubs::commodity_gold;
        roundtrip_case(&InstrumentAny::Commodity(commodity_gold()));
    }

    #[rstest]
    fn test_roundtrip_crypto_future() {
        use nautilus_model::instruments::stubs::crypto_future_btcusdt;

        let mut inst = crypto_future_btcusdt(2, 6, Price::from("0.01"), Quantity::from("0.000001"));
        inst.lot_size = Quantity::from("0.25");
        let any = InstrumentAny::CryptoFuture(inst.clone());
        roundtrip_case(&any);
        let metadata = any.metadata();
        let batch = InstrumentAny::encode_batch(&metadata, std::slice::from_ref(&any)).unwrap();
        let decoded = decode_instrument_any_batch(&metadata, &batch).unwrap();
        let InstrumentAny::CryptoFuture(decoded_inst) = &decoded[0] else {
            panic!("decoded variant is not CryptoFuture");
        };
        assert_eq!(decoded_inst.lot_size, inst.lot_size);
    }

    #[rstest]
    fn test_decode_crypto_future_without_lot_size_column_defaults_to_one() {
        use nautilus_model::instruments::stubs::crypto_future_btcusdt;

        let inst = crypto_future_btcusdt(2, 6, Price::from("0.01"), Quantity::from("0.000001"));
        let any = InstrumentAny::CryptoFuture(inst);
        let metadata = any.metadata();
        let batch = InstrumentAny::encode_batch(&metadata, std::slice::from_ref(&any)).unwrap();
        let batch = batch_without_column(&batch, "lot_size");

        let decoded = decode_instrument_any_batch(&metadata, &batch).unwrap();

        let InstrumentAny::CryptoFuture(decoded_inst) = &decoded[0] else {
            panic!("decoded variant is not CryptoFuture");
        };
        assert_eq!(decoded_inst.lot_size, Quantity::from(1));
    }

    #[rstest]
    fn test_decode_crypto_future_null_lot_size_defaults_to_one() {
        use nautilus_model::instruments::stubs::crypto_future_btcusdt;

        let inst = crypto_future_btcusdt(2, 6, Price::from("0.01"), Quantity::from("0.000001"));
        let any = InstrumentAny::CryptoFuture(inst);
        let metadata = any.metadata();
        let batch = InstrumentAny::encode_batch(&metadata, std::slice::from_ref(&any)).unwrap();
        let batch = batch_with_null_string_column(&batch, "lot_size");

        let decoded = decode_instrument_any_batch(&metadata, &batch).unwrap();

        let InstrumentAny::CryptoFuture(decoded_inst) = &decoded[0] else {
            panic!("decoded variant is not CryptoFuture");
        };
        assert_eq!(decoded_inst.lot_size, Quantity::from(1));
    }

    #[rstest]
    fn test_roundtrip_crypto_option() {
        use nautilus_model::instruments::stubs::crypto_option_btc_deribit;

        let mut inst = crypto_option_btc_deribit(3, 1, Price::from("0.001"), Quantity::from("0.1"));
        inst.lot_size = Quantity::from("0.5");
        let any = InstrumentAny::CryptoOption(inst.clone());
        roundtrip_case(&any);
        let metadata = any.metadata();
        let batch = InstrumentAny::encode_batch(&metadata, std::slice::from_ref(&any)).unwrap();
        let decoded = decode_instrument_any_batch(&metadata, &batch).unwrap();
        let InstrumentAny::CryptoOption(decoded_inst) = &decoded[0] else {
            panic!("decoded variant is not CryptoOption");
        };
        assert_eq!(decoded_inst.lot_size, inst.lot_size);
    }

    #[rstest]
    fn test_decode_crypto_option_without_lot_size_column_defaults_to_one() {
        use nautilus_model::instruments::stubs::crypto_option_btc_deribit;

        let inst = crypto_option_btc_deribit(3, 1, Price::from("0.001"), Quantity::from("0.1"));
        let any = InstrumentAny::CryptoOption(inst);
        let metadata = any.metadata();
        let batch = InstrumentAny::encode_batch(&metadata, std::slice::from_ref(&any)).unwrap();
        let batch = batch_without_column(&batch, "lot_size");

        let decoded = decode_instrument_any_batch(&metadata, &batch).unwrap();

        let InstrumentAny::CryptoOption(decoded_inst) = &decoded[0] else {
            panic!("decoded variant is not CryptoOption");
        };
        assert_eq!(decoded_inst.lot_size, Quantity::from(1));
    }

    #[rstest]
    fn test_decode_crypto_option_null_lot_size_defaults_to_one() {
        use nautilus_model::instruments::stubs::crypto_option_btc_deribit;

        let inst = crypto_option_btc_deribit(3, 1, Price::from("0.001"), Quantity::from("0.1"));
        let any = InstrumentAny::CryptoOption(inst);
        let metadata = any.metadata();
        let batch = InstrumentAny::encode_batch(&metadata, std::slice::from_ref(&any)).unwrap();
        let batch = batch_with_null_string_column(&batch, "lot_size");

        let decoded = decode_instrument_any_batch(&metadata, &batch).unwrap();

        let InstrumentAny::CryptoOption(decoded_inst) = &decoded[0] else {
            panic!("decoded variant is not CryptoOption");
        };
        assert_eq!(decoded_inst.lot_size, Quantity::from(1));
    }

    #[rstest]
    fn test_roundtrip_crypto_futures_spread() {
        use nautilus_model::instruments::{Instrument, stubs::crypto_futures_spread_btc_deribit};
        let inst = crypto_futures_spread_btc_deribit();
        let any = InstrumentAny::CryptoFuturesSpread(inst.clone());
        roundtrip_case(&any);
        let metadata = any.metadata();
        let batch = InstrumentAny::encode_batch(&metadata, std::slice::from_ref(&any)).unwrap();
        let decoded = decode_instrument_any_batch(&metadata, &batch).unwrap();
        let InstrumentAny::CryptoFuturesSpread(decoded_inst) = &decoded[0] else {
            panic!("decoded variant is not CryptoFuturesSpread");
        };
        assert_eq!(decoded_inst.lot_size, inst.lot_size);
        assert_eq!(decoded_inst.is_inverse, inst.is_inverse);
        assert_eq!(decoded_inst.strategy_type, inst.strategy_type);
        assert_eq!(decoded_inst.settlement_currency, inst.settlement_currency);
        assert_eq!(Instrument::id(decoded_inst), Instrument::id(&inst));
    }

    #[rstest]
    fn test_roundtrip_crypto_option_spread() {
        use nautilus_model::instruments::{Instrument, stubs::crypto_option_spread_btc_deribit};
        let inst = crypto_option_spread_btc_deribit();
        let any = InstrumentAny::CryptoOptionSpread(inst.clone());
        roundtrip_case(&any);
        let metadata = any.metadata();
        let batch = InstrumentAny::encode_batch(&metadata, std::slice::from_ref(&any)).unwrap();
        let decoded = decode_instrument_any_batch(&metadata, &batch).unwrap();
        let InstrumentAny::CryptoOptionSpread(decoded_inst) = &decoded[0] else {
            panic!("decoded variant is not CryptoOptionSpread");
        };
        // Deribit BTC option combos carry min_trade_amount=0.1, which sets
        // lot_size=0.1; dropping the lot_size Arrow column would silently
        // default it back to 1
        assert_eq!(decoded_inst.lot_size, inst.lot_size);
        assert_eq!(decoded_inst.size_precision, inst.size_precision);
        assert_eq!(decoded_inst.size_increment, inst.size_increment);
        assert_eq!(decoded_inst.is_inverse, inst.is_inverse);
        assert_eq!(decoded_inst.strategy_type, inst.strategy_type);
        assert_eq!(decoded_inst.settlement_currency, inst.settlement_currency);
        assert_eq!(Instrument::id(decoded_inst), Instrument::id(&inst));
    }

    #[rstest]
    fn test_roundtrip_crypto_perpetual_inverse() {
        use nautilus_model::instruments::stubs::xbtusd_bitmex;
        roundtrip_case(&InstrumentAny::CryptoPerpetual(xbtusd_bitmex()));
    }

    #[rstest]
    fn test_roundtrip_crypto_perpetual_linear() {
        use nautilus_model::instruments::stubs::crypto_perpetual_ethusdt;

        let mut inst = crypto_perpetual_ethusdt();
        inst.lot_size = Quantity::from("0.005");
        let any = InstrumentAny::CryptoPerpetual(inst.clone());
        roundtrip_case(&any);
        let metadata = any.metadata();
        let batch = InstrumentAny::encode_batch(&metadata, std::slice::from_ref(&any)).unwrap();
        let decoded = decode_instrument_any_batch(&metadata, &batch).unwrap();
        let InstrumentAny::CryptoPerpetual(decoded_inst) = &decoded[0] else {
            panic!("decoded variant is not CryptoPerpetual");
        };
        assert_eq!(decoded_inst.lot_size, inst.lot_size);
    }

    #[rstest]
    fn test_decode_crypto_perpetual_without_lot_size_column_defaults_to_one() {
        use nautilus_model::instruments::stubs::crypto_perpetual_ethusdt;

        let inst = crypto_perpetual_ethusdt();
        let any = InstrumentAny::CryptoPerpetual(inst);
        let metadata = any.metadata();
        let batch = InstrumentAny::encode_batch(&metadata, std::slice::from_ref(&any)).unwrap();
        let batch = batch_without_column(&batch, "lot_size");

        let decoded = decode_instrument_any_batch(&metadata, &batch).unwrap();

        let InstrumentAny::CryptoPerpetual(decoded_inst) = &decoded[0] else {
            panic!("decoded variant is not CryptoPerpetual");
        };
        assert_eq!(decoded_inst.lot_size, Quantity::from(1));
    }

    #[rstest]
    fn test_decode_crypto_perpetual_null_lot_size_defaults_to_one() {
        use nautilus_model::instruments::stubs::crypto_perpetual_ethusdt;

        let inst = crypto_perpetual_ethusdt();
        let any = InstrumentAny::CryptoPerpetual(inst);
        let metadata = any.metadata();
        let batch = InstrumentAny::encode_batch(&metadata, std::slice::from_ref(&any)).unwrap();
        let batch = batch_with_null_string_column(&batch, "lot_size");

        let decoded = decode_instrument_any_batch(&metadata, &batch).unwrap();

        let InstrumentAny::CryptoPerpetual(decoded_inst) = &decoded[0] else {
            panic!("decoded variant is not CryptoPerpetual");
        };
        assert_eq!(decoded_inst.lot_size, Quantity::from(1));
    }

    #[rstest]
    fn test_roundtrip_futures_contract() {
        use nautilus_model::instruments::stubs::futures_contract_es;
        roundtrip_case(&InstrumentAny::FuturesContract(futures_contract_es(
            None, None,
        )));
    }

    #[rstest]
    fn test_roundtrip_futures_spread() {
        use nautilus_model::instruments::stubs::futures_spread_es;
        roundtrip_case(&InstrumentAny::FuturesSpread(futures_spread_es()));
    }

    #[rstest]
    fn test_roundtrip_index_instrument() {
        use nautilus_model::instruments::stubs::index_instrument_spx;
        roundtrip_case(&InstrumentAny::IndexInstrument(index_instrument_spx()));
    }

    #[rstest]
    fn test_roundtrip_option_contract() {
        use nautilus_model::instruments::stubs::option_contract_appl;
        roundtrip_case(&InstrumentAny::OptionContract(option_contract_appl()));
    }

    #[rstest]
    fn test_roundtrip_option_spread() {
        use nautilus_model::instruments::stubs::option_spread;
        roundtrip_case(&InstrumentAny::OptionSpread(option_spread()));
    }

    #[rstest]
    fn test_roundtrip_perpetual_contract() {
        use nautilus_model::instruments::stubs::perpetual_contract_eurusd;
        roundtrip_case(&InstrumentAny::PerpetualContract(
            perpetual_contract_eurusd(),
        ));
    }

    #[rstest]
    fn test_roundtrip_tokenized_asset() {
        use nautilus_model::instruments::stubs::tokenized_asset_aaplx;
        roundtrip_case(&InstrumentAny::TokenizedAsset(tokenized_asset_aaplx()));
    }
}
