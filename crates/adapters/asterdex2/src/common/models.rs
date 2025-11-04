// Asterdex data models
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsterdexSymbol {
    pub symbol: String,
    pub status: String,
    #[serde(rename = "baseAsset")]
    pub base_asset: String,
    #[serde(rename = "quoteAsset")]
    pub quote_asset: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsterdexExchangeInfo {
    pub symbols: Vec<AsterdexSymbol>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsterdexBalance {
    pub asset: String,
    pub free: String,
    pub locked: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsterdexAccount {
    pub balances: Vec<AsterdexBalance>,
}
