// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Uniswap Universal Router contract bindings for V4 swap execution.
//!
//! The Universal Router is required for all V4 swaps. It uses a command-based
//! architecture where operations are encoded as (commands, inputs) pairs.

use alloy::{
    primitives::{Address, Bytes, U256},
    sol,
    sol_types::SolCall,
};

sol! {
    /// Universal Router interface for executing batched commands.
    #[sol(rpc)]
    contract UniversalRouter {
        /// Execute a batch of commands with provided inputs.
        ///
        /// # Arguments
        /// * `commands` - Encoded command types (1 byte per command)
        /// * `inputs` - ABI-encoded parameters for each command
        /// * `deadline` - Unix timestamp after which the transaction reverts
        function execute(bytes calldata commands, bytes[] calldata inputs, uint256 deadline) external payable;
    }
}

/// Command types supported by Universal Router.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandType {
    /// V4 swap command - executes a V4 swap with encoded actions
    V4Swap = 0x10,
    /// Wrap ETH to WETH
    WrapEth = 0x0b,
    /// Unwrap WETH to ETH
    UnwrapWeth = 0x0c,
    /// Permit2 permit transfer
    Permit2Permit = 0x0a,
    /// Permit2 transfer from
    Permit2TransferFrom = 0x02,
}

/// V4 Actions for the V4Planner.
/// These are encoded as single bytes in the actions string.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V4Action {
    // Liquidity actions
    IncreaseLiquidity = 0x00,
    DecreaseLiquidity = 0x01,
    MintPosition = 0x02,
    BurnPosition = 0x03,

    // Swap actions
    SwapExactInSingle = 0x06,
    SwapExactIn = 0x07,
    SwapExactOutSingle = 0x08,
    SwapExactOut = 0x09,

    // Settlement actions
    Settle = 0x0b,
    SettleAll = 0x0c,
    SettlePair = 0x0d,

    // Take actions
    Take = 0x0e,
    TakeAll = 0x0f,
    TakePortion = 0x10,
    TakePair = 0x11,

    CloseCurrency = 0x12,
    Sweep = 0x14,
    Unwrap = 0x16,
}

/// PoolKey identifies a V4 pool.
#[derive(Debug, Clone)]
pub struct PoolKey {
    pub currency0: Address,
    pub currency1: Address,
    pub fee: u32,
    pub tick_spacing: i32,
    pub hooks: Address,
}

impl PoolKey {
    /// Create a new PoolKey with tokens sorted correctly.
    #[must_use]
    pub fn new(
        token_a: Address,
        token_b: Address,
        fee: u32,
        tick_spacing: i32,
        hooks: Address,
    ) -> Self {
        let (currency0, currency1) = if token_a < token_b {
            (token_a, token_b)
        } else {
            (token_b, token_a)
        };
        Self {
            currency0,
            currency1,
            fee,
            tick_spacing,
            hooks,
        }
    }

    /// ABI-encode the PoolKey as a tuple.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        use alloy::sol_types::SolValue;

        (
            self.currency0,
            self.currency1,
            self.fee,
            self.tick_spacing,
            self.hooks,
        )
            .abi_encode()
    }
}

/// Parameters for a single-hop exact input swap.
#[derive(Debug, Clone)]
pub struct SwapExactInSingleParams {
    pub pool_key: PoolKey,
    pub zero_for_one: bool,
    pub amount_in: u128,
    pub amount_out_minimum: u128,
    pub hook_data: Bytes,
}

impl SwapExactInSingleParams {
    /// ABI-encode the swap parameters.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        use alloy::sol_types::SolValue;

        // Encode as tuple matching the Solidity struct
        (
            (
                self.pool_key.currency0,
                self.pool_key.currency1,
                self.pool_key.fee,
                self.pool_key.tick_spacing,
                self.pool_key.hooks,
            ),
            self.zero_for_one,
            self.amount_in,
            self.amount_out_minimum,
            self.hook_data.clone(),
        )
            .abi_encode()
    }
}

/// V4Planner builds a sequence of actions for V4 swaps.
///
/// Actions are encoded as a bytes string where each byte is an action type,
/// and params is a vector of ABI-encoded parameters for each action.
///
/// Based on Uniswap V4 SDK pattern:
/// 1. `SWAP_EXACT_IN_SINGLE` - Perform the swap
/// 2. `SETTLE_ALL` - Pay all input tokens
/// 3. `TAKE_ALL` - Collect all output tokens
#[derive(Debug, Default)]
pub struct V4Planner {
    /// Action bytes (one byte per action)
    pub actions: Vec<u8>,
    /// ABI-encoded parameters for each action
    pub params: Vec<Vec<u8>>,
}

impl V4Planner {
    /// Create a new empty V4Planner.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a SWAP_EXACT_IN_SINGLE action.
    pub fn add_swap_exact_in_single(&mut self, params: SwapExactInSingleParams) -> &mut Self {
        self.actions.push(V4Action::SwapExactInSingle as u8);
        self.params.push(params.encode());
        self
    }

    /// Add a SETTLE action to pay input tokens.
    ///
    /// # Arguments
    /// * `currency` - Token address to settle
    /// * `amount` - Amount to settle (0 for open delta)
    /// * `payer_is_user` - Whether the payer is the user (vs router)
    pub fn add_settle(
        &mut self,
        currency: Address,
        amount: U256,
        payer_is_user: bool,
    ) -> &mut Self {
        use alloy::sol_types::SolValue;

        self.actions.push(V4Action::Settle as u8);
        self.params
            .push((currency, amount, payer_is_user).abi_encode());
        self
    }

    /// Add a SETTLE_ALL action to pay all input tokens.
    ///
    /// This is the recommended action for simple swaps per Uniswap SDK.
    ///
    /// # Arguments
    /// * `currency` - Token address to settle
    /// * `max_amount` - Maximum amount to settle
    pub fn add_settle_all(&mut self, currency: Address, max_amount: U256) -> &mut Self {
        use alloy::sol_types::SolValue;

        self.actions.push(V4Action::SettleAll as u8);
        self.params.push((currency, max_amount).abi_encode());
        self
    }

    /// Add a TAKE action to receive output tokens.
    ///
    /// # Arguments
    /// * `currency` - Token address to take
    /// * `recipient` - Address to receive tokens
    /// * `amount` - Amount to take (0 for open delta)
    pub fn add_take(&mut self, currency: Address, recipient: Address, amount: U256) -> &mut Self {
        use alloy::sol_types::SolValue;

        self.actions.push(V4Action::Take as u8);
        self.params.push((currency, recipient, amount).abi_encode());
        self
    }

    /// Add a TAKE_ALL action to collect all output tokens.
    ///
    /// This is the recommended action for simple swaps per Uniswap SDK.
    ///
    /// # Arguments
    /// * `currency` - Token address to take
    /// * `min_amount` - Minimum amount to take (slippage protection)
    pub fn add_take_all(&mut self, currency: Address, min_amount: U256) -> &mut Self {
        use alloy::sol_types::SolValue;

        self.actions.push(V4Action::TakeAll as u8);
        self.params.push((currency, min_amount).abi_encode());
        self
    }

    /// Get actions as bytes.
    #[must_use]
    pub fn actions_bytes(&self) -> Bytes {
        Bytes::from(self.actions.clone())
    }

    /// Get params as bytes array.
    #[must_use]
    pub fn params_bytes(&self) -> Vec<Bytes> {
        self.params.iter().map(|p| Bytes::from(p.clone())).collect()
    }

    /// Finalize and encode all actions for the Universal Router.
    ///
    /// Returns ABI-encoded (actions_bytes, params_array).
    #[must_use]
    pub fn finalize(&self) -> Bytes {
        use alloy::sol_types::SolValue;

        let actions_bytes = Bytes::from(self.actions.clone());
        let params_bytes: Vec<Bytes> = self.params.iter().map(|p| Bytes::from(p.clone())).collect();

        Bytes::from((actions_bytes, params_bytes).abi_encode())
    }
}

/// Route Planner for Universal Router commands.
#[derive(Debug, Default)]
pub struct RoutePlanner {
    commands: Vec<u8>,
    inputs: Vec<Bytes>,
}

impl RoutePlanner {
    /// Create a new empty RoutePlanner.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a V4_SWAP command with encoded V4Planner actions.
    pub fn add_v4_swap(&mut self, v4_planner: &V4Planner) -> &mut Self {
        self.commands.push(CommandType::V4Swap as u8);
        self.inputs.push(v4_planner.finalize());
        self
    }

    /// Get the commands bytes.
    #[must_use]
    pub fn commands(&self) -> Bytes {
        Bytes::from(self.commands.clone())
    }

    /// Get the inputs array.
    #[must_use]
    pub fn inputs(&self) -> Vec<Bytes> {
        self.inputs.clone()
    }

    /// Encode the execute call for Universal Router.
    #[must_use]
    pub fn encode_execute(&self, deadline: U256) -> Bytes {
        let call = UniversalRouter::executeCall {
            commands: self.commands(),
            inputs: self.inputs(),
            deadline,
        };
        Bytes::from(call.abi_encode())
    }
}

/// Universal Router deployment addresses by chain.
/// Reference: https://docs.uniswap.org/contracts/v4/deployments
pub mod deployments {
    use alloy::primitives::{Address, address};

    /// Ethereum Mainnet Universal Router (supports V4)
    pub const ETHEREUM: Address = address!("66a9893cc07d91d95644aedd05d03f95e1dba8af");
    /// Optimism Universal Router
    pub const OPTIMISM: Address = address!("851116d9223fabed8e56c0e6b8ad0c31d98b3507");
    /// BNB Smart Chain Universal Router
    pub const BNB: Address = address!("1906c1d672b88cd1b9ac7593301ca990f94eae07");
    /// Unichain Universal Router
    pub const UNICHAIN: Address = address!("ef740bf23acae26f6492b10de645d6b98dc8eaf3");
    /// Polygon Universal Router
    pub const POLYGON: Address = address!("1095692a6237d83c6a72f3f5efedb9a670c49223");
    /// Worldchain Universal Router
    pub const WORLDCHAIN: Address = address!("8ac7bee993bb44dab564ea4bc9ea67bf9eb5e743");
    /// Soneium Universal Router
    pub const SONEIUM: Address = address!("4cded7edf52c8aa5259a54ec6a3ce7c6d2a455df");
    /// Zora Universal Router
    pub const ZORA: Address = address!("3315ef7ca28db74abadc6c44570efdf06b04b020");
    /// Base Universal Router
    pub const BASE: Address = address!("6ff5693b99212da76ad316178a184ab56d299b43");
    /// Arbitrum One Universal Router
    pub const ARBITRUM: Address = address!("a51afafe0263b40edaef0df8781ea9aa03e381a3");
    /// Celo Universal Router
    pub const CELO: Address = address!("cb695bc5d3aa22cad1e6df07801b061a05a0233a");
    /// Avalanche Universal Router
    pub const AVALANCHE: Address = address!("94b75331ae8d42c1b61065089b7d48fe14aa73b7");
    /// Ink Universal Router
    pub const INK: Address = address!("112908dac86e20e7241b0927479ea3bf935d1fa0");
    /// Blast Universal Router
    pub const BLAST: Address = address!("eabbcb3e8e415306207ef514f660a3f820025be3");

    /// Get Universal Router address for a chain ID.
    #[must_use]
    pub fn get_router_address(chain_id: u64) -> Option<Address> {
        match chain_id {
            // Mainnets
            1 => Some(ETHEREUM),
            10 => Some(OPTIMISM),
            56 => Some(BNB),
            130 => Some(UNICHAIN),
            137 => Some(POLYGON),
            480 => Some(WORLDCHAIN),
            1868 => Some(SONEIUM),
            7777777 => Some(ZORA),
            8453 => Some(BASE),
            42161 => Some(ARBITRUM),
            42220 => Some(CELO),
            43114 => Some(AVALANCHE),
            57073 => Some(INK),
            81457 => Some(BLAST),
            // Testnets
            1301 => Some(address!("f70536b3bcc1bd1a972dc186a2cf84cc6da6be5d")), // Unichain Sepolia
            11155111 => Some(address!("3A9D48AB9751398BbFa63ad67599Bb04e4BdF98b")), // Sepolia
            84532 => Some(address!("492e6456d9528771018deb9e87ef7750ef184104")), // Base Sepolia
            421614 => Some(address!("efd1d4bd4cf1e86da286bb4cb1b8bced9c10ba47")), // Arbitrum Sepolia
            _ => None,
        }
    }
}

/// Permit2 contract addresses.
pub mod permit2 {
    use alloy::primitives::{Address, address};

    /// Permit2 is deployed at the same address on all chains.
    pub const PERMIT2: Address = address!("000000000022D473030F116dDEE9F6B43aC78BA3");
}

#[cfg(test)]
mod tests {
    use alloy::primitives::address;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_pool_key_sorting() {
        let token_a = address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"); // USDC
        let token_b = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"); // WETH

        let pool_key = PoolKey::new(token_b, token_a, 500, 10, Address::ZERO);

        // Should be sorted: USDC < WETH
        assert!(pool_key.currency0 < pool_key.currency1);
    }

    #[rstest]
    fn test_v4_planner_actions() {
        let mut planner = V4Planner::new();

        let pool_key = PoolKey::new(
            address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
            500,
            10,
            Address::ZERO,
        );

        planner.add_swap_exact_in_single(SwapExactInSingleParams {
            pool_key,
            zero_for_one: true,
            amount_in: 1_000_000, // 1 USDC
            amount_out_minimum: 0,
            hook_data: Bytes::new(),
        });

        assert_eq!(planner.actions.len(), 1);
        assert_eq!(planner.actions[0], V4Action::SwapExactInSingle as u8);
    }

    #[rstest]
    fn test_route_planner() {
        let mut v4_planner = V4Planner::new();
        let recipient = address!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

        let usdc = address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
        let weth = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");

        let pool_key = PoolKey::new(usdc, weth, 500, 10, Address::ZERO);

        v4_planner
            .add_swap_exact_in_single(SwapExactInSingleParams {
                pool_key,
                zero_for_one: true,
                amount_in: 1_000_000,
                amount_out_minimum: 0,
                hook_data: Bytes::new(),
            })
            .add_settle(usdc, U256::from(1_000_000), true)
            .add_take(weth, recipient, U256::ZERO);

        let mut route_planner = RoutePlanner::new();
        route_planner.add_v4_swap(&v4_planner);

        assert_eq!(route_planner.commands.len(), 1);
        assert_eq!(route_planner.commands[0], CommandType::V4Swap as u8);
    }
}
