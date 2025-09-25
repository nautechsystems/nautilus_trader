//! Response parsing functions for Hyperliquid HTTP models.
//!
//! This module converts upstream Hyperliquid "info" responses into normalized
//! instrument definitions suitable for later construction of Nautilus
//! `Instrument` instances inside an InstrumentProvider.
//!
//! Conventions:
//! - Mirror upstream field names via casing only (camelCase -> snake_case).
//! - Keep this module free of transport concerns (pure parsing).
//! - Return simple, adapter-local definition structs; mapping into
//!   Nautilus model types can be done by the InstrumentProvider.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::models::{PerpMeta, SpotMeta};

/// Market type enumeration for normalized instrument definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HyperliquidMarketType {
    /// Perpetual futures contract.
    Perp,
    /// Spot trading pair.
    Spot,
}

/// Normalized instrument definition produced by this parser.
///
/// This deliberately avoids any tight coupling to Nautilus' Cython types.
/// The InstrumentProvider can later convert this into Nautilus `Instrument`s.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidInstrumentDef {
    /// Human-readable symbol (e.g., "BTC-USD-PERP", "PURR-USDC-SPOT").
    pub symbol: String,
    /// Base currency/asset (e.g., "BTC", "PURR").
    pub base: String,
    /// Quote currency (e.g., "USD" for perps, "USDC" for spot).
    pub quote: String,
    /// Market type (perpetual or spot).
    pub market_type: HyperliquidMarketType,
    /// Number of decimal places for price precision.
    pub price_decimals: u32,
    /// Number of decimal places for size precision.
    pub size_decimals: u32,
    /// Price tick size as decimal.
    pub tick_size: Decimal,
    /// Size lot increment as decimal.
    pub lot_size: Decimal,
    /// Maximum leverage (for perps).
    pub max_leverage: Option<u32>,
    /// Whether requires isolated margin only.
    pub only_isolated: bool,
    /// Whether the instrument is active/tradeable.
    pub active: bool,
    /// Raw upstream data for debugging.
    pub raw_data: String,
}

/// Parse perpetual instrument definitions from Hyperliquid `meta` response.
///
/// Hyperliquid perps follow specific rules:
/// - Quote is always USD (USDC settled)
/// - Price decimals = max(0, 6 - sz_decimals) per venue docs
/// - Active = !is_delisted
pub fn parse_perp_instruments(meta: &PerpMeta) -> Result<Vec<HyperliquidInstrumentDef>, String> {
    const PERP_MAX_DECIMALS: i32 = 6; // Hyperliquid perps price decimal limit

    let mut defs = Vec::new();

    for asset in meta.universe.iter() {
        // Skip delisted assets
        if asset.is_delisted.unwrap_or(false) {
            continue;
        }

        let price_decimals = (PERP_MAX_DECIMALS - asset.sz_decimals as i32).max(0) as u32;
        let tick_size = pow10_neg(price_decimals)?;
        let lot_size = pow10_neg(asset.sz_decimals)?;

        let symbol = format!("{}-USD-PERP", asset.name);

        let def = HyperliquidInstrumentDef {
            symbol,
            base: asset.name.clone(),
            quote: "USD".to_string(), // Hyperliquid perps are USD-quoted (USDC settled)
            market_type: HyperliquidMarketType::Perp,
            price_decimals,
            size_decimals: asset.sz_decimals,
            tick_size,
            lot_size,
            max_leverage: asset.max_leverage,
            only_isolated: asset.only_isolated.unwrap_or(false),
            active: true,
            raw_data: serde_json::to_string(asset).unwrap_or_default(),
        };

        defs.push(def);
    }

    Ok(defs)
}

/// Parse spot instrument definitions from Hyperliquid `spotMeta` response.
///
/// Hyperliquid spot follows these rules:
/// - Price decimals = max(0, 8 - base_sz_decimals) per venue docs
/// - Size decimals from base token
/// - Active = is_canonical (only canonical pairs are tradeable)
pub fn parse_spot_instruments(meta: &SpotMeta) -> Result<Vec<HyperliquidInstrumentDef>, String> {
    const SPOT_MAX_DECIMALS: i32 = 8; // Hyperliquid spot price decimal limit

    let mut defs = Vec::new();

    // Build index -> token lookup
    let mut tokens_by_index = std::collections::HashMap::new();
    for token in &meta.tokens {
        tokens_by_index.insert(token.index, token);
    }

    for pair in &meta.universe {
        // Skip non-canonical pairs
        if !pair.is_canonical {
            continue;
        }

        // Resolve base and quote tokens
        let base_token = tokens_by_index
            .get(&pair.tokens[0])
            .ok_or_else(|| format!("Base token index {} not found", pair.tokens[0]))?;
        let quote_token = tokens_by_index
            .get(&pair.tokens[1])
            .ok_or_else(|| format!("Quote token index {} not found", pair.tokens[1]))?;

        let price_decimals = (SPOT_MAX_DECIMALS - base_token.sz_decimals as i32).max(0) as u32;
        let tick_size = pow10_neg(price_decimals)?;
        let lot_size = pow10_neg(base_token.sz_decimals)?;

        let symbol = format!("{}-{}-SPOT", base_token.name, quote_token.name);

        let def = HyperliquidInstrumentDef {
            symbol,
            base: base_token.name.clone(),
            quote: quote_token.name.clone(),
            market_type: HyperliquidMarketType::Spot,
            price_decimals,
            size_decimals: base_token.sz_decimals,
            tick_size,
            lot_size,
            max_leverage: None,
            only_isolated: false,
            active: true,
            raw_data: serde_json::to_string(pair).unwrap_or_default(),
        };

        defs.push(def);
    }

    Ok(defs)
}

/// Compute 10^(-decimals) as a Decimal.
///
/// This uses integer arithmetic to avoid floating-point precision issues.
fn pow10_neg(decimals: u32) -> Result<Decimal, String> {
    if decimals == 0 {
        return Ok(Decimal::ONE);
    }

    // Build 1 / 10^decimals using integer arithmetic
    Ok(Decimal::from_i128_with_scale(1, decimals))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;

    use super::{
        super::models::{PerpAsset, SpotPair, SpotToken},
        *,
    };

    #[rstest]
    fn test_pow10_neg() {
        assert_eq!(pow10_neg(0).unwrap(), Decimal::from(1));
        assert_eq!(pow10_neg(1).unwrap(), Decimal::from_str("0.1").unwrap());
        assert_eq!(pow10_neg(5).unwrap(), Decimal::from_str("0.00001").unwrap());
    }

    #[test]
    fn test_parse_perp_instruments() {
        let meta = PerpMeta {
            universe: vec![
                PerpAsset {
                    name: "BTC".to_string(),
                    sz_decimals: 5,
                    max_leverage: Some(50),
                    only_isolated: None,
                    is_delisted: None,
                },
                PerpAsset {
                    name: "DELIST".to_string(),
                    sz_decimals: 3,
                    max_leverage: Some(10),
                    only_isolated: Some(true),
                    is_delisted: Some(true), // Should be filtered out
                },
            ],
            margin_tables: vec![],
        };

        let defs = parse_perp_instruments(&meta).unwrap();

        // Should only have BTC (DELIST filtered out)
        assert_eq!(defs.len(), 1);

        let btc = &defs[0];
        assert_eq!(btc.symbol, "BTC-USD-PERP");
        assert_eq!(btc.base, "BTC");
        assert_eq!(btc.quote, "USD");
        assert_eq!(btc.market_type, HyperliquidMarketType::Perp);
        assert_eq!(btc.price_decimals, 1); // 6 - 5 = 1
        assert_eq!(btc.size_decimals, 5);
        assert_eq!(btc.tick_size, Decimal::from_str("0.1").unwrap());
        assert_eq!(btc.lot_size, Decimal::from_str("0.00001").unwrap());
        assert_eq!(btc.max_leverage, Some(50));
        assert!(!btc.only_isolated);
        assert!(btc.active);
    }

    #[rstest]
    fn test_parse_spot_instruments() {
        let tokens = vec![
            SpotToken {
                name: "USDC".to_string(),
                sz_decimals: 6,
                wei_decimals: 6,
                index: 0,
                token_id: "0x1".to_string(),
                is_canonical: true,
                evm_contract: None,
                full_name: None,
            },
            SpotToken {
                name: "PURR".to_string(),
                sz_decimals: 0,
                wei_decimals: 5,
                index: 1,
                token_id: "0x2".to_string(),
                is_canonical: true,
                evm_contract: None,
                full_name: None,
            },
        ];

        let pairs = vec![
            SpotPair {
                name: "PURR/USDC".to_string(),
                tokens: [1, 0], // PURR base, USDC quote
                index: 0,
                is_canonical: true,
            },
            SpotPair {
                name: "ALIAS".to_string(),
                tokens: [1, 0],
                index: 1,
                is_canonical: false, // Should be filtered out
            },
        ];

        let meta = SpotMeta {
            tokens,
            universe: pairs,
        };

        let defs = parse_spot_instruments(&meta).unwrap();

        // Should only have PURR/USDC (ALIAS filtered out)
        assert_eq!(defs.len(), 1);

        let purr_usdc = &defs[0];
        assert_eq!(purr_usdc.symbol, "PURR-USDC-SPOT");
        assert_eq!(purr_usdc.base, "PURR");
        assert_eq!(purr_usdc.quote, "USDC");
        assert_eq!(purr_usdc.market_type, HyperliquidMarketType::Spot);
        assert_eq!(purr_usdc.price_decimals, 8); // 8 - 0 = 8 (PURR sz_decimals = 0)
        assert_eq!(purr_usdc.size_decimals, 0);
        assert_eq!(
            purr_usdc.tick_size,
            Decimal::from_str("0.00000001").unwrap()
        );
        assert_eq!(purr_usdc.lot_size, Decimal::from(1));
        assert_eq!(purr_usdc.max_leverage, None);
        assert!(!purr_usdc.only_isolated);
        assert!(purr_usdc.active);
    }

    #[rstest]
    fn test_price_decimals_clamping() {
        // Test that price decimals are clamped to >= 0
        let meta = PerpMeta {
            universe: vec![PerpAsset {
                name: "HIGHPREC".to_string(),
                sz_decimals: 10, // 6 - 10 = -4, should clamp to 0
                max_leverage: Some(1),
                only_isolated: None,
                is_delisted: None,
            }],
            margin_tables: vec![],
        };

        let defs = parse_perp_instruments(&meta).unwrap();
        assert_eq!(defs[0].price_decimals, 0);
        assert_eq!(defs[0].tick_size, Decimal::from(1));
    }
}
