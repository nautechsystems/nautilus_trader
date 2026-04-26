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

use std::{
    str::FromStr,
    time::{Duration, Instant},
};

use anyhow::Context;
use nautilus_binance::{
    common::{
        credential::resolve_credentials,
        enums::{BinanceEnvironment, BinanceFuturesOrderType, BinanceProductType},
        symbol::format_binance_symbol,
    },
    config::{BinanceDataClientConfig, BinanceExecClientConfig},
    factories::{BinanceDataClientFactory, BinanceExecutionClientFactory},
    futures::{
        BinanceFuturesHttpClient,
        http::{models::BinanceFuturesAlgoOrder, query::BinanceMarkPriceParams},
    },
};
use nautilus_common::{enums::Environment, live::get_runtime};
use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_live::node::{LiveNode, LiveNodeHandle};
use nautilus_model::{
    enums::{OrderType, TrailingOffsetType, TriggerType},
    identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
    types::Quantity,
};
use nautilus_network::websocket::TransportBackend;
use nautilus_testkit::testers::{ExecTester, ExecTesterConfig};
use nautilus_trading::strategy::StrategyConfig;
use rust_decimal::Decimal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = BinanceEnvironment::Testnet;
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BINANCE-FUTURES-001");
    let node_name = "BINANCE-FUTURES-TRAILING-STOP-TESTER-001".to_string();
    let client_id = ClientId::new("BINANCE");
    let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
    let product_type = BinanceProductType::UsdM;
    let order_qty = Quantity::from("0.001");

    let (api_key, api_secret) = resolve_credentials(None, None, environment, product_type)?;
    let clock = get_atomic_clock_realtime();
    let http_client = BinanceFuturesHttpClient::new(
        product_type,
        environment,
        clock,
        Some(api_key.clone()),
        Some(api_secret.clone()),
        None,
        None,
        None,
        None,
        false,
    )?;

    http_client.cancel_all_algo_orders(instrument_id).await?;

    let data_config = BinanceDataClientConfig {
        product_types: vec![product_type],
        environment,
        api_key: Some(api_key.clone()),
        api_secret: Some(api_secret.clone()),
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    let exec_config = BinanceExecClientConfig {
        trader_id,
        account_id,
        product_types: vec![product_type],
        environment,
        api_key: Some(api_key),
        api_secret: Some(api_secret),
        transport_backend: TransportBackend::Sockudo,
        ..Default::default()
    };

    let data_factory = BinanceDataClientFactory::new();
    let exec_factory = BinanceExecutionClientFactory::new();

    let mut node = LiveNode::builder(trader_id, Environment::Live)?
        .with_name(node_name)
        .add_data_client(None, Box::new(data_factory), Box::new(data_config))?
        .add_exec_client(None, Box::new(exec_factory), Box::new(exec_config))?
        .with_reconciliation(true)
        .with_timeout_connection(10)
        .with_delay_post_stop_secs(2)
        .build()?;

    let tester_config = ExecTesterConfig::builder()
        .base(StrategyConfig {
            strategy_id: Some(StrategyId::from("EXEC_TESTER-TRAILING-001")),
            external_order_claims: Some(vec![instrument_id]),
            ..Default::default()
        })
        .instrument_id(instrument_id)
        .client_id(client_id)
        .order_qty(order_qty)
        .log_data(false)
        .enable_limit_buys(false)
        .enable_limit_sells(false)
        .enable_stop_buys(false)
        .enable_stop_sells(true)
        .stop_order_type(OrderType::TrailingStopMarket)
        .stop_offset_ticks(5_000)
        .stop_trigger_type(TriggerType::MarkPrice)
        .trailing_offset(Decimal::from(25))
        .trailing_offset_type(TrailingOffsetType::BasisPoints)
        .build();

    node.add_strategy(ExecTester::new(tester_config))?;

    let handle = node.handle();
    let validation_client = http_client.clone();
    let validation_task = get_runtime().spawn(async move {
        let result = validate_trailing_stop_order(&validation_client, &handle, instrument_id).await;
        let cancel_result = validation_client
            .cancel_all_algo_orders(instrument_id)
            .await;
        handle.stop();

        result?;
        cancel_result?;
        Ok::<(), anyhow::Error>(())
    });

    node.run().await?;
    validation_task.await??;

    Ok(())
}

async fn validate_trailing_stop_order(
    http_client: &BinanceFuturesHttpClient,
    handle: &LiveNodeHandle,
    instrument_id: InstrumentId,
) -> anyhow::Result<()> {
    wait_for_running(handle, Duration::from_secs(30)).await?;

    let order =
        wait_for_open_trailing_stop(http_client, instrument_id, Duration::from_secs(30)).await?;
    let activate_price = order
        .activate_price
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Binance algo order omitted activatePrice"))?;
    let callback_rate = order
        .callback_rate
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Binance algo order omitted callbackRate"))?;

    anyhow::ensure!(
        order.order_type == BinanceFuturesOrderType::TrailingStopMarket,
        "Expected TRAILING_STOP_MARKET, was {:?}",
        order.order_type
    );
    anyhow::ensure!(
        callback_rate == "0.25",
        "Expected callbackRate 0.25, received {callback_rate}"
    );

    let mark_price = fetch_mark_price(http_client, instrument_id).await?;
    let activate_price = Decimal::from_str(activate_price).context("invalid activatePrice")?;
    anyhow::ensure!(
        activate_price > mark_price,
        "Expected sell trailing stop activatePrice {activate_price} to be above mark price {mark_price}"
    );

    println!(
        "Validated Binance trailing stop: type={:?} activatePrice={} callbackRate={} markPrice={}",
        order.order_type, activate_price, callback_rate, mark_price,
    );

    Ok(())
}

async fn wait_for_running(handle: &LiveNodeHandle, timeout: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        if handle.is_running() {
            return Ok(());
        }

        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    anyhow::bail!("Timed out waiting for LiveNode to start")
}

async fn wait_for_open_trailing_stop(
    http_client: &BinanceFuturesHttpClient,
    instrument_id: InstrumentId,
    timeout: Duration,
) -> anyhow::Result<BinanceFuturesAlgoOrder> {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        let orders = http_client
            .query_open_algo_orders(Some(instrument_id))
            .await?;

        if let Some(order) = orders
            .into_iter()
            .find(|order| order.order_type == BinanceFuturesOrderType::TrailingStopMarket)
        {
            return Ok(order);
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    anyhow::bail!("Timed out waiting for an open trailing stop algo order")
}

async fn fetch_mark_price(
    http_client: &BinanceFuturesHttpClient,
    instrument_id: InstrumentId,
) -> anyhow::Result<Decimal> {
    let params = BinanceMarkPriceParams {
        symbol: Some(format_binance_symbol(&instrument_id)),
    };
    let prices = http_client.mark_price(&params).await?;
    let price = prices
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No mark price returned for {instrument_id}"))?;

    Decimal::from_str(&price.mark_price).context("invalid mark price")
}
