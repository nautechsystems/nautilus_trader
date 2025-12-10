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

/// Network selection for Lighter environments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LighterNetwork {
    Mainnet,
    Testnet,
}

impl LighterNetwork {
    #[must_use]
    pub const fn is_testnet(&self) -> bool {
        matches!(self, Self::Testnet)
    }
}

impl From<bool> for LighterNetwork {
    fn from(is_testnet: bool) -> Self {
        if is_testnet {
            Self::Testnet
        } else {
            Self::Mainnet
        }
    }
}
