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

//! Cancels all open swap/futures orders and flattens all swap/futures positions on OKX.
//!
//! Spot and margin are skipped because their holdings are balances rather than
//! derivatives positions. Options are skipped because flattening requires
//! per-leg pricing and risk checks that this script does not perform.
//!
//! Run with:
//! ```text
//! cargo run -p nautilus-okx --bin okx-flatten
//! ```
//!
//! Environment variables:
//! - `OKX_API_KEY` / `OKX_API_SECRET` / `OKX_API_PASSPHRASE`
//! - `OKX_DEMO=true` selects demo trading. Otherwise the script uses live trading.

use std::{str::FromStr, time::Duration};

use anyhow::Context;
use nautilus_core::UUID4;
use nautilus_okx::{
    common::{
        consts::{OKX_NAUTILUS_BROKER_ID, OKX_SUCCESS_CODE, validate_okx_client_order_id},
        enums::{
            OKXAlgoOrderType, OKXEnvironment, OKXInstrumentType, OKXMarginMode, OKXOrderType,
            OKXPositionSide, OKXSide, OKXTradeMode,
        },
    },
    http::{
        client::OKXHttpClient,
        models::{
            OKXCancelAlgoOrderRequest, OKXCancelOrderRequest, OKXOrderAlgo, OKXOrderHistory,
            OKXPlaceOrderRequest, OKXPosition,
        },
        query::{GetAlgoOrdersParams, GetOrderListParams, GetPositionsParams},
    },
};
use rust_decimal::Decimal;

const FLATTEN_INSTRUMENT_TYPES: &[OKXInstrumentType] =
    &[OKXInstrumentType::Swap, OKXInstrumentType::Futures];
const ALGO_ORDER_TYPES: &[OKXAlgoOrderType] = &[
    OKXAlgoOrderType::Conditional,
    OKXAlgoOrderType::Oco,
    OKXAlgoOrderType::Trigger,
    OKXAlgoOrderType::MoveOrderStop,
    OKXAlgoOrderType::Iceberg,
    OKXAlgoOrderType::Twap,
];
const REGULAR_CANCEL_BATCH_SIZE: usize = 20;
const ALGO_CANCEL_BATCH_SIZE: usize = 10;

#[derive(Debug)]
struct OpenPosition {
    position: OKXPosition,
    close_side: OKXSide,
    quantity: Decimal,
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(false)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    nautilus_common::logging::ensure_logging_initialized();

    let environment = if env_flag("OKX_DEMO") {
        OKXEnvironment::Demo
    } else {
        OKXEnvironment::Live
    };
    log::warn!("OKX flatten starting against {environment:?}");

    let client = OKXHttpClient::with_credentials(
        None,
        None,
        None,
        None,
        60,
        3,
        1_000,
        10_000,
        environment,
        None,
    )?;

    bootstrap_instruments(&client).await?;
    cancel_open_orders(&client).await?;

    let open = fetch_open_positions(&client).await?;
    if open.is_empty() {
        log::info!("Flatten complete: no open OKX swap/futures positions");
        return Ok(());
    }

    log::info!("Closing {} OKX swap/futures position(s)", open.len());
    for position in &open {
        close_position(&client, position).await?;
    }

    tokio::time::sleep(Duration::from_secs(2)).await;
    verify_flat(&client).await
}

async fn bootstrap_instruments(client: &OKXHttpClient) -> anyhow::Result<()> {
    for instrument_type in FLATTEN_INSTRUMENT_TYPES {
        let (instruments, _) = client.request_instruments(*instrument_type, None).await?;
        log::info!(
            "Bootstrapped {} {instrument_type:?} instruments",
            instruments.len()
        );
        client.cache_instruments(&instruments);
    }

    Ok(())
}

async fn cancel_open_orders(client: &OKXHttpClient) -> anyhow::Result<()> {
    for instrument_type in FLATTEN_INSTRUMENT_TYPES {
        cancel_regular_orders(client, *instrument_type).await?;
        cancel_algo_orders(client, *instrument_type).await?;

        let residual_regular = fetch_pending_orders(client, *instrument_type).await?;
        let residual_algo = fetch_pending_algo_orders(client, *instrument_type).await?;
        if !residual_regular.is_empty() || !residual_algo.is_empty() {
            anyhow::bail!(
                "{instrument_type:?}: {} regular order(s) and {} algo order(s) still live after cancel; aborting before close",
                residual_regular.len(),
                residual_algo.len(),
            );
        }
    }

    Ok(())
}

async fn cancel_regular_orders(
    client: &OKXHttpClient,
    instrument_type: OKXInstrumentType,
) -> anyhow::Result<()> {
    let pending = fetch_pending_orders(client, instrument_type).await?;
    if pending.is_empty() {
        log::info!("{instrument_type:?}: no open regular orders");
        return Ok(());
    }

    log::info!(
        "{instrument_type:?}: cancelling {} regular order(s)",
        pending.len()
    );

    for chunk in pending.chunks(REGULAR_CANCEL_BATCH_SIZE) {
        let requests = chunk.iter().map(cancel_order_request).collect();
        client.cancel_orders(requests).await?;
    }

    Ok(())
}

async fn cancel_algo_orders(
    client: &OKXHttpClient,
    instrument_type: OKXInstrumentType,
) -> anyhow::Result<()> {
    let pending = fetch_pending_algo_orders(client, instrument_type).await?;
    if pending.is_empty() {
        log::info!("{instrument_type:?}: no open algo orders");
        return Ok(());
    }

    let mut standard = Vec::new();
    let mut advance = Vec::new();

    for order in &pending {
        if uses_advance_algo_cancel(order.ord_type) {
            advance.push(order);
        } else {
            standard.push(order);
        }
    }

    log::info!(
        "{instrument_type:?}: cancelling {} algo order(s)",
        pending.len()
    );

    for chunk in standard.chunks(ALGO_CANCEL_BATCH_SIZE) {
        let requests = chunk
            .iter()
            .map(|order| cancel_algo_order_request(order))
            .collect();
        client.cancel_algo_orders(requests).await?;
    }

    for chunk in advance.chunks(ALGO_CANCEL_BATCH_SIZE) {
        let requests = chunk
            .iter()
            .map(|order| cancel_algo_order_request(order))
            .collect();
        client.cancel_advance_algo_orders(requests).await?;
    }

    Ok(())
}

async fn fetch_pending_orders(
    client: &OKXHttpClient,
    instrument_type: OKXInstrumentType,
) -> anyhow::Result<Vec<OKXOrderHistory>> {
    let mut after = None;
    let mut orders = Vec::new();

    loop {
        let params = GetOrderListParams {
            inst_type: Some(instrument_type),
            after: after.clone(),
            limit: Some(100),
            ..Default::default()
        };
        let page = client.get_orders_pending(params).await?;
        if page.is_empty() {
            break;
        }

        let page_len = page.len();
        let next_after = page.last().map(|order| order.ord_id.to_string());
        orders.extend(page);

        if page_len < 100 {
            break;
        }

        let Some(next_after) = next_after else {
            break;
        };

        if after.as_deref() == Some(next_after.as_str()) {
            anyhow::bail!("{instrument_type:?}: orders-pending cursor did not advance");
        }
        after = Some(next_after);
    }

    Ok(orders)
}

async fn fetch_pending_algo_orders(
    client: &OKXHttpClient,
    instrument_type: OKXInstrumentType,
) -> anyhow::Result<Vec<OKXOrderAlgo>> {
    let mut orders = Vec::new();

    for order_type in ALGO_ORDER_TYPES {
        orders
            .extend(fetch_pending_algo_orders_by_type(client, instrument_type, *order_type).await?);
    }

    Ok(orders)
}

async fn fetch_pending_algo_orders_by_type(
    client: &OKXHttpClient,
    instrument_type: OKXInstrumentType,
    order_type: OKXAlgoOrderType,
) -> anyhow::Result<Vec<OKXOrderAlgo>> {
    let mut after = None;
    let mut orders = Vec::new();

    loop {
        let params = GetAlgoOrdersParams {
            inst_type: instrument_type,
            ord_type: Some(order_type),
            after: after.clone(),
            limit: Some(100),
            ..Default::default()
        };
        let page = client.get_order_algo_pending(params).await?;
        if page.is_empty() {
            break;
        }

        let page_len = page.len();
        let next_after = page.last().map(|order| order.algo_id.clone());
        orders.extend(page);

        if page_len < 100 {
            break;
        }

        let Some(next_after) = next_after else {
            break;
        };

        if after.as_deref() == Some(next_after.as_str()) {
            anyhow::bail!(
                "{instrument_type:?} {order_type:?}: orders-algo-pending cursor did not advance"
            );
        }
        after = Some(next_after);
    }

    Ok(orders)
}

async fn fetch_open_positions(client: &OKXHttpClient) -> anyhow::Result<Vec<OpenPosition>> {
    let mut open = Vec::new();

    for instrument_type in FLATTEN_INSTRUMENT_TYPES {
        let params = GetPositionsParams {
            inst_type: Some(*instrument_type),
            ..Default::default()
        };
        let positions = client.get_positions(params).await?;
        for position in positions {
            if let Some(open_position) = parse_open_position(position)? {
                open.push(open_position);
            }
        }
    }

    Ok(open)
}

fn parse_open_position(position: OKXPosition) -> anyhow::Result<Option<OpenPosition>> {
    let signed = parse_decimal("position size", &position.pos)?;
    if signed.is_zero() {
        return Ok(None);
    }

    let close_side = match position.pos_side {
        OKXPositionSide::Net | OKXPositionSide::None => {
            if signed.is_sign_positive() {
                OKXSide::Sell
            } else {
                OKXSide::Buy
            }
        }
        OKXPositionSide::Long => OKXSide::Sell,
        OKXPositionSide::Short => OKXSide::Buy,
    };

    Ok(Some(OpenPosition {
        position,
        close_side,
        quantity: signed.abs(),
    }))
}

async fn close_position(client: &OKXHttpClient, open: &OpenPosition) -> anyhow::Result<()> {
    let position = &open.position;
    let td_mode = trade_mode_from_margin_mode(position.mgn_mode)?;
    let cl_ord_id = new_flatten_client_order_id()?;
    let pos_side = Some(match position.pos_side {
        OKXPositionSide::None => OKXPositionSide::Net,
        side => side,
    });

    log::info!(
        "{}: closing {} {:?} via reduce-only Market",
        position.inst_id,
        open.quantity,
        open.close_side,
    );

    let response = client
        .place_order(OKXPlaceOrderRequest {
            inst_id: position.inst_id.to_string(),
            td_mode,
            ccy: None,
            cl_ord_id: Some(cl_ord_id),
            tag: Some(OKX_NAUTILUS_BROKER_ID.to_string()),
            side: open.close_side,
            pos_side,
            ord_type: OKXOrderType::Market,
            sz: open.quantity.normalize().to_string(),
            px: None,
            px_usd: None,
            px_vol: None,
            reduce_only: Some(true),
            tgt_ccy: None,
            attach_algo_ords: None,
            speed_bump: None,
            outcome: None,
            slippage_pct: None,
        })
        .await?;

    check_response(
        "place close order",
        response.s_code.as_deref(),
        response.s_msg.as_deref(),
    )?;

    let venue_order_id = response
        .ord_id
        .map_or_else(|| "<none>".to_string(), |ord_id| ord_id.to_string());
    log::info!(
        "{}: submitted close venue_order_id={venue_order_id}",
        position.inst_id,
    );
    Ok(())
}

async fn verify_flat(client: &OKXHttpClient) -> anyhow::Result<()> {
    cancel_open_orders(client).await?;

    let residual = fetch_open_positions(client).await?;
    if residual.is_empty() {
        log::info!("Flatten complete: all OKX swap/futures positions are flat");
        return Ok(());
    }

    let details = residual
        .iter()
        .map(|position| {
            format!(
                "{} {} {}",
                position.position.inst_id, position.position.pos_side, position.quantity
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    anyhow::bail!("Residual OKX swap/futures position(s) after flatten: {details}");
}

fn cancel_order_request(order: &OKXOrderHistory) -> OKXCancelOrderRequest {
    OKXCancelOrderRequest {
        inst_id: order.inst_id.to_string(),
        inst_id_code: None,
        ord_id: Some(order.ord_id.to_string()),
        cl_ord_id: None,
    }
}

fn cancel_algo_order_request(order: &OKXOrderAlgo) -> OKXCancelAlgoOrderRequest {
    OKXCancelAlgoOrderRequest {
        inst_id: order.inst_id.to_string(),
        inst_id_code: None,
        algo_id: Some(order.algo_id.clone()),
        algo_cl_ord_id: None,
    }
}

fn trade_mode_from_margin_mode(margin_mode: OKXMarginMode) -> anyhow::Result<OKXTradeMode> {
    match margin_mode {
        OKXMarginMode::Isolated => Ok(OKXTradeMode::Isolated),
        OKXMarginMode::Cross => Ok(OKXTradeMode::Cross),
        OKXMarginMode::None => {
            anyhow::bail!("position margin mode is empty; cannot choose tdMode")
        }
    }
}

fn uses_advance_algo_cancel(order_type: OKXAlgoOrderType) -> bool {
    matches!(
        order_type,
        OKXAlgoOrderType::MoveOrderStop | OKXAlgoOrderType::Iceberg | OKXAlgoOrderType::Twap
    )
}

fn parse_decimal(name: &str, value: &str) -> anyhow::Result<Decimal> {
    Decimal::from_str(value).with_context(|| format!("failed to parse {name} decimal {value:?}"))
}

fn check_response(label: &str, code: Option<&str>, message: Option<&str>) -> anyhow::Result<()> {
    if let Some(code) = code
        && code != OKX_SUCCESS_CODE
    {
        let message = message.unwrap_or("");
        anyhow::bail!("{label} failed: sCode={code} sMsg={message}");
    }

    Ok(())
}

fn new_flatten_client_order_id() -> anyhow::Result<String> {
    let suffix: String = UUID4::new()
        .to_string()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(31)
        .collect();
    let cl_ord_id = format!("F{suffix}");
    validate_okx_client_order_id(&cl_ord_id).map_err(anyhow::Error::msg)?;
    Ok(cl_ord_id)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

    #[rstest]
    #[case::net_long(OKXPositionSide::Net, "1.5", OKXSide::Sell, "1.5")]
    #[case::net_short(OKXPositionSide::Net, "-2.25", OKXSide::Buy, "2.25")]
    #[case::none_short(OKXPositionSide::None, "-3", OKXSide::Buy, "3")]
    #[case::hedge_long(OKXPositionSide::Long, "4", OKXSide::Sell, "4")]
    #[case::hedge_short(OKXPositionSide::Short, "5", OKXSide::Buy, "5")]
    fn test_parse_open_position_maps_close_side_and_quantity(
        #[case] position_side: OKXPositionSide,
        #[case] position_size: &str,
        #[case] expected_close_side: OKXSide,
        #[case] expected_quantity: &str,
    ) {
        let position = stub_position(position_side, position_size, OKXMarginMode::Cross);

        let parsed = parse_open_position(position)
            .unwrap()
            .expect("non-zero position must parse");

        assert_eq!(parsed.position.pos_side, position_side);
        assert_eq!(parsed.close_side, expected_close_side);
        assert_eq!(
            parsed.quantity,
            Decimal::from_str(expected_quantity).unwrap()
        );
    }

    #[rstest]
    fn test_parse_open_position_ignores_zero_position() {
        let position = stub_position(OKXPositionSide::Net, "0", OKXMarginMode::Cross);

        let parsed = parse_open_position(position).unwrap();

        assert!(parsed.is_none());
    }

    #[rstest]
    fn test_parse_open_position_rejects_invalid_decimal() {
        let position = stub_position(OKXPositionSide::Net, "not-a-decimal", OKXMarginMode::Cross);

        let err = parse_open_position(position).expect_err("invalid decimal must fail");

        assert!(
            err.to_string()
                .contains("failed to parse position size decimal"),
            "unexpected error: {err}"
        );
    }

    #[rstest]
    #[case::isolated(OKXMarginMode::Isolated, OKXTradeMode::Isolated)]
    #[case::cross(OKXMarginMode::Cross, OKXTradeMode::Cross)]
    fn test_trade_mode_from_margin_mode(
        #[case] margin_mode: OKXMarginMode,
        #[case] expected: OKXTradeMode,
    ) {
        assert_eq!(trade_mode_from_margin_mode(margin_mode).unwrap(), expected);
    }

    #[rstest]
    fn test_trade_mode_from_margin_mode_rejects_empty_margin_mode() {
        let err = trade_mode_from_margin_mode(OKXMarginMode::None)
            .expect_err("empty margin mode must fail");

        assert!(
            err.to_string().contains("position margin mode is empty"),
            "unexpected error: {err}"
        );
    }

    #[rstest]
    #[case::conditional(OKXAlgoOrderType::Conditional, false)]
    #[case::oco(OKXAlgoOrderType::Oco, false)]
    #[case::trigger(OKXAlgoOrderType::Trigger, false)]
    #[case::move_order_stop(OKXAlgoOrderType::MoveOrderStop, true)]
    #[case::iceberg(OKXAlgoOrderType::Iceberg, true)]
    #[case::twap(OKXAlgoOrderType::Twap, true)]
    fn test_uses_advance_algo_cancel(#[case] order_type: OKXAlgoOrderType, #[case] expected: bool) {
        assert_eq!(uses_advance_algo_cancel(order_type), expected);
    }

    #[rstest]
    fn test_check_response_allows_missing_or_success_code() {
        assert!(check_response("test", None, None).is_ok());
        assert!(check_response("test", Some(OKX_SUCCESS_CODE), None).is_ok());
    }

    #[rstest]
    fn test_check_response_rejects_non_success_code() {
        let err = check_response("place close order", Some("51000"), Some("bad request"))
            .expect_err("non-success code must fail");

        assert_eq!(
            err.to_string(),
            "place close order failed: sCode=51000 sMsg=bad request"
        );
    }

    #[rstest]
    fn test_new_flatten_client_order_id_is_okx_compatible() {
        for _ in 0..100 {
            let cl_ord_id = new_flatten_client_order_id().unwrap();

            assert_eq!(cl_ord_id.len(), 32);
            assert!(cl_ord_id.starts_with('F'));
            assert!(cl_ord_id.bytes().all(|b| b.is_ascii_alphanumeric()));
            validate_okx_client_order_id(&cl_ord_id).unwrap();
        }
    }

    fn stub_position(
        position_side: OKXPositionSide,
        position_size: &str,
        margin_mode: OKXMarginMode,
    ) -> OKXPosition {
        OKXPosition {
            inst_id: Ustr::from("ETH-USDT-SWAP"),
            inst_type: OKXInstrumentType::Swap,
            mgn_mode: margin_mode,
            pos_id: Some(Ustr::from("pos-1")),
            pos_side: position_side,
            pos: position_size.to_string(),
            base_bal: String::new(),
            ccy: String::new(),
            fee: String::new(),
            lever: String::new(),
            last: String::new(),
            mark_px: String::new(),
            liq_px: String::new(),
            mmr: String::new(),
            interest: String::new(),
            trade_id: Ustr::from("trade-1"),
            notional_usd: String::new(),
            avg_px: String::new(),
            upl: String::new(),
            upl_ratio: String::new(),
            u_time: 0,
            margin: String::new(),
            mgn_ratio: String::new(),
            adl: String::new(),
            c_time: String::new(),
            realized_pnl: String::new(),
            upl_last_px: String::new(),
            upl_ratio_last_px: String::new(),
            avail_pos: String::new(),
            be_px: String::new(),
            funding_fee: String::new(),
            idx_px: String::new(),
            liq_penalty: String::new(),
            opt_val: String::new(),
            pending_close_ord_liab_val: String::new(),
            pnl: String::new(),
            pos_ccy: String::new(),
            quote_bal: String::new(),
            quote_borrowed: String::new(),
            quote_interest: String::new(),
            spot_in_use_amt: String::new(),
            spot_in_use_ccy: String::new(),
            usd_px: String::new(),
            delta_bs: String::new(),
            gamma_bs: String::new(),
            theta_bs: String::new(),
            vega_bs: String::new(),
        }
    }
}
