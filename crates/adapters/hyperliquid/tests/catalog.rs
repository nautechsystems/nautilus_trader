// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

#![cfg(feature = "arrow")]

use std::sync::Arc;

use nautilus_core::{Params, UnixNanos};
use nautilus_hyperliquid::data_types::{HyperliquidPublicTrade, register_hyperliquid_custom_data};
use nautilus_model::{
    data::{CustomData, Data, DataType},
    enums::AggressorSide,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use rstest::rstest;
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
        AggressorSide::Buyer,
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
