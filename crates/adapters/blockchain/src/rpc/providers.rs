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

use nautilus_model::defi::Blockchain;

pub fn check_infura_rpc_provider(chain: &Blockchain) -> Option<String> {
    if let Ok(infura_api_key) = std::env::var("INFURA_API_KEY") {
        return match chain {
            Blockchain::Ethereum => Some(format!("https://mainnet.infura.io/v3/{infura_api_key}")),
            Blockchain::Polygon => Some(format!(
                "https://polygon-mainnet.infura.io/v3/{infura_api_key}"
            )),
            Blockchain::Base => Some(format!(
                "https://base-mainnet.infura.io/v3/{infura_api_key}"
            )),
            Blockchain::Optimism => Some(format!(
                "https://optimism-mainnet.infura.io/v3/{infura_api_key}"
            )),
            Blockchain::Arbitrum => Some(format!(
                "https://arbitrum-mainnet.infura.io/v3/{infura_api_key}"
            )),
            _ => None, // We can specify other chains here
        };
    }

    None
}
