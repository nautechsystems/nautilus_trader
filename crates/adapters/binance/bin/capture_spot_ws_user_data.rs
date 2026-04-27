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

//! Capture raw SBE binary fixtures from the Binance Spot WebSocket user data stream.
//!
//! Connects to the Spot WS API with `responseFormat=sbe`, authenticates via
//! `session.logon`, subscribes to the user data stream, and optionally places
//! and cancels a limit order to trigger execution reports and account position
//! updates. Saves raw binary frames to `test_data/spot/user_data_sbe/`.
//!
//! Each capture run writes:
//!
//! - Raw SBE payload bytes in a `.sbe` file.
//! - Per-fixture metadata in a `.metadata.json` file.
//! - An aggregate `manifest.json` for the full capture run.
//!
//! # Usage
//!
//! Listen-only capture (waits for external order activity):
//! ```bash
//! cargo run --bin binance-spot-ws-user-data-capture --package nautilus-binance -- \
//!   --environment testnet
//! ```
//!
//! Order-flow capture (places and cancels a limit order to trigger events):
//! ```bash
//! cargo run --bin binance-spot-ws-user-data-capture --package nautilus-binance -- \
//!   --environment testnet --include-order-flow --symbol BTCUSDT \
//!   --order-quantity 0.001 --order-price 10000
//! ```
//!
//! # Environment Variables
//!
//! For testnet: `BINANCE_TESTNET_API_KEY`, `BINANCE_TESTNET_API_SECRET`
//! For demo: `BINANCE_DEMO_API_KEY`, `BINANCE_DEMO_API_SECRET`
//! For mainnet: `BINANCE_API_KEY`, `BINANCE_API_SECRET`

use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use chrono::Utc;
use nautilus_binance::common::{
    consts,
    credential::{SigningCredential, resolve_credentials},
    enums::{BinanceEnvironment, BinanceProductType},
};
use nautilus_network::websocket::{
    PingHandler, TransportBackend, WebSocketClient, WebSocketConfig, channel_message_handler,
};
use serde::Serialize;
use tokio_tungstenite::tungstenite::Message;

const DEFAULT_SYMBOL: &str = "BTCUSDT";
const DEFAULT_TIMEOUT_SECS: u64 = 60;

const SPOT_USER_DATA_DOCS: &str =
    "https://developers.binance.com/docs/binance-spot-api-docs/user-data-stream";

fn ws_api_url(environment: BinanceEnvironment) -> &'static str {
    match environment {
        BinanceEnvironment::Mainnet => consts::BINANCE_SPOT_SBE_WS_API_URL,
        BinanceEnvironment::Testnet => consts::BINANCE_SPOT_SBE_WS_API_TESTNET_URL,
        BinanceEnvironment::Demo => consts::BINANCE_SPOT_SBE_WS_API_DEMO_URL,
    }
}

fn environment_name(env: BinanceEnvironment) -> &'static str {
    match env {
        BinanceEnvironment::Mainnet => "mainnet",
        BinanceEnvironment::Testnet => "testnet",
        BinanceEnvironment::Demo => "demo",
    }
}

fn template_name(template_id: u16) -> &'static str {
    match template_id {
        50 => "web_socket_response",
        601 => "balance_update_event",
        603 => "execution_report_event",
        606 => "list_status_event",
        607 => "outbound_account_position_event",
        _ => "unknown",
    }
}

fn parser_functions(template_id: u16) -> &'static [&'static str] {
    match template_id {
        601 => &["nautilus_binance::spot::websocket::trading::decode_sbe::decode_balance_update"],
        603 => &["nautilus_binance::spot::websocket::trading::decode_sbe::decode_execution_report"],
        607 => &["nautilus_binance::spot::websocket::trading::decode_sbe::decode_account_position"],
        _ => &[],
    }
}

struct CaptureConfig {
    environment: BinanceEnvironment,
    output_dir: PathBuf,
    symbol: String,
    include_order_flow: bool,
    order_quantity: Option<String>,
    order_price: Option<String>,
    timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize)]
struct FixtureManifest {
    command: String,
    captured_at: String,
    environment: String,
    symbol: String,
    output_dir: String,
    fixtures: Vec<FixtureRecord>,
}

#[derive(Debug, Clone, Serialize)]
struct FixtureRecord {
    name: String,
    category: String,
    docs_url: String,
    parser_functions: Vec<String>,
    payload_path: String,
    metadata_path: String,
    bytes: usize,
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
    let output_root = config.output_dir.join(environment_name(config.environment));
    fs::create_dir_all(&output_root)?;

    let (api_key, api_secret) =
        resolve_credentials(None, None, config.environment, BinanceProductType::Spot)?;
    let credential = SigningCredential::new(api_key.clone(), api_secret);

    let url = ws_api_url(config.environment).to_string();
    println!("Connecting to {url}");

    let (raw_handler, mut raw_rx) = channel_message_handler();
    let ping_handler: PingHandler = Arc::new(move |_| {});

    let headers = vec![("X-MBX-APIKEY".to_string(), api_key)];

    let ws_config = WebSocketConfig {
        url,
        headers,
        heartbeat: Some(20),
        heartbeat_msg: None,
        reconnect_timeout_ms: None,
        reconnect_delay_initial_ms: None,
        reconnect_delay_max_ms: None,
        reconnect_backoff_factor: None,
        reconnect_jitter_ms: None,
        reconnect_max_attempts: Some(0),
        idle_timeout_ms: None,
        backend: TransportBackend::Tungstenite,
        proxy_url: None,
    };

    let client = WebSocketClient::connect(
        ws_config,
        Some(raw_handler),
        Some(ping_handler),
        None,
        vec![],
        None,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Connection failed: {e}"))?;

    println!("Connected");

    // Session logon
    let logon_request = build_signed_request("ws-logon", "session.logon", &credential)?;
    client
        .send_text(logon_request, None)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to send session.logon: {e}"))?;
    let mut early_frames =
        wait_for_response(&mut raw_rx, "session.logon", Duration::from_secs(10)).await?;
    println!("Authenticated");

    // Subscribe to user data stream
    let subscribe_json = serde_json::json!({
        "id": "ws-subscribe",
        "method": "userDataStream.subscribe",
        "params": {}
    });
    client
        .send_text(serde_json::to_string(&subscribe_json)?, None)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to send userDataStream.subscribe: {e}"))?;
    early_frames.extend(
        wait_for_response(
            &mut raw_rx,
            "userDataStream.subscribe",
            Duration::from_secs(10),
        )
        .await?,
    );
    println!("Subscribed to user data stream");

    // Optionally place and cancel an order to trigger user data events
    if config.include_order_flow {
        let qty = config
            .order_quantity
            .as_ref()
            .expect("validated order_quantity");
        let price = config.order_price.as_ref().expect("validated order_price");

        let client_order_id = format!(
            "capture-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );

        println!(
            "Placing limit order: {} {} @ {} (clientOrderId={client_order_id})",
            config.symbol, qty, price
        );

        let place_params = serde_json::json!({
            "symbol": config.symbol,
            "side": "BUY",
            "type": "LIMIT",
            "timeInForce": "GTC",
            "quantity": qty,
            "price": price,
            "newClientOrderId": client_order_id,
        });
        let place_full =
            build_signed_request_with_params("ws-place", "order.place", place_params, &credential)?;
        client
            .send_text(place_full, None)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send order.place: {e}"))?;

        // Drain frames while waiting for order events
        drain_frames(&mut raw_rx, &mut early_frames, Duration::from_secs(3)).await;

        // Cancel the order
        println!("Canceling order for {}", config.symbol);
        let cancel_params = serde_json::json!({
            "symbol": config.symbol,
            "origClientOrderId": client_order_id,
        });
        let cancel_request = build_signed_request_with_params(
            "ws-cancel",
            "order.cancel",
            cancel_params,
            &credential,
        )?;
        client
            .send_text(cancel_request, None)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send order.cancel: {e}"))?;

        // Drain frames while waiting for cancel events
        drain_frames(&mut raw_rx, &mut early_frames, Duration::from_secs(3)).await;
    }

    // Capture raw binary frames
    println!(
        "Capturing user data SBE frames for {} seconds (Ctrl+C to stop early)...",
        config.timeout_secs
    );

    let mut fixtures = Vec::new();
    let mut counts: std::collections::HashMap<u16, usize> = std::collections::HashMap::new();

    // Process any binary frames received during handshake and order flow
    if !early_frames.is_empty() {
        println!("Processing {} early binary frames...", early_frames.len());
    }

    for data in early_frames {
        capture_binary_frame(&data, &output_root, &mut counts, &mut fixtures)?;
    }

    let deadline = tokio::time::Instant::now() + Duration::from_secs(config.timeout_secs);

    loop {
        tokio::select! {
            msg = raw_rx.recv() => {
                match msg {
                    Some(Message::Binary(data)) => {
                        capture_binary_frame(&data, &output_root, &mut counts, &mut fixtures)?;
                    }
                    Some(Message::Text(text)) => {
                        println!("  Text frame: {text}");
                    }
                    Some(_) => {}
                    None => {
                        println!("Channel closed");
                        break;
                    }
                }
            }
            () = tokio::time::sleep_until(deadline) => {
                println!("Capture timeout reached");
                break;
            }
            _ = tokio::signal::ctrl_c() => {
                println!("Received Ctrl+C");
                break;
            }
        }
    }

    // Write manifest
    let manifest = FixtureManifest {
        command: env::args().collect::<Vec<_>>().join(" "),
        captured_at: Utc::now().to_rfc3339(),
        environment: environment_name(config.environment).to_string(),
        symbol: config.symbol,
        output_dir: output_root.display().to_string(),
        fixtures,
    };
    let manifest_path = output_root.join("manifest.json");
    write_json(&manifest_path, &manifest)?;

    println!("\nCapture summary:");
    for (template_id, count) in &counts {
        println!(
            "  {} (template {}): {}",
            template_name(*template_id),
            template_id,
            count
        );
    }
    println!("  Output directory: {}", output_root.display());
    println!("  Manifest: {}", manifest_path.display());

    Ok(())
}

fn capture_binary_frame(
    data: &[u8],
    output_root: &Path,
    counts: &mut std::collections::HashMap<u16, usize>,
    fixtures: &mut Vec<FixtureRecord>,
) -> anyhow::Result<()> {
    if data.len() < 8 {
        return Ok(());
    }

    let template_id = u16::from_le_bytes([data[2], data[3]]);
    let name = template_name(template_id);
    let count = counts.entry(template_id).or_insert(0);
    *count += 1;

    let payload_relpath = format!("{name}_{}.sbe", *count);

    println!(
        "  Captured {name} (template={template_id}, {} bytes): {payload_relpath}",
        data.len()
    );

    record_fixture(
        output_root,
        &FixtureCapture {
            name: &format!("{name}_{}", *count),
            category: "user_data",
            docs_url: SPOT_USER_DATA_DOCS,
            parser_functions: parser_functions(template_id),
            payload_relpath: &payload_relpath,
        },
        data,
        fixtures,
    )
}

struct FixtureCapture<'a> {
    name: &'a str,
    category: &'a str,
    docs_url: &'a str,
    parser_functions: &'a [&'a str],
    payload_relpath: &'a str,
}

fn record_fixture(
    output_root: &Path,
    capture: &FixtureCapture<'_>,
    payload: &[u8],
    fixtures: &mut Vec<FixtureRecord>,
) -> anyhow::Result<()> {
    let payload_path = output_root.join(capture.payload_relpath);
    fs::write(&payload_path, payload)?;

    let record = FixtureRecord {
        name: capture.name.to_string(),
        category: capture.category.to_string(),
        docs_url: capture.docs_url.to_string(),
        parser_functions: capture
            .parser_functions
            .iter()
            .map(|p| (*p).to_string())
            .collect(),
        payload_path: capture.payload_relpath.to_string(),
        metadata_path: metadata_relpath(capture.payload_relpath),
        bytes: payload.len(),
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
        .and_then(|s| s.to_str())
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

fn build_signed_request(
    id: &str,
    method: &str,
    credential: &SigningCredential,
) -> anyhow::Result<String> {
    build_signed_request_with_params(id, method, serde_json::json!({}), credential)
}

fn build_signed_request_with_params(
    id: &str,
    method: &str,
    mut params: serde_json::Value,
    credential: &SigningCredential,
) -> anyhow::Result<String> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as i64;

    if let Some(obj) = params.as_object_mut() {
        obj.insert("timestamp".to_string(), serde_json::json!(timestamp));
        obj.insert(
            "apiKey".to_string(),
            serde_json::json!(credential.api_key()),
        );
    }

    let query_string = serde_urlencoded::to_string(&params)?;
    let signature = credential.sign(&query_string);

    if let Some(obj) = params.as_object_mut() {
        obj.insert("signature".to_string(), serde_json::json!(signature));
    }

    let request = serde_json::json!({
        "id": id,
        "method": method,
        "params": params,
    });

    Ok(serde_json::to_string(&request)?)
}

/// Drains binary frames from the channel for a duration, collecting them.
async fn drain_frames(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Message>,
    collected: &mut Vec<Vec<u8>>,
    duration: Duration,
) {
    let deadline = tokio::time::Instant::now() + duration;

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(Message::Binary(data)) => {
                        collected.push(data.to_vec());
                    }
                    Some(Message::Text(text)) => {
                        println!("  Text frame during drain: {text}");
                    }
                    Some(_) => {}
                    None => break,
                }
            }
            () = tokio::time::sleep_until(deadline) => {
                break;
            }
        }
    }
}

/// Waits for a JSON text response matching the given context.
///
/// Returns any binary frames received while waiting so they are not lost.
async fn wait_for_response(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Message>,
    context: &str,
    timeout: Duration,
) -> anyhow::Result<Vec<Vec<u8>>> {
    let mut stray_binary = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(Message::Text(text)) => {
                        let json: serde_json::Value = match serde_json::from_str(&text) {
                            Ok(v) => v,
                            Err(_) => {
                                log::debug!("Non-JSON text frame: {text}");
                                continue;
                            }
                        };

                        if let Some(error) = json.get("error") {
                            anyhow::bail!("{context} error: {error}");
                        }

                        if json.get("id").is_some() || json.get("result").is_some() {
                            return Ok(stray_binary);
                        }
                    }
                    Some(Message::Binary(data)) => {
                        // Could be an SBE response to our request (template 50)
                        // or a user data event that arrived concurrently.
                        if data.len() >= 4 {
                            let template_id = u16::from_le_bytes([data[2], data[3]]);

                            if template_id == 50 {
                                // WebSocketResponse envelope: this is our ack
                                return Ok(stray_binary);
                            }
                        }

                        // User data event received during handshake: preserve it
                        stray_binary.push(data.to_vec());
                    }
                    Some(_) => {}
                    None => anyhow::bail!("Channel closed while waiting for {context}"),
                }
            }
            () = tokio::time::sleep_until(deadline) => {
                anyhow::bail!("Timeout waiting for {context}");
            }
        }
    }
}

fn parse_args<I>(args: I) -> anyhow::Result<CaptureConfig>
where
    I: IntoIterator<Item = String>,
{
    let mut config = CaptureConfig {
        environment: BinanceEnvironment::Testnet,
        output_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join("spot")
            .join("user_data_sbe"),
        symbol: DEFAULT_SYMBOL.to_string(),
        include_order_flow: false,
        order_quantity: None,
        order_price: None,
        timeout_secs: DEFAULT_TIMEOUT_SECS,
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
            "--timeout" => {
                index += 1;
                config.timeout_secs = value_at(&args, index, "--timeout")?.parse()?;
            }
            other => anyhow::bail!("Unknown argument: {other}"),
        }
        index += 1;
    }

    if config.include_order_flow
        && (config.order_quantity.is_none() || config.order_price.is_none())
    {
        anyhow::bail!("--include-order-flow requires both --order-quantity and --order-price");
    }

    Ok(config)
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

fn print_usage() {
    println!(
        "Usage: cargo run --bin binance-spot-ws-user-data-capture --package nautilus-binance -- [OPTIONS]\n\
         \n\
         Options:\n\
           --environment, --env <testnet|demo|mainnet>  (default: testnet)\n\
           --output-dir <PATH>\n\
           --symbol <SYMBOL>                            (default: BTCUSDT)\n\
           --include-order-flow                         Place and cancel a limit order\n\
           --order-quantity <QTY>                        Required with --include-order-flow\n\
           --order-price <PRICE>                         Required with --include-order-flow\n\
           --timeout <SECONDS>                           Capture duration (default: 60)\n\
           --help, -h"
    );
}
