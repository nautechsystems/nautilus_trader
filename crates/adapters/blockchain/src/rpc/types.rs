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

use nautilus_model::defi::Block;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

use crate::events::{
    burn::BurnEvent, collect::CollectEvent, flash::FlashEvent, mint::MintEvent, swap::SwapEvent,
};

/// Represents normalized blockchain messages.
#[derive(Debug, Clone)]
pub enum BlockchainMessage {
    Block(Block),
    SwapEvent(SwapEvent),
    MintEvent(MintEvent),
    BurnEvent(BurnEvent),
    CollectEvent(CollectEvent),
    FlashEvent(FlashEvent),
}

/// Represents the types of events that can be subscribed to via the blockchain RPC interface.
///
/// This enum defines the various event types that the application can subscribe to using
/// the WebSocket-based RPC subscription.
#[derive(
    Debug, Clone, Hash, PartialOrd, Ord, PartialEq, Eq, Display, EnumString, Serialize, Deserialize,
)]
pub enum RpcEventType {
    NewBlock,
}
