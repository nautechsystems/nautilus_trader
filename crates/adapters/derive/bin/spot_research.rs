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

//! Phase 0 venue-research probe for Derive ERC-20 spot.
//!
//! Submits a far-from-spread limit BUY on `ETH-USDC` signed via the Trade
//! module, captures the venue response, cancels, then repeats with
//! `reduce_only=true` to test the venue's spot reduce-only semantics.
//!
//! Run with `cargo run -p nautilus-derive --bin derive-spot-research`.
//!
//! Credentials come from the standard Derive env vars resolved by
//! [`credential_env_vars`] (`DERIVE_*` on mainnet, `DERIVE_TESTNET_*` on
//! testnet).

use std::{env, fs, time::Duration};

use alloy::{
    primitives::{Address, B256, U256},
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
        models::DeriveInstrument,
        query::{
            DeriveCancelAllParams, DeriveCancelParams, DeriveOrderParams, DeriveSignedEnvelope,
        },
    },
    signing::{
        eip712::{ActionContext, SignedAction},
        modules::trade::TradeModuleData,
        nonce::NonceManager,
    },
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::{Value, json};

const SIGNATURE_EXPIRY_SECS: i64 = 600;
const POST_SUBMIT_DELAY: Duration = Duration::from_secs(2);

fn parse_environment_from_args() -> anyhow::Result<DeriveEnvironment> {
    match env::args().nth(1).as_deref() {
        // Default to testnet for safety: callers must opt into mainnet explicitly.
        None | Some("testnet") => Ok(DeriveEnvironment::Testnet),
        Some("mainnet") => Ok(DeriveEnvironment::Mainnet),
        Some(other) => anyhow::bail!(
            "unknown environment '{other}'. Expected 'testnet' (default) or 'mainnet'"
        ),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    nautilus_common::logging::ensure_logging_initialized();

    let environment = parse_environment_from_args()?;
    let out_dir = match environment {
        DeriveEnvironment::Mainnet => "/tmp/derive_spot_probe_mainnet",
        DeriveEnvironment::Testnet => "/tmp/derive_spot_probe",
    };
    fs::create_dir_all(out_dir)?;

    let (wallet_var, session_var, subaccount_var) = credential_env_vars(environment);
    let wallet_address =
        env::var(wallet_var).map_err(|_| anyhow::anyhow!("missing env var {wallet_var}"))?;
    let session_private_key =
        env::var(session_var).map_err(|_| anyhow::anyhow!("missing env var {session_var}"))?;
    let subaccount_id: u64 = env::var(subaccount_var)
        .map_err(|_| anyhow::anyhow!("missing env var {subaccount_var}"))?
        .parse()
        .map_err(|e| anyhow::anyhow!("{subaccount_var} is not a u64: {e}"))?;
    log::warn!(
        "Spot research probe: environment={environment:?} subaccount={subaccount_id} wallet={wallet_address}"
    );

    let base_url = match environment {
        DeriveEnvironment::Mainnet => REST_URL_MAINNET,
        DeriveEnvironment::Testnet => REST_URL_TESTNET,
    };

    let credentials = DeriveCredentials::new(wallet_address.clone(), &session_private_key)
        .map_err(|e| anyhow::anyhow!("failed to build credentials: {e}"))?;
    let client = DeriveHttpClient::with_credentials(base_url, credentials, None, None, None)
        .map_err(|e| anyhow::anyhow!("failed to build http client: {e}"))?;

    let signer: PrivateKeySigner = session_private_key
        .parse()
        .map_err(|e| anyhow::anyhow!("failed to parse session private key: {e}"))?;
    let owner_addr: Address = wallet_address
        .parse()
        .map_err(|e| anyhow::anyhow!("failed to parse wallet address: {e}"))?;
    let signer_addr = signer.address();
    let module_addr: Address = trade_module_address_for(environment)
        .parse()
        .map_err(|e| anyhow::anyhow!("failed to parse trade module address: {e}"))?;
    let domain_separator: B256 = domain_separator_for(environment)
        .parse()
        .map_err(|e| anyhow::anyhow!("failed to parse domain separator: {e}"))?;
    let action_typehash: B256 = ACTION_TYPEHASH
        .parse()
        .map_err(|e| anyhow::anyhow!("failed to parse ACTION_TYPEHASH: {e}"))?;
    let nonce_manager = NonceManager::default();

    let instruments = client
        .get_instruments("ETH", DeriveInstrumentType::Erc20, false)
        .await
        .map_err(|e| anyhow::anyhow!("get_instruments ETH erc20 failed: {e}"))?;
    let instrument = instruments
        .into_iter()
        .find(|i| i.instrument_name.as_str() == "ETH-USDC")
        .ok_or_else(|| anyhow::anyhow!("ETH-USDC not in instruments list"))?;
    fs::write(
        format!("{out_dir}/instrument_eth_usdc.json"),
        serde_json::to_vec_pretty(&instrument)?,
    )?;
    log::info!(
        "ETH-USDC: tick={} min_amt={} maker_fee={} taker_fee={} base_fee={}",
        instrument.tick_size,
        instrument.minimum_amount,
        instrument.maker_fee_rate,
        instrument.taker_fee_rate,
        instrument.base_fee,
    );

    // Pre-trade book sanity: confirm we know the mid before signing so we
    // can guarantee the BUY price sits well below it. Mainnet has real
    // liquidity, so a far-OTM bid is the safest write probe.
    let ticker = client
        .get_ticker("ETH-USDC")
        .await
        .map_err(|e| anyhow::anyhow!("get_ticker ETH-USDC failed: {e}"))?;
    fs::write(
        format!("{out_dir}/ticker_eth_usdc.json"),
        serde_json::to_vec_pretty(&ticker)?,
    )?;
    let index_price = ticker.index_price;
    log::warn!(
        "ETH-USDC ticker: index={index_price} best_bid={} best_ask={}",
        ticker.best_bid_price,
        ticker.best_ask_price,
    );

    // Limit BUY at 1% of the index price (e.g. ~$25 if ETH=$2500). Won't
    // fill unless ETH craters 100x in seconds; the order is cancelled within
    // ~1s of placement either way.
    let probe_price = round_down_to_tick(index_price / Decimal::from(100), instrument.tick_size)
        .max(instrument.tick_size);
    let probe_amount = instrument.minimum_amount;
    log::warn!("Submitting probe BUY at {probe_price} (index={index_price}) qty={probe_amount}",);

    // Q4 / mainnet smoke: submit + cancel.
    let buy_response = submit_signed_spot_order(
        &client,
        &instrument,
        subaccount_id,
        owner_addr,
        signer_addr,
        module_addr,
        domain_separator,
        action_typehash,
        &nonce_manager,
        &signer,
        &wallet_address,
        probe_price,
        probe_amount,
        DeriveOrderSide::Buy,
        false,
        DeriveTimeInForce::Gtc,
        "research-q4-buy",
        out_dir,
    )
    .await;
    write_outcome(out_dir, "q4_submit_buy", &buy_response);

    if let Ok(resp) = &buy_response
        && let Some(order_id) = extract_order_id(resp)
    {
        log::info!("Cancel order_id={order_id}");
        let cancel = client
            .cancel_order(&DeriveCancelParams::new(
                subaccount_id,
                "ETH-USDC",
                order_id.as_str(),
            ))
            .await
            .map(|_| json!({}))
            .map_err(|e| anyhow::anyhow!("{e}"));
        write_outcome(out_dir, "q4_cancel_buy", &cancel);
    }

    // Q7 reduce_only tests only on testnet: we already characterised the
    // venue's reduce_only error surface; rerunning on mainnet is wasted
    // signed traffic.
    if matches!(environment, DeriveEnvironment::Testnet) {
        tokio::time::sleep(POST_SUBMIT_DELAY).await;

        // Q7a: spot + GTC + reduce_only=true.
        let q7a = submit_signed_spot_order(
            &client,
            &instrument,
            subaccount_id,
            owner_addr,
            signer_addr,
            module_addr,
            domain_separator,
            action_typehash,
            &nonce_manager,
            &signer,
            &wallet_address,
            dec!(100),
            dec!(0.1),
            DeriveOrderSide::Buy,
            true,
            DeriveTimeInForce::Gtc,
            "research-q7a-reduce-gtc",
            out_dir,
        )
        .await;
        write_outcome(out_dir, "q7a_spot_gtc_reduce_only", &q7a);
        cancel_if_open(
            &client,
            subaccount_id,
            "ETH-USDC",
            &q7a,
            "q7a_cancel",
            out_dir,
        )
        .await;

        tokio::time::sleep(POST_SUBMIT_DELAY).await;

        // Q7b: spot + IOC + reduce_only=true.
        let q7b = submit_signed_spot_order(
            &client,
            &instrument,
            subaccount_id,
            owner_addr,
            signer_addr,
            module_addr,
            domain_separator,
            action_typehash,
            &nonce_manager,
            &signer,
            &wallet_address,
            dec!(100),
            dec!(0.1),
            DeriveOrderSide::Buy,
            true,
            DeriveTimeInForce::Ioc,
            "research-q7b-reduce-ioc",
            out_dir,
        )
        .await;
        write_outcome(out_dir, "q7b_spot_ioc_reduce_only", &q7b);
        cancel_if_open(
            &client,
            subaccount_id,
            "ETH-USDC",
            &q7b,
            "q7b_cancel",
            out_dir,
        )
        .await;

        tokio::time::sleep(POST_SUBMIT_DELAY).await;

        // Q7c: perp control.
        let perp_instruments = client
            .get_instruments("ETH", DeriveInstrumentType::Perp, false)
            .await
            .map_err(|e| anyhow::anyhow!("get_instruments ETH perp failed: {e}"))?;
        let perp = perp_instruments
            .into_iter()
            .find(|i| i.instrument_name.as_str() == "ETH-PERP")
            .ok_or_else(|| anyhow::anyhow!("ETH-PERP not in instruments list"))?;
        let q7c = submit_signed_spot_order(
            &client,
            &perp,
            subaccount_id,
            owner_addr,
            signer_addr,
            module_addr,
            domain_separator,
            action_typehash,
            &nonce_manager,
            &signer,
            &wallet_address,
            dec!(100),
            dec!(0.1),
            DeriveOrderSide::Buy,
            true,
            DeriveTimeInForce::Ioc,
            "research-q7c-perp-ioc-reduce",
            out_dir,
        )
        .await;
        write_outcome(out_dir, "q7c_perp_ioc_reduce_only", &q7c);
        cancel_if_open(
            &client,
            subaccount_id,
            "ETH-PERP",
            &q7c,
            "q7c_cancel",
            out_dir,
        )
        .await;
    }

    // Safety belt: cancel_all spot orders on the subaccount.
    let cancel_all = client
        .cancel_all(&DeriveCancelAllParams::new(subaccount_id).with_instrument_name("ETH-USDC"))
        .await
        .map(|_| json!({}))
        .map_err(|e| anyhow::anyhow!("{e}"));
    write_outcome(out_dir, "safety_cancel_all", &cancel_all);

    log::warn!("Spot research probe complete. Outputs under {out_dir}");
    Ok(())
}

fn round_down_to_tick(value: Decimal, tick: Decimal) -> Decimal {
    if tick <= Decimal::ZERO {
        return value;
    }
    (value / tick).floor() * tick
}

#[allow(clippy::too_many_arguments)]
async fn submit_signed_spot_order(
    client: &DeriveHttpClient,
    instrument: &DeriveInstrument,
    subaccount_id: u64,
    owner: Address,
    signer_address: Address,
    module_address: Address,
    domain_separator: B256,
    action_typehash: B256,
    nonce_manager: &NonceManager,
    signer: &PrivateKeySigner,
    wallet_address: &str,
    limit_price: Decimal,
    amount: Decimal,
    side: DeriveOrderSide,
    reduce_only: bool,
    tif: DeriveTimeInForce,
    label: &str,
    out_dir: &str,
) -> anyhow::Result<Value> {
    let asset_address: Address = instrument
        .base_asset_address
        .as_str()
        .parse()
        .map_err(|e| anyhow::anyhow!("parse base_asset_address: {e}"))?;
    let sub_id = U256::from_str_radix(instrument.base_asset_sub_id.as_str(), 10)
        .map_err(|e| anyhow::anyhow!("parse base_asset_sub_id as U256: {e}"))?;
    let max_fee = dec!(1000);

    let trade = TradeModuleData {
        asset_address,
        sub_id,
        limit_price,
        amount,
        max_fee,
        recipient_id: subaccount_id,
        is_bid: matches!(side, DeriveOrderSide::Buy),
    };

    let nonce = nonce_manager
        .next_nonce(wallet_address, subaccount_id)
        .map_err(|e| anyhow::anyhow!("nonce allocation: {e}"))?;
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;

    let ctx = ActionContext {
        subaccount_id,
        nonce,
        module_address,
        signature_expiry_sec: now_secs + SIGNATURE_EXPIRY_SECS,
        owner,
        signer: signer_address,
    };
    let mut action = SignedAction::new(ctx, &trade, domain_separator, action_typehash);
    action
        .sign(signer)
        .map_err(|e| anyhow::anyhow!("sign trade: {e}"))?;

    let payload = DeriveOrderParams {
        envelope: DeriveSignedEnvelope::from_signed_action(&action),
        instrument_name: instrument.instrument_name,
        direction: side,
        order_type: DeriveOrderType::Limit,
        time_in_force: tif,
        limit_price,
        amount,
        max_fee,
        label: label.to_string(),
        referral_code: DERIVE_NAUTILUS_REFERRAL_CODE.to_string(),
        reduce_only: reduce_only.then_some(true),
        mmp: None,
    };
    fs::write(
        format!("{out_dir}/{label}_request.json"),
        serde_json::to_vec_pretty(&payload)?,
    )?;

    log::info!(
        "submitting spot order label={label} price={limit_price} amount={amount} side={side:?} reduce_only={reduce_only}"
    );
    let response = client
        .submit_order(&payload)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(json!({"order": response, "trades": []}))
}

fn extract_order_id(response: &Value) -> Option<String> {
    response
        .get("order")
        .and_then(|o| o.get("order_id"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

async fn cancel_if_open(
    client: &DeriveHttpClient,
    subaccount_id: u64,
    instrument_name: &str,
    submit_outcome: &anyhow::Result<Value>,
    out_label: &str,
    out_dir: &str,
) {
    if let Ok(resp) = submit_outcome {
        let is_open = resp
            .get("order")
            .and_then(|o| o.get("order_status"))
            .and_then(|s| s.as_str())
            .is_some_and(|s| s == "open");
        if let Some(order_id) = extract_order_id(resp)
            && is_open
        {
            log::info!("{out_label}: cancelling order_id={order_id}");
            let cancel = client
                .cancel_order(&DeriveCancelParams::new(
                    subaccount_id,
                    instrument_name,
                    order_id.as_str(),
                ))
                .await
                .map(|_| json!({}))
                .map_err(|e| anyhow::anyhow!("{e}"));
            write_outcome(out_dir, out_label, &cancel);
        }
    }
}

fn write_outcome(out_dir: &str, name: &str, outcome: &anyhow::Result<Value>) {
    match outcome {
        Ok(v) => {
            log::info!("{name}: ok");
            let _ = fs::write(
                format!("{out_dir}/{name}.json"),
                serde_json::to_vec_pretty(&json!({"ok": v})).unwrap(),
            );
        }
        Err(e) => {
            log::warn!("{name}: err={e}");
            let _ = fs::write(
                format!("{out_dir}/{name}.json"),
                serde_json::to_vec_pretty(&json!({"err": e.to_string()})).unwrap(),
            );
        }
    }
}
