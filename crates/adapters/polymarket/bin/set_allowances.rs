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

//! Sets the Polygon pUSD and CTF approvals required by the Polymarket CLOB.
//!
//! # Usage
//!
//! ```sh
//! POLYMARKET_PK=0x... cargo run -p nautilus-polymarket --bin polymarket-set-allowances
//! ```

use std::str::FromStr;

use alloy::{
    network::{EthereumWallet, ReceiptResponse},
    providers::ProviderBuilder,
    signers::local::PrivateKeySigner,
};
use nautilus_polymarket::{
    common::credential::EvmPrivateKey,
    signing::eip712::{PolymarketApproval, approval_plan},
};

const DEFAULT_POLYGON_RPC_URL: &str = "https://polygon.drpc.org";
const POLYGON_CHAIN_ID: u64 = 137;

alloy::sol! {
    #[sol(rpc)]
    interface Erc20 {
        function approve(address spender, uint256 value) external returns (bool);
    }

    #[sol(rpc)]
    interface Erc1155 {
        function setApprovalForAll(address operator, bool approved) external;
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let private_key =
        std::env::var("POLYMARKET_PK").expect("POLYMARKET_PK environment variable must be set");
    let rpc_url =
        std::env::var("POLYGON_RPC_URL").unwrap_or_else(|_| DEFAULT_POLYGON_RPC_URL.to_string());

    run(&private_key, &rpc_url).await
}

async fn run(private_key: &str, rpc_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let private_key = EvmPrivateKey::new(private_key)?;
    let signer = PrivateKeySigner::from_str(private_key.as_hex())?;
    let wallet = EthereumWallet::from(signer);
    let provider = ProviderBuilder::new()
        .with_chain_id(POLYGON_CHAIN_ID)
        .wallet(wallet)
        .connect_http(rpc_url.parse()?);

    for approval in approval_plan() {
        match approval {
            PolymarketApproval::Collateral {
                contract,
                spender,
                amount,
            } => {
                let collateral = Erc20::new(contract, provider.clone());
                let call = collateral.approve(spender, amount);
                let receipt = call.send().await?.get_receipt().await?;
                receipt.ensure_success()?;
                println!(
                    "Approved pUSD collateral for {spender}: {}",
                    receipt.transaction_hash(),
                );
            }
            PolymarketApproval::ConditionalTokens {
                contract,
                operator,
                approved,
            } => {
                let ctf = Erc1155::new(contract, provider.clone());
                let call = ctf.setApprovalForAll(operator, approved);
                let receipt = call.send().await?.get_receipt().await?;
                receipt.ensure_success()?;
                println!(
                    "Approved CTF tokens for {operator}: {}",
                    receipt.transaction_hash(),
                );
            }
        }
    }

    Ok(())
}
