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

//! Cancels all open perp orders and flattens all perp positions on Hyperliquid.
//!
//! Scope is perp-only, matching the bybit flatten which iterates `Linear` and
//! `Inverse` product types. Spot and HIP-4 outcome orders/positions are
//! intentionally out of scope: spot holdings are token balances rather than
//! derivative positions, and "flattening" them would mean dumping into a
//! different quote asset — a real trade decision rather than cleanup. Working
//! orders on those product types are left untouched and will continue to be
//! eligible to fill after this binary exits.
//!
//! Run with:
//! ```bash
//! cargo run -p nautilus-hyperliquid --bin hyperliquid-flatten            # mainnet (default)
//! cargo run -p nautilus-hyperliquid --bin hyperliquid-flatten -- testnet # testnet
//! ```
//!
//! Environment variables:
//! - `HYPERLIQUID_PK` (mainnet) or `HYPERLIQUID_TESTNET_PK` (testnet)
//! - `HYPERLIQUID_ACCOUNT_ADDRESS` (optional, master account for agent wallets)
//! - `HYPERLIQUID_VAULT` / `HYPERLIQUID_TESTNET_VAULT` (optional, for vault trading)

use std::{env, str::FromStr, time::Duration};

use ahash::AHashMap;
use nautilus_hyperliquid::{
    common::{
        enums::{HyperliquidEnvironment, HyperliquidProductType},
        parse::{
            clamp_price_to_precision, derive_limit_from_trigger, extract_inner_error,
            round_to_sig_figs,
        },
    },
    http::{
        client::HyperliquidHttpClient,
        models::{
            HyperliquidExchangeResponse, HyperliquidExecAction, HyperliquidExecCancelOrderRequest,
            HyperliquidExecGrouping, HyperliquidExecLimitParams, HyperliquidExecOrderKind,
            HyperliquidExecPlaceOrderRequest, HyperliquidExecTif, HyperliquidL2Book,
        },
    },
};
use nautilus_model::{
    identifiers::AccountId,
    instruments::{Instrument, InstrumentAny},
};
use rust_decimal::Decimal;
use ustr::Ustr;

const VENUE_SUFFIX: &str = "HYPERLIQUID";
const CLOSE_SLIPPAGE_BPS: u32 = 50;

// Explicit CLI arg, not an env var, so stale shell state can't pick the wrong account
fn parse_environment() -> anyhow::Result<HyperliquidEnvironment> {
    parse_environment_from(env::args().skip(1))
}

fn parse_environment_from<I, S>(mut args: I) -> anyhow::Result<HyperliquidEnvironment>
where
    I: Iterator<Item = S>,
    S: AsRef<str>,
{
    match args.next().as_ref().map(AsRef::as_ref) {
        None | Some("mainnet") => Ok(HyperliquidEnvironment::Mainnet),
        Some("testnet") => Ok(HyperliquidEnvironment::Testnet),
        Some(other) => anyhow::bail!(
            "Unknown environment '{other}'. Expected 'mainnet' (default) or 'testnet'."
        ),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    nautilus_common::logging::ensure_logging_initialized();

    let environment = parse_environment()?;
    log::warn!("Hyperliquid flatten starting against {environment:?}");

    let mut client = HyperliquidHttpClient::from_env(environment)?;
    client.set_account_id(AccountId::new(format!("{VENUE_SUFFIX}-001")));
    if let Ok(addr) = env::var("HYPERLIQUID_ACCOUNT_ADDRESS") {
        client.set_account_address(Some(addr));
    }

    let user = client.get_account_address()?;
    log::info!("Account address: {user}");

    let instruments = client.request_instruments().await?;
    log::info!("Bootstrapped {} instruments", instruments.len());
    for instrument in &instruments {
        client.cache_instrument(instrument);
    }
    // HIP-3 perps need the venue raw_symbol (wildcard form) for `info_l2_book`,
    // so key by raw_symbol; perp-only matches the flatten contract.
    let perp_by_coin: AHashMap<Ustr, &InstrumentAny> = instruments
        .iter()
        .filter(|inst| {
            HyperliquidProductType::from_symbol(inst.id().symbol.as_str())
                .ok()
                .is_some_and(|pt| pt == HyperliquidProductType::Perp)
        })
        .map(|inst| (inst.raw_symbol().inner(), inst))
        .collect();

    cancel_open_orders(&client, &user, &perp_by_coin).await?;

    let open = fetch_perp_positions(&client, &user, &perp_by_coin).await?;
    if open.is_empty() {
        log::info!("Flatten complete: no open perp positions");
        return Ok(());
    }

    log::info!("Closing {} perp position(s)", open.len());

    for position in &open {
        close_position(&client, position).await?;
    }

    tokio::time::sleep(Duration::from_secs(2)).await;
    verify_flat(&client, &user, &perp_by_coin).await
}

async fn cancel_open_orders(
    client: &HyperliquidHttpClient,
    user: &str,
    perp_by_coin: &AHashMap<Ustr, &InstrumentAny>,
) -> anyhow::Result<()> {
    let cancels = fetch_open_perp_cancels(client, user, perp_by_coin).await?;
    if cancels.is_empty() {
        log::info!("No open perp orders to cancel");
        return Ok(());
    }

    log::info!("Cancelling {} open perp order(s)", cancels.len());
    let action = HyperliquidExecAction::Cancel { cancels };
    let response = client
        .post_action_exec(&action)
        .await
        .map_err(|e| anyhow::anyhow!("Cancel-all transport failure: {e}"))?;
    check_response("cancel_all", &response)?;
    // Per-item errors are advisory; the residual re-query below is the real check
    if let Some(inner) = extract_inner_error(&response) {
        log::warn!("Cancel-all returned per-item error: {inner}");
    }

    let residual = fetch_open_perp_cancels(client, user, perp_by_coin).await?;
    if !residual.is_empty() {
        anyhow::bail!(
            "{} perp order(s) still live after cancel; aborting before close",
            residual.len(),
        );
    }
    Ok(())
}

// Bails on any unresolved perp order; non-perp orders are filtered (out of scope)
async fn fetch_open_perp_cancels(
    client: &HyperliquidHttpClient,
    user: &str,
    perp_by_coin: &AHashMap<Ustr, &InstrumentAny>,
) -> anyhow::Result<Vec<HyperliquidExecCancelOrderRequest>> {
    let raw = client.info_frontend_open_orders(user).await?;
    parse_perp_cancels(&raw, perp_by_coin, |symbol| client.get_asset_index(symbol))
}

// Pure parsing step; split out so tests can drive it with synthetic JSON
fn parse_perp_cancels<F>(
    raw: &serde_json::Value,
    perp_by_coin: &AHashMap<Ustr, &InstrumentAny>,
    mut asset_index: F,
) -> anyhow::Result<Vec<HyperliquidExecCancelOrderRequest>>
where
    F: FnMut(&str) -> Option<u32>,
{
    let arr = raw
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("frontendOpenOrders response was not an array"))?;

    let mut cancels = Vec::with_capacity(arr.len());
    for order in arr {
        let coin_str = order
            .get("coin")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("open order missing coin field"))?;
        let coin = Ustr::from(coin_str);

        let Some(instrument) = perp_by_coin.get(&coin) else {
            log::debug!("Skipping non-perp open order coin={coin_str}");
            continue;
        };

        let oid = order
            .get("oid")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("open order on {coin_str} missing oid"))?;
        let asset = asset_index(instrument.id().symbol.as_str()).ok_or_else(|| {
            anyhow::anyhow!("Asset index unresolved for perp coin {coin_str}; cannot cancel")
        })?;
        cancels.push(HyperliquidExecCancelOrderRequest { asset, oid });
    }
    Ok(cancels)
}

// Sign of `signed_size` determines long/short; absolute value is the close quantity
#[derive(Debug)]
struct PerpPosition<'a> {
    instrument: &'a InstrumentAny,
    signed_size: Decimal,
}

// Hits `clearinghouseState` directly to avoid the high-level method's spot
// fetch, which would couple perp flatten to spot-endpoint failures.
async fn fetch_perp_positions<'a>(
    client: &HyperliquidHttpClient,
    user: &str,
    perp_by_coin: &'a AHashMap<Ustr, &InstrumentAny>,
) -> anyhow::Result<Vec<PerpPosition<'a>>> {
    let state = client.info_clearinghouse_state(user).await?;
    parse_perp_positions(&state, perp_by_coin)
}

// Pure parsing step; split out so tests can drive it with synthetic JSON
fn parse_perp_positions<'a>(
    state: &serde_json::Value,
    perp_by_coin: &'a AHashMap<Ustr, &InstrumentAny>,
) -> anyhow::Result<Vec<PerpPosition<'a>>> {
    let asset_positions = state
        .get("assetPositions")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("assetPositions missing from clearinghouseState"))?;

    let mut out = Vec::new();

    for entry in asset_positions {
        let position = entry
            .get("position")
            .ok_or_else(|| anyhow::anyhow!("assetPosition entry missing position field"))?;
        let coin_str = position
            .get("coin")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("perp position missing coin field"))?;
        let szi_str = position
            .get("szi")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("perp position on {coin_str} missing szi field"))?;
        let signed_size = Decimal::from_str(szi_str).map_err(|e| {
            anyhow::anyhow!("perp position on {coin_str} has unparsable szi='{szi_str}': {e}")
        })?;

        if signed_size.is_zero() {
            continue;
        }

        let instrument = perp_by_coin.get(&Ustr::from(coin_str)).ok_or_else(|| {
            anyhow::anyhow!(
                "Unresolved perp position on coin {coin_str} (size {signed_size}); \
                 refusing to flatten on partial data"
            )
        })?;
        out.push(PerpPosition {
            instrument,
            signed_size,
        });
    }
    Ok(out)
}

async fn close_position(
    client: &HyperliquidHttpClient,
    position: &PerpPosition<'_>,
) -> anyhow::Result<()> {
    let instrument = position.instrument;
    let instrument_id = instrument.id();
    let raw_symbol = instrument.raw_symbol().inner();
    let asset = client
        .get_asset_index(instrument_id.symbol.as_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Asset index not cached for {instrument_id}; instrument bootstrap incomplete"
            )
        })?;

    let is_buy = position.signed_size.is_sign_negative();
    let close_qty = position.signed_size.abs();

    // Mirror `derive_market_order_price`: slippage, 5-sig-fig cap, tick clamp
    let book = client.info_l2_book(raw_symbol.as_str()).await?;
    let reference = top_of_book_reference(&book, is_buy).ok_or_else(|| {
        anyhow::anyhow!(
            "L2 book for {raw_symbol} has no levels on the {} side",
            if is_buy { "ask" } else { "bid" }
        )
    })?;
    let price_precision = instrument.price_precision();
    let derived = derive_limit_from_trigger(reference, is_buy, CLOSE_SLIPPAGE_BPS);
    let sig_rounded = round_to_sig_figs(derived, 5);
    let price = clamp_price_to_precision(sig_rounded, price_precision, is_buy).normalize();

    log::info!(
        "{instrument_id}: closing {close_qty} {} @ IOC limit {price} (reduce_only)",
        if is_buy { "BUY" } else { "SELL" },
    );

    let order = HyperliquidExecPlaceOrderRequest {
        asset,
        is_buy,
        price,
        size: close_qty.normalize(),
        reduce_only: true,
        kind: HyperliquidExecOrderKind::Limit {
            limit: HyperliquidExecLimitParams {
                tif: HyperliquidExecTif::Ioc,
            },
        },
        cloid: None,
    };

    let action = HyperliquidExecAction::Order {
        orders: vec![order],
        grouping: HyperliquidExecGrouping::Na,
        builder: None,
    };
    let response = client.post_action_exec(&action).await?;
    check_response(&format!("close {instrument_id}"), &response)?;
    if let Some(inner) = extract_inner_error(&response) {
        anyhow::bail!("close {instrument_id} rejected by venue: {inner}");
    }
    Ok(())
}

fn top_of_book_reference(book: &HyperliquidL2Book, is_buy: bool) -> Option<Decimal> {
    // levels[0] = bids, levels[1] = asks; close-BUY (cover short) targets asks
    let side = usize::from(is_buy);
    let level = book.levels.get(side).and_then(|v| v.first())?;
    Decimal::from_str(&level.px).ok()
}

fn check_response(context: &str, response: &HyperliquidExchangeResponse) -> anyhow::Result<()> {
    match response {
        HyperliquidExchangeResponse::Status { status, response } if status == "err" => {
            anyhow::bail!("{context} failed: {response}")
        }
        HyperliquidExchangeResponse::Error { error } => {
            anyhow::bail!("{context} error: {error}")
        }
        _ => Ok(()),
    }
}

async fn verify_flat(
    client: &HyperliquidHttpClient,
    user: &str,
    perp_by_coin: &AHashMap<Ustr, &InstrumentAny>,
) -> anyhow::Result<()> {
    let residual = fetch_perp_positions(client, user, perp_by_coin).await?;
    if residual.is_empty() {
        log::info!("Flatten complete: all perp positions closed");
        return Ok(());
    }

    for p in &residual {
        log::error!(
            "Residual {} position: signed_size={}",
            p.instrument.id(),
            p.signed_size,
        );
    }
    anyhow::bail!("{} residual perp position(s) after flatten", residual.len())
}

#[cfg(test)]
mod tests {
    use nautilus_core::nanos::UnixNanos;
    use nautilus_hyperliquid::{common::consts::HYPERLIQUID_VENUE, http::models::HyperliquidLevel};
    use nautilus_model::{
        identifiers::{InstrumentId, Symbol},
        instruments::CryptoPerpetual,
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use serde_json::json;

    use super::*;

    fn btc_perp() -> InstrumentAny {
        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            InstrumentId::new(Symbol::new("BTC-USD-PERP"), *HYPERLIQUID_VENUE),
            Symbol::new("BTC"),
            Currency::from("BTC"),
            Currency::from("USDC"),
            Currency::from("USDC"),
            false,
            2,
            3,
            Price::from("0.01"),
            Quantity::from("0.001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    fn book_with(bids: &[(&str, &str)], asks: &[(&str, &str)]) -> HyperliquidL2Book {
        let to_levels = |entries: &[(&str, &str)]| -> Vec<HyperliquidLevel> {
            entries
                .iter()
                .map(|(px, sz)| HyperliquidLevel {
                    px: (*px).to_string(),
                    sz: (*sz).to_string(),
                })
                .collect()
        };
        HyperliquidL2Book {
            coin: Ustr::from("BTC"),
            levels: vec![to_levels(bids), to_levels(asks)],
            time: 0,
        }
    }

    #[rstest]
    #[case::no_arg_defaults_mainnet(&[], HyperliquidEnvironment::Mainnet)]
    #[case::explicit_mainnet(&["mainnet"], HyperliquidEnvironment::Mainnet)]
    #[case::explicit_testnet(&["testnet"], HyperliquidEnvironment::Testnet)]
    fn test_parse_environment_accepts(
        #[case] args: &[&str],
        #[case] expected: HyperliquidEnvironment,
    ) {
        let result = parse_environment_from(args.iter().copied()).unwrap();
        assert_eq!(result, expected);
    }

    // Pins fail-loud on bad input; silent fallback to Mainnet would be a footgun
    #[rstest]
    #[case::bogus("bogus")]
    #[case::wrong_case_mainnet("MAINNET")]
    #[case::wrong_case_testnet("Testnet")]
    fn test_parse_environment_rejects(#[case] arg: &str) {
        let result = parse_environment_from(std::iter::once(arg));
        assert!(result.is_err(), "expected error for arg '{arg}'");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains(arg),
            "error must surface bad input '{arg}', was: {msg}"
        );
    }

    // Pins side selection; a flipped selector would target the wrong book side
    #[rstest]
    #[case::buy_uses_ask(true, dec!(101))]
    #[case::sell_uses_bid(false, dec!(100))]
    fn test_top_of_book_reference_side_selection(#[case] is_buy: bool, #[case] expected: Decimal) {
        let book = book_with(&[("100", "1")], &[("101", "1")]);
        assert_eq!(top_of_book_reference(&book, is_buy), Some(expected));
    }

    #[rstest]
    fn test_top_of_book_reference_empty_side_returns_none() {
        let book = book_with(&[("100", "1")], &[]);
        assert_eq!(top_of_book_reference(&book, true), None);
        let book = book_with(&[], &[("101", "1")]);
        assert_eq!(top_of_book_reference(&book, false), None);
    }

    #[rstest]
    fn test_top_of_book_reference_unparsable_px_returns_none() {
        let book = book_with(&[("not_a_number", "1")], &[("101", "1")]);
        assert_eq!(top_of_book_reference(&book, false), None);
    }

    #[rstest]
    fn test_parse_perp_positions_emits_non_zero_perps() {
        let btc = btc_perp();
        let perp_by_coin = AHashMap::from([(btc.raw_symbol().inner(), &btc)]);

        let state = json!({
            "assetPositions": [
                { "type": "oneWay", "position": { "coin": "BTC", "szi": "0.5" } },
            ]
        });

        let out = parse_perp_positions(&state, &perp_by_coin).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].signed_size, dec!(0.5));
        assert_eq!(out[0].instrument.id(), btc.id());
    }

    // Mutation guard for the zero-szi continue: a drop would post a 0-qty close
    #[rstest]
    fn test_parse_perp_positions_skips_zero_szi() {
        let btc = btc_perp();
        let perp_by_coin = AHashMap::from([(btc.raw_symbol().inner(), &btc)]);

        let state = json!({
            "assetPositions": [
                { "type": "oneWay", "position": { "coin": "BTC", "szi": "0" } },
            ]
        });

        let out = parse_perp_positions(&state, &perp_by_coin).unwrap();
        assert!(out.is_empty());
    }

    // Pins the bail-on-unresolved invariant
    #[rstest]
    fn test_parse_perp_positions_bails_on_unresolved_coin() {
        let perp_by_coin: AHashMap<Ustr, &InstrumentAny> = AHashMap::new();

        let state = json!({
            "assetPositions": [
                { "type": "oneWay", "position": { "coin": "WEIRD", "szi": "1" } },
            ]
        });

        let err = parse_perp_positions(&state, &perp_by_coin)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("WEIRD"),
            "error must name the unresolved coin: {err}"
        );
        assert!(err.contains("Unresolved perp position"));
    }

    #[rstest]
    fn test_parse_perp_positions_bails_on_missing_szi() {
        let btc = btc_perp();
        let perp_by_coin = AHashMap::from([(btc.raw_symbol().inner(), &btc)]);

        let state = json!({
            "assetPositions": [
                { "type": "oneWay", "position": { "coin": "BTC" } },
            ]
        });

        let err = parse_perp_positions(&state, &perp_by_coin)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("szi"),
            "error must name the missing field: {err}"
        );
    }

    #[rstest]
    fn test_parse_perp_cancels_collects_perp_orders() {
        let btc = btc_perp();
        let perp_by_coin = AHashMap::from([(btc.raw_symbol().inner(), &btc)]);

        let raw = json!([
            { "coin": "BTC", "oid": 12345_u64 },
        ]);

        let cancels = parse_perp_cancels(&raw, &perp_by_coin, |sym| {
            (sym == "BTC-USD-PERP").then_some(7)
        })
        .unwrap();
        assert_eq!(cancels.len(), 1);
        assert_eq!(cancels[0].asset, 7);
        assert_eq!(cancels[0].oid, 12345);
    }

    // Non-perp orders are out of scope: filter, don't fail
    #[rstest]
    fn test_parse_perp_cancels_filters_non_perp() {
        let btc = btc_perp();
        let perp_by_coin = AHashMap::from([(btc.raw_symbol().inner(), &btc)]);

        let raw = json!([
            { "coin": "BTC", "oid": 1_u64 },
            { "coin": "PURR/USDC", "oid": 2_u64 },
            { "coin": "#500", "oid": 3_u64 },
        ]);

        let cancels = parse_perp_cancels(&raw, &perp_by_coin, |sym| {
            (sym == "BTC-USD-PERP").then_some(7)
        })
        .unwrap();
        assert_eq!(cancels.len(), 1);
        assert_eq!(cancels[0].oid, 1);
    }

    // Bail on unresolvable asset_index, don't skip the order
    #[rstest]
    fn test_parse_perp_cancels_bails_on_unresolved_asset() {
        let btc = btc_perp();
        let perp_by_coin = AHashMap::from([(btc.raw_symbol().inner(), &btc)]);

        let raw = json!([
            { "coin": "BTC", "oid": 1_u64 },
        ]);

        let err = parse_perp_cancels(&raw, &perp_by_coin, |_| None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Asset index unresolved"), "was: {err}");
        assert!(err.contains("BTC"));
    }

    #[rstest]
    fn test_parse_perp_cancels_bails_on_missing_oid() {
        let btc = btc_perp();
        let perp_by_coin = AHashMap::from([(btc.raw_symbol().inner(), &btc)]);

        let raw = json!([{ "coin": "BTC" }]);

        let err = parse_perp_cancels(&raw, &perp_by_coin, |_| Some(7))
            .unwrap_err()
            .to_string();
        assert!(err.contains("oid"), "was: {err}");
    }
}
