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

//! Hyperliquid testnet smoke for rapid chained order modifications.
//!
//! Verifies the venue cancel-replace ordering that the execution client's modify
//! chain relies on: a passive order is placed, then modified sequentially (by OID
//! and by CLOID) and in a rapid back-to-back burst. The venue must keep a single
//! resting order under one CLOID with sequential new OIDs, converging to the last
//! burst price. Prices stay well below market (post-only) so nothing ever fills.
//!
//! Edit `COIN` below to change the instrument.
//!
//! Run with:
//! `cargo run --example hyperliquid-chained-modify-smoke --package nautilus-hyperliquid --features examples`
//!
//! Required credential environment variable: `HYPERLIQUID_TESTNET_PK`.

use std::{fmt::Display, time::Duration};

use nautilus_hyperliquid::{
    common::enums::HyperliquidEnvironment,
    http::{
        client::HyperliquidHttpClient,
        models::{
            Cloid, HyperliquidExecAction, HyperliquidExecCancelByCloidRequest,
            HyperliquidExecGrouping, HyperliquidExecLimitParams, HyperliquidExecModifyOrderRequest,
            HyperliquidExecModifyTarget, HyperliquidExecOrderKind,
            HyperliquidExecPlaceOrderRequest, HyperliquidExecTif,
        },
    },
};
use nautilus_model::identifiers::ClientOrderId;
use rust_decimal::Decimal;
use serde_json::Value;

const COIN: &str = "ETH";

fn place_order(
    asset: u32,
    price: Decimal,
    size: Decimal,
    cloid: Cloid,
) -> HyperliquidExecPlaceOrderRequest {
    HyperliquidExecPlaceOrderRequest {
        asset,
        is_buy: true,
        price,
        size,
        reduce_only: false,
        kind: HyperliquidExecOrderKind::Limit {
            limit: HyperliquidExecLimitParams {
                tif: HyperliquidExecTif::Alo, // post-only: never take, always rest
            },
        },
        cloid: Some(cloid),
    }
}

fn modify_action(
    target: HyperliquidExecModifyTarget,
    order: HyperliquidExecPlaceOrderRequest,
) -> HyperliquidExecAction {
    HyperliquidExecAction::Modify {
        modify: HyperliquidExecModifyOrderRequest { oid: target, order },
    }
}

// Returns the resting (oid, limit_px) entries under `cloid_hex` from frontendOpenOrders
fn resting_for_cloid(open: &Value, cloid_hex: &str) -> Vec<(u64, String)> {
    open.as_array()
        .map(|orders| {
            orders
                .iter()
                .filter(|o| o.get("cloid").and_then(Value::as_str) == Some(cloid_hex))
                .filter_map(|o| {
                    let oid = o.get("oid").and_then(Value::as_u64)?;
                    let px = o
                        .get("limitPx")
                        .and_then(Value::as_str)
                        .unwrap_or("?")
                        .to_string();
                    Some((oid, px))
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn post_and_log(client: &HyperliquidHttpClient, label: &str, action: &HyperliquidExecAction) {
    match client.post_action_exec(action).await {
        Ok(resp) => log::info!("{label}: ok -> {resp:?}"),
        Err(e) => log::error!("{label}: ERROR -> {e}"),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let environment = HyperliquidEnvironment::Testnet;
    log::info!("=== Hyperliquid {environment:?} chained-modify smoke ({COIN}) ===");

    let client = HyperliquidHttpClient::from_env(environment)?;
    let user = client.get_user_address()?;
    log::info!("User: {user}");

    let meta = client.info_meta().await?;
    let asset = meta
        .universe
        .iter()
        .position(|a| a.name == COIN)
        .expect("coin not found") as u32;
    log::info!("Asset {COIN} id={asset}");

    let book = client.info_l2_book(COIN).await?;
    let best_bid = book.levels[0][0].px;
    log::info!("Best bid: {best_bid}");

    // Passive base ~10% below market, rounded to whole dollars (valid integer
    // price for ETH) so it rests and never takes
    let base: Decimal = (best_bid * Decimal::new(90, 2)).round();
    let size = Decimal::new(1, 2); // 0.01 ETH (min size)

    let client_order_id = ClientOrderId::from("O-HL-STAGE5-SMOKE-1");
    let cloid = Cloid::from_client_order_id(client_order_id);
    let cloid_hex = cloid.to_hex();
    log::info!("Cloid: {cloid_hex}, base price: {base}, size: {size}");

    let mut pass = true;

    // 1) Place the passive order
    let place = HyperliquidExecAction::Order {
        orders: vec![place_order(asset, base, size, cloid)],
        grouping: HyperliquidExecGrouping::Na,
        builder: None,
    };
    post_and_log(&client, "place", &place).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let resting = resting_for_cloid(&client.info_frontend_open_orders(&user).await?, &cloid_hex);
    report(
        &mut pass,
        resting.len() == 1,
        format!("one resting order after place: {resting:?}"),
    );
    let Some(v0) = resting.first().map(|r| r.0) else {
        log::error!("no resting order; aborting");
        return Ok(());
    };
    log::info!("V0 oid={v0}");

    // 2) Sequential modify by OID -> new resting oid under same cloid
    post_and_log(
        &client,
        "modify-by-oid",
        &modify_action(
            HyperliquidExecModifyTarget::Oid(v0),
            place_order(asset, base + Decimal::ONE, size, cloid),
        ),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let resting = resting_for_cloid(&client.info_frontend_open_orders(&user).await?, &cloid_hex);
    report(
        &mut pass,
        resting.len() == 1,
        format!("one resting order after modify-by-oid: {resting:?}"),
    );
    let v1 = resting.first().map_or(0, |r| r.0);
    report(
        &mut pass,
        v1 != v0,
        format!("modify-by-oid produced a NEW oid: {v0} -> {v1}"),
    );

    // 3) Sequential modify by CLOID -> new resting oid, same cloid
    post_and_log(
        &client,
        "modify-by-cloid",
        &modify_action(
            HyperliquidExecModifyTarget::Cloid(cloid),
            place_order(asset, base + Decimal::TWO, size, cloid),
        ),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let resting = resting_for_cloid(&client.info_frontend_open_orders(&user).await?, &cloid_hex);
    report(
        &mut pass,
        resting.len() == 1,
        format!("one resting order after modify-by-cloid: {resting:?}"),
    );
    let v2 = resting.first().map_or(0, |r| r.0);
    report(
        &mut pass,
        v2 != v1,
        format!("modify-by-cloid produced a NEW oid: {v1} -> {v2}"),
    );

    // 4) RAPID BURST: three modify-by-cloid actions back-to-back, no settle
    // between, each repricing; the venue processes them in order and the chain
    // must converge to one resting order at the last price under the same cloid
    for (i, bump) in [3i64, 4, 5].into_iter().enumerate() {
        post_and_log(
            &client,
            &format!("burst-modify-{i}"),
            &modify_action(
                HyperliquidExecModifyTarget::Cloid(cloid),
                place_order(asset, base + Decimal::from(bump), size, cloid),
            ),
        )
        .await;
    }
    tokio::time::sleep(Duration::from_millis(3000)).await;
    let resting = resting_for_cloid(&client.info_frontend_open_orders(&user).await?, &cloid_hex);
    report(
        &mut pass,
        resting.len() == 1,
        format!("burst converged to ONE resting order (no dupes): {resting:?}"),
    );
    let want = base + Decimal::from(5);
    let final_px = resting.first().map(|r| r.1.clone()).unwrap_or_default();
    report(
        &mut pass,
        final_px.parse::<Decimal>().ok() == Some(want),
        format!("final resting price is the last burst price {want}: was {final_px}"),
    );

    // 5) Cleanup: cancel by cloid, confirm nothing rests
    post_and_log(
        &client,
        "cancel-by-cloid",
        &HyperliquidExecAction::CancelByCloid {
            cancels: vec![HyperliquidExecCancelByCloidRequest { asset, cloid }],
            fast: None,
        },
    )
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let resting = resting_for_cloid(&client.info_frontend_open_orders(&user).await?, &cloid_hex);
    report(
        &mut pass,
        resting.is_empty(),
        format!("no resting order after cancel: {resting:?}"),
    );

    log::info!(
        "=== chained-modify smoke {} ===",
        if pass { "PASSED" } else { "FAILED" }
    );
    Ok(())
}

fn report(pass: &mut bool, cond: bool, msg: impl Display) {
    if cond {
        log::info!("PASS: {msg}");
    } else {
        log::error!("FAIL: {msg}");
        *pass = false;
    }
}
