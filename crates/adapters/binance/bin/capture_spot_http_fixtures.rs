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

//! Capture Binance Spot HTTP SBE fixtures under the crate-local `test_data` tree.
//!
//! Use Binance docs examples as the primary source for JSON fixtures. Use this
//! tool to capture raw SBE payloads when the docs do not publish wire bytes or
//! when a live payload needs verification.
//!
//! Public fixture capture:
//! `cargo run --bin binance-spot-http-capture-fixtures --package nautilus-binance`
//!
//! Signed read-only capture on Spot testnet:
//! `cargo run --bin binance-spot-http-capture-fixtures --package nautilus-binance -- \
//!   --environment testnet --include-private`
//!
//! Signed order-flow capture on Spot testnet or demo:
//! `cargo run --bin binance-spot-http-capture-fixtures --package nautilus-binance -- \
//!   --environment testnet --include-order-flow --order-quantity 0.001 --order-price 10000`

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use chrono::Utc;
use nautilus_binance::{
    common::{
        credential::resolve_credentials,
        enums::{BinanceEnvironment, BinanceProductType, BinanceSide, BinanceTimeInForce},
    },
    spot::http::{
        BinanceRawSpotHttpClient,
        parse::decode_new_order_full,
        query::{
            AccountInfoParams, AccountTradesParams, AllOrdersParams, CancelOpenOrdersParams,
            CancelOrderParams, DepthParams, KlinesParams, NewOrderParams, OpenOrdersParams,
            QueryOrderParams, TradesParams,
        },
    },
};
use serde::Serialize;

const DEFAULT_DEPTH_LIMIT: u32 = 5;
const DEFAULT_TRADES_LIMIT: u32 = 10;
const DEFAULT_KLINES_LIMIT: u32 = 10;
const DEFAULT_TIMEOUT_SECS: u64 = 30;
const DEFAULT_INTERVAL: &str = "1m";
const DEFAULT_SYMBOL: &str = "BTCUSDT";
const SPOT_MARKET_DOCS: &str =
    "https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints";
const SPOT_ACCOUNT_DOCS: &str =
    "https://developers.binance.com/docs/binance-spot-api-docs/rest-api/account-endpoints";
const SPOT_TRADING_DOCS: &str =
    "https://developers.binance.com/docs/binance-spot-api-docs/rest-api/trading-endpoints";

#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureConfig {
    environment: BinanceEnvironment,
    output_dir: PathBuf,
    symbol: String,
    interval: String,
    depth_limit: u32,
    trades_limit: u32,
    klines_limit: u32,
    include_private: bool,
    include_order_flow: bool,
    order_quantity: Option<String>,
    order_price: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FixtureAuth {
    Public,
    Signed,
}

#[derive(Debug, Clone, Serialize)]
struct FixtureManifest {
    command: String,
    captured_at: String,
    environment: String,
    symbol: String,
    interval: String,
    output_dir: String,
    fixtures: Vec<FixtureRecord>,
}

#[derive(Debug, Clone, Serialize)]
struct FixtureRecord {
    name: String,
    category: String,
    auth: FixtureAuth,
    method: String,
    endpoint: String,
    docs_url: String,
    parser_functions: Vec<String>,
    payload_path: String,
    metadata_path: String,
    bytes: usize,
    symbol: Option<String>,
    interval: Option<String>,
    order_id: Option<i64>,
    client_order_id: Option<String>,
    sbe_header: Option<SbeHeaderRecord>,
}

#[derive(Debug, Clone, Serialize)]
struct FixtureMetadata {
    fixture: FixtureRecord,
    captured_at: String,
}

#[derive(Debug, Clone, Serialize)]
struct SbeHeaderRecord {
    block_length: u16,
    template_id: u16,
    schema_id: u16,
    version: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    nautilus_common::logging::ensure_logging_initialized();

    let config = parse_args(env::args().skip(1))?;
    let manifest = capture_fixtures(&config).await?;
    let manifest_path = config
        .output_dir
        .join(environment_name(config.environment))
        .join("manifest.json");
    write_json(&manifest_path, &manifest)?;

    println!(
        "Captured {} fixture(s) under {}",
        manifest.fixtures.len(),
        manifest_path
            .parent()
            .unwrap_or(&config.output_dir)
            .display()
    );

    Ok(())
}

async fn capture_fixtures(config: &CaptureConfig) -> anyhow::Result<FixtureManifest> {
    let output_root = config.output_dir.join(environment_name(config.environment));
    fs::create_dir_all(&output_root)?;

    let credentials = if config.include_private || config.include_order_flow {
        let (api_key, api_secret) =
            resolve_credentials(None, None, config.environment, BinanceProductType::Spot)?;
        Some((api_key, api_secret))
    } else {
        None
    };

    let client = BinanceRawSpotHttpClient::new(
        config.environment,
        credentials.as_ref().map(|(api_key, _)| api_key.clone()),
        credentials
            .as_ref()
            .map(|(_, api_secret)| api_secret.clone()),
        None,
        None,
        Some(DEFAULT_TIMEOUT_SECS),
        None,
    )?;

    let mut fixtures = Vec::new();

    capture_public_fixtures(config, &client, &output_root, &mut fixtures).await?;

    if config.include_private {
        capture_private_read_fixtures(config, &client, &output_root, &mut fixtures).await?;
    }

    if config.include_order_flow {
        capture_order_flow_fixtures(config, &client, &output_root, &mut fixtures).await?;
    }

    Ok(FixtureManifest {
        command: env::args().collect::<Vec<_>>().join(" "),
        captured_at: Utc::now().to_rfc3339(),
        environment: environment_name(config.environment).to_string(),
        symbol: config.symbol.clone(),
        interval: config.interval.clone(),
        output_dir: output_root.display().to_string(),
        fixtures,
    })
}

async fn capture_public_fixtures(
    config: &CaptureConfig,
    client: &BinanceRawSpotHttpClient,
    output_root: &Path,
    fixtures: &mut Vec<FixtureRecord>,
) -> anyhow::Result<()> {
    let public_dir = output_root.join("public");

    let ping = client.get("ping", None::<&()>).await?;
    record_fixture(
        output_root,
        &public_dir,
        &FixtureCapture {
            name: "ping_response",
            category: "public",
            auth: FixtureAuth::Public,
            method: "GET",
            endpoint: "/api/v3/ping",
            docs_url: SPOT_MARKET_DOCS,
            parser_functions: &["nautilus_binance::spot::http::parse::decode_ping"],
            payload_relpath: "public/ping_response.sbe",
            symbol: None,
            interval: None,
            order_id: None,
            client_order_id: None,
        },
        &ping,
        fixtures,
    )?;

    let server_time = client.get("time", None::<&()>).await?;
    record_fixture(
        output_root,
        &public_dir,
        &FixtureCapture {
            name: "server_time_response",
            category: "public",
            auth: FixtureAuth::Public,
            method: "GET",
            endpoint: "/api/v3/time",
            docs_url: SPOT_MARKET_DOCS,
            parser_functions: &["nautilus_binance::spot::http::parse::decode_server_time"],
            payload_relpath: "public/server_time_response.sbe",
            symbol: None,
            interval: None,
            order_id: None,
            client_order_id: None,
        },
        &server_time,
        fixtures,
    )?;

    let exchange_info = client.get("exchangeInfo", None::<&()>).await?;
    record_fixture(
        output_root,
        &public_dir,
        &FixtureCapture {
            name: "exchange_info_response",
            category: "public",
            auth: FixtureAuth::Public,
            method: "GET",
            endpoint: "/api/v3/exchangeInfo",
            docs_url: SPOT_MARKET_DOCS,
            parser_functions: &[
                "nautilus_binance::spot::http::parse::decode_exchange_info",
                "nautilus_binance::common::parse::parse_spot_instrument_sbe",
            ],
            payload_relpath: "public/exchange_info_response.sbe",
            symbol: Some(config.symbol.as_str()),
            interval: None,
            order_id: None,
            client_order_id: None,
        },
        &exchange_info,
        fixtures,
    )?;

    let depth_params = DepthParams::new(&config.symbol).with_limit(config.depth_limit);
    let depth = client.get("depth", Some(&depth_params)).await?;
    record_fixture(
        output_root,
        &public_dir,
        &FixtureCapture {
            name: "depth_response",
            category: "public",
            auth: FixtureAuth::Public,
            method: "GET",
            endpoint: "/api/v3/depth",
            docs_url: SPOT_MARKET_DOCS,
            parser_functions: &["nautilus_binance::spot::http::parse::decode_depth"],
            payload_relpath: &format!(
                "public/depth_{}_limit{}.sbe",
                sanitized(&config.symbol),
                config.depth_limit
            ),
            symbol: Some(config.symbol.as_str()),
            interval: None,
            order_id: None,
            client_order_id: None,
        },
        &depth,
        fixtures,
    )?;

    let trades_params = TradesParams::new(&config.symbol).with_limit(config.trades_limit);
    let trades = client.get("trades", Some(&trades_params)).await?;
    record_fixture(
        output_root,
        &public_dir,
        &FixtureCapture {
            name: "trades_response",
            category: "public",
            auth: FixtureAuth::Public,
            method: "GET",
            endpoint: "/api/v3/trades",
            docs_url: SPOT_MARKET_DOCS,
            parser_functions: &[
                "nautilus_binance::spot::http::parse::decode_trades",
                "nautilus_binance::common::parse::parse_spot_trades_sbe",
            ],
            payload_relpath: &format!(
                "public/trades_{}_limit{}.sbe",
                sanitized(&config.symbol),
                config.trades_limit
            ),
            symbol: Some(config.symbol.as_str()),
            interval: None,
            order_id: None,
            client_order_id: None,
        },
        &trades,
        fixtures,
    )?;

    let klines_params = KlinesParams {
        symbol: config.symbol.clone(),
        interval: config.interval.clone(),
        start_time: None,
        end_time: None,
        time_zone: None,
        limit: Some(config.klines_limit),
    };
    let klines = client.get("klines", Some(&klines_params)).await?;
    record_fixture(
        output_root,
        &public_dir,
        &FixtureCapture {
            name: "klines_response",
            category: "public",
            auth: FixtureAuth::Public,
            method: "GET",
            endpoint: "/api/v3/klines",
            docs_url: SPOT_MARKET_DOCS,
            parser_functions: &[
                "nautilus_binance::spot::http::parse::decode_klines",
                "nautilus_binance::common::parse::parse_klines_to_bars",
            ],
            payload_relpath: &format!(
                "public/klines_{}_{}_limit{}.sbe",
                sanitized(&config.symbol),
                sanitized(&config.interval),
                config.klines_limit
            ),
            symbol: Some(config.symbol.as_str()),
            interval: Some(config.interval.as_str()),
            order_id: None,
            client_order_id: None,
        },
        &klines,
        fixtures,
    )?;

    Ok(())
}

async fn capture_private_read_fixtures(
    config: &CaptureConfig,
    client: &BinanceRawSpotHttpClient,
    output_root: &Path,
    fixtures: &mut Vec<FixtureRecord>,
) -> anyhow::Result<()> {
    let private_dir = output_root.join("private_read");

    let account_params = AccountInfoParams::new().omit_zero_balances();
    let account = client.get_signed("account", Some(&account_params)).await?;
    record_fixture(
        output_root,
        &private_dir,
        &FixtureCapture {
            name: "account_response",
            category: "private_read",
            auth: FixtureAuth::Signed,
            method: "GET",
            endpoint: "/api/v3/account",
            docs_url: SPOT_ACCOUNT_DOCS,
            parser_functions: &["nautilus_binance::spot::http::parse::decode_account"],
            payload_relpath: "private_read/account_response.sbe",
            symbol: None,
            interval: None,
            order_id: None,
            client_order_id: None,
        },
        &account,
        fixtures,
    )?;

    let account_trades_params =
        AccountTradesParams::new(&config.symbol).with_limit(config.trades_limit);
    let account_trades = client
        .get_signed("myTrades", Some(&account_trades_params))
        .await?;
    record_fixture(
        output_root,
        &private_dir,
        &FixtureCapture {
            name: "account_trades_response",
            category: "private_read",
            auth: FixtureAuth::Signed,
            method: "GET",
            endpoint: "/api/v3/myTrades",
            docs_url: SPOT_ACCOUNT_DOCS,
            parser_functions: &[
                "nautilus_binance::spot::http::parse::decode_account_trades",
                "nautilus_binance::common::parse::parse_fill_report_sbe",
            ],
            payload_relpath: &format!(
                "private_read/account_trades_{}_limit{}.sbe",
                sanitized(&config.symbol),
                config.trades_limit
            ),
            symbol: Some(config.symbol.as_str()),
            interval: None,
            order_id: None,
            client_order_id: None,
        },
        &account_trades,
        fixtures,
    )?;

    let open_orders_params = OpenOrdersParams::for_symbol(&config.symbol);
    let open_orders = client
        .get_signed("openOrders", Some(&open_orders_params))
        .await?;
    record_fixture(
        output_root,
        &private_dir,
        &FixtureCapture {
            name: "open_orders_response",
            category: "private_read",
            auth: FixtureAuth::Signed,
            method: "GET",
            endpoint: "/api/v3/openOrders",
            docs_url: SPOT_ACCOUNT_DOCS,
            parser_functions: &["nautilus_binance::spot::http::parse::decode_orders"],
            payload_relpath: &format!("private_read/open_orders_{}.sbe", sanitized(&config.symbol)),
            symbol: Some(config.symbol.as_str()),
            interval: None,
            order_id: None,
            client_order_id: None,
        },
        &open_orders,
        fixtures,
    )?;

    let all_orders_params = AllOrdersParams::new(&config.symbol).with_limit(config.trades_limit);
    let all_orders = client
        .get_signed("allOrders", Some(&all_orders_params))
        .await?;
    record_fixture(
        output_root,
        &private_dir,
        &FixtureCapture {
            name: "all_orders_response",
            category: "private_read",
            auth: FixtureAuth::Signed,
            method: "GET",
            endpoint: "/api/v3/allOrders",
            docs_url: SPOT_ACCOUNT_DOCS,
            parser_functions: &["nautilus_binance::spot::http::parse::decode_orders"],
            payload_relpath: &format!(
                "private_read/all_orders_{}_limit{}.sbe",
                sanitized(&config.symbol),
                config.trades_limit
            ),
            symbol: Some(config.symbol.as_str()),
            interval: None,
            order_id: None,
            client_order_id: None,
        },
        &all_orders,
        fixtures,
    )?;

    Ok(())
}

async fn capture_order_flow_fixtures(
    config: &CaptureConfig,
    client: &BinanceRawSpotHttpClient,
    output_root: &Path,
    fixtures: &mut Vec<FixtureRecord>,
) -> anyhow::Result<()> {
    let order_flow_dir = output_root.join("order_flow");
    let order_quantity = config
        .order_quantity
        .as_ref()
        .expect("validated order_quantity");
    let order_price = config.order_price.as_ref().expect("validated order_price");

    let client_order_id = format!("ntfx{}a", Utc::now().timestamp_millis().unsigned_abs());
    let new_order_params = NewOrderParams::limit(
        &config.symbol,
        BinanceSide::Buy,
        order_quantity,
        order_price,
    )
    .with_client_order_id(client_order_id.clone())
    .with_time_in_force(BinanceTimeInForce::Gtc);

    let new_order = client.post_signed("order", Some(&new_order_params)).await?;
    record_fixture(
        output_root,
        &order_flow_dir,
        &FixtureCapture {
            name: "new_order_full_response_cancel_order",
            category: "order_flow",
            auth: FixtureAuth::Signed,
            method: "POST",
            endpoint: "/api/v3/order",
            docs_url: SPOT_TRADING_DOCS,
            parser_functions: &[
                "nautilus_binance::spot::http::parse::decode_new_order_full",
                "nautilus_binance::common::parse::parse_new_order_response_sbe",
            ],
            payload_relpath: "order_flow/new_order_full_cancel_order.sbe",
            symbol: Some(config.symbol.as_str()),
            interval: None,
            order_id: None,
            client_order_id: Some(client_order_id.as_str()),
        },
        &new_order,
        fixtures,
    )?;
    let new_order_response = decode_new_order_full(&new_order)?;
    let order_id = new_order_response.order_id;

    let query_order_params = QueryOrderParams::by_order_id(&config.symbol, order_id);
    let query_order = client
        .get_signed("order", Some(&query_order_params))
        .await?;
    record_fixture(
        output_root,
        &order_flow_dir,
        &FixtureCapture {
            name: "order_response_after_new_order",
            category: "order_flow",
            auth: FixtureAuth::Signed,
            method: "GET",
            endpoint: "/api/v3/order",
            docs_url: SPOT_TRADING_DOCS,
            parser_functions: &[
                "nautilus_binance::spot::http::parse::decode_order",
                "nautilus_binance::common::parse::parse_order_status_report_sbe",
            ],
            payload_relpath: "order_flow/order_after_new.sbe",
            symbol: Some(config.symbol.as_str()),
            interval: None,
            order_id: Some(order_id),
            client_order_id: Some(client_order_id.as_str()),
        },
        &query_order,
        fixtures,
    )?;

    let open_orders_params = OpenOrdersParams::for_symbol(&config.symbol);
    let open_orders = client
        .get_signed("openOrders", Some(&open_orders_params))
        .await?;
    record_fixture(
        output_root,
        &order_flow_dir,
        &FixtureCapture {
            name: "open_orders_response_after_new_order",
            category: "order_flow",
            auth: FixtureAuth::Signed,
            method: "GET",
            endpoint: "/api/v3/openOrders",
            docs_url: SPOT_ACCOUNT_DOCS,
            parser_functions: &["nautilus_binance::spot::http::parse::decode_orders"],
            payload_relpath: "order_flow/open_orders_after_new.sbe",
            symbol: Some(config.symbol.as_str()),
            interval: None,
            order_id: Some(order_id),
            client_order_id: Some(client_order_id.as_str()),
        },
        &open_orders,
        fixtures,
    )?;

    let cancel_order_params = CancelOrderParams::by_order_id(&config.symbol, order_id);
    let cancel_order = client
        .delete_signed("order", Some(&cancel_order_params))
        .await?;
    record_fixture(
        output_root,
        &order_flow_dir,
        &FixtureCapture {
            name: "cancel_order_response",
            category: "order_flow",
            auth: FixtureAuth::Signed,
            method: "DELETE",
            endpoint: "/api/v3/order",
            docs_url: SPOT_TRADING_DOCS,
            parser_functions: &["nautilus_binance::spot::http::parse::decode_cancel_order"],
            payload_relpath: "order_flow/cancel_order_response.sbe",
            symbol: Some(config.symbol.as_str()),
            interval: None,
            order_id: Some(order_id),
            client_order_id: Some(client_order_id.as_str()),
        },
        &cancel_order,
        fixtures,
    )?;

    let cancel_all_client_order_id =
        format!("ntfx{}b", Utc::now().timestamp_millis().unsigned_abs());
    let cancel_all_order_params = NewOrderParams::limit(
        &config.symbol,
        BinanceSide::Buy,
        order_quantity,
        order_price,
    )
    .with_client_order_id(cancel_all_client_order_id.clone())
    .with_time_in_force(BinanceTimeInForce::Gtc);
    let new_order_cancel_all = client
        .post_signed("order", Some(&cancel_all_order_params))
        .await?;
    record_fixture(
        output_root,
        &order_flow_dir,
        &FixtureCapture {
            name: "new_order_full_response_cancel_all",
            category: "order_flow",
            auth: FixtureAuth::Signed,
            method: "POST",
            endpoint: "/api/v3/order",
            docs_url: SPOT_TRADING_DOCS,
            parser_functions: &["nautilus_binance::spot::http::parse::decode_new_order_full"],
            payload_relpath: "order_flow/new_order_full_cancel_all.sbe",
            symbol: Some(config.symbol.as_str()),
            interval: None,
            order_id: None,
            client_order_id: Some(cancel_all_client_order_id.as_str()),
        },
        &new_order_cancel_all,
        fixtures,
    )?;
    let cancel_all_response = decode_new_order_full(&new_order_cancel_all)?;

    let cancel_open_orders_params = CancelOpenOrdersParams::new(config.symbol.clone());
    let cancel_open_orders = client
        .delete_signed("openOrders", Some(&cancel_open_orders_params))
        .await?;
    record_fixture(
        output_root,
        &order_flow_dir,
        &FixtureCapture {
            name: "cancel_open_orders_response",
            category: "order_flow",
            auth: FixtureAuth::Signed,
            method: "DELETE",
            endpoint: "/api/v3/openOrders",
            docs_url: SPOT_TRADING_DOCS,
            parser_functions: &["nautilus_binance::spot::http::parse::decode_cancel_open_orders"],
            payload_relpath: "order_flow/cancel_open_orders_response.sbe",
            symbol: Some(config.symbol.as_str()),
            interval: None,
            order_id: Some(cancel_all_response.order_id),
            client_order_id: Some(cancel_all_client_order_id.as_str()),
        },
        &cancel_open_orders,
        fixtures,
    )?;

    let all_orders_params = AllOrdersParams::new(&config.symbol).with_limit(config.trades_limit);
    let all_orders = client
        .get_signed("allOrders", Some(&all_orders_params))
        .await?;
    record_fixture(
        output_root,
        &order_flow_dir,
        &FixtureCapture {
            name: "all_orders_response_after_order_flow",
            category: "order_flow",
            auth: FixtureAuth::Signed,
            method: "GET",
            endpoint: "/api/v3/allOrders",
            docs_url: SPOT_ACCOUNT_DOCS,
            parser_functions: &["nautilus_binance::spot::http::parse::decode_orders"],
            payload_relpath: "order_flow/all_orders_after_order_flow.sbe",
            symbol: Some(config.symbol.as_str()),
            interval: None,
            order_id: None,
            client_order_id: None,
        },
        &all_orders,
        fixtures,
    )?;

    Ok(())
}

struct FixtureCapture<'a> {
    name: &'a str,
    category: &'a str,
    auth: FixtureAuth,
    method: &'a str,
    endpoint: &'a str,
    docs_url: &'a str,
    parser_functions: &'a [&'a str],
    payload_relpath: &'a str,
    symbol: Option<&'a str>,
    interval: Option<&'a str>,
    order_id: Option<i64>,
    client_order_id: Option<&'a str>,
}

fn record_fixture(
    output_root: &Path,
    category_dir: &Path,
    capture: &FixtureCapture<'_>,
    payload: &[u8],
    fixtures: &mut Vec<FixtureRecord>,
) -> anyhow::Result<()> {
    fs::create_dir_all(category_dir)?;

    let payload_path = output_root.join(capture.payload_relpath);
    fs::write(&payload_path, payload)?;

    let record = FixtureRecord {
        name: capture.name.to_string(),
        category: capture.category.to_string(),
        auth: capture.auth,
        method: capture.method.to_string(),
        endpoint: capture.endpoint.to_string(),
        docs_url: capture.docs_url.to_string(),
        parser_functions: capture
            .parser_functions
            .iter()
            .map(|parser| (*parser).to_string())
            .collect(),
        payload_path: capture.payload_relpath.to_string(),
        metadata_path: metadata_relpath(capture.payload_relpath),
        bytes: payload.len(),
        symbol: capture.symbol.map(ToOwned::to_owned),
        interval: capture.interval.map(ToOwned::to_owned),
        order_id: capture.order_id,
        client_order_id: capture.client_order_id.map(ToOwned::to_owned),
        sbe_header: decode_sbe_header(payload),
    };

    let metadata = FixtureMetadata {
        fixture: record.clone(),
        captured_at: Utc::now().to_rfc3339(),
    };
    write_json(&output_root.join(&record.metadata_path), &metadata)?;

    fixtures.push(record);
    Ok(())
}

fn metadata_relpath(payload_relpath: &str) -> String {
    let path = Path::new(payload_relpath);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("fixture");
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    parent
        .join(format!("{stem}.metadata.json"))
        .display()
        .to_string()
}

fn decode_sbe_header(payload: &[u8]) -> Option<SbeHeaderRecord> {
    if payload.len() < 8 {
        return None;
    }

    Some(SbeHeaderRecord {
        block_length: u16::from_le_bytes([payload[0], payload[1]]),
        template_id: u16::from_le_bytes([payload[2], payload[3]]),
        schema_id: u16::from_le_bytes([payload[4], payload[5]]),
        version: u16::from_le_bytes([payload[6], payload[7]]),
    })
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let data = serde_json::to_vec_pretty(value)?;
    fs::write(path, data)?;
    Ok(())
}

fn parse_args<I>(args: I) -> anyhow::Result<CaptureConfig>
where
    I: IntoIterator<Item = String>,
{
    let mut config = CaptureConfig {
        environment: BinanceEnvironment::Mainnet,
        output_dir: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join("spot")
            .join("http_sbe"),
        symbol: DEFAULT_SYMBOL.to_string(),
        interval: DEFAULT_INTERVAL.to_string(),
        depth_limit: DEFAULT_DEPTH_LIMIT,
        trades_limit: DEFAULT_TRADES_LIMIT,
        klines_limit: DEFAULT_KLINES_LIMIT,
        include_private: false,
        include_order_flow: false,
        order_quantity: None,
        order_price: None,
    };

    let args: Vec<String> = args.into_iter().collect();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            "--environment" | "--env" => {
                index += 1;
                config.environment = parse_environment(&value_at(&args, index, "--environment")?)?;
            }
            "--output-dir" => {
                index += 1;
                config.output_dir = PathBuf::from(value_at(&args, index, "--output-dir")?);
            }
            "--symbol" => {
                index += 1;
                config.symbol = value_at(&args, index, "--symbol")?;
            }
            "--interval" => {
                index += 1;
                config.interval = value_at(&args, index, "--interval")?;
            }
            "--depth-limit" => {
                index += 1;
                config.depth_limit = value_at(&args, index, "--depth-limit")?.parse::<u32>()?;
            }
            "--trades-limit" => {
                index += 1;
                config.trades_limit = value_at(&args, index, "--trades-limit")?.parse::<u32>()?;
            }
            "--klines-limit" => {
                index += 1;
                config.klines_limit = value_at(&args, index, "--klines-limit")?.parse::<u32>()?;
            }
            "--include-private" => {
                config.include_private = true;
            }
            "--include-order-flow" => {
                config.include_order_flow = true;
            }
            "--order-quantity" => {
                index += 1;
                config.order_quantity = Some(value_at(&args, index, "--order-quantity")?);
            }
            "--order-price" => {
                index += 1;
                config.order_price = Some(value_at(&args, index, "--order-price")?);
            }
            other => anyhow::bail!("Unknown argument: {other}"),
        }
        index += 1;
    }

    validate_config(&config)?;
    Ok(config)
}

fn validate_config(config: &CaptureConfig) -> anyhow::Result<()> {
    if config.symbol.trim().is_empty() {
        anyhow::bail!("--symbol must not be empty");
    }

    if config.interval.trim().is_empty() {
        anyhow::bail!("--interval must not be empty");
    }

    if config.include_order_flow {
        if matches!(config.environment, BinanceEnvironment::Mainnet) {
            anyhow::bail!("--include-order-flow is only allowed on testnet or demo");
        }

        if config.order_quantity.is_none() || config.order_price.is_none() {
            anyhow::bail!("--include-order-flow requires both --order-quantity and --order-price");
        }
    }

    Ok(())
}

fn parse_environment(value: &str) -> anyhow::Result<BinanceEnvironment> {
    match value.to_ascii_lowercase().as_str() {
        "mainnet" | "live" => Ok(BinanceEnvironment::Mainnet),
        "testnet" | "test" => Ok(BinanceEnvironment::Testnet),
        "demo" => Ok(BinanceEnvironment::Demo),
        _ => anyhow::bail!("Unsupported environment: {value}"),
    }
}

fn value_at(args: &[String], index: usize, flag: &str) -> anyhow::Result<String> {
    args.get(index)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("{flag} requires a value"))
}

fn environment_name(environment: BinanceEnvironment) -> &'static str {
    match environment {
        BinanceEnvironment::Mainnet => "mainnet",
        BinanceEnvironment::Testnet => "testnet",
        BinanceEnvironment::Demo => "demo",
    }
}

fn sanitized(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn print_usage() {
    println!(
        "Usage: cargo run --bin binance-spot-http-capture-fixtures --package nautilus-binance -- [OPTIONS]\n\
         \n\
         Options:\n\
           --environment, --env <mainnet|testnet|demo>\n\
           --output-dir <PATH>\n\
           --symbol <SYMBOL>\n\
           --interval <INTERVAL>\n\
           --depth-limit <N>\n\
           --trades-limit <N>\n\
           --klines-limit <N>\n\
           --include-private\n\
           --include-order-flow\n\
           --order-quantity <QTY>\n\
           --order-price <PRICE>\n\
           --help, -h"
    );
}
