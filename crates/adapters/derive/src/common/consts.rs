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

//! Static constants for the Derive adapter.
//!
//! Protocol constants (`DOMAIN_SEPARATOR`, `ACTION_TYPEHASH`, per-action module
//! addresses) are sourced from the "Protocol Constants" reference at
//! <https://docs.derive.xyz/reference/protocol-constants>. Both mainnet and
//! testnet values are populated; per-instance overrides on
//! [`crate::config::DeriveExecClientConfig`] take precedence.

use std::{sync::LazyLock, time::Duration};

use nautilus_model::identifiers::{ClientId, Venue};
use ustr::Ustr;

use crate::common::enums::DeriveEnvironment;

/// Venue identifier string.
pub const DERIVE: &str = "DERIVE";

/// Static venue instance.
pub static DERIVE_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(DERIVE)));

/// Static client ID instance.
pub static DERIVE_CLIENT_ID: LazyLock<ClientId> =
    LazyLock::new(|| ClientId::new(Ustr::from(DERIVE)));

/// Referral code for order attribution (zero additional fees),
/// see <https://docs.derive.xyz/reference/api-broker> for further details.
pub const DERIVE_NAUTILUS_REFERRAL_CODE: &str = "nautilus";

pub const REST_URL_MAINNET: &str = "https://api.lyra.finance";
pub const REST_URL_TESTNET: &str = "https://api-demo.lyra.finance";
pub const WS_URL_MAINNET: &str = "wss://api.lyra.finance/ws";
pub const WS_URL_TESTNET: &str = "wss://api-demo.lyra.finance/ws";

pub const DERIVE_TRADES_PAGE_SIZE: u32 = 1000;
pub const DERIVE_CANDLES_DEFAULT_LIMIT: usize = 1000;
pub const DERIVE_CANDLES_MAX_PAGES: usize = 100;

/// EIP-712 domain separator (mainnet).
pub const DOMAIN_SEPARATOR_MAINNET: &str =
    "0xd96e5f90797da7ec8dc4e276260c7f3f87fedf68775fbe1ef116e996fc60441b";

/// EIP-712 domain separator (testnet).
pub const DOMAIN_SEPARATOR_TESTNET: &str =
    "0x9bcf4dc06df5d8bf23af818d5716491b995020f377d3b7b64c29ed14e3dd1105";

/// EIP-712 action typehash. Identical across networks per Derive's published
/// Protocol Constants.
pub const ACTION_TYPEHASH: &str =
    "0x4d7a9f27c403ff9c0f19bce61d76d82f9aa29f8d6d4b0c5474607d9770d1af17";

/// Trade module contract address (mainnet).
pub const TRADE_MODULE_ADDRESS_MAINNET: &str = "0xB8D20c2B7a1Ad2EE33Bc50eF10876eD3035b5e7b";

/// Trade module contract address (testnet).
pub const TRADE_MODULE_ADDRESS_TESTNET: &str = "0x87F2863866D85E3192a35A73b388BD625D83f2be";

/// Withdrawal module contract address (mainnet).
pub const WITHDRAW_MODULE_ADDRESS_MAINNET: &str = "0x9d0E8f5b25384C7310CB8C6aE32C8fbeb645d083";

/// Withdrawal module contract address (testnet).
pub const WITHDRAW_MODULE_ADDRESS_TESTNET: &str = "0xe850641C5207dc5E9423fB15f89ae6031A05fd92";

/// Transfer module contract address (mainnet).
pub const TRANSFER_MODULE_ADDRESS_MAINNET: &str = "0x01259207A40925b794C8ac320456F7F6c8FE2636";

/// Transfer module contract address (testnet).
pub const TRANSFER_MODULE_ADDRESS_TESTNET: &str = "0x0CFC1a4a90741aB242cAfaCD798b409E12e68926";

/// Deposit module contract address (mainnet).
pub const DEPOSIT_MODULE_ADDRESS_MAINNET: &str = "0x9B3FE5E5a3bcEa5df4E08c41Ce89C4e3Ff01Ace3";

/// Deposit module contract address (testnet).
pub const DEPOSIT_MODULE_ADDRESS_TESTNET: &str = "0x43223Db33AdA0575D2E100829543f8B04A37a1ec";

/// Returns the EIP-712 domain separator constant for the configured environment.
#[must_use]
pub const fn domain_separator_for(environment: DeriveEnvironment) -> &'static str {
    match environment {
        DeriveEnvironment::Mainnet => DOMAIN_SEPARATOR_MAINNET,
        DeriveEnvironment::Testnet => DOMAIN_SEPARATOR_TESTNET,
    }
}

/// Returns the Trade module contract address constant for the configured environment.
#[must_use]
pub const fn trade_module_address_for(environment: DeriveEnvironment) -> &'static str {
    match environment {
        DeriveEnvironment::Mainnet => TRADE_MODULE_ADDRESS_MAINNET,
        DeriveEnvironment::Testnet => TRADE_MODULE_ADDRESS_TESTNET,
    }
}

/// REST authentication header carrying the Derive Chain smart-contract wallet.
pub const HEADER_LYRA_WALLET: &str = "X-LYRAWALLET";

/// REST authentication header carrying the signed timestamp.
pub const HEADER_LYRA_TIMESTAMP: &str = "X-LYRATIMESTAMP";

/// REST authentication header carrying the session-key signature.
pub const HEADER_LYRA_SIGNATURE: &str = "X-LYRASIGNATURE";

/// Minimum signature TTL the venue accepts for self-custodial actions.
///
/// Per the v2 onboarding docs, `signature_expiry_sec` must be at least five
/// minutes in the future of `now`.
pub const MIN_SIGNATURE_TTL: Duration = Duration::from_secs(5 * 60);

/// Fixed-point scale used by all on-chain decimal fields (1e18).
pub const DECIMAL_SCALE: u128 = 1_000_000_000_000_000_000;

pub const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Default timeout for blocking on account registration during execution
/// client `connect()`. Matches the BitMEX / Bybit / OKX adapters' 30-second
/// budget.
pub const DERIVE_ACCOUNT_REGISTRATION_TIMEOUT_SECS: f64 = 30.0;

pub const WS_HEARTBEAT_SECS: u64 = 30;

pub const RECONNECT_BASE_BACKOFF: Duration = Duration::from_millis(250);
pub const RECONNECT_MAX_BACKOFF: Duration = Duration::from_secs(30);
pub const RECONNECT_JITTER_MS: u64 = 200;
pub const RECONNECT_BACKOFF_FACTOR: f64 = 2.0;
pub const RECONNECT_TIMEOUT: Duration = Duration::from_secs(15);

pub const WS_DISCONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_venue_constant() {
        assert_eq!(DERIVE_VENUE.as_str(), DERIVE);
    }

    #[rstest]
    fn test_url_constants_have_expected_schemes() {
        assert!(REST_URL_MAINNET.starts_with("https://"));
        assert!(REST_URL_TESTNET.starts_with("https://"));
        assert!(WS_URL_MAINNET.starts_with("wss://"));
        assert!(WS_URL_TESTNET.starts_with("wss://"));
    }

    #[rstest]
    fn test_decimal_scale_is_1e18() {
        assert_eq!(DECIMAL_SCALE, 10u128.pow(18));
    }

    #[rstest]
    fn test_min_signature_ttl_is_five_minutes() {
        assert_eq!(MIN_SIGNATURE_TTL, Duration::from_secs(300));
    }

    #[rstest]
    fn test_domain_separator_for_routes_per_environment() {
        assert_eq!(
            domain_separator_for(DeriveEnvironment::Mainnet),
            DOMAIN_SEPARATOR_MAINNET,
        );
        assert_eq!(
            domain_separator_for(DeriveEnvironment::Testnet),
            DOMAIN_SEPARATOR_TESTNET,
        );
    }

    #[rstest]
    fn test_trade_module_address_for_routes_per_environment() {
        assert_eq!(
            trade_module_address_for(DeriveEnvironment::Mainnet),
            TRADE_MODULE_ADDRESS_MAINNET,
        );
        assert_eq!(
            trade_module_address_for(DeriveEnvironment::Testnet),
            TRADE_MODULE_ADDRESS_TESTNET,
        );
    }
}
