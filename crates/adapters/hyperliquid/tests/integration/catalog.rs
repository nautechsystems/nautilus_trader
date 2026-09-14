// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

#![cfg(feature = "arrow")]

use std::{str::FromStr, sync::Arc};

use nautilus_core::{Params, UnixNanos};
use nautilus_hyperliquid::{
    common::enums::HyperliquidTwapStatus,
    data_types::{
        HyperliquidPublicTrade, HyperliquidTwapHistory, HyperliquidTwapSliceFill,
        register_hyperliquid_custom_data,
    },
};
use nautilus_model::{
    data::{CustomData, Data, DataType},
    enums::{AggressorSide, OrderSide},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use rstest::rstest;
use rust_decimal::Decimal;
use tempfile::TempDir;

fn public_trade_data_type(instrument_id: InstrumentId) -> DataType {
    let mut metadata = Params::new();
    metadata.insert(
        "instrument_id".to_string(),
        serde_json::Value::String(instrument_id.to_string()),
    );
    DataType::new(
        "HyperliquidPublicTrade",
        Some(metadata),
        Some(instrument_id.to_string()),
    )
}

fn twap_user_data_type(type_name: &str, user: &str) -> DataType {
    let mut metadata = Params::new();
    metadata.insert(
        "user".to_string(),
        serde_json::Value::String(user.to_string()),
    );
    DataType::new(type_name, Some(metadata), Some(user.to_string()))
}

#[rstest]
fn public_trade_catalog_round_trip_preserves_counterparties() {
    register_hyperliquid_custom_data();
    let temp_dir = TempDir::new().unwrap();
    let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
    let instrument_id = InstrumentId::from("BTC-USD-PERP.HYPERLIQUID");
    let data_type = public_trade_data_type(instrument_id);
    let original = HyperliquidPublicTrade::new(
        instrument_id,
        Price::from("100000.50"),
        Quantity::from("0.123"),
        AggressorSide::Buy,
        "123456".to_string(),
        "0xbuyer".to_string(),
        "0xseller".to_string(),
        "0xhash".to_string(),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );

    catalog
        .write_custom_data_batch(
            vec![CustomData::new(Arc::new(original.clone()), data_type)],
            None,
            None,
            Some(false),
        )
        .unwrap();

    let ids = vec![instrument_id.to_string()];
    let loaded: Vec<Data> = catalog
        .query_custom_data_dynamic(
            "HyperliquidPublicTrade",
            Some(&ids),
            None,
            None,
            None,
            None,
            true,
        )
        .unwrap();

    let Data::Custom(custom) = &loaded[0] else {
        panic!("Expected Data::Custom");
    };
    let trade = custom
        .data
        .as_any()
        .downcast_ref::<HyperliquidPublicTrade>()
        .expect("expected HyperliquidPublicTrade");
    assert_eq!(trade.buyer, original.buyer);
    assert_eq!(trade.seller, original.seller);
    assert_eq!(trade.hash, original.hash);
    assert_eq!(trade.trade_id, original.trade_id);
}

#[rstest]
fn twap_history_catalog_round_trip_preserves_option_enum_and_decimals() {
    register_hyperliquid_custom_data();
    let temp_dir = TempDir::new().unwrap();
    let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
    let user = "0xabc123def456";
    let data_type = twap_user_data_type("HyperliquidTwapHistory", user);
    let original = HyperliquidTwapHistory::new(
        user.to_string(),
        Some(42),
        "BTC".to_string(),
        Some(InstrumentId::from("BTC-USD-PERP.HYPERLIQUID")),
        OrderSide::Buy,
        Decimal::from_str("1.5").unwrap(),
        Decimal::from_str("0.75").unwrap(),
        Decimal::from_str("75012.345678901234").unwrap(),
        30,
        false,
        true,
        HyperliquidTwapStatus::Finished,
        "finished".to_string(),
        UnixNanos::from(1_700_000_000_000_000_000),
        true,
        UnixNanos::from(1_700_000_001_000_000_000),
        UnixNanos::from(1_700_000_002_000_000_000),
    );

    catalog
        .write_custom_data_batch(
            vec![CustomData::new(Arc::new(original.clone()), data_type)],
            None,
            None,
            Some(false),
        )
        .unwrap();

    let ids = vec![user.to_string()];
    let loaded: Vec<Data> = catalog
        .query_custom_data_dynamic(
            "HyperliquidTwapHistory",
            Some(&ids),
            None,
            None,
            None,
            None,
            true,
        )
        .unwrap();

    assert_eq!(loaded.len(), 1);
    let Data::Custom(custom) = &loaded[0] else {
        panic!("Expected Data::Custom");
    };
    assert_eq!(custom.data_type.type_name(), "HyperliquidTwapHistory");
    assert_eq!(custom.data_type.identifier(), Some(user));
    let history = custom
        .data
        .as_any()
        .downcast_ref::<HyperliquidTwapHistory>()
        .expect("expected HyperliquidTwapHistory");
    assert_eq!(history, &original);
}

#[rstest]
fn twap_history_catalog_round_trip_preserves_none_optionals() {
    register_hyperliquid_custom_data();
    let temp_dir = TempDir::new().unwrap();
    let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
    let user = "0xnoneoptions";
    let data_type = twap_user_data_type("HyperliquidTwapHistory", user);
    let original = HyperliquidTwapHistory::new(
        user.to_string(),
        None,
        "UNKNOWNCOIN".to_string(),
        None,
        OrderSide::Sell,
        Decimal::from_str("10").unwrap(),
        Decimal::ZERO,
        Decimal::ZERO,
        60,
        true,
        false,
        HyperliquidTwapStatus::Activated,
        "activated".to_string(),
        UnixNanos::from(100),
        false,
        UnixNanos::from(200),
        UnixNanos::from(300),
    );

    catalog
        .write_custom_data_batch(
            vec![CustomData::new(Arc::new(original.clone()), data_type)],
            None,
            None,
            Some(false),
        )
        .unwrap();

    let ids = vec![user.to_string()];
    let loaded: Vec<Data> = catalog
        .query_custom_data_dynamic(
            "HyperliquidTwapHistory",
            Some(&ids),
            None,
            None,
            None,
            None,
            true,
        )
        .unwrap();

    let Data::Custom(custom) = &loaded[0] else {
        panic!("Expected Data::Custom");
    };
    let history = custom
        .data
        .as_any()
        .downcast_ref::<HyperliquidTwapHistory>()
        .expect("expected HyperliquidTwapHistory");
    assert_eq!(history.twap_id, None);
    assert_eq!(history.instrument_id, None);
    assert_eq!(history, &original);
}

#[rstest]
fn twap_slice_fill_catalog_round_trip_preserves_decimals_and_flags() {
    register_hyperliquid_custom_data();
    let temp_dir = TempDir::new().unwrap();
    let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
    let user = "0xslicefilluser";
    let data_type = twap_user_data_type("HyperliquidTwapSliceFill", user);
    let original = HyperliquidTwapSliceFill::new(
        user.to_string(),
        99,
        "ETH".to_string(),
        Some(InstrumentId::from("ETH-USD-PERP.HYPERLIQUID")),
        Decimal::from_str("3456.78").unwrap(),
        Decimal::from_str("0.012345678901234567").unwrap(),
        OrderSide::Buy,
        "0xfillhash".to_string(),
        1_234_567,
        9_876_543,
        true,
        Decimal::from_str("-0.001234").unwrap(),
        "USDC".to_string(),
        "Open Long".to_string(),
        Decimal::from_str("12.34").unwrap(),
        true,
        UnixNanos::from(1_700_000_010_000_000_000),
        UnixNanos::from(1_700_000_011_000_000_000),
    );

    catalog
        .write_custom_data_batch(
            vec![CustomData::new(Arc::new(original.clone()), data_type)],
            None,
            None,
            Some(false),
        )
        .unwrap();

    let ids = vec![user.to_string()];
    let loaded: Vec<Data> = catalog
        .query_custom_data_dynamic(
            "HyperliquidTwapSliceFill",
            Some(&ids),
            None,
            None,
            None,
            None,
            true,
        )
        .unwrap();

    assert_eq!(loaded.len(), 1);
    let Data::Custom(custom) = &loaded[0] else {
        panic!("Expected Data::Custom");
    };
    assert_eq!(custom.data_type.type_name(), "HyperliquidTwapSliceFill");
    assert_eq!(custom.data_type.identifier(), Some(user));
    let fill = custom
        .data
        .as_any()
        .downcast_ref::<HyperliquidTwapSliceFill>()
        .expect("expected HyperliquidTwapSliceFill");
    assert_eq!(fill, &original);
}
