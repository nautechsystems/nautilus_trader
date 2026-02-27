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

//! Approve the Nautilus builder fee for Hyperliquid trading.
//!
//! This is a ONE-TIME setup step required before trading on Hyperliquid.
//!
//! What you are approving:
//! - 1 bp (0.01%) approval ceiling (perpetuals only, no fee on spot)
//! - Taker: 1.0 bp (0.01%) base on perpetual fills, scales down with volume
//! - Maker: 0.4 bp (0.004%) base on perpetual post-only fills, scales down with volume
//!
//! Hyperliquid allows builders to charge up to 10 bp on perps and 100 bp on spot.
//!
//! The script will display full details and prompt for confirmation before proceeding.
//! Use --yes to skip the confirmation prompt.
//!
//! Prerequisites:
//! - Set environment variable: HYPERLIQUID_PK (mainnet) or HYPERLIQUID_TESTNET_PK (testnet)
//!
//! Usage:
//!     # Mainnet (interactive)
//!     cargo run --bin hyperliquid-builder-fee-approve
//!
//!     # Mainnet (non-interactive)
//!     cargo run --bin hyperliquid-builder-fee-approve -- --yes
//!
//!     # Testnet
//!     HYPERLIQUID_TESTNET=true cargo run --bin hyperliquid-builder-fee-approve

use nautilus_hyperliquid::common::builder_fee;

#[tokio::main]
async fn main() {
    let non_interactive = std::env::args().any(|arg| arg == "--yes" || arg == "-y");
    let success = builder_fee::approve_from_env(non_interactive).await;
    if !success {
        std::process::exit(1);
    }
}
