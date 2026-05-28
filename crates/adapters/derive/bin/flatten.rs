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

//! Cancels every open order on the configured Derive subaccount and flattens
//! every non-zero perp / option position with a reduce-only IOC limit close.
//!
//! Scope matches the Hyperliquid flatten: derivative positions only, since
//! flattening a spot balance would mean dumping into a different quote asset.
//!
//! Run with:
//! ```bash
//! cargo run -p nautilus-derive --bin derive-flatten            # mainnet (default)
//! cargo run -p nautilus-derive --bin derive-flatten -- testnet # testnet
//! ```
//!
//! Environment variables (mainnet / testnet pair):
//! - `DERIVE_WALLET_ADDRESS` / `DERIVE_TESTNET_WALLET_ADDRESS`
//! - `DERIVE_SESSION_PRIVATE_KEY` / `DERIVE_TESTNET_SESSION_PRIVATE_KEY`
//! - `DERIVE_SUBACCOUNT_ID` / `DERIVE_TESTNET_SUBACCOUNT_ID`

use std::{env, str::FromStr, time::Duration};

use alloy::{
    primitives::{Address, U256},
    signers::local::PrivateKeySigner,
};
use nautilus_derive::{
    common::{
        consts::{
            ACTION_TYPEHASH, DERIVE_NAUTILUS_REFERRAL_CODE, REST_URL_MAINNET, REST_URL_TESTNET,
            domain_separator_for, trade_module_address_for,
        },
        credential::credential_env_vars,
        enums::{
            DeriveEnvironment, DeriveInstrumentType, DeriveOrderSide, DeriveOrderType,
            DeriveTimeInForce,
        },
    },
    http::{
        client::{DeriveCredentials, DeriveHttpClient},
        models::{DerivePosition, DeriveTickerSnapshot},
        query::{
            DeriveCancelAllParams, DeriveGetOpenOrdersParams, DeriveGetPositionsParams,
            DeriveOrderParams, DeriveSignedEnvelope,
        },
    },
    signing::{
        eip712::{ActionContext, SignedAction},
        modules::trade::TradeModuleData,
        nonce::NonceManager,
    },
};
use rust_decimal::Decimal;

const CLOSE_SLIPPAGE_BPS: u32 = 50;
const MAX_FEE_PER_CONTRACT: &str = "1000";
const SIGNATURE_EXPIRY_SECS: i64 = 600;
const POST_CANCEL_DELAY: Duration = Duration::from_secs(1);
const POST_CLOSE_DELAY: Duration = Duration::from_secs(2);

fn parse_environment() -> anyhow::Result<DeriveEnvironment> {
    parse_environment_from(env::args().skip(1))
}

fn parse_environment_from<I, S>(mut args: I) -> anyhow::Result<DeriveEnvironment>
where
    I: Iterator<Item = S>,
    S: AsRef<str>,
{
    match args.next().as_ref().map(AsRef::as_ref) {
        None | Some("mainnet") => Ok(DeriveEnvironment::Mainnet),
        Some("testnet") => Ok(DeriveEnvironment::Testnet),
        Some(other) => anyhow::bail!(
            "Unknown environment '{other}'. Expected 'mainnet' (default) or 'testnet'."
        ),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    nautilus_common::logging::ensure_logging_initialized();

    let environment = parse_environment()?;
    log::warn!("Derive flatten starting against {environment:?}");

    let (wallet_var, session_var, subaccount_var) = credential_env_vars(environment);
    let wallet_address =
        env::var(wallet_var).map_err(|_| anyhow::anyhow!("missing env var {wallet_var}"))?;
    let session_private_key =
        env::var(session_var).map_err(|_| anyhow::anyhow!("missing env var {session_var}"))?;
    let subaccount_id: u64 = env::var(subaccount_var)
        .map_err(|_| anyhow::anyhow!("missing env var {subaccount_var}"))?
        .parse()
        .map_err(|e| anyhow::anyhow!("{subaccount_var} is not a u64: {e}"))?;

    let credentials = DeriveCredentials::new(wallet_address.clone(), &session_private_key)
        .map_err(|e| anyhow::anyhow!("failed to build credentials: {e}"))?;
    let base_url = match environment {
        DeriveEnvironment::Mainnet => REST_URL_MAINNET,
        DeriveEnvironment::Testnet => REST_URL_TESTNET,
    };
    let client = DeriveHttpClient::with_credentials(base_url, credentials, None, None, None)
        .map_err(|e| anyhow::anyhow!("failed to build http client: {e}"))?;

    let signer: PrivateKeySigner = session_private_key
        .parse()
        .map_err(|e| anyhow::anyhow!("failed to parse session private key: {e}"))?;
    let owner_addr = parse_address(&wallet_address, "wallet_address")?;
    let signer_addr = signer.address();
    let module_addr = parse_address(
        trade_module_address_for(environment),
        "trade_module_address",
    )?;
    let domain_separator = parse_b256(domain_separator_for(environment), "domain_separator")?;
    let action_typehash_bytes = parse_b256(ACTION_TYPEHASH, "ACTION_TYPEHASH")?;
    let nonce_manager = NonceManager::default();

    cancel_all_orders(&client, subaccount_id).await?;
    verify_no_open_orders(&client, subaccount_id).await?;

    let positions = fetch_non_zero_positions(&client, subaccount_id).await?;
    if positions.is_empty() {
        log::info!("Flatten complete: no non-zero positions on subaccount {subaccount_id}");
        return Ok(());
    }
    log::info!("Closing {} position(s)", positions.len());

    for position in &positions {
        close_position(
            &client,
            subaccount_id,
            position,
            owner_addr,
            signer_addr,
            module_addr,
            domain_separator,
            action_typehash_bytes,
            &nonce_manager,
            &signer,
            &wallet_address,
        )
        .await?;
    }

    tokio::time::sleep(POST_CLOSE_DELAY).await;
    verify_flat(&client, subaccount_id).await
}

async fn cancel_all_orders(client: &DeriveHttpClient, subaccount_id: u64) -> anyhow::Result<()> {
    log::info!("Cancelling every open order on subaccount {subaccount_id}");
    client
        .cancel_all(&DeriveCancelAllParams::new(subaccount_id))
        .await
        .map_err(|e| anyhow::anyhow!("cancel_all failed: {e}"))?;
    tokio::time::sleep(POST_CANCEL_DELAY).await;
    Ok(())
}

async fn verify_no_open_orders(
    client: &DeriveHttpClient,
    subaccount_id: u64,
) -> anyhow::Result<()> {
    let result = client
        .get_open_orders(&DeriveGetOpenOrdersParams::new(subaccount_id))
        .await
        .map_err(|e| anyhow::anyhow!("get_open_orders failed: {e}"))?;

    if result.orders.is_empty() {
        log::info!("All open orders cancelled");
        Ok(())
    } else {
        anyhow::bail!(
            "{} open order(s) still live after cancel_all; aborting before close",
            result.orders.len(),
        )
    }
}

async fn fetch_non_zero_positions(
    client: &DeriveHttpClient,
    subaccount_id: u64,
) -> anyhow::Result<Vec<DerivePosition>> {
    let positions = client
        .get_positions(&DeriveGetPositionsParams::new(subaccount_id))
        .await
        .map_err(|e| anyhow::anyhow!("get_positions failed: {e}"))?
        .positions;
    Ok(positions
        .into_iter()
        .filter(is_flattenable_position)
        .collect())
}

// Excludes ERC-20 spot: flattening a spot balance would dump the base asset
// into a different quote rather than close a derivative exposure.
fn is_flattenable_position(position: &DerivePosition) -> bool {
    !position.amount.is_zero() && position.instrument_type != DeriveInstrumentType::Erc20
}

#[allow(clippy::too_many_arguments)]
async fn close_position(
    client: &DeriveHttpClient,
    subaccount_id: u64,
    position: &DerivePosition,
    owner: Address,
    signer_address: Address,
    module_address: Address,
    domain_separator: alloy::primitives::B256,
    action_typehash_bytes: alloy::primitives::B256,
    nonce_manager: &NonceManager,
    signer: &PrivateKeySigner,
    wallet_address: &str,
) -> anyhow::Result<()> {
    let instrument_name = position.instrument_name.as_str();
    let is_long = position.amount.is_sign_positive();
    let close_qty = position.amount.abs();

    let ticker = client
        .get_ticker(instrument_name)
        .await
        .map_err(|e| anyhow::anyhow!("get_ticker for {instrument_name} failed: {e}"))?;
    let instrument = client
        .get_instrument(instrument_name)
        .await
        .map_err(|e| anyhow::anyhow!("get_instrument for {instrument_name} failed: {e}"))?;

    let close_side = if is_long {
        DeriveOrderSide::Sell
    } else {
        DeriveOrderSide::Buy
    };
    let limit_price =
        close_limit_price(&ticker, close_side, instrument.tick_size).ok_or_else(|| {
            anyhow::anyhow!(
                "ticker for {instrument_name} has no top-of-book on the {} side",
                if is_long { "bid" } else { "ask" },
            )
        })?;

    let asset_address =
        parse_address(instrument.base_asset_address.as_str(), "base_asset_address")?;
    let sub_id = U256::from_str_radix(instrument.base_asset_sub_id.as_str(), 10).map_err(|e| {
        anyhow::anyhow!(
            "base_asset_sub_id `{}` is not a U256: {e}",
            instrument.base_asset_sub_id,
        )
    })?;
    let max_fee = Decimal::from_str(MAX_FEE_PER_CONTRACT).expect("constant decimal");

    let trade = TradeModuleData {
        asset_address,
        sub_id,
        limit_price,
        amount: close_qty,
        max_fee,
        recipient_id: subaccount_id,
        is_bid: matches!(close_side, DeriveOrderSide::Buy),
    };

    let nonce = nonce_manager
        .next_nonce(wallet_address, subaccount_id)
        .map_err(|e| anyhow::anyhow!("nonce allocation failed: {e}"))?;
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| anyhow::anyhow!("system clock before UNIX epoch: {e}"))?
        .as_secs() as i64;
    let expiry = now_secs + SIGNATURE_EXPIRY_SECS;

    let ctx = ActionContext {
        subaccount_id,
        nonce,
        module_address,
        signature_expiry_sec: expiry,
        owner,
        signer: signer_address,
    };

    let mut action = SignedAction::new(ctx, &trade, domain_separator, action_typehash_bytes);
    action
        .sign(signer)
        .map_err(|e| anyhow::anyhow!("failed to sign close action for {instrument_name}: {e}"))?;

    let payload = DeriveOrderParams {
        envelope: DeriveSignedEnvelope::from_signed_action(&action),
        instrument_name: instrument_name.into(),
        direction: close_side,
        order_type: DeriveOrderType::Limit,
        time_in_force: DeriveTimeInForce::Ioc,
        limit_price,
        amount: close_qty,
        max_fee,
        label: "flatten".to_string(),
        referral_code: DERIVE_NAUTILUS_REFERRAL_CODE.to_string(),
        reduce_only: Some(true),
        mmp: None,
    };

    log::info!(
        "{instrument_name}: closing {close_qty} {} @ IOC limit {limit_price} (reduce_only)",
        if matches!(close_side, DeriveOrderSide::Buy) {
            "BUY"
        } else {
            "SELL"
        },
    );

    let response = client
        .submit_order(&payload)
        .await
        .map_err(|e| anyhow::anyhow!("close {instrument_name} rejected by venue: {e}"))?;
    log::debug!(
        "Close response for {instrument_name}: {}",
        response.order_id
    );
    Ok(())
}

// Computes the worst-acceptable close limit rounded to the instrument's
// tick_size. Closing a long (SELL) drops the bid by the slippage; closing a
// short (BUY) lifts the ask by the slippage. Returned price is rounded toward
// the aggressive side so the IOC fills.
fn close_limit_price(
    ticker: &DeriveTickerSnapshot,
    close_side: DeriveOrderSide,
    tick_size: Decimal,
) -> Option<Decimal> {
    let bps = Decimal::from(CLOSE_SLIPPAGE_BPS);
    let scale = Decimal::from(10_000_u32);
    let one = Decimal::ONE;

    match close_side {
        DeriveOrderSide::Sell => {
            let bid = ticker.best_bid_price;
            if bid <= Decimal::ZERO {
                return None;
            }
            let raw = bid * (one - bps / scale);
            let rounded = round_down_to_tick(raw, tick_size);
            // Cheap options can be bid at one tick: `bid * 0.995` then floored
            // to tick is zero, which signs into a `limit_price="0"` the venue
            // will reject. Clamp to one tick (the most aggressive valid sell
            // price) so the IOC still sweeps any bid >= one tick.
            if tick_size > Decimal::ZERO && rounded < tick_size {
                Some(tick_size)
            } else {
                Some(rounded)
            }
        }
        DeriveOrderSide::Buy => {
            let ask = ticker.best_ask_price;
            if ask <= Decimal::ZERO {
                return None;
            }
            let raw = ask * (one + bps / scale);
            Some(round_up_to_tick(raw, tick_size))
        }
    }
}

fn round_down_to_tick(value: Decimal, tick: Decimal) -> Decimal {
    if tick <= Decimal::ZERO {
        return value;
    }
    (value / tick).floor() * tick
}

fn round_up_to_tick(value: Decimal, tick: Decimal) -> Decimal {
    if tick <= Decimal::ZERO {
        return value;
    }
    (value / tick).ceil() * tick
}

fn parse_address(s: &str, field: &str) -> anyhow::Result<Address> {
    s.parse()
        .map_err(|e| anyhow::anyhow!("failed to parse {field} `{s}`: {e}"))
}

fn parse_b256(s: &str, field: &str) -> anyhow::Result<alloy::primitives::B256> {
    s.parse()
        .map_err(|e| anyhow::anyhow!("failed to parse {field} `{s}`: {e}"))
}

async fn verify_flat(client: &DeriveHttpClient, subaccount_id: u64) -> anyhow::Result<()> {
    let residual = fetch_non_zero_positions(client, subaccount_id).await?;
    if residual.is_empty() {
        log::info!("Flatten complete: all positions closed");
        return Ok(());
    }

    for p in &residual {
        log::error!(
            "Residual {} position: amount={}",
            p.instrument_name,
            p.amount,
        );
    }
    anyhow::bail!("{} residual position(s) after flatten", residual.len())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use serde_json::json;

    use super::*;

    fn ticker_with(bid: Decimal, ask: Decimal) -> DeriveTickerSnapshot {
        DeriveTickerSnapshot {
            instrument_name: "ETH-PERP".into(),
            best_ask_amount: dec!(1),
            best_ask_price: ask,
            best_bid_amount: dec!(1),
            best_bid_price: bid,
            funding_rate: None,
            index_price: dec!(0),
            mark_price: dec!(0),
            max_price: dec!(0),
            min_price: dec!(0),
            option_pricing: None,
            stats: None,
            timestamp: 0,
        }
    }

    #[rstest]
    #[case::no_arg_defaults_mainnet(&[][..], DeriveEnvironment::Mainnet)]
    #[case::explicit_mainnet(&["mainnet"][..], DeriveEnvironment::Mainnet)]
    #[case::explicit_testnet(&["testnet"][..], DeriveEnvironment::Testnet)]
    fn test_parse_environment_accepts(#[case] args: &[&str], #[case] expected: DeriveEnvironment) {
        let result = parse_environment_from(args.iter().copied()).unwrap();
        assert_eq!(result, expected);
    }

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

    #[rstest]
    fn test_close_limit_price_long_close_rounds_down_to_tick() {
        // Long close = SELL: drop the bid by 50bps, round DOWN to tick so the
        // IOC fills aggressively. bid=3500.49, slippage=50bps, raw=3482.9876,
        // tick=0.01 -> 3482.98.
        let ticker = ticker_with(dec!(3500.49), dec!(3501.50));
        let price = close_limit_price(&ticker, DeriveOrderSide::Sell, dec!(0.01)).unwrap();
        assert_eq!(price, dec!(3482.98));
    }

    #[rstest]
    fn test_close_limit_price_short_close_rounds_up_to_tick() {
        // Short close = BUY: lift the ask by 50bps, round UP to tick.
        // ask=3501.50, slippage=50bps, raw=3519.0075625, tick=0.01 -> 3519.01.
        let ticker = ticker_with(dec!(3500.49), dec!(3501.50));
        let price = close_limit_price(&ticker, DeriveOrderSide::Buy, dec!(0.01)).unwrap();
        assert_eq!(price, dec!(3519.01));
    }

    #[rstest]
    fn test_close_limit_price_non_positive_reference_returns_none() {
        let ticker = ticker_with(dec!(0), dec!(0));
        assert!(close_limit_price(&ticker, DeriveOrderSide::Sell, dec!(0.01)).is_none());
        assert!(close_limit_price(&ticker, DeriveOrderSide::Buy, dec!(0.01)).is_none());
    }

    fn position_with(instrument_type: &str, amount: &str) -> DerivePosition {
        let body = json!({
            "amount": amount,
            "average_price": "3500",
            "creation_timestamp": 0,
            "cumulative_funding": "0",
            "delta": "0",
            "gamma": "0",
            "index_price": "3500",
            "initial_margin": "0",
            "instrument_name": "ETH-PERP",
            "instrument_type": instrument_type,
            "leverage": null,
            "liquidation_price": null,
            "maintenance_margin": "0",
            "mark_price": "3500",
            "mark_value": "0",
            "net_settlements": "0",
            "open_orders_margin": "0",
            "pending_funding": "0",
            "realized_pnl": "0",
            "theta": "0",
            "unrealized_pnl": "0",
            "vega": "0",
        });
        serde_json::from_value(body).expect("position parses")
    }

    #[rstest]
    #[case::perp_non_zero("perp", "1", true)]
    #[case::erc20_non_zero("erc20", "1", false)]
    #[case::perp_zero("perp", "0", false)]
    #[case::erc20_zero("erc20", "0", false)]
    fn test_is_flattenable_position(
        #[case] instrument_type: &str,
        #[case] amount: &str,
        #[case] expected: bool,
    ) {
        // The flatten bin closes non-zero derivative exposure only; ERC-20 spot
        // is excluded regardless of amount.
        let position = position_with(instrument_type, amount);
        assert_eq!(is_flattenable_position(&position), expected);
    }

    #[rstest]
    fn test_close_limit_price_long_close_clamps_to_one_tick_when_rounding_underflows() {
        // Cheap option scenario: bid is one tick, slippage-adjusted price
        // floors to zero. Must clamp to one tick so we don't sign a
        // `limit_price="0"` the venue rejects.
        let ticker = ticker_with(dec!(0.01), dec!(0.02));
        let price = close_limit_price(&ticker, DeriveOrderSide::Sell, dec!(0.01)).unwrap();
        assert_eq!(price, dec!(0.01));
    }
}
