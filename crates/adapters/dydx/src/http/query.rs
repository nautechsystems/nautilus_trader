//! Query parameter builders for dYdX v4 Indexer REST API endpoints.

use derive_builder::Builder;
use serde::Serialize;

use crate::common::enums::DydxCandleResolution;

/// Query parameters for fetching orderbook.
#[derive(Debug, Clone, Default, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct GetOrderbookParams {
    pub ticker: String,
}

/// Query parameters for fetching trades.
#[derive(Debug, Clone, Default, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct GetTradesParams {
    pub ticker: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Query parameters for fetching candles.
#[derive(Debug, Clone, Default, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct GetCandlesParams {
    pub ticker: String,
    pub resolution: DydxCandleResolution,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Query parameters for fetching subaccount.
#[derive(Debug, Clone, Default, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct GetSubaccountParams {
    pub address: String,
    #[serde(rename = "subaccountNumber")]
    pub subaccount_number: u32,
}
