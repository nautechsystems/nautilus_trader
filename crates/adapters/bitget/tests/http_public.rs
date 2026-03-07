// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use std::path::PathBuf;

use nautilus_bitget::{
    common::{enums::{BitgetEnvironment, BitgetProductType}, symbol::nautilus_symbol_for_delivery},
    http::{
        client::BitgetHttpClient,
        models::{BitgetApiResponse, BitgetContractConfigResponse, BitgetContractSymbol,
                 BitgetOrderBookSnapshot, BitgetSpotSymbolsResponse, BitgetSpotSymbol},
    },
};
use nautilus_core::UnixNanos;
use nautilus_model::instruments::InstrumentAny;
use serde::de::DeserializeOwned;

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load_test_data(filename: &str) -> String {
    let path = manifest_path().join("test_data").join(filename);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to load fixture {path:?}: {e}"))
}

fn load_api_response<T: DeserializeOwned>(filename: &str) -> BitgetApiResponse<T> {
    let payload = load_test_data(filename);
    serde_json::from_str::<BitgetApiResponse<T>>(&payload)
        .expect("fixture should deserialize to BitgetApiResponse")
}

#[test]
fn test_deserialize_spot_symbols() {
    let response: BitgetSpotSymbolsResponse =
        load_api_response::<Vec<BitgetSpotSymbol>>("http_spot_symbols.json");

    assert_eq!(response.code, "00000");
    assert_eq!(response.data.len(), 2);
    assert_eq!(response.data[0].symbol, "BTCUSDT");
}

#[test]
fn test_deserialize_contract_config() {
    let response: BitgetContractConfigResponse =
        load_api_response::<Vec<BitgetContractSymbol>>("http_contract_config.json");

    assert_eq!(response.code, "00000");
    assert_eq!(response.data.len(), 2);
    assert_eq!(response.data[1].symbol, "ETHUSDT");
}

#[test]
fn test_build_instruments_from_fixture_responses() {
    let client = BitgetHttpClient::new(BitgetEnvironment::Mainnet);

    let spot = load_api_response::<Vec<BitgetSpotSymbol>>("http_spot_symbols.json");
    let contracts = load_api_response::<Vec<BitgetContractSymbol>>("http_contract_config.json");

    let instruments = client.build_instruments(
        &spot.data,
        &contracts.data,
        UnixNanos::from(1_700_000_000_000_000_000_u64),
    );

    assert_eq!(instruments.len(), 4);

    let mut has_spot = false;
    let mut has_perpetual = false;
    let mut has_future = false;

    let mut spot_currency_symbol = String::new();
    let mut future_symbol = String::new();

    for instrument in &instruments {
        match instrument {
            InstrumentAny::CurrencyPair(pair) => {
                has_spot = true;
                assert!(!pair.id.to_string().is_empty());
                assert!(!pair.raw_symbol.to_string().is_empty());
                assert!(!pair.base_currency.code.as_str().is_empty());
                assert!(!pair.quote_currency.code.as_str().is_empty());
                spot_currency_symbol = pair.raw_symbol.to_string();
            }
            InstrumentAny::CryptoPerpetual(perp) => {
                has_perpetual = true;
                assert!(!perp.id.to_string().is_empty());
                assert!(!perp.raw_symbol.to_string().is_empty());
                assert!(!perp.base_currency.code.as_str().is_empty());
                assert!(!perp.quote_currency.code.as_str().is_empty());
                assert!(!perp.settlement_currency.code.as_str().is_empty());
            }
            InstrumentAny::CryptoFuture(future) => {
                has_future = true;
                assert!(!future.id.to_string().is_empty());
                assert!(!future.raw_symbol.to_string().is_empty());
                assert!(!future.underlying.code.as_str().is_empty());
                assert!(!future.quote_currency.code.as_str().is_empty());
                assert!(!future.settlement_currency.code.as_str().is_empty());
                future_symbol = future.raw_symbol.to_string();
            }
            _ => {}
        }
    }

    assert!(has_spot);
    assert!(has_perpetual);
    assert!(has_future);
    assert!(spot_currency_symbol.starts_with("BTC") || spot_currency_symbol.starts_with("ETH"));

    let expected_future_symbol = nautilus_symbol_for_delivery("ETHUSDT", 1_672_531_200_000);
    assert_eq!(future_symbol, expected_future_symbol);
}

#[test]
fn test_deserialize_spot_merge_depth_snapshot() {
    let response: BitgetApiResponse<BitgetOrderBookSnapshot> =
        load_api_response::<BitgetOrderBookSnapshot>("http_spot_merge_depth.json");

    assert_eq!(response.code, "00000");
    assert_eq!(response.data.bids.len(), 2);
    assert_eq!(response.data.asks[0][0], "38084.5");
}

#[test]
fn test_build_order_book_snapshot_deltas_from_spot_snapshot() {
    let client = BitgetHttpClient::new(BitgetEnvironment::Mainnet);
    let response: BitgetApiResponse<BitgetOrderBookSnapshot> =
        load_api_response::<BitgetOrderBookSnapshot>("http_spot_merge_depth.json");
    let instruments = client.build_instruments(
        &load_api_response::<Vec<BitgetSpotSymbol>>("http_spot_symbols.json").data,
        &load_api_response::<Vec<BitgetContractSymbol>>("http_contract_config.json").data,
        UnixNanos::from(1_700_000_000_000_000_000_u64),
    );
    let instrument = instruments
        .into_iter()
        .find(|inst| matches!(inst, InstrumentAny::CurrencyPair(pair) if pair.raw_symbol.to_string() == "BTCUSDT"))
        .expect("spot instrument should exist");

    let deltas = client
        .build_order_book_snapshot_deltas(
            &response.data,
            &instrument,
            UnixNanos::from(1_700_000_000_000_000_000_u64),
        )
        .expect("snapshot deltas should build");

    assert_eq!(deltas.deltas.len(), 5);
    assert_eq!(deltas.sequence, 1_622_102_974_025);
}

#[test]
fn test_build_order_book_snapshot_deltas_from_mix_snapshot() {
    let client = BitgetHttpClient::new(BitgetEnvironment::Mainnet);
    let response: BitgetApiResponse<BitgetOrderBookSnapshot> =
        load_api_response::<BitgetOrderBookSnapshot>("http_mix_merge_depth.json");
    let instruments = client.build_instruments(
        &load_api_response::<Vec<BitgetSpotSymbol>>("http_spot_symbols.json").data,
        &load_api_response::<Vec<BitgetContractSymbol>>("http_contract_config.json").data,
        UnixNanos::from(1_700_000_000_000_000_000_u64),
    );
    let instrument = instruments
        .into_iter()
        .find(|inst| matches!(inst, InstrumentAny::CryptoPerpetual(perp) if perp.raw_symbol.to_string() == "BTCUSDT-PERP"))
        .expect("perpetual instrument should exist");

    let deltas = client
        .build_order_book_snapshot_deltas(
            &response.data,
            &instrument,
            UnixNanos::from(1_700_000_000_000_000_000_u64),
        )
        .expect("snapshot deltas should build");

    assert_eq!(deltas.deltas.len(), 5);
    assert_eq!(deltas.sequence, 1_695_870_968_804);
    assert_eq!(BitgetProductType::UsdtFutures.as_api_str(), "USDT-FUTURES");
}
