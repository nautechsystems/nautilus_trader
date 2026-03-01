# Kalshi Adapter Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a data-only Kalshi prediction market adapter for NautilusTrader supporting backtesting (public REST) and live paper trading (authenticated WebSocket).

**Architecture:** Pure Rust crate at `crates/adapters/kalshi/` with HTTP and WebSocket modules exposed via PyO3, plus a Python application layer in `nautilus_trader/adapters/kalshi/`. Authentication uses RSA-PSS signing (API key ID + PEM private key) via `aws-lc-rs`. The adapter mirrors the Polymarket adapter structure exactly.

**Tech Stack:** Rust, PyO3, aws-lc-rs (RSA-PSS), nautilus-network (HttpClient/WebSocketClient), rust_decimal, serde_json, tokio, axum (tests only).

**Design doc:** `docs/plans/2026-03-01-kalshi-adapter-design.md`

**Reference adapter:** `crates/adapters/polymarket/` — mirror its structure throughout.

---

### Task 1: Create Crate Skeleton

**Files:**
- Create: `crates/adapters/kalshi/Cargo.toml`
- Create: `crates/adapters/kalshi/src/lib.rs`
- Create: `crates/adapters/kalshi/src/config.rs`
- Create: `crates/adapters/kalshi/src/common/mod.rs`
- Create: `crates/adapters/kalshi/src/http/mod.rs`
- Create: `crates/adapters/kalshi/src/websocket/mod.rs`
- Create: `crates/adapters/kalshi/src/python/mod.rs`
- Modify: `Cargo.toml` (root workspace — add member + dependency)
- Modify: `crates/pyo3/Cargo.toml` (add kalshi dependency)

**Step 1: Create `crates/adapters/kalshi/Cargo.toml`**

```toml
[package]
name = "nautilus-kalshi"
readme = "README.md"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
description = "Kalshi integration adapter for the Nautilus trading engine"
categories.workspace = true
keywords.workspace = true
documentation.workspace = true
repository.workspace = true
homepage.workspace = true
publish = false

[lints]
workspace = true

[lib]
name = "nautilus_kalshi"
crate-type = ["rlib", "cdylib"]

[features]
default = ["high-precision"]
extension-module = [
  "nautilus-common/extension-module",
  "nautilus-core/extension-module",
  "nautilus-model/extension-module",
  "nautilus-network/extension-module",
  "python",
  "pyo3/extension-module",
]
python = [
  "nautilus-common/python",
  "nautilus-core/python",
  "nautilus-model/python",
  "nautilus-network/python",
  "pyo3",
  "pyo3-async-runtimes",
]
high-precision = ["nautilus-model/high-precision"]

[package.metadata.docs.rs]
all-features = true
rustdoc-args = ["--cfg", "docsrs"]

[dependencies]
nautilus-common = { workspace = true, features = ["live"] }
nautilus-core = { workspace = true }
nautilus-model = { workspace = true }
nautilus-network = { workspace = true }

anyhow = { workspace = true }
async-trait = { workspace = true }
aws-lc-rs = { workspace = true }
base64 = { workspace = true }
chrono = { workspace = true }
log = { workspace = true }
rust_decimal = { workspace = true }
rust_decimal_macros = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
strum = { workspace = true }
thiserror = { workspace = true }
tokio = { workspace = true }
ustr = { workspace = true }
zeroize = { workspace = true }

pyo3 = { workspace = true, optional = true }
pyo3-async-runtimes = { workspace = true, optional = true }

[dev-dependencies]
nautilus-testkit = { workspace = true }
axum = { workspace = true }
rstest = { workspace = true }
tokio = { workspace = true, features = ["full"] }
```

**Step 2: Create `crates/adapters/kalshi/src/lib.rs`**

```rust
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

//! [NautilusTrader](http://nautilustrader.io) adapter for the [Kalshi](https://kalshi.com)
//! prediction market exchange.

#![warn(rustc::all)]
#![deny(unsafe_code)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod common;
pub mod config;
pub mod http;
pub mod websocket;

#[cfg(feature = "python")]
pub mod python;
```

**Step 3: Create `crates/adapters/kalshi/src/common/mod.rs`**

```rust
pub mod consts;
pub mod credential;
pub mod enums;
pub mod parse;
pub mod urls;
```

**Step 4: Create `crates/adapters/kalshi/src/http/mod.rs`**

```rust
pub mod client;
pub mod error;
pub mod models;
pub mod parse;
pub mod rate_limits;
```

**Step 5: Create `crates/adapters/kalshi/src/websocket/mod.rs`**

```rust
pub mod client;
pub mod error;
pub mod handler;
pub mod messages;
```

**Step 6: Create `crates/adapters/kalshi/src/python/mod.rs`**

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

//! Python bindings from `pyo3`.

use pyo3::prelude::*;

/// Loaded as `nautilus_pyo3.kalshi`.
#[pymodule]
pub fn kalshi(_m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}
```

**Step 7: Create `crates/adapters/kalshi/src/config.rs`**

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

//! Configuration for the Kalshi adapter.

use crate::common::urls;

/// Configuration for the Kalshi data client.
#[derive(Clone, Debug)]
pub struct KalshiDataClientConfig {
    /// REST base URL. Defaults to production.
    pub base_url: Option<String>,
    /// WebSocket base URL. Defaults to production.
    pub ws_url: Option<String>,
    pub http_timeout_secs: u64,
    pub ws_timeout_secs: u64,
    /// Series tickers to include, e.g. `["KXBTC", "PRES-2024"]`.
    pub series_tickers: Vec<String>,
    /// Optional additional filter by event ticker.
    pub event_tickers: Vec<String>,
    /// How often to refresh instruments (minutes).
    pub instrument_reload_interval_mins: u64,
    /// REST requests per second. Default: 20 (Basic tier).
    pub rate_limit_rps: u32,
    /// Kalshi API key ID. Falls back to `KALSHI_API_KEY_ID` env var.
    pub api_key_id: Option<String>,
    /// RSA private key in PEM format. Falls back to `KALSHI_PRIVATE_KEY_PEM` env var.
    pub private_key_pem: Option<String>,
}

impl Default for KalshiDataClientConfig {
    fn default() -> Self {
        Self {
            base_url: None,
            ws_url: None,
            http_timeout_secs: 60,
            ws_timeout_secs: 30,
            series_tickers: Vec::new(),
            event_tickers: Vec::new(),
            instrument_reload_interval_mins: 60,
            rate_limit_rps: 20,
            api_key_id: None,
            private_key_pem: None,
        }
    }
}

impl KalshiDataClientConfig {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn http_url(&self) -> String {
        self.base_url
            .clone()
            .unwrap_or_else(|| urls::rest_base_url().to_string())
    }

    #[must_use]
    pub fn websocket_url(&self) -> String {
        self.ws_url
            .clone()
            .unwrap_or_else(|| urls::ws_base_url().to_string())
    }

    /// Resolve credentials from config fields or environment variables.
    #[must_use]
    pub fn resolved_api_key_id(&self) -> Option<String> {
        self.api_key_id
            .clone()
            .or_else(|| std::env::var("KALSHI_API_KEY_ID").ok())
    }

    /// Resolve credentials from config fields or environment variables.
    #[must_use]
    pub fn resolved_private_key_pem(&self) -> Option<String> {
        self.private_key_pem
            .clone()
            .or_else(|| std::env::var("KALSHI_PRIVATE_KEY_PEM").ok())
    }
}
```

**Step 8: Add to root `Cargo.toml` workspace**

In the `[workspace]` members list, add:
```toml
"crates/adapters/kalshi",
```

In the `[workspace.dependencies]` section, add:
```toml
nautilus-kalshi = { path = "crates/adapters/kalshi", version = "0.54.0", default-features = false }
```

**Step 9: Add to `crates/pyo3/Cargo.toml`**

Find the existing adapter dependencies section (near `nautilus-polymarket`) and add:
```toml
nautilus-kalshi = { workspace = true, features = ["python"] }
```

**Step 10: Verify crate compiles**

```bash
cargo build -p nautilus-kalshi
```

Expected: compiles with warnings about empty modules (that is fine).

**Step 11: Commit**

```bash
git add crates/adapters/kalshi/ Cargo.toml crates/pyo3/Cargo.toml
git commit -m "feat(kalshi): add crate skeleton with config and module structure"
```

---

### Task 2: Constants, URLs, and Enums

**Files:**
- Create: `crates/adapters/kalshi/src/common/consts.rs`
- Create: `crates/adapters/kalshi/src/common/urls.rs`
- Create: `crates/adapters/kalshi/src/common/enums.rs`

**Step 1: Write failing enum parse test**

Create `crates/adapters/kalshi/src/common/enums.rs` with just the test first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_status_deserialize() {
        let s: KalshiMarketStatus = serde_json::from_str(r#""active""#).unwrap();
        assert_eq!(s, KalshiMarketStatus::Active);
    }

    #[test]
    fn test_taker_side_deserialize() {
        let s: KalshiTakerSide = serde_json::from_str(r#""yes""#).unwrap();
        assert_eq!(s, KalshiTakerSide::Yes);
    }

    #[test]
    fn test_candlestick_interval_minutes() {
        assert_eq!(CandlestickInterval::Minutes1 as u32, 1);
        assert_eq!(CandlestickInterval::Hours1 as u32, 60);
        assert_eq!(CandlestickInterval::Days1 as u32, 1440);
    }
}
```

**Step 2: Run test to confirm it fails**

```bash
cargo test -p nautilus-kalshi 2>&1 | head -20
```

Expected: compile error — `KalshiMarketStatus` not found.

**Step 3: Implement `consts.rs`**

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

//! Constants for the Kalshi adapter.

use std::sync::LazyLock;

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

pub const KALSHI: &str = "KALSHI";

pub static KALSHI_VENUE: LazyLock<Venue> =
    LazyLock::new(|| Venue::new(Ustr::from(KALSHI)));

pub const USD: &str = "USD";

/// Minimum YES price (dollar string).
pub const MIN_PRICE: &str = "0.0001";
/// Maximum YES price (dollar string).
pub const MAX_PRICE: &str = "0.9999";

/// Price precision: 4 decimal places (supports subpenny pricing from 2026).
pub const PRICE_PRECISION: u8 = 4;
/// Size precision: 2 decimal places.
pub const SIZE_PRECISION: u8 = 2;

/// Default REST requests per second (Basic tier).
pub const HTTP_RATE_LIMIT_RPS: u32 = 20;
```

**Step 4: Implement `urls.rs`**

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

//! URL resolution for the Kalshi API endpoints.

const REST_BASE_URL: &str = "https://api.elections.kalshi.com/trade-api/v2";
const WS_BASE_URL: &str = "wss://api.elections.kalshi.com/trade-api/ws/v2";
const DEMO_REST_BASE_URL: &str = "https://demo-api.kalshi.co/trade-api/v2";
const DEMO_WS_BASE_URL: &str = "wss://demo-api.kalshi.co/trade-api/ws/v2";

#[must_use]
pub const fn rest_base_url() -> &'static str {
    REST_BASE_URL
}

#[must_use]
pub const fn ws_base_url() -> &'static str {
    WS_BASE_URL
}

#[must_use]
pub const fn demo_rest_base_url() -> &'static str {
    DEMO_REST_BASE_URL
}

#[must_use]
pub const fn demo_ws_base_url() -> &'static str {
    DEMO_WS_BASE_URL
}
```

**Step 5: Implement `enums.rs`**

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

//! Kalshi-specific enums mapped to NautilusTrader core types.

use serde::{Deserialize, Serialize};

/// Status of a Kalshi market (contract).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KalshiMarketStatus {
    Initialized,
    Inactive,
    Active,
    Closed,
    Determined,
    Disputed,
    Amended,
    Finalized,
}

/// Which side the taker (aggressor) was on in a trade.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KalshiTakerSide {
    Yes,
    No,
}

/// Market type (binary or scalar).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KalshiMarketType {
    Binary,
    Scalar,
}

/// Candlestick interval in minutes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum CandlestickInterval {
    Minutes1 = 1,
    Hours1 = 60,
    Days1 = 1440,
}

impl CandlestickInterval {
    #[must_use]
    pub fn as_minutes(self) -> u32 {
        self as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_market_status_deserialize() {
        let s: KalshiMarketStatus = serde_json::from_str(r#""active""#).unwrap();
        assert_eq!(s, KalshiMarketStatus::Active);
    }

    #[test]
    fn test_taker_side_deserialize() {
        let s: KalshiTakerSide = serde_json::from_str(r#""yes""#).unwrap();
        assert_eq!(s, KalshiTakerSide::Yes);
    }

    #[test]
    fn test_candlestick_interval_minutes() {
        assert_eq!(CandlestickInterval::Minutes1 as u32, 1);
        assert_eq!(CandlestickInterval::Hours1 as u32, 60);
        assert_eq!(CandlestickInterval::Days1 as u32, 1440);
    }
}
```

**Step 6: Update `common/mod.rs` to declare modules**

Already done in Task 1. Verify the file declares `consts`, `credential`, `enums`, `parse`, `urls`.

**Step 7: Run tests**

```bash
cargo test -p nautilus-kalshi common
```

Expected: 3 tests pass.

**Step 8: Commit**

```bash
git add crates/adapters/kalshi/
git commit -m "feat(kalshi): add constants, URLs, and enums"
```

---

### Task 3: RSA-PSS Credential

**Files:**
- Create: `crates/adapters/kalshi/src/common/credential.rs`

**Step 1: Write failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credential_new_invalid_pem_fails() {
        let result = KalshiCredential::new("key-id".to_string(), "not-a-pem");
        assert!(result.is_err());
    }

    #[test]
    fn test_credential_sign_produces_three_header_values() {
        // Use a fresh 2048-bit RSA key generated for tests.
        // We generate one inline using aws-lc-rs so no test file is needed.
        let cred = make_test_credential();
        let (ts, sig) = cred.sign("GET", "/trade-api/ws/v2");
        assert!(!ts.is_empty());
        assert!(!sig.is_empty());
        // Timestamp is numeric milliseconds.
        ts.parse::<u64>().expect("timestamp must be numeric");
        // Signature is valid base64.
        base64::engine::general_purpose::STANDARD.decode(&sig).expect("must be base64");
    }

    #[test]
    fn test_sign_strips_query_from_path() {
        let cred = make_test_credential();
        let (ts1, sig1) = cred.sign("GET", "/trade-api/v2/markets");
        let (ts2, sig2) = cred.sign("GET", "/trade-api/v2/markets");
        // Different timestamps → different signatures (RSA-PSS is randomized).
        // Just verify both are non-empty valid base64.
        assert!(!sig1.is_empty());
        assert!(!sig2.is_empty());
        let _ = (ts1, ts2);
    }
}
```

**Step 2: Run to confirm compile failure**

```bash
cargo test -p nautilus-kalshi credential 2>&1 | head -10
```

**Step 3: Implement `credential.rs`**

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

//! RSA-PSS credential for authenticating Kalshi API requests.
//!
//! Kalshi uses RSA-PSS with SHA-256 (MGF1-SHA256, salt = digest length = 32 bytes).
//! Each request is independently signed — there are no session tokens.
//!
//! Required headers on authenticated requests:
//! - `KALSHI-ACCESS-KEY`: the API key ID (UUID)
//! - `KALSHI-ACCESS-TIMESTAMP`: Unix time in milliseconds (string)
//! - `KALSHI-ACCESS-SIGNATURE`: Base64-encoded RSA-PSS signature
//!
//! Signature message: `{timestamp_ms}{HTTP_METHOD_UPPERCASE}{path_without_query}`

use aws_lc_rs::{
    rand::SystemRandom,
    signature::{KeyPair, RsaKeyPair, RSA_PSS_SHA256},
};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use zeroize::ZeroizeOnDrop;

pub const HEADER_ACCESS_KEY: &str = "KALSHI-ACCESS-KEY";
pub const HEADER_TIMESTAMP: &str = "KALSHI-ACCESS-TIMESTAMP";
pub const HEADER_SIGNATURE: &str = "KALSHI-ACCESS-SIGNATURE";

/// Parses a PEM-encoded private key (PKCS#8 "BEGIN PRIVATE KEY") into raw DER bytes.
fn pem_to_der(pem: &str) -> anyhow::Result<Vec<u8>> {
    let body: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect();
    B64.decode(body.trim()).map_err(|e| anyhow::anyhow!("PEM base64 decode error: {e}"))
}

/// RSA-PSS signing credential for Kalshi API authentication.
///
/// Thread-safe: `SystemRandom` and `RsaKeyPair` are `Send + Sync`.
#[derive(Debug, ZeroizeOnDrop)]
pub struct KalshiCredential {
    api_key_id: String,
    #[zeroize(skip)]
    key_pair: RsaKeyPair,
    #[zeroize(skip)]
    rng: SystemRandom,
}

impl KalshiCredential {
    /// Create a new credential from an API key ID and PEM-encoded RSA private key.
    ///
    /// # Errors
    ///
    /// Returns an error if the PEM cannot be decoded or the key is invalid.
    pub fn new(api_key_id: String, private_key_pem: &str) -> anyhow::Result<Self> {
        let der = pem_to_der(private_key_pem)?;
        let key_pair = RsaKeyPair::from_pkcs8(&der)
            .map_err(|e| anyhow::anyhow!("Invalid RSA private key (PKCS#8 required): {e}"))?;
        Ok(Self {
            api_key_id,
            key_pair,
            rng: SystemRandom::new(),
        })
    }

    /// Returns the API key ID (for the `KALSHI-ACCESS-KEY` header).
    #[must_use]
    pub fn api_key_id(&self) -> &str {
        &self.api_key_id
    }

    /// Signs a request and returns `(timestamp_ms, signature_b64)`.
    ///
    /// The caller must set all three headers:
    /// - `KALSHI-ACCESS-KEY` = `self.api_key_id()`
    /// - `KALSHI-ACCESS-TIMESTAMP` = returned `timestamp_ms`
    /// - `KALSHI-ACCESS-SIGNATURE` = returned `signature_b64`
    ///
    /// # Panics
    ///
    /// Panics if the system random number generator fails (should never happen).
    #[must_use]
    pub fn sign(&self, method: &str, path: &str) -> (String, String) {
        let ts_ms = chrono::Utc::now().timestamp_millis().to_string();
        // Strip query string from path before signing.
        let clean_path = path.split('?').next().unwrap_or(path);
        let msg = format!("{ts_ms}{}{clean_path}", method.to_ascii_uppercase());

        let mut sig = vec![0u8; self.key_pair.public_modulus_len()];
        self.key_pair
            .sign(&RSA_PSS_SHA256, &self.rng, msg.as_bytes(), &mut sig)
            .expect("RSA-PSS signing failed");

        (ts_ms, B64.encode(&sig))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate a test credential using a freshly-created RSA key.
    /// The key is generated by openssl in the test — no stored test key needed.
    pub(crate) fn make_test_credential() -> KalshiCredential {
        // Generate a 2048-bit RSA key pair in PKCS#8 DER format using aws-lc-rs.
        use aws_lc_rs::signature::RsaKeyPair;
        let (_, pkcs8_der) = aws_lc_rs::rsa::KeyPair::generate(2048)
            .map(|kp| {
                let doc = kp.private_key().as_der().to_vec();
                (kp, doc)
            })
            .unwrap_or_else(|_| {
                // Fallback: use a pre-generated test key in PEM form.
                panic!("Failed to generate RSA key for tests");
            });
        // Wrap in KalshiCredential directly from DER bytes (bypass PEM for tests).
        let key_pair = RsaKeyPair::from_pkcs8(&pkcs8_der).expect("valid generated key");
        KalshiCredential {
            api_key_id: "test-key-id".to_string(),
            key_pair,
            rng: SystemRandom::new(),
        }
    }

    #[test]
    fn test_credential_new_invalid_pem_fails() {
        let result = KalshiCredential::new("key-id".to_string(), "not-a-pem");
        assert!(result.is_err());
    }

    #[test]
    fn test_credential_sign_produces_valid_output() {
        let cred = make_test_credential();
        let (ts, sig) = cred.sign("GET", "/trade-api/ws/v2");
        assert!(!ts.is_empty());
        assert!(!sig.is_empty());
        ts.parse::<u64>().expect("timestamp must be numeric milliseconds");
        B64.decode(&sig).expect("signature must be valid base64");
    }

    #[test]
    fn test_sign_strips_query_from_path() {
        let cred = make_test_credential();
        // The signed message must NOT include the query string.
        // We can't verify the internal message, but we verify no panic occurs.
        let (ts, sig) = cred.sign("GET", "/trade-api/v2/markets?ticker=KXBTC");
        assert!(!ts.is_empty());
        assert!(!sig.is_empty());
    }
}
```

> **Note on test key generation:** The `aws_lc_rs::rsa::KeyPair::generate` API may differ by version. If it doesn't compile, replace the `make_test_credential` helper by loading a hardcoded PEM from a test fixture file at `crates/adapters/kalshi/test_data/test_rsa_private_key.pem`. Generate one with: `openssl genrsa 2048 | openssl pkcs8 -topk8 -nocrypt -out test_data/test_rsa_private_key.pem`. Then load it in the test with `std::fs::read_to_string(...)`.

**Step 4: Run tests**

```bash
cargo test -p nautilus-kalshi credential
```

Expected: 3 tests pass.

**Step 5: Commit**

```bash
git add crates/adapters/kalshi/src/common/credential.rs
git commit -m "feat(kalshi): add RSA-PSS credential with signing"
```

---

### Task 4: HTTP Models and Test Fixtures

**Files:**
- Create: `crates/adapters/kalshi/src/http/models.rs`
- Create: `crates/adapters/kalshi/test_data/http_markets.json`
- Create: `crates/adapters/kalshi/test_data/http_orderbook.json`
- Create: `crates/adapters/kalshi/test_data/http_trades.json`
- Create: `crates/adapters/kalshi/test_data/http_candlesticks.json`

**Step 1: Create test fixtures**

`crates/adapters/kalshi/test_data/http_markets.json`:
```json
{
  "markets": [
    {
      "ticker": "KXBTC-25MAR15-B100000",
      "event_ticker": "KXBTC-25MAR15",
      "market_type": "binary",
      "title": "Will Bitcoin exceed $100,000 by March 15?",
      "subtitle": "",
      "yes_sub_title": "Yes",
      "no_sub_title": "No",
      "open_time": "2025-03-01T00:00:00Z",
      "close_time": "2025-03-15T23:59:59Z",
      "latest_expiration_time": "2025-03-15T23:59:59Z",
      "status": "active",
      "yes_bid_dollars": "0.4200",
      "yes_ask_dollars": "0.4500",
      "no_bid_dollars": "0.5400",
      "no_ask_dollars": "0.5700",
      "last_price_dollars": "0.4300",
      "volume_fp": "125000.00",
      "open_interest_fp": "50000.00",
      "notional_value_dollars": "1.0000",
      "rules_primary": "Resolves Yes if BTC/USD spot price exceeds $100,000 at market close."
    }
  ],
  "cursor": ""
}
```

`crates/adapters/kalshi/test_data/http_orderbook.json`:
```json
{
  "orderbook_fp": {
    "yes_dollars": [["0.3500", "200.00"], ["0.4200", "13.00"]],
    "no_dollars": [["0.5400", "400.00"], ["0.5600", "17.00"]]
  }
}
```

`crates/adapters/kalshi/test_data/http_trades.json`:
```json
{
  "trades": [
    {
      "trade_id": "d91bc706-ee49-470d-82d8-11418bda6fed",
      "ticker": "KXBTC-25MAR15-B100000",
      "yes_price_dollars": "0.3600",
      "no_price_dollars": "0.6400",
      "count_fp": "136.00",
      "taker_side": "no",
      "created_time": "2025-03-01T18:44:01Z"
    }
  ],
  "cursor": ""
}
```

`crates/adapters/kalshi/test_data/http_candlesticks.json`:
```json
{
  "candlesticks": [
    {
      "end_period_ts": 1741046400,
      "yes_bid": {
        "open": "0.4100",
        "high": "0.4500",
        "low": "0.3900",
        "close": "0.4200"
      },
      "yes_ask": {
        "open": "0.4400",
        "high": "0.4800",
        "low": "0.4200",
        "close": "0.4500"
      },
      "price": {
        "open": "0.4100",
        "high": "0.4500",
        "low": "0.3900",
        "close": "0.4200",
        "mean": "0.4150",
        "previous": "0.4000"
      },
      "volume": "1250.00",
      "open_interest": "50000.00"
    }
  ]
}
```

**Step 2: Write failing parse tests**

At the bottom of `models.rs`, add tests before writing the structs:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn load_fixture(name: &str) -> String {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing fixture: {}", path.display()))
    }

    #[test]
    fn test_parse_markets_response() {
        let json = load_fixture("http_markets.json");
        let resp: KalshiMarketsResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp.markets.len(), 1);
        let m = &resp.markets[0];
        assert_eq!(m.ticker.as_str(), "KXBTC-25MAR15-B100000");
        assert_eq!(m.event_ticker.as_str(), "KXBTC-25MAR15");
        assert_eq!(m.yes_bid_dollars.as_deref(), Some("0.4200"));
    }

    #[test]
    fn test_parse_orderbook_response() {
        let json = load_fixture("http_orderbook.json");
        let resp: KalshiOrderbookResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp.orderbook_fp.yes_dollars.len(), 2);
        // Prices are ascending; best bid is last.
        assert_eq!(resp.orderbook_fp.yes_dollars[1].0, "0.4200");
        assert_eq!(resp.orderbook_fp.yes_dollars[1].1, "13.00");
    }

    #[test]
    fn test_parse_trades_response() {
        let json = load_fixture("http_trades.json");
        let resp: KalshiTradesResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp.trades.len(), 1);
        let t = &resp.trades[0];
        assert_eq!(t.ticker.as_str(), "KXBTC-25MAR15-B100000");
        assert_eq!(t.yes_price_dollars, "0.3600");
        assert_eq!(t.taker_side, KalshiTakerSide::No);
    }

    #[test]
    fn test_parse_candlesticks_response() {
        let json = load_fixture("http_candlesticks.json");
        let resp: KalshiCandlesticksResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp.candlesticks.len(), 1);
        let c = &resp.candlesticks[0];
        assert_eq!(c.end_period_ts, 1741046400);
        assert_eq!(c.price.close.as_deref(), Some("0.4200"));
        assert_eq!(c.volume, "1250.00");
    }
}
```

**Step 3: Run to confirm failure**

```bash
cargo test -p nautilus-kalshi models 2>&1 | head -10
```

Expected: compile error — structs not defined yet.

**Step 4: Implement `models.rs`**

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

//! HTTP REST model types for the Kalshi API.

use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::enums::{KalshiMarketStatus, KalshiMarketType, KalshiTakerSide};

// ---------------------------------------------------------------------------
// Instrument discovery
// ---------------------------------------------------------------------------

/// A Kalshi market (binary contract) as returned by `GET /markets`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiMarket {
    pub ticker: Ustr,
    pub event_ticker: Ustr,
    pub market_type: KalshiMarketType,
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    pub yes_sub_title: Option<String>,
    pub no_sub_title: Option<String>,
    /// ISO 8601 open time.
    pub open_time: Option<String>,
    /// ISO 8601 close time.
    pub close_time: Option<String>,
    pub latest_expiration_time: Option<String>,
    pub status: KalshiMarketStatus,
    /// Best YES bid as dollar string (e.g. "0.4200").
    pub yes_bid_dollars: Option<String>,
    pub yes_ask_dollars: Option<String>,
    pub no_bid_dollars: Option<String>,
    pub no_ask_dollars: Option<String>,
    pub last_price_dollars: Option<String>,
    pub volume_fp: Option<String>,
    pub open_interest_fp: Option<String>,
    pub notional_value_dollars: Option<String>,
    pub rules_primary: Option<String>,
}

/// Paginated response from `GET /markets`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiMarketsResponse {
    pub markets: Vec<KalshiMarket>,
    pub cursor: Option<String>,
}

// ---------------------------------------------------------------------------
// Orderbook
// ---------------------------------------------------------------------------

/// A single price level: (price_dollars, count_fp).
pub type KalshiPriceLevel = (String, String);

/// Fixed-point orderbook with dollar string prices.
///
/// Levels are sorted ascending by price; the best bid is the **last** entry.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiOrderbookFp {
    /// YES bids sorted ascending.
    pub yes_dollars: Vec<KalshiPriceLevel>,
    /// NO bids sorted ascending.
    pub no_dollars: Vec<KalshiPriceLevel>,
}

/// Response from `GET /markets/{ticker}/orderbook`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiOrderbookResponse {
    pub orderbook_fp: KalshiOrderbookFp,
}

// ---------------------------------------------------------------------------
// Trades
// ---------------------------------------------------------------------------

/// A single trade as returned by `GET /markets/trades`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiTrade {
    pub trade_id: String,
    pub ticker: Ustr,
    /// YES execution price as dollar string.
    pub yes_price_dollars: String,
    /// NO execution price as dollar string.
    pub no_price_dollars: String,
    /// Contract count with 2 decimal places (e.g. "136.00").
    pub count_fp: String,
    pub taker_side: KalshiTakerSide,
    /// ISO 8601 creation time.
    pub created_time: String,
}

/// Paginated response from `GET /markets/trades`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiTradesResponse {
    pub trades: Vec<KalshiTrade>,
    pub cursor: Option<String>,
}

// ---------------------------------------------------------------------------
// Candlesticks (OHLCV)
// ---------------------------------------------------------------------------

/// OHLC price data for one candle side (bid or ask).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiOhlc {
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
}

/// OHLC + mean + previous for trade prices.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiPriceOhlc {
    pub open: Option<String>,
    pub high: Option<String>,
    pub low: Option<String>,
    pub close: Option<String>,
    pub mean: Option<String>,
    pub previous: Option<String>,
}

/// One OHLCV candlestick from `GET /historical/markets/{ticker}/candlesticks`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiCandlestick {
    /// Unix timestamp of the period end (seconds).
    pub end_period_ts: u64,
    /// YES bid OHLC.
    pub yes_bid: KalshiOhlc,
    /// YES ask OHLC.
    pub yes_ask: KalshiOhlc,
    /// Trade price OHLC + statistics.
    pub price: KalshiPriceOhlc,
    /// Total contracts traded in this period (2 decimal places).
    pub volume: String,
    pub open_interest: String,
}

/// Response from `GET /historical/markets/{ticker}/candlesticks`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KalshiCandlesticksResponse {
    pub candlesticks: Vec<KalshiCandlestick>,
}

// tests defined above — include them here
#[cfg(test)]
mod tests {
    use super::*;

    fn load_fixture(name: &str) -> String {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing fixture: {}", path.display()))
    }

    #[test]
    fn test_parse_markets_response() {
        let json = load_fixture("http_markets.json");
        let resp: KalshiMarketsResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp.markets.len(), 1);
        let m = &resp.markets[0];
        assert_eq!(m.ticker.as_str(), "KXBTC-25MAR15-B100000");
        assert_eq!(m.event_ticker.as_str(), "KXBTC-25MAR15");
        assert_eq!(m.yes_bid_dollars.as_deref(), Some("0.4200"));
    }

    #[test]
    fn test_parse_orderbook_response() {
        let json = load_fixture("http_orderbook.json");
        let resp: KalshiOrderbookResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp.orderbook_fp.yes_dollars.len(), 2);
        assert_eq!(resp.orderbook_fp.yes_dollars[1].0, "0.4200");
        assert_eq!(resp.orderbook_fp.yes_dollars[1].1, "13.00");
    }

    #[test]
    fn test_parse_trades_response() {
        let json = load_fixture("http_trades.json");
        let resp: KalshiTradesResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp.trades.len(), 1);
        let t = &resp.trades[0];
        assert_eq!(t.ticker.as_str(), "KXBTC-25MAR15-B100000");
        assert_eq!(t.yes_price_dollars, "0.3600");
        assert_eq!(t.taker_side, KalshiTakerSide::No);
    }

    #[test]
    fn test_parse_candlesticks_response() {
        let json = load_fixture("http_candlesticks.json");
        let resp: KalshiCandlesticksResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp.candlesticks.len(), 1);
        let c = &resp.candlesticks[0];
        assert_eq!(c.end_period_ts, 1741046400);
        assert_eq!(c.price.close.as_deref(), Some("0.4200"));
        assert_eq!(c.volume, "1250.00");
    }
}
```

**Step 5: Run tests**

```bash
cargo test -p nautilus-kalshi http::models
```

Expected: 4 tests pass.

**Step 6: Commit**

```bash
git add crates/adapters/kalshi/
git commit -m "feat(kalshi): add HTTP models and test fixtures"
```

---

### Task 5: HTTP Parse — Market to BinaryOption

**Files:**
- Create: `crates/adapters/kalshi/src/common/parse.rs`
- Create: `crates/adapters/kalshi/src/http/parse.rs`

**Step 1: Write failing test**

```rust
// In http/parse.rs tests:
#[test]
fn test_market_to_binary_option() {
    let json = load_fixture("http_markets.json");
    let resp: KalshiMarketsResponse = serde_json::from_str(&json).unwrap();
    let instrument = market_to_binary_option(&resp.markets[0]).unwrap();
    assert_eq!(instrument.id.symbol.as_str(), "KXBTC-25MAR15-B100000");
    assert_eq!(instrument.price_precision, 4);
    assert_eq!(instrument.size_precision, 2);
}
```

**Step 2: Implement `common/parse.rs`** (shared parsing utilities)

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

//! Shared parsing utilities for the Kalshi adapter.

use chrono::DateTime;
use nautilus_core::UnixNanos;

/// Parse an ISO 8601 datetime string to `UnixNanos`.
///
/// # Errors
///
/// Returns an error if the string is not a valid RFC 3339 datetime.
pub fn parse_datetime_to_nanos(s: &str) -> anyhow::Result<UnixNanos> {
    let dt = DateTime::parse_from_rfc3339(s)
        .map_err(|e| anyhow::anyhow!("Invalid datetime '{s}': {e}"))?;
    Ok(UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64))
}
```

**Step 3: Implement `http/parse.rs`**

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

//! Parsing utilities: convert Kalshi HTTP models to NautilusTrader domain types.

use std::str::FromStr;

use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::BinaryOption,
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    common::{
        consts::{KALSHI_VENUE, PRICE_PRECISION, SIZE_PRECISION},
        parse::parse_datetime_to_nanos,
    },
    http::models::KalshiMarket,
};

/// Convert a `KalshiMarket` to a `BinaryOption` instrument.
///
/// Uses the YES side as the primary instrument (outcome = "Yes").
///
/// # Errors
///
/// Returns an error if required fields are missing or unparseable.
pub fn market_to_binary_option(market: &KalshiMarket) -> anyhow::Result<BinaryOption> {
    let symbol = Symbol::new(market.ticker);
    let id = InstrumentId::new(symbol, *KALSHI_VENUE);
    let currency = Currency::USD();

    let price_increment = Price::from_str("0.0001")?;
    let size_increment = Quantity::from_str("0.01")?;

    let activation_ns = market
        .open_time
        .as_deref()
        .map(parse_datetime_to_nanos)
        .transpose()?
        .unwrap_or_default();

    let expiration_ns = market
        .close_time
        .as_deref()
        .or(market.latest_expiration_time.as_deref())
        .map(parse_datetime_to_nanos)
        .transpose()?
        .unwrap_or_default();

    let now = UnixNanos::from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64,
    );

    Ok(BinaryOption::new(
        id,
        market.ticker,           // raw_symbol
        currency,
        activation_ns,
        expiration_ns,
        PRICE_PRECISION,
        SIZE_PRECISION,
        price_increment,
        size_increment,
        Decimal::ZERO,           // margin_init
        Decimal::ZERO,           // margin_maint
        Decimal::ZERO,           // maker_fee (Kalshi fees are in the order flow)
        Decimal::ZERO,           // taker_fee
        Some(Ustr::from("Yes")), // outcome
        Some(Ustr::from(market.title.as_str())), // description
        None,                    // max_quantity
        None,                    // min_quantity
        None,                    // max_notional
        None,                    // min_notional
        Some(Price::from_str("0.9999")?), // max_price
        Some(Price::from_str("0.0001")?), // min_price
        None,                    // info
        now,                     // ts_event
        now,                     // ts_init
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::models::KalshiMarketsResponse;

    fn load_fixture(name: &str) -> String {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing fixture: {}", path.display()))
    }

    #[test]
    fn test_market_to_binary_option() {
        let json = load_fixture("http_markets.json");
        let resp: KalshiMarketsResponse = serde_json::from_str(&json).unwrap();
        let instrument = market_to_binary_option(&resp.markets[0]).unwrap();
        assert_eq!(instrument.id.symbol.as_str(), "KXBTC-25MAR15-B100000");
        assert_eq!(instrument.price_precision, 4);
        assert_eq!(instrument.size_precision, 2);
        assert_eq!(instrument.outcome.as_deref(), Some("Yes"));
    }
}
```

> **Note:** `BinaryOption::new(...)` signature must match the actual struct in `nautilus-model`. Check `crates/model/src/instruments/binary_option.rs` for the exact constructor parameters and adjust accordingly. Use the Polymarket adapter's instrument parsing as the canonical reference.

**Step 4: Run tests**

```bash
cargo test -p nautilus-kalshi parse
```

**Step 5: Commit**

```bash
git add crates/adapters/kalshi/src/common/parse.rs crates/adapters/kalshi/src/http/parse.rs
git commit -m "feat(kalshi): add market-to-BinaryOption parse conversion"
```

---

### Task 6: HTTP Client

**Files:**
- Create: `crates/adapters/kalshi/src/http/error.rs`
- Create: `crates/adapters/kalshi/src/http/rate_limits.rs`
- Create: `crates/adapters/kalshi/src/http/client.rs`

**Step 1: Implement `http/error.rs`**

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use thiserror::Error;

#[derive(Debug, Error)]
pub enum KalshiHttpError {
    #[error("HTTP request failed: {0}")]
    Request(String),
    #[error("JSON parse error: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("Authentication required but no credential provided")]
    NoCredential,
    #[error("Rate limit exceeded")]
    RateLimit,
    #[error("Kalshi API error {status}: {message}")]
    Api { status: u16, message: String },
}
```

**Step 2: Implement `http/rate_limits.rs`**

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use crate::common::consts::HTTP_RATE_LIMIT_RPS;

/// Default rate limit header name used by Kalshi.
pub const RATE_LIMIT_HEADER: &str = "X-RateLimit-Remaining";

/// Build the rate limit configuration for the HTTP client.
///
/// Kalshi tiers: Basic 20/s, Advanced 30/s, Premier 100/s, Prime 400/s.
/// Defaults to Basic (20 req/s).
#[must_use]
pub fn default_rate_limit_rps() -> u32 {
    HTTP_RATE_LIMIT_RPS
}
```

**Step 3: Write failing HTTP client test**

Create `crates/adapters/kalshi/tests/http.rs`:

```rust
//! Integration tests for the Kalshi HTTP client using a mock server.

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{Router, extract::State, response::Json, routing::get};
use nautilus_kalshi::{
    config::KalshiDataClientConfig,
    http::{client::KalshiHttpClient, models::KalshiMarketsResponse},
};
use serde_json::Value;

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_json(filename: &str) -> Value {
    let content = std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("missing fixture: {filename}"));
    serde_json::from_str(&content).expect("invalid json")
}

#[derive(Clone)]
struct State_ {
    markets: Arc<tokio::sync::Mutex<Value>>,
}

async fn handle_markets(axum::extract::State(s): axum::extract::State<Arc<State_>>) -> Json<Value> {
    Json(s.markets.lock().await.clone())
}

async fn start_mock(markets: Value) -> SocketAddr {
    let state = Arc::new(State_ {
        markets: Arc::new(tokio::sync::Mutex::new(markets)),
    });
    let app = Router::new()
        .route("/trade-api/v2/markets", get(handle_markets))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, app).into_future());
    addr
}

#[tokio::test]
async fn test_get_markets_parses_response() {
    let markets_json = load_json("http_markets.json");
    let addr = start_mock(markets_json).await;

    let mut config = KalshiDataClientConfig::new();
    config.base_url = Some(format!("http://{addr}/trade-api/v2"));
    config.series_tickers = vec!["KXBTC".to_string()];

    let client = KalshiHttpClient::new(config);
    let markets = client.get_markets(&[], &["KXBTC"]).await.unwrap();
    assert_eq!(markets.len(), 1);
    assert_eq!(markets[0].ticker.as_str(), "KXBTC-25MAR15-B100000");
}
```

**Step 4: Run to confirm failure**

```bash
cargo test -p nautilus-kalshi --test http 2>&1 | head -10
```

**Step 5: Implement `http/client.rs`**

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

//! Kalshi REST HTTP client.

use std::sync::Arc;

use nautilus_network::http::HttpClient;

use crate::{
    common::{
        credential::KalshiCredential,
        enums::CandlestickInterval,
    },
    config::KalshiDataClientConfig,
    http::{
        error::KalshiHttpError,
        models::{
            KalshiCandlestick, KalshiCandlesticksResponse, KalshiMarket, KalshiMarketsResponse,
            KalshiOrderbookResponse, KalshiTrade, KalshiTradesResponse,
        },
    },
};

/// Raw HTTP client for the Kalshi REST API.
///
/// Public endpoints (market discovery, trades, candlesticks) need no credentials.
/// The orderbook snapshot endpoint requires authentication.
#[derive(Debug)]
pub struct KalshiHttpClient {
    base_url: String,
    inner: HttpClient,
    credential: Option<Arc<KalshiCredential>>,
}

impl KalshiHttpClient {
    /// Create a new client from config. Credentials are optional.
    ///
    /// # Panics
    ///
    /// Panics if the `HttpClient` cannot be constructed (invalid config).
    pub fn new(config: KalshiDataClientConfig) -> Self {
        let base_url = config.http_url();

        let credential = match (
            config.resolved_api_key_id(),
            config.resolved_private_key_pem(),
        ) {
            (Some(key_id), Some(pem)) => {
                match KalshiCredential::new(key_id, &pem) {
                    Ok(cred) => {
                        log::info!("Kalshi: authenticated HTTP client initialized");
                        Some(Arc::new(cred))
                    }
                    Err(e) => {
                        log::warn!("Kalshi: failed to load credential: {e}");
                        None
                    }
                }
            }
            _ => {
                log::info!("Kalshi: unauthenticated HTTP client (public endpoints only)");
                None
            }
        };

        let timeout = std::time::Duration::from_secs(config.http_timeout_secs);
        let inner = HttpClient::builder()
            .timeout(timeout)
            .build()
            .expect("failed to build HttpClient");

        Self { base_url, inner, credential }
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    async fn get_public<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, KalshiHttpError> {
        let url = format!("{}{path}", self.base_url);
        let resp = self
            .inner
            .get(&url)
            .send()
            .await
            .map_err(|e| KalshiHttpError::Request(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(KalshiHttpError::Api {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        resp.json::<T>()
            .await
            .map_err(|e| KalshiHttpError::Request(e.to_string()))
    }

    async fn get_authenticated<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, KalshiHttpError> {
        let cred = self.credential.as_ref().ok_or(KalshiHttpError::NoCredential)?;
        let (ts, sig) = cred.sign("GET", path);

        let url = format!("{}{path}", self.base_url);
        let resp = self
            .inner
            .get(&url)
            .header(crate::common::credential::HEADER_ACCESS_KEY, cred.api_key_id())
            .header(crate::common::credential::HEADER_TIMESTAMP, ts)
            .header(crate::common::credential::HEADER_SIGNATURE, sig)
            .send()
            .await
            .map_err(|e| KalshiHttpError::Request(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(KalshiHttpError::Api {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        resp.json::<T>()
            .await
            .map_err(|e| KalshiHttpError::Request(e.to_string()))
    }

    // ------------------------------------------------------------------
    // Public API — Instrument Discovery
    // ------------------------------------------------------------------

    /// Fetch markets filtered by event and/or series tickers.
    ///
    /// Handles cursor-based pagination automatically.
    pub async fn get_markets(
        &self,
        event_tickers: &[&str],
        series_tickers: &[&str],
    ) -> Result<Vec<KalshiMarket>, KalshiHttpError> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let mut params = vec![("limit", "1000".to_string())];
            if !event_tickers.is_empty() {
                params.push(("event_ticker", event_tickers.join(",")));
            }
            if !series_tickers.is_empty() {
                params.push(("series_ticker", series_tickers.join(",")));
            }
            if let Some(ref c) = cursor {
                if !c.is_empty() {
                    params.push(("cursor", c.clone()));
                }
            }
            let qs = params
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("&");
            let path = format!("/markets?{qs}");
            let resp: KalshiMarketsResponse = self.get_public(&path).await?;
            all.extend(resp.markets);
            match resp.cursor {
                Some(ref c) if !c.is_empty() => cursor = Some(c.clone()),
                _ => break,
            }
        }

        Ok(all)
    }

    // ------------------------------------------------------------------
    // Public API — Historical Data
    // ------------------------------------------------------------------

    /// Fetch historical trades for a market.
    ///
    /// Returns `(trades, next_cursor)`. Pass `next_cursor` back to continue pagination.
    pub async fn get_trades(
        &self,
        market_ticker: &str,
        min_ts: Option<u64>,
        max_ts: Option<u64>,
        cursor: Option<&str>,
    ) -> Result<(Vec<KalshiTrade>, Option<String>), KalshiHttpError> {
        let mut params = vec![
            ("ticker", market_ticker.to_string()),
            ("limit", "1000".to_string()),
        ];
        if let Some(ts) = min_ts {
            params.push(("min_ts", ts.to_string()));
        }
        if let Some(ts) = max_ts {
            params.push(("max_ts", ts.to_string()));
        }
        if let Some(c) = cursor {
            params.push(("cursor", c.to_string()));
        }
        let qs = params
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        let path = format!("/markets/trades?{qs}");
        let resp: KalshiTradesResponse = self.get_public(&path).await?;
        let next_cursor = resp.cursor.filter(|c| !c.is_empty());
        Ok((resp.trades, next_cursor))
    }

    /// Fetch OHLCV candlesticks for a market.
    ///
    /// `period_interval`: use `CandlestickInterval::Minutes1`, `Hours1`, or `Days1`.
    pub async fn get_candlesticks(
        &self,
        market_ticker: &str,
        start_ts: u64,
        end_ts: u64,
        period_interval: CandlestickInterval,
    ) -> Result<Vec<KalshiCandlestick>, KalshiHttpError> {
        let path = format!(
            "/historical/markets/{market_ticker}/candlesticks?start_ts={start_ts}&end_ts={end_ts}&period_interval={}",
            period_interval.as_minutes()
        );
        let resp: KalshiCandlesticksResponse = self.get_public(&path).await?;
        Ok(resp.candlesticks)
    }

    // ------------------------------------------------------------------
    // Authenticated API — Live Snapshot
    // ------------------------------------------------------------------

    /// Fetch the current orderbook for a market (requires credentials).
    pub async fn get_orderbook(
        &self,
        market_ticker: &str,
        depth: Option<u32>,
    ) -> Result<KalshiOrderbookResponse, KalshiHttpError> {
        let depth_str = depth.unwrap_or(0).to_string();
        let path = format!("/markets/{market_ticker}/orderbook?depth={depth_str}");
        self.get_authenticated(&path).await
    }
}
```

> **Note:** The `nautilus-network` `HttpClient` API may differ from the reqwest-style API shown above. Check how `PolymarketRawHttpClient` uses `HttpClient` in `crates/adapters/polymarket/src/http/client.rs` and adapt the request building accordingly. The core logic (URL construction, pagination, header injection) remains the same.

**Step 6: Run tests**

```bash
cargo test -p nautilus-kalshi --test http
```

Expected: 1 test passes.

**Step 7: Commit**

```bash
git add crates/adapters/kalshi/src/http/ crates/adapters/kalshi/tests/
git commit -m "feat(kalshi): add HTTP client with instrument discovery and historical data"
```

---

### Task 7: WebSocket Messages and Fixtures

**Files:**
- Create: `crates/adapters/kalshi/src/websocket/messages.rs`
- Create: `crates/adapters/kalshi/src/websocket/error.rs`
- Create: `crates/adapters/kalshi/test_data/ws_orderbook_snapshot.json`
- Create: `crates/adapters/kalshi/test_data/ws_orderbook_delta.json`
- Create: `crates/adapters/kalshi/test_data/ws_trade.json`

**Step 1: Create WebSocket test fixtures**

`test_data/ws_orderbook_snapshot.json`:
```json
{
  "type": "orderbook_snapshot",
  "sid": 1,
  "seq": 1,
  "msg": {
    "market_ticker": "KXBTC-25MAR15-B100000",
    "market_id": "9b0f6b43-5b68-4f9f-9f02-9a2d1b8ac1a1",
    "yes_dollars_fp": [["0.3500", "200.00"], ["0.4200", "13.00"]],
    "no_dollars_fp": [["0.5400", "400.00"], ["0.5600", "17.00"]]
  }
}
```

`test_data/ws_orderbook_delta.json`:
```json
{
  "type": "orderbook_delta",
  "sid": 1,
  "seq": 2,
  "msg": {
    "market_ticker": "KXBTC-25MAR15-B100000",
    "price_dollars": "0.4200",
    "delta_fp": "50.00",
    "side": "yes",
    "ts": "2025-03-01T18:26:53.361842Z"
  }
}
```

`test_data/ws_trade.json`:
```json
{
  "type": "trade",
  "sid": 2,
  "seq": 1,
  "msg": {
    "trade_id": "d91bc706-ee49-470d-82d8-11418bda6fed",
    "market_ticker": "KXBTC-25MAR15-B100000",
    "yes_price_dollars": "0.3600",
    "no_price_dollars": "0.6400",
    "count_fp": "136.00",
    "taker_side": "no",
    "ts": 1741052641
  }
}
```

**Step 2: Implement `websocket/error.rs`**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum KalshiWsError {
    #[error("WebSocket connection error: {0}")]
    Connection(String),
    #[error("Sequence gap detected: expected {expected}, got {got}")]
    SequenceGap { expected: u64, got: u64 },
    #[error("JSON parse error: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("Authentication required for WebSocket")]
    NoCredential,
}
```

**Step 3: Write failing message parse test**

```rust
#[test]
fn test_parse_orderbook_snapshot() { /* stub */ }
```

**Step 4: Implement `websocket/messages.rs`**

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

//! WebSocket message types for the Kalshi real-time feed.
//!
//! Kalshi WebSocket messages use a `type` field to distinguish message kinds.
//! Each message also carries `sid` (subscription ID) and `seq` (sequence number).

use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::enums::KalshiTakerSide;

// ---------------------------------------------------------------------------
// Subscription command (client → server)
// ---------------------------------------------------------------------------

/// Command sent to subscribe to a channel.
#[derive(Clone, Debug, Serialize)]
pub struct KalshiSubscribeCmd {
    pub id: u32,
    pub cmd: &'static str,
    pub params: KalshiSubscribeParams,
}

#[derive(Clone, Debug, Serialize)]
pub struct KalshiSubscribeParams {
    pub channels: Vec<String>,
    pub market_tickers: Vec<String>,
}

impl KalshiSubscribeCmd {
    #[must_use]
    pub fn orderbook(id: u32, market_tickers: Vec<String>) -> Self {
        Self {
            id,
            cmd: "subscribe",
            params: KalshiSubscribeParams {
                channels: vec!["orderbook_delta".to_string()],
                market_tickers,
            },
        }
    }

    #[must_use]
    pub fn trades(id: u32, market_tickers: Vec<String>) -> Self {
        Self {
            id,
            cmd: "subscribe",
            params: KalshiSubscribeParams {
                channels: vec!["trade".to_string()],
                market_tickers,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Inbound envelope
// ---------------------------------------------------------------------------

/// Top-level WebSocket message envelope (server → client).
#[derive(Clone, Debug, Deserialize)]
pub struct KalshiWsEnvelope {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub sid: Option<u32>,
    pub seq: Option<u64>,
    pub msg: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Orderbook snapshot
// ---------------------------------------------------------------------------

/// Price level from the WebSocket feed: `(price_dollars, count_fp)`.
pub type WsPriceLevel = (String, String);

/// Orderbook snapshot — first message after subscribing to `orderbook_delta`.
#[derive(Clone, Debug, Deserialize)]
pub struct KalshiWsOrderbookSnapshot {
    pub market_ticker: Ustr,
    /// YES bids sorted ascending by price (best bid is last).
    #[serde(default)]
    pub yes_dollars_fp: Vec<WsPriceLevel>,
    /// NO bids sorted ascending by price (best NO bid is last).
    #[serde(default)]
    pub no_dollars_fp: Vec<WsPriceLevel>,
}

// ---------------------------------------------------------------------------
// Orderbook delta
// ---------------------------------------------------------------------------

/// Orderbook delta — incremental update after a snapshot.
#[derive(Clone, Debug, Deserialize)]
pub struct KalshiWsOrderbookDelta {
    pub market_ticker: Ustr,
    /// Price level being updated (dollar string).
    pub price_dollars: String,
    /// Signed change in quantity. Negative = contracts removed.
    /// `delta_fp == "0.00"` at a price level means that level is removed.
    pub delta_fp: String,
    /// Which side is updated: "yes" or "no".
    pub side: String,
    /// ISO 8601 timestamp.
    pub ts: Option<String>,
}

// ---------------------------------------------------------------------------
// Trade event
// ---------------------------------------------------------------------------

/// A public trade event from the `trade` channel.
#[derive(Clone, Debug, Deserialize)]
pub struct KalshiWsTrade {
    pub trade_id: String,
    pub market_ticker: Ustr,
    pub yes_price_dollars: String,
    pub no_price_dollars: String,
    pub count_fp: String,
    pub taker_side: KalshiTakerSide,
    /// Unix timestamp (seconds).
    pub ts: u64,
}

// ---------------------------------------------------------------------------
// Error message
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize)]
pub struct KalshiWsErrorMsg {
    pub code: u32,
    pub msg: String,
}

// ---------------------------------------------------------------------------
// Parsed message enum
// ---------------------------------------------------------------------------

/// Parsed, typed WebSocket message.
#[derive(Clone, Debug)]
pub enum KalshiWsMessage {
    OrderbookSnapshot { sid: u32, seq: u64, data: KalshiWsOrderbookSnapshot },
    OrderbookDelta { sid: u32, seq: u64, data: KalshiWsOrderbookDelta },
    Trade { sid: u32, seq: u64, data: KalshiWsTrade },
    Error(KalshiWsErrorMsg),
    Unknown(String),
}

impl KalshiWsMessage {
    /// Parse a raw JSON string into a typed `KalshiWsMessage`.
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON is invalid.
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        let env: KalshiWsEnvelope = serde_json::from_str(json)?;
        let sid = env.sid.unwrap_or(0);
        let seq = env.seq.unwrap_or(0);
        let raw_msg = env.msg.unwrap_or(serde_json::Value::Null);

        let msg = match env.msg_type.as_str() {
            "orderbook_snapshot" => {
                let data: KalshiWsOrderbookSnapshot = serde_json::from_value(raw_msg)?;
                Self::OrderbookSnapshot { sid, seq, data }
            }
            "orderbook_delta" => {
                let data: KalshiWsOrderbookDelta = serde_json::from_value(raw_msg)?;
                Self::OrderbookDelta { sid, seq, data }
            }
            "trade" => {
                let data: KalshiWsTrade = serde_json::from_value(raw_msg)?;
                Self::Trade { sid, seq, data }
            }
            "error" => {
                let err: KalshiWsErrorMsg = serde_json::from_value(raw_msg)?;
                Self::Error(err)
            }
            other => Self::Unknown(other.to_string()),
        };

        Ok(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_fixture(name: &str) -> String {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing: {}", path.display()))
    }

    #[test]
    fn test_parse_orderbook_snapshot() {
        let json = load_fixture("ws_orderbook_snapshot.json");
        let msg = KalshiWsMessage::from_json(&json).unwrap();
        match msg {
            KalshiWsMessage::OrderbookSnapshot { sid, seq, data } => {
                assert_eq!(sid, 1);
                assert_eq!(seq, 1);
                assert_eq!(data.market_ticker.as_str(), "KXBTC-25MAR15-B100000");
                assert_eq!(data.yes_dollars_fp.len(), 2);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn test_parse_orderbook_delta() {
        let json = load_fixture("ws_orderbook_delta.json");
        let msg = KalshiWsMessage::from_json(&json).unwrap();
        match msg {
            KalshiWsMessage::OrderbookDelta { sid, seq, data } => {
                assert_eq!(sid, 1);
                assert_eq!(seq, 2);
                assert_eq!(data.price_dollars, "0.4200");
                assert_eq!(data.delta_fp, "50.00");
                assert_eq!(data.side, "yes");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn test_parse_trade() {
        let json = load_fixture("ws_trade.json");
        let msg = KalshiWsMessage::from_json(&json).unwrap();
        match msg {
            KalshiWsMessage::Trade { sid: _, seq, data } => {
                assert_eq!(seq, 1);
                assert_eq!(data.yes_price_dollars, "0.3600");
                assert_eq!(data.taker_side, KalshiTakerSide::No);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
}
```

**Step 5: Run tests**

```bash
cargo test -p nautilus-kalshi websocket::messages
```

Expected: 3 tests pass.

**Step 6: Commit**

```bash
git add crates/adapters/kalshi/src/websocket/ crates/adapters/kalshi/test_data/ws_*.json
git commit -m "feat(kalshi): add WebSocket message types and fixtures"
```

---

### Task 8: WebSocket Handler

**Files:**
- Create: `crates/adapters/kalshi/src/websocket/handler.rs`

**Step 1: Implement `websocket/handler.rs`**

The handler owns per-subscription sequence state, detects gaps, and converts raw Kalshi messages into NautilusTrader data types.

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

//! WebSocket message handler with sequence tracking and orderbook reconstruction.
//!
//! ## Orderbook YES/NO Duality
//!
//! Kalshi only exposes bids. The YES/NO relationship is:
//! - YES bid at price X → occupies the bid side
//! - NO bid at price Y → equivalent to YES ask at `1.00 - Y`
//!
//! This handler converts NO bids into YES asks so NautilusTrader sees a
//! standard bid/ask orderbook on the YES side.

use std::collections::HashMap;

use log::{debug, warn};
use ustr::Ustr;

use crate::websocket::{
    error::KalshiWsError,
    messages::{KalshiWsMessage, KalshiWsOrderbookDelta, KalshiWsOrderbookSnapshot},
};

/// Tracks sequence numbers per subscription to detect gaps.
#[derive(Debug, Default)]
pub struct SequenceTracker {
    /// `sid` → last seen `seq`.
    last_seq: HashMap<u32, u64>,
}

impl SequenceTracker {
    /// Validate a sequence number for a subscription.
    ///
    /// Returns `Ok(())` if in order, `Err(KalshiWsError::SequenceGap)` if a gap is detected.
    pub fn check(&mut self, sid: u32, seq: u64) -> Result<(), KalshiWsError> {
        let entry = self.last_seq.entry(sid).or_insert(0);
        if *entry == 0 {
            // First message on this subscription — accept any seq.
            *entry = seq;
            return Ok(());
        }
        if seq != *entry + 1 {
            return Err(KalshiWsError::SequenceGap {
                expected: *entry + 1,
                got: seq,
            });
        }
        *entry = seq;
        Ok(())
    }

    /// Reset tracking for a subscription (e.g. after re-subscribe).
    pub fn reset(&mut self, sid: u32) {
        self.last_seq.remove(&sid);
    }
}

/// Holds the in-memory orderbook for a single Kalshi market.
///
/// Prices are stored as 4-decimal-place strings to avoid float precision issues.
/// YES bids and derived YES asks (from NO bids) are maintained separately.
#[derive(Debug, Default)]
pub struct KalshiOrderBook {
    /// YES bids: price_str → quantity_str
    pub yes_bids: HashMap<String, String>,
    /// YES asks derived from NO bids: price_str → quantity_str
    pub yes_asks: HashMap<String, String>,
}

impl KalshiOrderBook {
    /// Apply an orderbook snapshot, replacing all existing levels.
    pub fn apply_snapshot(&mut self, snapshot: &KalshiWsOrderbookSnapshot) {
        self.yes_bids.clear();
        self.yes_asks.clear();

        for (price, qty) in &snapshot.yes_dollars_fp {
            self.yes_bids.insert(price.clone(), qty.clone());
        }
        for (no_price, qty) in &snapshot.no_dollars_fp {
            // Convert NO bid at Y to YES ask at (1.0000 - Y).
            if let Some(yes_ask_price) = complement_price(no_price) {
                self.yes_asks.insert(yes_ask_price, qty.clone());
            }
        }
    }

    /// Apply a single delta update.
    pub fn apply_delta(&mut self, delta: &KalshiWsOrderbookDelta) {
        match delta.side.as_str() {
            "yes" => apply_level(&mut self.yes_bids, &delta.price_dollars, &delta.delta_fp),
            "no" => {
                if let Some(yes_ask_price) = complement_price(&delta.price_dollars) {
                    apply_level(&mut self.yes_asks, &yes_ask_price, &delta.delta_fp);
                }
            }
            other => warn!("Kalshi: unknown orderbook side '{other}'"),
        }
    }
}

/// Convert a NO bid price to the equivalent YES ask price: `1.0000 - no_price`.
fn complement_price(no_price: &str) -> Option<String> {
    use std::str::FromStr;
    let p: rust_decimal::Decimal = rust_decimal::Decimal::from_str(no_price).ok()?;
    let one = rust_decimal::Decimal::ONE;
    let ask = one - p;
    // Format to 4 decimal places matching Kalshi's precision.
    Some(format!("{:.4}", ask))
}

/// Apply a quantity delta to a price level map.
///
/// - Positive delta: add to existing quantity (or insert new level).
/// - Negative delta: subtract from quantity.
/// - Zero delta or quantity reaches zero: remove the level.
fn apply_level(levels: &mut HashMap<String, String>, price: &str, delta_fp: &str) {
    use std::str::FromStr;
    let delta = match rust_decimal::Decimal::from_str(delta_fp) {
        Ok(d) => d,
        Err(e) => {
            warn!("Kalshi: invalid delta_fp '{delta_fp}': {e}");
            return;
        }
    };

    if delta == rust_decimal::Decimal::ZERO {
        levels.remove(price);
        return;
    }

    let existing = levels
        .get(price)
        .and_then(|q| rust_decimal::Decimal::from_str(q).ok())
        .unwrap_or(rust_decimal::Decimal::ZERO);

    let new_qty = existing + delta;
    if new_qty <= rust_decimal::Decimal::ZERO {
        levels.remove(price);
    } else {
        levels.insert(price.to_string(), format!("{:.2}", new_qty));
    }
}

/// Top-level message handler: dispatches messages, tracks sequence numbers,
/// and updates in-memory orderbooks.
///
/// On sequence gap, returns `Err(KalshiWsError::SequenceGap)` — the caller
/// should re-subscribe to get a fresh snapshot.
#[derive(Debug, Default)]
pub struct KalshiWsHandler {
    pub seq_tracker: SequenceTracker,
    /// Per-market orderbook state.
    pub books: HashMap<Ustr, KalshiOrderBook>,
}

impl KalshiWsHandler {
    /// Process one raw JSON message from the WebSocket.
    ///
    /// Returns the parsed message on success, or an error on sequence gap or parse failure.
    pub fn handle(&mut self, raw: &str) -> Result<KalshiWsMessage, KalshiWsError> {
        let msg = KalshiWsMessage::from_json(raw)?;

        match &msg {
            KalshiWsMessage::OrderbookSnapshot { sid, seq, data } => {
                // Snapshots reset the sequence for this subscription.
                self.seq_tracker.reset(*sid);
                self.seq_tracker.check(*sid, *seq)?;
                let book = self.books.entry(data.market_ticker).or_default();
                book.apply_snapshot(data);
                debug!("Kalshi: snapshot applied for {}", data.market_ticker);
            }
            KalshiWsMessage::OrderbookDelta { sid, seq, data } => {
                self.seq_tracker.check(*sid, *seq)?;
                if let Some(book) = self.books.get_mut(&data.market_ticker) {
                    book.apply_delta(data);
                } else {
                    warn!("Kalshi: delta before snapshot for {}", data.market_ticker);
                }
            }
            KalshiWsMessage::Trade { sid: _, seq: _, data: _ } => {
                // Trades do not update the orderbook — just pass through.
            }
            KalshiWsMessage::Error(e) => {
                warn!("Kalshi WS error {}: {}", e.code, e.msg);
            }
            KalshiWsMessage::Unknown(t) => {
                debug!("Kalshi: unknown WS message type '{t}'");
            }
        }

        Ok(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_fixture(name: &str) -> String {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing: {name}"))
    }

    #[test]
    fn test_snapshot_populates_book() {
        let mut handler = KalshiWsHandler::default();
        handler.handle(&load_fixture("ws_orderbook_snapshot.json")).unwrap();
        let ticker = Ustr::from("KXBTC-25MAR15-B100000");
        let book = handler.books.get(&ticker).unwrap();
        assert!(book.yes_bids.contains_key("0.4200"));
        // NO bid at 0.5600 → YES ask at 0.4400
        assert!(book.yes_asks.contains_key("0.4400"));
    }

    #[test]
    fn test_delta_updates_book() {
        let mut handler = KalshiWsHandler::default();
        handler.handle(&load_fixture("ws_orderbook_snapshot.json")).unwrap();
        handler.handle(&load_fixture("ws_orderbook_delta.json")).unwrap();
        let ticker = Ustr::from("KXBTC-25MAR15-B100000");
        let book = handler.books.get(&ticker).unwrap();
        // 13.00 + 50.00 = 63.00
        assert_eq!(book.yes_bids.get("0.4200").map(String::as_str), Some("63.00"));
    }

    #[test]
    fn test_sequence_gap_returns_error() {
        let mut tracker = SequenceTracker::default();
        tracker.check(1, 1).unwrap();
        tracker.check(1, 2).unwrap();
        // Gap: skipped seq 3.
        let err = tracker.check(1, 4).unwrap_err();
        assert!(matches!(err, KalshiWsError::SequenceGap { expected: 3, got: 4 }));
    }

    #[test]
    fn test_complement_price() {
        assert_eq!(complement_price("0.5600"), Some("0.4400".to_string()));
        assert_eq!(complement_price("0.5400"), Some("0.4600".to_string()));
    }
}
```

**Step 2: Run tests**

```bash
cargo test -p nautilus-kalshi websocket::handler
```

Expected: 4 tests pass.

**Step 3: Commit**

```bash
git add crates/adapters/kalshi/src/websocket/handler.rs
git commit -m "feat(kalshi): add WebSocket handler with sequence tracking and orderbook reconstruction"
```

---

### Task 9: WebSocket Client

**Files:**
- Create: `crates/adapters/kalshi/src/websocket/client.rs`
- Create: `crates/adapters/kalshi/tests/websocket.rs`

**Step 1: Implement `websocket/client.rs`**

```rust
// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

//! Kalshi WebSocket client for real-time market data.
//!
//! Requires authentication for all channels (orderbook, trades).
//! Authentication headers are sent during the HTTP upgrade handshake.

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use log::{error, info, warn};
use tokio::sync::Mutex;

use crate::{
    common::credential::KalshiCredential,
    websocket::{
        error::KalshiWsError,
        handler::KalshiWsHandler,
        messages::{KalshiSubscribeCmd, KalshiWsMessage},
    },
};

/// WebSocket client for Kalshi real-time market data.
///
/// Maintains a single authenticated connection and handles multiple subscriptions.
#[derive(Debug)]
pub struct KalshiWebSocketClient {
    ws_url: String,
    credential: Arc<KalshiCredential>,
    handler: Arc<Mutex<KalshiWsHandler>>,
    next_cmd_id: AtomicU32,
}

impl KalshiWebSocketClient {
    /// Create a new WebSocket client.
    ///
    /// `credential` is required — all Kalshi WebSocket channels require authentication.
    #[must_use]
    pub fn new(ws_url: String, credential: Arc<KalshiCredential>) -> Self {
        Self {
            ws_url,
            credential,
            handler: Arc::new(Mutex::new(KalshiWsHandler::default())),
            next_cmd_id: AtomicU32::new(1),
        }
    }

    fn next_id(&self) -> u32 {
        self.next_cmd_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Build the authentication headers for the WebSocket upgrade request.
    ///
    /// Signs `GET /trade-api/ws/v2` — the path is always fixed for the WS endpoint.
    fn auth_headers(&self) -> Vec<(String, String)> {
        let ws_path = "/trade-api/ws/v2";
        let (ts, sig) = self.credential.sign("GET", ws_path);
        vec![
            (
                crate::common::credential::HEADER_ACCESS_KEY.to_string(),
                self.credential.api_key_id().to_string(),
            ),
            (crate::common::credential::HEADER_TIMESTAMP.to_string(), ts),
            (crate::common::credential::HEADER_SIGNATURE.to_string(), sig),
        ]
    }

    /// Subscribe to real-time orderbook deltas for the given market tickers.
    ///
    /// The first message received for each market will be an `orderbook_snapshot`,
    /// followed by incremental `orderbook_delta` messages.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established or the
    /// subscription command cannot be sent.
    pub async fn subscribe_orderbook(
        &self,
        market_tickers: Vec<String>,
    ) -> Result<(), KalshiWsError> {
        let cmd = KalshiSubscribeCmd::orderbook(self.next_id(), market_tickers.clone());
        let cmd_json =
            serde_json::to_string(&cmd).map_err(|e| KalshiWsError::Connection(e.to_string()))?;
        info!("Kalshi WS: subscribing orderbook for {market_tickers:?}");
        self.send_command(cmd_json).await
    }

    /// Subscribe to real-time public trade events for the given market tickers.
    pub async fn subscribe_trades(
        &self,
        market_tickers: Vec<String>,
    ) -> Result<(), KalshiWsError> {
        let cmd = KalshiSubscribeCmd::trades(self.next_id(), market_tickers.clone());
        let cmd_json =
            serde_json::to_string(&cmd).map_err(|e| KalshiWsError::Connection(e.to_string()))?;
        info!("Kalshi WS: subscribing trades for {market_tickers:?}");
        self.send_command(cmd_json).await
    }

    /// Internal: send a serialized command JSON over the WebSocket.
    ///
    /// NOTE: The actual WebSocket connection management (connect, reconnect,
    /// message receive loop) should follow the pattern in
    /// `crates/adapters/polymarket/src/websocket/client.rs`, using
    /// `nautilus-network`'s `WebSocketClient`. This stub documents the
    /// interface; the full implementation must adapt to the `nautilus-network` API.
    async fn send_command(&self, _cmd_json: String) -> Result<(), KalshiWsError> {
        // TODO: obtain or reuse active WebSocket connection and send the command.
        // Reference: PolymarketWebSocketClient::subscribe_market / subscribe_user
        // in crates/adapters/polymarket/src/websocket/client.rs
        Ok(())
    }

    /// Process a raw WebSocket text message.
    ///
    /// On sequence gap, logs a warning and returns the gap error so the
    /// caller can re-subscribe.
    pub async fn handle_message(&self, raw: &str) -> Result<KalshiWsMessage, KalshiWsError> {
        let mut handler = self.handler.lock().await;
        match handler.handle(raw) {
            Ok(msg) => Ok(msg),
            Err(KalshiWsError::SequenceGap { expected, got }) => {
                warn!("Kalshi WS: sequence gap (expected {expected}, got {got}) — re-subscribe needed");
                Err(KalshiWsError::SequenceGap { expected, got })
            }
            Err(e) => {
                error!("Kalshi WS: message error: {e}");
                Err(e)
            }
        }
    }
}
```

> **Important implementation note:** The `send_command` stub must be completed by following `PolymarketWebSocketClient` in `crates/adapters/polymarket/src/websocket/client.rs`. Key steps:
> 1. On first subscribe call, establish a WebSocket connection to `self.ws_url` with the `auth_headers()` injected into the HTTP upgrade request.
> 2. Spawn a receive loop that calls `self.handle_message(raw_text)` for each incoming frame.
> 3. On sequence gap error from the handler, close and re-open the connection, then re-subscribe.
> 4. Handle WebSocket Ping frames automatically (nautilus-network likely does this).

**Step 2: Create `tests/websocket.rs`** (basic integration test using mock server)

```rust
//! Integration tests for the Kalshi WebSocket client.

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{
    Router,
    extract::{State, ws::{Message, WebSocket, WebSocketUpgrade}},
    response::Response,
    routing::get,
};
use futures_util::StreamExt;
use nautilus_kalshi::{
    common::credential::KalshiCredential,
    websocket::messages::KalshiWsMessage,
};

fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

fn load_str(filename: &str) -> String {
    std::fs::read_to_string(data_path().join(filename))
        .unwrap_or_else(|_| panic!("missing: {filename}"))
}

#[tokio::test]
async fn test_ws_message_parsing_snapshot() {
    let raw = load_str("ws_orderbook_snapshot.json");
    let msg = KalshiWsMessage::from_json(&raw).unwrap();
    assert!(matches!(msg, KalshiWsMessage::OrderbookSnapshot { .. }));
}

#[tokio::test]
async fn test_ws_message_parsing_trade() {
    let raw = load_str("ws_trade.json");
    let msg = KalshiWsMessage::from_json(&raw).unwrap();
    assert!(matches!(msg, KalshiWsMessage::Trade { .. }));
}
```

**Step 3: Run tests**

```bash
cargo test -p nautilus-kalshi --test websocket
```

Expected: 2 tests pass.

**Step 4: Commit**

```bash
git add crates/adapters/kalshi/src/websocket/client.rs crates/adapters/kalshi/tests/websocket.rs
git commit -m "feat(kalshi): add WebSocket client skeleton"
```

---

### Task 10: PyO3 Registration

**Files:**
- Modify: `crates/pyo3/src/lib.rs`

**Step 1: Find the polymarket registration block**

```bash
grep -n "polymarket" crates/pyo3/src/lib.rs
```

**Step 2: Add kalshi registration immediately after polymarket**

In `crates/pyo3/src/lib.rs`, after the polymarket block (4 lines), add:

```rust
    let n = "kalshi";
    let submodule = pyo3::wrap_pymodule!(nautilus_kalshi::python::kalshi);
    m.add_wrapped(submodule)?;
    sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
    #[cfg(feature = "cython-compat")]
    re_export_module_attributes(m, n)?;
```

Also add the use/import at the top of the file if needed (check if polymarket uses one).

**Step 3: Add `nautilus-kalshi` to `crates/pyo3/Cargo.toml`**

Find where `nautilus-polymarket` is listed and add:
```toml
nautilus-kalshi = { workspace = true, features = ["python"] }
```

**Step 4: Verify build**

```bash
cargo build -p nautilus-pyo3 2>&1 | tail -5
```

Expected: builds cleanly.

**Step 5: Commit**

```bash
git add crates/pyo3/
git commit -m "feat(kalshi): register Kalshi PyO3 module in nautilus-pyo3"
```

---

### Task 11: Python Layer — Config and `__init__`

**Files:**
- Create: `nautilus_trader/adapters/kalshi/__init__.py`
- Create: `nautilus_trader/adapters/kalshi/config.py`

**Step 1: Check polymarket Python adapter for reference**

```bash
ls nautilus_trader/adapters/polymarket/
```

**Step 2: Create `__init__.py`**

```python
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------
```

**Step 3: Create `config.py`**

```python
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import os
from dataclasses import dataclass, field


@dataclass
class KalshiDataClientConfig:
    """
    Configuration for the Kalshi data client.

    Parameters
    ----------
    base_url : str, optional
        REST base URL. Defaults to production (https://api.elections.kalshi.com/trade-api/v2).
    ws_url : str, optional
        WebSocket URL. Defaults to production (wss://api.elections.kalshi.com/trade-api/ws/v2).
    series_tickers : list[str]
        Series tickers to load instruments for, e.g. ``["KXBTC", "PRES-2024"]``.
    event_tickers : list[str]
        Optional event tickers for finer-grained filtering.
    instrument_reload_interval_mins : int
        How often to refresh instruments from the API. Default: 60.
    rate_limit_rps : int
        REST requests per second. Default: 20 (Basic tier).
    api_key_id : str, optional
        Kalshi API key ID. Falls back to ``KALSHI_API_KEY_ID`` env var.
    private_key_pem : str, optional
        RSA private key in PEM format. Falls back to ``KALSHI_PRIVATE_KEY_PEM`` env var.
    """

    base_url: str | None = None
    ws_url: str | None = None
    series_tickers: list[str] = field(default_factory=list)
    event_tickers: list[str] = field(default_factory=list)
    instrument_reload_interval_mins: int = 60
    rate_limit_rps: int = 20
    api_key_id: str | None = None
    private_key_pem: str | None = None

    def resolved_api_key_id(self) -> str | None:
        return self.api_key_id or os.environ.get("KALSHI_API_KEY_ID")

    def resolved_private_key_pem(self) -> str | None:
        return self.private_key_pem or os.environ.get("KALSHI_PRIVATE_KEY_PEM")

    def has_credentials(self) -> bool:
        return bool(self.resolved_api_key_id() and self.resolved_private_key_pem())
```

**Step 4: Commit**

```bash
git add nautilus_trader/adapters/kalshi/
git commit -m "feat(kalshi): add Python config dataclass"
```

---

### Task 12: Python Instrument Provider

**Files:**
- Create: `nautilus_trader/adapters/kalshi/providers.py`

**Step 1: Reference the polymarket instrument provider**

```bash
cat nautilus_trader/adapters/polymarket/providers.py
```

Use it as the template for structure and base class usage.

**Step 2: Write test**

Create `tests/unit_tests/adapters/kalshi/test_providers.py`:

```python
import pytest
from unittest.mock import AsyncMock, MagicMock

from nautilus_trader.adapters.kalshi.config import KalshiDataClientConfig
from nautilus_trader.adapters.kalshi.providers import KalshiInstrumentProvider


@pytest.mark.asyncio
async def test_provider_load_filters_by_series():
    config = KalshiDataClientConfig(series_tickers=["KXBTC"])
    provider = KalshiInstrumentProvider(config=config)
    # Mock the HTTP client
    provider._http_client.get_markets = AsyncMock(return_value=[])
    await provider.load_all_async()
    provider._http_client.get_markets.assert_called_once()
```

**Step 3: Implement `providers.py`**

```python
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import asyncio
from typing import TYPE_CHECKING

from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.nautilus_pyo3 import kalshi  # Rust bindings

from nautilus_trader.adapters.kalshi.config import KalshiDataClientConfig

if TYPE_CHECKING:
    from nautilus_trader.model.instruments import BinaryOption


KALSHI_REST_BASE = "https://api.elections.kalshi.com/trade-api/v2"


class KalshiInstrumentProvider(InstrumentProvider):
    """
    Provides Kalshi prediction market instruments as ``BinaryOption`` objects.

    Instruments are fetched from the Kalshi REST API and filtered by the
    configured series and/or event tickers.

    Parameters
    ----------
    config : KalshiDataClientConfig
        Configuration for the Kalshi adapter.
    """

    def __init__(self, config: KalshiDataClientConfig) -> None:
        super().__init__()
        self._config = config
        self._base_url = config.base_url or KALSHI_REST_BASE
        # The HTTP client is the Rust KalshiHttpClient exposed via PyO3.
        # If the Rust client is not yet wrapped in PyO3, use httpx/aiohttp directly.
        # Reference: nautilus_trader/adapters/polymarket/providers.py for the pattern.
        self._http_client = self._build_http_client()

    def _build_http_client(self):
        # TODO: Return the Rust-backed HTTP client when PyO3 bindings expose it.
        # For now, return a Python HTTP client using httpx.
        try:
            import httpx
            return httpx.AsyncClient(base_url=self._base_url, timeout=60)
        except ImportError:
            raise RuntimeError("httpx is required for KalshiInstrumentProvider")

    async def load_all_async(self, filters: dict | None = None) -> None:
        """Fetch and cache all instruments matching the configured filters."""
        markets = await self._fetch_markets()
        for market in markets:
            try:
                instrument = self._market_to_instrument(market)
                self.add(instrument)
            except Exception as e:
                import logging
                logging.getLogger(__name__).warning(
                    f"Kalshi: failed to parse market {market.get('ticker')}: {e}"
                )

    async def _fetch_markets(self) -> list[dict]:
        """Fetch markets from Kalshi REST API with series/event filtering."""
        params: dict = {"limit": 1000, "status": "active"}
        if self._config.series_tickers:
            params["series_ticker"] = ",".join(self._config.series_tickers)

        markets: list[dict] = []
        cursor: str | None = None

        while True:
            if cursor:
                params["cursor"] = cursor
            resp = await self._http_client.get("/markets", params=params)
            resp.raise_for_status()
            data = resp.json()
            markets.extend(data.get("markets", []))
            cursor = data.get("cursor") or None
            if not cursor:
                break

        # Apply event_ticker filter if specified (client-side).
        if self._config.event_tickers:
            markets = [
                m for m in markets
                if m.get("event_ticker") in self._config.event_tickers
            ]

        return markets

    def _market_to_instrument(self, market: dict) -> BinaryOption:
        """Convert a Kalshi market dict to a NautilusTrader BinaryOption."""
        from nautilus_trader.model.instruments import BinaryOption
        from nautilus_trader.model.identifiers import InstrumentId, Symbol, Venue
        from nautilus_trader.model.objects import Currency, Price, Quantity
        from nautilus_trader.core.datetime import dt_to_unix_nanos
        from datetime import datetime, timezone
        import decimal

        ticker = market["ticker"]
        venue = Venue("KALSHI")
        instrument_id = InstrumentId(Symbol(ticker), venue)

        def parse_ts(s: str | None) -> int:
            if not s:
                return 0
            dt = datetime.fromisoformat(s.replace("Z", "+00:00"))
            return dt_to_unix_nanos(dt)

        return BinaryOption(
            instrument_id=instrument_id,
            raw_symbol=Symbol(ticker),
            currency=Currency.from_str("USD"),
            activation_ns=parse_ts(market.get("open_time")),
            expiration_ns=parse_ts(
                market.get("close_time") or market.get("latest_expiration_time")
            ),
            price_precision=4,
            size_precision=2,
            price_increment=Price.from_str("0.0001"),
            size_increment=Quantity.from_str("0.01"),
            margin_init=decimal.Decimal("0"),
            margin_maint=decimal.Decimal("0"),
            maker_fee=decimal.Decimal("0"),
            taker_fee=decimal.Decimal("0"),
            outcome="Yes",
            description=market.get("title"),
            max_price=Price.from_str("0.9999"),
            min_price=Price.from_str("0.0001"),
            ts_event=0,
            ts_init=0,
        )
```

> **Note:** Check `BinaryOption.__init__` signature in `nautilus_trader/model/instruments/binary_option.pyi` or the Polymarket providers.py for the exact Python constructor parameters.

**Step 4: Commit**

```bash
git add nautilus_trader/adapters/kalshi/providers.py
git commit -m "feat(kalshi): add Python instrument provider"
```

---

### Task 13: Python Data Client and Factories

**Files:**
- Create: `nautilus_trader/adapters/kalshi/data.py`
- Create: `nautilus_trader/adapters/kalshi/factories.py`

**Step 1: Reference polymarket data client**

```bash
cat nautilus_trader/adapters/polymarket/data.py
cat nautilus_trader/adapters/polymarket/factories.py
```

**Step 2: Implement `data.py`**

```python
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

from typing import TYPE_CHECKING

from nautilus_trader.live.data_client import LiveMarketDataClient

from nautilus_trader.adapters.kalshi.config import KalshiDataClientConfig
from nautilus_trader.adapters.kalshi.providers import KalshiInstrumentProvider

if TYPE_CHECKING:
    from nautilus_trader.cache.cache import Cache
    from nautilus_trader.common.component import LiveClock
    from nautilus_trader.common.component import MessageBus


class KalshiDataClient(LiveMarketDataClient):
    """
    Provides a Kalshi market data client for live paper trading and backtesting.

    For backtesting, this client feeds historical REST data into the engine.
    For live paper trading, it uses authenticated WebSocket streams.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop.
    client_id : ClientId
        The data client ID.
    msgbus : MessageBus
        The message bus.
    cache : Cache
        The cache.
    clock : LiveClock
        The clock.
    config : KalshiDataClientConfig
        The adapter configuration.
    instrument_provider : KalshiInstrumentProvider
        The instrument provider.
    """

    def __init__(
        self,
        loop,
        client_id,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        config: KalshiDataClientConfig,
        instrument_provider: KalshiInstrumentProvider,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=client_id,
            venue=None,  # Multi-venue adapter
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )
        self._config = config
        self._instrument_provider = instrument_provider

    async def _connect(self) -> None:
        await self._instrument_provider.load_all_async()

    async def _disconnect(self) -> None:
        pass

    # TODO: implement subscribe_order_book_deltas, subscribe_trade_ticks,
    # subscribe_bars using the Rust WebSocket client and HTTP candlestick client.
    # Reference: nautilus_trader/adapters/polymarket/data.py for subscription patterns.
```

**Step 3: Implement `factories.py`**

```python
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

from functools import lru_cache
from typing import TYPE_CHECKING

from nautilus_trader.adapters.kalshi.config import KalshiDataClientConfig
from nautilus_trader.adapters.kalshi.data import KalshiDataClient
from nautilus_trader.adapters.kalshi.providers import KalshiInstrumentProvider
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.model.identifiers import ClientId

if TYPE_CHECKING:
    from nautilus_trader.cache.cache import Cache
    from nautilus_trader.common.component import LiveClock, MessageBus


class KalshiLiveDataClientFactory(LiveDataClientFactory):
    """
    Factory for creating Kalshi live data clients.
    """

    @staticmethod
    def create(
        loop,
        name: str,
        config: KalshiDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> KalshiDataClient:
        provider = KalshiInstrumentProvider(config=config)
        return KalshiDataClient(
            loop=loop,
            client_id=ClientId(name),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
            instrument_provider=provider,
        )
```

**Step 4: Verify imports work**

```bash
uv run python -c "from nautilus_trader.adapters.kalshi.config import KalshiDataClientConfig; print('OK')"
```

Expected: `OK`

**Step 5: Commit**

```bash
git add nautilus_trader/adapters/kalshi/
git commit -m "feat(kalshi): add Python data client and factory"
```

---

### Task 14: Final Verification

**Step 1: Run full Rust test suite for the crate**

```bash
make cargo-test-crate-kalshi
```

Expected: all tests pass.

**Step 2: Run linter**

```bash
make check-code
```

Fix any Clippy warnings before continuing.

**Step 3: Run Python tests**

```bash
uv run pytest tests/unit_tests/adapters/kalshi/ -v
```

**Step 4: Build the full v2 extension to verify PyO3 integration**

```bash
make build-debug-v2
```

Expected: builds without errors.

**Step 5: Verify Python import**

```bash
uv run python -c "
from nautilus_trader.adapters.kalshi.config import KalshiDataClientConfig
from nautilus_trader.adapters.kalshi.providers import KalshiInstrumentProvider
print('Python layer: OK')
"
```

**Step 6: Final commit**

```bash
git add -A
git commit -m "feat(kalshi): complete data-only MVP adapter"
```

---

## Implementation Notes

### Adapting the HTTP Client to `nautilus-network`

The `KalshiHttpClient.get_public` and `get_authenticated` methods above use a reqwest-style API for clarity. The actual `nautilus-network` `HttpClient` may have a different interface. Follow `PolymarketRawHttpClient` in `crates/adapters/polymarket/src/http/client.rs` exactly for:
- How to construct requests
- How to add headers
- How to handle rate limiting
- How to read response bytes and parse JSON

### RSA Key Generation for Tests

If `aws_lc_rs::rsa::KeyPair::generate` is not available in the workspace's version of `aws-lc-rs`, generate a test key file:

```bash
openssl genrsa 2048 | openssl pkcs8 -topk8 -nocrypt \
  -out crates/adapters/kalshi/test_data/test_rsa_private_key.pem
```

Then load it in tests:
```rust
fn make_test_credential() -> KalshiCredential {
    let pem = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data/test_rsa_private_key.pem")
    ).unwrap();
    KalshiCredential::new("test-key-id".to_string(), &pem).unwrap()
}
```

### BinaryOption Constructor

The exact `BinaryOption::new(...)` signature must be verified against `crates/model/src/instruments/binary_option.rs`. Use the Polymarket adapter's instrument parsing (`http/parse.rs`) as the canonical reference — it uses the same type.

### WebSocket Connection Management

`KalshiWebSocketClient::send_command` is a stub. Complete it by following `PolymarketWebSocketClient` in `crates/adapters/polymarket/src/websocket/client.rs`, which shows:
- How to open a `nautilus-network` WebSocket connection with custom headers
- How to spawn the receive loop
- How to reconnect on drop/gap
