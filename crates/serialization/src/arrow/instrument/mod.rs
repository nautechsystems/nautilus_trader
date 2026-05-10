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

use std::collections::HashMap;

use arrow::{datatypes::Schema, error::ArrowError, record_batch::RecordBatch};
use nautilus_model::{
    instruments::{
        Instrument, InstrumentAny, betting::BettingInstrument, binary_option::BinaryOption,
        cfd::Cfd, commodity::Commodity, crypto_future::CryptoFuture, crypto_option::CryptoOption,
        crypto_perpetual::CryptoPerpetual, currency_pair::CurrencyPair, equity::Equity,
        futures_contract::FuturesContract, futures_spread::FuturesSpread,
        index_instrument::IndexInstrument, option_contract::OptionContract,
        option_spread::OptionSpread, perpetual_contract::PerpetualContract,
        tokenized_asset::TokenizedAsset,
    },
    types::Currency,
};

#[allow(unused)]
use crate::arrow::{
    ArrowSchemaProvider, Data, DecodeDataFromRecordBatch, DecodeFromRecordBatch,
    EncodeToRecordBatch, EncodingError, KEY_INSTRUMENT_ID,
};

pub mod betting;
pub mod binary_option;
pub mod cfd;
pub mod commodity;
pub mod crypto_future;
pub mod crypto_option;
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

impl ArrowSchemaProvider for InstrumentAny {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let instrument_type = metadata
            .as_ref()
            .and_then(|m| m.get("class"))
            .map_or("CurrencyPair", |s| s.as_str());

        match instrument_type {
            "BettingInstrument" => BettingInstrument::get_schema(metadata),
            "BinaryOption" => BinaryOption::get_schema(metadata),
            "Cfd" => Cfd::get_schema(metadata),
            "Commodity" => Commodity::get_schema(metadata),
            "CryptoFuture" => CryptoFuture::get_schema(metadata),
            "CryptoOption" => CryptoOption::get_schema(metadata),
            "CryptoPerpetual" => CryptoPerpetual::get_schema(metadata),
            "CurrencyPair" => CurrencyPair::get_schema(metadata),
            "Equity" => Equity::get_schema(metadata),
            "FuturesContract" => FuturesContract::get_schema(metadata),
            "FuturesSpread" => FuturesSpread::get_schema(metadata),
            "IndexInstrument" => IndexInstrument::get_schema(metadata),
            "OptionContract" => OptionContract::get_schema(metadata),
            "OptionSpread" => OptionSpread::get_schema(metadata),
            "PerpetualContract" => PerpetualContract::get_schema(metadata),
            "TokenizedAsset" => TokenizedAsset::get_schema(metadata),
            _ => {
                // Fallback to CurrencyPair schema if type is unknown
                CurrencyPair::get_schema(metadata)
            }
        }
    }
}

impl EncodeToRecordBatch for InstrumentAny {
    fn encode_batch(
        #[allow(unused)] metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        if data.is_empty() {
            return Err(ArrowError::InvalidArgumentError(
                "Cannot encode empty instrument batch".to_string(),
            ));
        }

        let mut by_type: HashMap<String, Vec<&Self>> = HashMap::new();

        for instrument in data {
            let type_name = match instrument {
                Self::Cfd(_) => "Cfd",
                Self::Commodity(_) => "Commodity",
                Self::CurrencyPair(_) => "CurrencyPair",
                Self::Equity(_) => "Equity",
                Self::CryptoFuture(_) => "CryptoFuture",
                Self::CryptoPerpetual(_) => "CryptoPerpetual",
                Self::CryptoOption(_) => "CryptoOption",
                Self::FuturesContract(_) => "FuturesContract",
                Self::FuturesSpread(_) => "FuturesSpread",
                Self::IndexInstrument(_) => "IndexInstrument",
                Self::OptionContract(_) => "OptionContract",
                Self::OptionSpread(_) => "OptionSpread",
                Self::BinaryOption(_) => "BinaryOption",
                Self::Betting(_) => "BettingInstrument",
                Self::PerpetualContract(_) => "PerpetualContract",
                Self::TokenizedAsset(_) => "TokenizedAsset",
            };
            by_type
                .entry(type_name.to_string())
                .or_default()
                .push(instrument);
        }

        if by_type.len() > 1 {
            return Err(ArrowError::InvalidArgumentError(
                "Cannot encode mixed instrument types in a single batch. Use separate batches for each type.".to_string(),
            ));
        }

        let (type_name, instruments) = by_type.iter().next().unwrap();
        match type_name.as_str() {
            "Cfd" => {
                let cfds: Vec<_> = instruments
                    .iter()
                    .map(|i| {
                        if let Self::Cfd(c) = i {
                            c
                        } else {
                            unreachable!()
                        }
                    })
                    .cloned()
                    .collect();
                Cfd::encode_batch(metadata, &cfds)
            }
            "Commodity" => {
                let commodities: Vec<_> = instruments
                    .iter()
                    .map(|i| {
                        if let Self::Commodity(c) = i {
                            c
                        } else {
                            unreachable!()
                        }
                    })
                    .cloned()
                    .collect();
                Commodity::encode_batch(metadata, &commodities)
            }
            "BettingInstrument" => {
                let betting: Vec<_> = instruments
                    .iter()
                    .map(|i| {
                        if let Self::Betting(b) = i {
                            b
                        } else {
                            unreachable!()
                        }
                    })
                    .cloned()
                    .collect();
                BettingInstrument::encode_batch(metadata, &betting)
            }
            "BinaryOption" => {
                let binary_options: Vec<_> = instruments
                    .iter()
                    .map(|i| {
                        if let Self::BinaryOption(bo) = i {
                            bo
                        } else {
                            unreachable!()
                        }
                    })
                    .cloned()
                    .collect();
                BinaryOption::encode_batch(metadata, &binary_options)
            }
            "CryptoFuture" => {
                let crypto_futures: Vec<_> = instruments
                    .iter()
                    .map(|i| {
                        if let Self::CryptoFuture(cf) = i {
                            cf
                        } else {
                            unreachable!()
                        }
                    })
                    .cloned()
                    .collect();
                CryptoFuture::encode_batch(metadata, &crypto_futures)
            }
            "CryptoOption" => {
                let crypto_options: Vec<_> = instruments
                    .iter()
                    .map(|i| {
                        if let Self::CryptoOption(co) = i {
                            co
                        } else {
                            unreachable!()
                        }
                    })
                    .cloned()
                    .collect();
                CryptoOption::encode_batch(metadata, &crypto_options)
            }
            "CryptoPerpetual" => {
                let crypto_perps: Vec<_> = instruments
                    .iter()
                    .map(|i| {
                        if let Self::CryptoPerpetual(cp) = i {
                            cp
                        } else {
                            unreachable!()
                        }
                    })
                    .cloned()
                    .collect();
                CryptoPerpetual::encode_batch(metadata, &crypto_perps)
            }
            "CurrencyPair" => {
                let currency_pairs: Vec<_> = instruments
                    .iter()
                    .map(|i| {
                        if let Self::CurrencyPair(cp) = i {
                            cp
                        } else {
                            unreachable!()
                        }
                    })
                    .cloned()
                    .collect();
                CurrencyPair::encode_batch(metadata, &currency_pairs)
            }
            "Equity" => {
                let equities: Vec<_> = instruments
                    .iter()
                    .map(|i| {
                        if let Self::Equity(e) = i {
                            e
                        } else {
                            unreachable!()
                        }
                    })
                    .cloned()
                    .collect();
                Equity::encode_batch(metadata, &equities)
            }
            "FuturesContract" => {
                let futures_contracts: Vec<_> = instruments
                    .iter()
                    .map(|i| {
                        if let Self::FuturesContract(fc) = i {
                            fc
                        } else {
                            unreachable!()
                        }
                    })
                    .cloned()
                    .collect();
                FuturesContract::encode_batch(metadata, &futures_contracts)
            }
            "FuturesSpread" => {
                let futures_spreads: Vec<_> = instruments
                    .iter()
                    .map(|i| {
                        if let Self::FuturesSpread(fs) = i {
                            fs
                        } else {
                            unreachable!()
                        }
                    })
                    .cloned()
                    .collect();
                FuturesSpread::encode_batch(metadata, &futures_spreads)
            }
            "IndexInstrument" => {
                let index_instruments: Vec<_> = instruments
                    .iter()
                    .map(|i| {
                        if let Self::IndexInstrument(ii) = i {
                            ii
                        } else {
                            unreachable!()
                        }
                    })
                    .cloned()
                    .collect();
                IndexInstrument::encode_batch(metadata, &index_instruments)
            }
            "OptionContract" => {
                let option_contracts: Vec<_> = instruments
                    .iter()
                    .map(|i| {
                        if let Self::OptionContract(oc) = i {
                            oc
                        } else {
                            unreachable!()
                        }
                    })
                    .cloned()
                    .collect();
                OptionContract::encode_batch(metadata, &option_contracts)
            }
            "OptionSpread" => {
                let option_spreads: Vec<_> = instruments
                    .iter()
                    .map(|i| {
                        if let Self::OptionSpread(os) = i {
                            os
                        } else {
                            unreachable!()
                        }
                    })
                    .cloned()
                    .collect();
                OptionSpread::encode_batch(metadata, &option_spreads)
            }
            "PerpetualContract" => {
                let perpetual_contracts: Vec<_> = instruments
                    .iter()
                    .map(|i| {
                        if let Self::PerpetualContract(pc) = i {
                            pc
                        } else {
                            unreachable!()
                        }
                    })
                    .cloned()
                    .collect();
                PerpetualContract::encode_batch(metadata, &perpetual_contracts)
            }
            "TokenizedAsset" => {
                let tokenized_assets: Vec<_> = instruments
                    .iter()
                    .map(|i| {
                        if let Self::TokenizedAsset(ta) = i {
                            ta
                        } else {
                            unreachable!()
                        }
                    })
                    .cloned()
                    .collect();
                TokenizedAsset::encode_batch(metadata, &tokenized_assets)
            }
            _ => Err(ArrowError::InvalidArgumentError(format!(
                "Instrument type {type_name} serialization not yet implemented"
            ))),
        }
    }

    fn metadata(&self) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert(
            KEY_INSTRUMENT_ID.to_string(),
            Instrument::id(self).to_string(),
        );

        let type_name = match self {
            Self::Cfd(_) => "Cfd",
            Self::Commodity(_) => "Commodity",
            Self::CurrencyPair(_) => "CurrencyPair",
            Self::Equity(_) => "Equity",
            Self::CryptoFuture(_) => "CryptoFuture",
            Self::CryptoPerpetual(_) => "CryptoPerpetual",
            Self::CryptoOption(_) => "CryptoOption",
            Self::FuturesContract(_) => "FuturesContract",
            Self::FuturesSpread(_) => "FuturesSpread",
            Self::IndexInstrument(_) => "IndexInstrument",
            Self::OptionContract(_) => "OptionContract",
            Self::OptionSpread(_) => "OptionSpread",
            Self::BinaryOption(_) => "BinaryOption",
            Self::Betting(_) => "BettingInstrument",
            Self::PerpetualContract(_) => "PerpetualContract",
            Self::TokenizedAsset(_) => "TokenizedAsset",
        };
        metadata.insert("class".to_string(), type_name.to_string());
        metadata
    }
}

/// Decode InstrumentAny from RecordBatch
/// (Cannot implement DecodeFromRecordBatch trait due to `Into<Data>` bound)
///
/// # Errors
///
/// Returns an `EncodingError` if the RecordBatch cannot be decoded.
pub fn decode_instrument_any_batch(
    #[allow(unused)] metadata: &HashMap<String, String>,
    record_batch: &RecordBatch,
) -> Result<Vec<InstrumentAny>, EncodingError> {
    let type_name = metadata
        .get("class")
        .map(String::as_str)
        .ok_or_else(|| EncodingError::MissingMetadata("class"))?;

    match type_name {
        "Cfd" => {
            let cfds = cfd::decode_cfd_batch(metadata, record_batch)?;
            Ok(cfds.into_iter().map(InstrumentAny::Cfd).collect())
        }
        "Commodity" => {
            let commodities = commodity::decode_commodity_batch(metadata, record_batch)?;
            Ok(commodities
                .into_iter()
                .map(InstrumentAny::Commodity)
                .collect())
        }
        "BettingInstrument" => {
            let betting = betting::decode_betting_instrument_batch(metadata, record_batch)?;
            Ok(betting.into_iter().map(InstrumentAny::Betting).collect())
        }
        "BinaryOption" => {
            let binary_options = binary_option::decode_binary_option_batch(metadata, record_batch)?;
            Ok(binary_options
                .into_iter()
                .map(InstrumentAny::BinaryOption)
                .collect())
        }
        "CryptoFuture" => {
            let crypto_futures = crypto_future::decode_crypto_future_batch(metadata, record_batch)?;
            Ok(crypto_futures
                .into_iter()
                .map(InstrumentAny::CryptoFuture)
                .collect())
        }
        "CryptoOption" => {
            let crypto_options = crypto_option::decode_crypto_option_batch(metadata, record_batch)?;
            Ok(crypto_options
                .into_iter()
                .map(InstrumentAny::CryptoOption)
                .collect())
        }
        "CryptoPerpetual" => {
            let crypto_perps =
                crypto_perpetual::decode_crypto_perpetual_batch(metadata, record_batch)?;
            Ok(crypto_perps
                .into_iter()
                .map(InstrumentAny::CryptoPerpetual)
                .collect())
        }
        "CurrencyPair" => {
            let currency_pairs = currency_pair::decode_currency_pair_batch(metadata, record_batch)?;
            Ok(currency_pairs
                .into_iter()
                .map(InstrumentAny::CurrencyPair)
                .collect())
        }
        "Equity" => {
            let equities = equity::decode_equity_batch(metadata, record_batch)?;
            Ok(equities.into_iter().map(InstrumentAny::Equity).collect())
        }
        "FuturesContract" => {
            let futures_contracts =
                futures_contract::decode_futures_contract_batch(metadata, record_batch)?;
            Ok(futures_contracts
                .into_iter()
                .map(InstrumentAny::FuturesContract)
                .collect())
        }
        "FuturesSpread" => {
            let futures_spreads =
                futures_spread::decode_futures_spread_batch(metadata, record_batch)?;
            Ok(futures_spreads
                .into_iter()
                .map(InstrumentAny::FuturesSpread)
                .collect())
        }
        "IndexInstrument" => {
            let index_instruments =
                index_instrument::decode_index_instrument_batch(metadata, record_batch)?;
            Ok(index_instruments
                .into_iter()
                .map(InstrumentAny::IndexInstrument)
                .collect())
        }
        "OptionContract" => {
            let option_contracts =
                option_contract::decode_option_contract_batch(metadata, record_batch)?;
            Ok(option_contracts
                .into_iter()
                .map(InstrumentAny::OptionContract)
                .collect())
        }
        "OptionSpread" => {
            let option_spreads = option_spread::decode_option_spread_batch(metadata, record_batch)?;
            Ok(option_spreads
                .into_iter()
                .map(InstrumentAny::OptionSpread)
                .collect())
        }
        "PerpetualContract" => {
            let perpetual_contracts =
                perpetual_contract::decode_perpetual_contract_batch(metadata, record_batch)?;
            Ok(perpetual_contracts
                .into_iter()
                .map(InstrumentAny::PerpetualContract)
                .collect())
        }
        "TokenizedAsset" => {
            let tokenized_assets =
                tokenized_asset::decode_tokenized_asset_batch(metadata, record_batch)?;
            Ok(tokenized_assets
                .into_iter()
                .map(InstrumentAny::TokenizedAsset)
                .collect())
        }
        _ => Err(EncodingError::ParseError(
            "class",
            format!("Unknown instrument type: {type_name}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        enums::CurrencyType,
        identifiers::{InstrumentId, Symbol},
        instruments::{InstrumentAny, currency_pair::CurrencyPair},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_get_schema() {
        let mut metadata = HashMap::new();
        metadata.insert("class".to_string(), "CurrencyPair".to_string());
        let schema = InstrumentAny::get_schema(Some(metadata));
        assert!(schema.fields().len() >= 20);
        assert_eq!(schema.field(0).name(), "id");
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
        use nautilus_model::instruments::Instrument;
        let instrument_id = InstrumentId::from("EUR/USD.SIM");
        let currency_pair = CurrencyPair::new(
            instrument_id,
            Symbol::from("EUR/USD"),
            Currency::from("EUR"),
            Currency::from("USD"),
            5,
            0, // size_precision must match size_increment precision (0)
            Price::new(0.00001, 5),
            Quantity::new(1.0, 0), // precision 0
            None,                  // multiplier
            None,                  // lot_size
            None,                  // max_quantity
            None,                  // min_quantity
            None,                  // max_notional
            None,                  // min_notional
            None,                  // max_price
            None,                  // min_price
            None,                  // margin_init
            None,                  // margin_maint
            None,                  // maker_fee
            None,                  // taker_fee
            None,                  // info
            UnixNanos::default(),
            UnixNanos::default(),
        );
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
            }
            _ => panic!("Decoded instrument type mismatch"),
        }
    }

    #[rstest]
    fn test_encode_decode_round_trip_equity() {
        use nautilus_model::instruments::{Instrument, equity::Equity};

        let instrument_id = InstrumentId::from("AAPL.NASDAQ");
        let equity = Equity::new(
            instrument_id,
            Symbol::from("AAPL"),
            None, // isin
            Currency::from("USD"),
            2,
            Price::new(0.01, 2),
            None, // lot_size
            None, // max_quantity
            None, // min_quantity
            None, // max_price
            None, // min_price
            None, // margin_init
            None, // margin_maint
            None, // maker_fee
            None, // taker_fee
            None, // info
            UnixNanos::default(),
            UnixNanos::default(),
        );
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

    fn roundtrip_case(instrument: &InstrumentAny) {
        use nautilus_model::instruments::Instrument;

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
        roundtrip_case(&InstrumentAny::CryptoFuture(crypto_future_btcusdt(
            2,
            6,
            Price::from("0.01"),
            Quantity::from("0.000001"),
        )));
    }

    #[rstest]
    fn test_roundtrip_crypto_option() {
        use nautilus_model::instruments::stubs::crypto_option_btc_deribit;
        roundtrip_case(&InstrumentAny::CryptoOption(crypto_option_btc_deribit(
            3,
            1,
            Price::from("0.001"),
            Quantity::from("0.1"),
        )));
    }

    #[rstest]
    fn test_roundtrip_crypto_perpetual_inverse() {
        use nautilus_model::instruments::stubs::xbtusd_bitmex;
        roundtrip_case(&InstrumentAny::CryptoPerpetual(xbtusd_bitmex()));
    }

    #[rstest]
    fn test_roundtrip_crypto_perpetual_linear() {
        use nautilus_model::instruments::stubs::crypto_perpetual_ethusdt;
        roundtrip_case(&InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()));
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
