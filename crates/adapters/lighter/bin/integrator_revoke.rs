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

//! Revoke the Nautilus integrator approval when leaving the adapter.
//!
//! This cleanup utility is not a trading-mode toggle. Live trading through this
//! adapter requires the approval; the next execution-client startup records a
//! fresh zero-fee approval.
//!
//! Submits a signed `ApproveIntegrator` tx with `approval_expiry = 0` and all
//! max fees `= 0`.
//!
//! See the Lighter integration guide:
//! <https://nautilustrader.io/docs/nightly/integrations/lighter.html#integrator-attribution>.
//!
//! Environment selection: pass `testnet` as the first argv to target
//! testnet; defaults to mainnet.
//!
//! Environment variables:
//! - `LIGHTER_API_KEY_INDEX`, `LIGHTER_API_SECRET`, `LIGHTER_ACCOUNT_INDEX`
//!   (or `LIGHTER_TESTNET_*` variants): the L2 Schnorr trading credentials.
//!
//! Run with:
//!
//! ```bash
//! cargo run -p nautilus-lighter --bin lighter-integrator-revoke           # mainnet
//! cargo run -p nautilus-lighter --bin lighter-integrator-revoke testnet   # testnet
//! ```

use std::{
    env,
    io::{BufRead, Write},
    time::{SystemTime, UNIX_EPOCH},
};

use nautilus_lighter::{
    common::{
        consts::{LIGHTER_INTEGRATOR_APPROVAL_DOCS_URL, LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX},
        credential::Credential,
        enums::LighterEnvironment,
        urls::lighter_chain_id,
    },
    http::{
        client::{LighterHttpClient, LighterRawHttpClient},
        models::LighterSendTxRequest,
    },
    signing::{
        auth_token::fresh_k,
        tx::{ApproveIntegratorTxInfo, LighterTx, TxContext, TxInfoJson, sign_tx},
    },
};

const TX_EXPIRY_MS: i64 = 5 * 60 * 1_000;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let args: Vec<String> = env::args().collect();
    let environment = if args.get(1).is_some_and(|s| s == "testnet") {
        LighterEnvironment::Testnet
    } else {
        LighterEnvironment::Mainnet
    };

    let credential = Credential::resolve(None, None, None, environment)?
        .ok_or_else(|| anyhow::anyhow!("no Lighter L2 credentials in env"))?;
    let chain_id = lighter_chain_id(environment);

    println!();
    println!("Lighter integrator revocation");
    println!("  environment      : {environment:?}");
    println!("  chain_id         : {chain_id}");
    println!("  account_index    : {}", credential.account_index());
    println!("  api_key_index    : {}", credential.api_key_index());
    println!("  integrator       : {LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX}");
    println!("  approval_expiry  : 0  (revoke marker)");
    println!("  max fees         : 0  (revoke marker)");
    println!();
    println!("Use this cleanup utility only when leaving the Lighter adapter.");
    println!("Tagged orders will fail until approval is recorded again.");
    println!("Docs: {LIGHTER_INTEGRATOR_APPROVAL_DOCS_URL}");
    print!("\nProceed? [Enter to continue, Ctrl+C to abort] ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;

    let raw = LighterRawHttpClient::new(environment, None, 30, None)?;
    let http = LighterHttpClient::from_raw(raw);
    let next_nonce = http
        .get_next_nonce(credential.account_index(), credential.api_key_index())
        .await?
        .nonce;
    log::info!("Bootstrapped venue next_nonce={next_nonce}");

    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
    let tx = ApproveIntegratorTxInfo {
        context: TxContext {
            account_index: credential.account_index(),
            api_key_index: credential.api_key_index(),
            nonce: next_nonce,
            expired_at: now_ms.saturating_add(TX_EXPIRY_MS),
        },
        integrator_account_index: LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX as i64,
        max_perps_taker_fee: 0,
        max_perps_maker_fee: 0,
        max_spot_taker_fee: 0,
        max_spot_maker_fee: 0,
        approval_expiry: 0,
        skip_nonce: 0,
    };

    let l2_signed = sign_tx(&tx, chain_id, &credential.private_key()?, fresh_k());
    let tx_info_str = TxInfoJson::approve_integrator(&tx, &l2_signed, "");
    let request = LighterSendTxRequest::new(tx.tx_type() as u8, tx_info_str);
    log::info!(
        "Dispatching ApproveIntegrator revoke (tx_type={})",
        tx.tx_type() as u8
    );
    let response = http.send_tx(&request).await?;
    log::info!(
        "RevokeIntegrator: send_tx submitted tx_hash={}",
        response.tx_hash
    );
    log::info!("=== Done ===");
    Ok(())
}
