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
    primitives::{Address, U256, address},
    providers::ProviderBuilder,
    signers::local::PrivateKeySigner,
};
use nautilus_polymarket::{
    common::credential::EvmPrivateKey,
    signing::eip712::{CTF_EXCHANGE, NEG_RISK_CTF_EXCHANGE},
};

const DEFAULT_POLYGON_RPC_URL: &str = "https://polygon.drpc.org";
const POLYGON_CHAIN_ID: u64 = 137;
const PUSD_COLLATERAL: Address = address!("0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB");
const CONDITIONAL_TOKENS: Address = address!("0x4D97DCd97eC945f40cF65F87097ACe5EA0476045");
const NEG_RISK_ADAPTER: Address = address!("0xadA2005600Dec949baf300f4C6120000bDB6eAab");
const APPROVAL_TARGETS: [Address; 3] = [CTF_EXCHANGE, NEG_RISK_CTF_EXCHANGE, NEG_RISK_ADAPTER];

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Approval {
    Collateral { spender: Address, amount: U256 },
    Ctf { operator: Address, approved: bool },
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
    let collateral = Erc20::new(PUSD_COLLATERAL, provider.clone());
    let ctf = Erc1155::new(CONDITIONAL_TOKENS, provider);

    for approval in approval_transactions() {
        match approval {
            Approval::Collateral { spender, amount } => {
                let call = collateral.approve(spender, amount);
                let receipt = call.send().await?.get_receipt().await?;
                receipt.ensure_success()?;
                println!(
                    "Approved pUSD collateral for {spender}: {}",
                    receipt.transaction_hash(),
                );
            }
            Approval::Ctf { operator, approved } => {
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

fn approval_transactions() -> impl Iterator<Item = Approval> {
    APPROVAL_TARGETS.into_iter().flat_map(|target| {
        [
            Approval::Collateral {
                spender: target,
                amount: U256::MAX,
            },
            Approval::Ctf {
                operator: target,
                approved: true,
            },
        ]
    })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn approval_transactions_match_exact_polygon_target_plan() {
        let pusd_collateral = address!("0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB");
        let conditional_tokens = address!("0x4D97DCd97eC945f40cF65F87097ACe5EA0476045");
        let targets = [
            address!("0xE111180000d2663C0091e4f400237545B87B996B"),
            address!("0xe2222d279d744050d28e00520010520000310F59"),
            address!("0xadA2005600Dec949baf300f4C6120000bDB6eAab"),
        ];
        let expected: Vec<_> = targets
            .into_iter()
            .flat_map(|target| {
                [
                    Approval::Collateral {
                        spender: target,
                        amount: U256::MAX,
                    },
                    Approval::Ctf {
                        operator: target,
                        approved: true,
                    },
                ]
            })
            .collect();

        assert_eq!(PUSD_COLLATERAL, pusd_collateral);
        assert_eq!(CONDITIONAL_TOKENS, conditional_tokens);
        assert_eq!(approval_transactions().collect::<Vec<_>>(), expected);
        assert_eq!(DEFAULT_POLYGON_RPC_URL, "https://polygon.drpc.org");
        assert_eq!(POLYGON_CHAIN_ID, 137);
    }
}
