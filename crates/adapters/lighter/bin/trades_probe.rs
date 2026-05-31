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

//! Direct probe for the Lighter `/api/v1/trades` endpoint.
//!
//! Mints an auth token from local credentials and replays the same
//! [`LighterTradesQuery`] that `generate_fill_reports` builds during
//! mass-status reconciliation, then prints the response or the full error
//! chain. Used to diagnose the `failed to fetch Lighter fills` failure
//! without spinning up the whole LiveNode.
//!
//! Run with `cargo run -p nautilus-lighter --bin lighter-trades-probe --features examples`.
//!
//! Read-only: no orders are placed.

use std::error::Error as _;

use nautilus_common::logging::{init_logging, logger::LoggerConfig};
use nautilus_core::{UUID4, string::secret::mask_api_key};
use nautilus_lighter::{
    common::{credential::Credential, enums::LighterEnvironment},
    http::{
        client::{LIGHTER_REST_PAGE_SIZE, LighterHttpClient, LighterRawHttpClient},
        query::{
            LighterRecentTradesQuery, LighterSortDirection, LighterTradeSortBy, LighterTradesQuery,
        },
    },
    signing::auth_token::build_auth_token_for,
};
use nautilus_model::identifiers::TraderId;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Hold the guard for the whole of `main` so trace logging stays alive.
    let _log_guard = init_logging(
        TraderId::from("PROBE-001"),
        UUID4::new(),
        LoggerConfig {
            // INFO, not TRACE: nautilus_network's HTTP client traces
            // request URLs at TRACE level, which would expose the auth
            // bearer token. Explicit URL prints in this probe are
            // redacted via `redact_auth`.
            stdout_level: log::LevelFilter::Info,
            ..Default::default()
        },
        Default::default(),
    )?;

    let environment = LighterEnvironment::Mainnet;
    let credential = Credential::resolve(None, None, None, environment)?
        .ok_or_else(|| anyhow::anyhow!("no credentials in env"))?;

    println!(
        "Resolved credential: account_index={}, api_key_index={}",
        credential.account_index(),
        credential.api_key_index(),
    );

    let raw = LighterRawHttpClient::new(environment, None, 30, None)?;
    let client = LighterHttpClient::from_raw_with_registry(raw, Default::default());

    let auth = build_auth_token_for(&credential)?;
    println!("Auth token minted ({} chars)", auth.len());

    // Mirror production; probe `L` keeps the explicit negative case.
    let query = LighterTradesQuery {
        authorization: None,
        auth: Some(auth.clone()),
        market_id: None,
        account_index: Some(credential.account_index()),
        order_index: None,
        sort_by: LighterTradeSortBy::Timestamp,
        sort_dir: Some(LighterSortDirection::Desc),
        cursor: None,
        from_timestamp: None,
        ask_filter: None,
        role: None,
        trade_type: None,
        limit: LIGHTER_REST_PAGE_SIZE,
        aggregate: None,
    };

    println!("Probe #1: mirror of generate_fill_reports (no market_id, no from_timestamp)");

    match client.get_trades(&query).await {
        Ok(response) => {
            println!(
                "  OK: code={}, trades={}, next_cursor={:?}",
                response.code,
                response.trades.len(),
                response.next_cursor,
            );
        }
        Err(e) => {
            println!("  ERR (chain follows):");
            println!("    {e:#}");
        }
    }

    println!();
    println!("Probe #2: add market_id=0");
    let query_with_market = LighterTradesQuery {
        market_id: Some(0),
        ..query.clone()
    };

    match client.get_trades(&query_with_market).await {
        Ok(response) => {
            println!(
                "  OK: code={}, trades={}, next_cursor={:?}",
                response.code,
                response.trades.len(),
                response.next_cursor,
            );
        }
        Err(e) => {
            println!("  ERR (chain follows):");
            println!("    {e:#}");
        }
    }

    println!();
    println!("Probe #3: drop account_index, market_id=0 only");
    let market_only_query = LighterTradesQuery {
        account_index: None,
        market_id: Some(0),
        ..query.clone()
    };

    match client.get_trades(&market_only_query).await {
        Ok(response) => {
            println!(
                "  OK: code={}, trades={}, next_cursor={:?}",
                response.code,
                response.trades.len(),
                response.next_cursor,
            );
        }
        Err(e) => {
            println!("  ERR (chain follows):");
            println!("    {e:#}");
        }
    }

    println!();
    println!("Probe #4: add a recent from_timestamp (last 24h)");
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as i64;
    let one_day_ms = 24 * 60 * 60 * 1_000_i64;
    let timestamped_query = LighterTradesQuery {
        from_timestamp: Some(now_ms - one_day_ms),
        ..query.clone()
    };

    match client.get_trades(&timestamped_query).await {
        Ok(response) => {
            println!(
                "  OK: code={}, trades={}, next_cursor={:?}",
                response.code,
                response.trades.len(),
                response.next_cursor,
            );
        }
        Err(e) => {
            println!("  ERR (chain follows):");
            println!("    {e:#}");
        }
    }

    println!();
    println!("Probe #5: market_id=0, sort_by=trade_id (default), no auth, no account_index");
    let public_query = LighterTradesQuery {
        auth: None,
        market_id: Some(0),
        account_index: None,
        sort_by: LighterTradeSortBy::TradeId,
        sort_dir: None,
        ..query.clone()
    };

    match client.get_trades(&public_query).await {
        Ok(response) => {
            println!(
                "  OK: code={}, trades={}, next_cursor={:?}",
                response.code,
                response.trades.len(),
                response.next_cursor,
            );
        }
        Err(e) => {
            println!("  ERR (chain follows):");
            println!("    {e:#}");
        }
    }

    println!();
    println!("Probe #6: market_id=0, sort_by=timestamp, no auth, no account_index");
    let market_timestamp_query = LighterTradesQuery {
        auth: None,
        market_id: Some(0),
        account_index: None,
        sort_by: LighterTradeSortBy::Timestamp,
        sort_dir: Some(LighterSortDirection::Desc),
        ..query.clone()
    };

    match client.get_trades(&market_timestamp_query).await {
        Ok(response) => {
            println!(
                "  OK: code={}, trades={}, next_cursor={:?}",
                response.code,
                response.trades.len(),
                response.next_cursor,
            );
        }
        Err(e) => {
            println!("  ERR (chain follows):");
            println!("    {e:#}");
        }
    }

    println!();
    println!("Probe #7: account_index + market_id + sort_by=trade_id + auth");
    let scoped_default_sort = LighterTradesQuery {
        sort_by: LighterTradeSortBy::TradeId,
        market_id: Some(0),
        ..query.clone()
    };

    match client.get_trades(&scoped_default_sort).await {
        Ok(response) => {
            println!(
                "  OK: code={}, trades={}, next_cursor={:?}",
                response.code,
                response.trades.len(),
                response.next_cursor,
            );
        }
        Err(e) => {
            println!("  ERR (chain follows):");
            println!("    {e:#}");
        }
    }

    println!();
    println!("Probe #8: account_index only, sort_by=trade_id, auth");
    let account_trade_id = LighterTradesQuery {
        sort_by: LighterTradeSortBy::TradeId,
        ..query.clone()
    };

    match client.get_trades(&account_trade_id).await {
        Ok(response) => {
            println!(
                "  OK: code={}, trades={}, next_cursor={:?}",
                response.code,
                response.trades.len(),
                response.next_cursor,
            );
        }
        Err(e) => {
            println!("  ERR (chain follows):");
            println!("    {e:#}");
        }
    }

    println!();
    println!("Probe #9: GET /api/v1/recentTrades (sanity, simpler endpoint) market_id=0");
    let recent = LighterRecentTradesQuery {
        market_id: 0,
        limit: 5,
    };

    match client.get_recent_trades(&recent).await {
        Ok(response) => {
            println!(
                "  OK: code={}, trades={}",
                response.code,
                response.trades.len()
            );
        }
        Err(e) => {
            println!("  ERR (chain follows):");
            println!("    {e:#}");
        }
    }

    println!();
    println!("=== URL serialized from our struct (probe #1 shape) ===");
    let probe1 = LighterTradesQuery {
        authorization: None,
        auth: Some(auth.clone()),
        market_id: None,
        account_index: Some(credential.account_index()),
        order_index: None,
        sort_by: LighterTradeSortBy::Timestamp,
        sort_dir: Some(LighterSortDirection::Desc),
        cursor: None,
        from_timestamp: None,
        ask_filter: None,
        role: None,
        trade_type: None,
        limit: LIGHTER_REST_PAGE_SIZE,
        aggregate: None,
    };
    let serialized = reqwest::Client::new()
        .get("https://x/api/v1/trades")
        .query(&probe1)
        .build()
        .unwrap()
        .url()
        .to_string();
    println!("  URL: {}", redact_auth(&serialized));

    println!();
    println!("=== Raw reqwest probes (bypass our query struct) ===");
    let raw = reqwest::Client::new();
    let base = "https://mainnet.zklighter.elliot.ai/api/v1/trades";
    let url_variants: &[(&str, Vec<(&str, String)>)] = &[
        (
            "A: bare market_id+limit, no sort_by",
            vec![("market_id", "0".into()), ("limit", "5".into())],
        ),
        (
            "B: market_id+limit+sort_by=trade_id",
            vec![
                ("market_id", "0".into()),
                ("limit", "5".into()),
                ("sort_by", "trade_id".into()),
            ],
        ),
        (
            "C: market_id+limit+index=0 (cursor-shaped)",
            vec![
                ("market_id", "0".into()),
                ("limit", "5".into()),
                ("index", "0".into()),
            ],
        ),
        (
            "D: account_index only with auth, no sort_by",
            vec![
                ("account_index", credential.account_index().to_string()),
                ("auth", auth.clone()),
                ("limit", "5".into()),
            ],
        ),
        (
            "E: account_index+sort_by=timestamp+order_index=0",
            vec![
                ("account_index", credential.account_index().to_string()),
                ("auth", auth.clone()),
                ("limit", "5".into()),
                ("sort_by", "timestamp".into()),
                ("order_index", "0".into()),
            ],
        ),
        (
            "F: market_id=0+sort_by=trade_id+sort_dir=desc+index=0",
            vec![
                ("market_id", "0".into()),
                ("limit", "5".into()),
                ("sort_by", "trade_id".into()),
                ("sort_dir", "desc".into()),
                ("index", "0".into()),
            ],
        ),
        (
            "G: account_index+auth+limit+sort_by=timestamp+sort_dir=desc (no order_index)",
            vec![
                ("account_index", credential.account_index().to_string()),
                ("auth", auth.clone()),
                ("limit", "5".into()),
                ("sort_by", "timestamp".into()),
                ("sort_dir", "desc".into()),
            ],
        ),
        (
            "H: account_index+auth+limit+sort_by=timestamp (no order_index, no sort_dir)",
            vec![
                ("account_index", credential.account_index().to_string()),
                ("auth", auth.clone()),
                ("limit", "5".into()),
                ("sort_by", "timestamp".into()),
            ],
        ),
        (
            "I: account_index+auth+limit (no sort_by at all)",
            vec![
                ("account_index", credential.account_index().to_string()),
                ("auth", auth.clone()),
                ("limit", "5".into()),
            ],
        ),
        (
            "J: market_id=0+auth+limit+sort_by=timestamp+order_index=0",
            vec![
                ("market_id", "0".into()),
                ("auth", auth.clone()),
                ("limit", "5".into()),
                ("sort_by", "timestamp".into()),
                ("order_index", "0".into()),
            ],
        ),
        (
            "K: account_index+auth+limit+sort_by=timestamp+order_index=0+sort_dir=desc",
            vec![
                ("account_index", credential.account_index().to_string()),
                ("auth", auth.clone()),
                ("limit", "5".into()),
                ("sort_by", "timestamp".into()),
                ("order_index", "0".into()),
                ("sort_dir", "desc".into()),
            ],
        ),
        (
            "L: limit=200 (negative case: above LIGHTER_REST_PAGE_SIZE)",
            vec![
                ("account_index", credential.account_index().to_string()),
                ("auth", auth.clone()),
                ("limit", "200".into()),
                ("sort_by", "timestamp".into()),
                ("sort_dir", "desc".into()),
            ],
        ),
        (
            "M: limit=100",
            vec![
                ("account_index", credential.account_index().to_string()),
                ("auth", auth.clone()),
                ("limit", "100".into()),
                ("sort_by", "timestamp".into()),
                ("sort_dir", "desc".into()),
            ],
        ),
        (
            "N: limit=50",
            vec![
                ("account_index", credential.account_index().to_string()),
                ("auth", auth.clone()),
                ("limit", "50".into()),
                ("sort_by", "timestamp".into()),
                ("sort_dir", "desc".into()),
            ],
        ),
    ];

    for (label, params) in url_variants {
        let resp = raw.get(base).query(&params).send().await;
        match resp {
            Ok(r) => {
                let status = r.status();
                let url = r.url().clone();
                let body = r.text().await.unwrap_or_else(|_| "<bin>".into());
                let preview = if body.len() > 220 {
                    format!("{}...(+{} bytes)", &body[..220], body.len() - 220)
                } else {
                    body
                };
                println!("Probe {label}");
                println!("  URL: {}", redact_auth(url.as_ref()));
                println!("  STATUS: {status}");
                println!("  BODY: {preview}");
            }
            Err(e) => {
                // `reqwest::Error`'s Display can include the offending
                // URL (auth-bearing). Strip the URL and surface only the
                // source chain.
                let chained = e.source().map_or_else(|| e.to_string(), |s| s.to_string());
                println!("Probe {label}: transport err: {chained}");
            }
        }
    }

    Ok(())
}

/// Replace the value of any `auth` query parameter with a masked form so
/// printed URLs don't leak a live Lighter L2 bearer token. Uses
/// [`mask_api_key`] for the substitution: keeps the leading/trailing 4
/// chars when long enough so output is still useful for triage.
fn redact_auth(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return url.to_string();
    };
    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(k, v)| {
            let masked = if k == "auth" {
                mask_api_key(&v)
            } else {
                v.into_owned()
            };
            (k.into_owned(), masked)
        })
        .collect();
    let mut out = parsed.clone();
    out.set_query(None);
    {
        let mut q = out.query_pairs_mut();
        for (k, v) in &pairs {
            q.append_pair(k, v);
        }
    }
    out.to_string()
}
