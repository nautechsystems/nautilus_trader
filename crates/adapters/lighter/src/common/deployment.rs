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

//! Resolved protocol settings for supported Lighter deployments.

use nautilus_model::{identifiers::Venue, types::Currency};

use super::{
    consts::{
        LIGHTER_MAINNET_CHAIN_ID, LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX,
        LIGHTER_ROBINHOOD_CHAIN_ID, LIGHTER_ROBINHOOD_VENUE, LIGHTER_TESTNET_CHAIN_ID,
        LIGHTER_VENUE,
    },
    enums::{LighterDeployment, LighterEnvironment},
};

const LIGHTER_MAINNET_HTTP_URL: &str = "https://mainnet.zklighter.elliot.ai";
const LIGHTER_MAINNET_WS_URL: &str = "wss://mainnet.zklighter.elliot.ai/stream";
const LIGHTER_TESTNET_HTTP_URL: &str = "https://testnet.zklighter.elliot.ai";
const LIGHTER_TESTNET_WS_URL: &str = "wss://testnet.zklighter.elliot.ai/stream";
const ROBINHOOD_MAINNET_HTTP_URL: &str = "https://api.rh.lighter.xyz";
const ROBINHOOD_MAINNET_WS_URL: &str = "wss://api.rh.lighter.xyz/stream";
const ROBINHOOD_TESTNET_HTTP_URL: &str = "https://api.rh-testnet.lighter.xyz";
const ROBINHOOD_TESTNET_WS_URL: &str = "wss://api.rh-testnet.lighter.xyz/stream";
const LIGHTER_NAUTILUS_REFERRAL_CODE: &str = "NAUTILUS";

pub(crate) const fn http_base_url(
    deployment: LighterDeployment,
    environment: LighterEnvironment,
) -> &'static str {
    match (deployment, environment) {
        (LighterDeployment::Lighter, LighterEnvironment::Mainnet) => LIGHTER_MAINNET_HTTP_URL,
        (LighterDeployment::Lighter, LighterEnvironment::Testnet) => LIGHTER_TESTNET_HTTP_URL,
        (LighterDeployment::Robinhood, LighterEnvironment::Mainnet) => ROBINHOOD_MAINNET_HTTP_URL,
        (LighterDeployment::Robinhood, LighterEnvironment::Testnet) => ROBINHOOD_TESTNET_HTTP_URL,
    }
}

pub(crate) const fn ws_url(
    deployment: LighterDeployment,
    environment: LighterEnvironment,
) -> &'static str {
    match (deployment, environment) {
        (LighterDeployment::Lighter, LighterEnvironment::Mainnet) => LIGHTER_MAINNET_WS_URL,
        (LighterDeployment::Lighter, LighterEnvironment::Testnet) => LIGHTER_TESTNET_WS_URL,
        (LighterDeployment::Robinhood, LighterEnvironment::Mainnet) => ROBINHOOD_MAINNET_WS_URL,
        (LighterDeployment::Robinhood, LighterEnvironment::Testnet) => ROBINHOOD_TESTNET_WS_URL,
    }
}

pub(crate) const fn chain_id(
    deployment: LighterDeployment,
    environment: LighterEnvironment,
) -> u32 {
    match (deployment, environment) {
        (LighterDeployment::Lighter, LighterEnvironment::Mainnet) => LIGHTER_MAINNET_CHAIN_ID,
        (
            LighterDeployment::Lighter | LighterDeployment::Robinhood,
            LighterEnvironment::Testnet,
        ) => LIGHTER_TESTNET_CHAIN_ID,
        (LighterDeployment::Robinhood, LighterEnvironment::Mainnet) => LIGHTER_ROBINHOOD_CHAIN_ID,
    }
}

pub(crate) fn venue(deployment: LighterDeployment) -> Venue {
    match deployment {
        LighterDeployment::Lighter => *LIGHTER_VENUE,
        LighterDeployment::Robinhood => *LIGHTER_ROBINHOOD_VENUE,
    }
}

pub(crate) fn settlement_currency(deployment: LighterDeployment) -> Currency {
    match deployment {
        LighterDeployment::Lighter => Currency::get_or_create_crypto("USDC"),
        LighterDeployment::Robinhood => Currency::USDG(),
    }
}

pub(crate) const fn integrator_account_index(
    deployment: LighterDeployment,
    environment: LighterEnvironment,
) -> Option<u64> {
    match (deployment, environment) {
        (LighterDeployment::Lighter, LighterEnvironment::Mainnet) => {
            Some(LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX)
        }
        (LighterDeployment::Lighter, LighterEnvironment::Testnet)
        | (LighterDeployment::Robinhood, _) => None,
    }
}

pub(crate) const fn referral_code(
    deployment: LighterDeployment,
    environment: LighterEnvironment,
) -> Option<&'static str> {
    match (deployment, environment) {
        (LighterDeployment::Robinhood, LighterEnvironment::Mainnet) => {
            Some(LIGHTER_NAUTILUS_REFERRAL_CODE)
        }
        (LighterDeployment::Lighter, _)
        | (LighterDeployment::Robinhood, LighterEnvironment::Testnet) => None,
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::lighter_mainnet(
        LighterDeployment::Lighter,
        LighterEnvironment::Mainnet,
        Some(723_813),
        None
    )]
    #[case::lighter_testnet(LighterDeployment::Lighter, LighterEnvironment::Testnet, None, None)]
    #[case::robinhood_mainnet(
        LighterDeployment::Robinhood,
        LighterEnvironment::Mainnet,
        None,
        Some("NAUTILUS")
    )]
    #[case::robinhood_testnet(
        LighterDeployment::Robinhood,
        LighterEnvironment::Testnet,
        None,
        None
    )]
    fn attribution_is_selected_by_deployment(
        #[case] deployment: LighterDeployment,
        #[case] environment: LighterEnvironment,
        #[case] expected_integrator: Option<u64>,
        #[case] expected_referral: Option<&'static str>,
    ) {
        assert_eq!(
            integrator_account_index(deployment, environment),
            expected_integrator,
        );
        assert_eq!(referral_code(deployment, environment), expected_referral);
    }
}
