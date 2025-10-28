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

//! DeFi (Decentralized Finance) specific messages.

use std::any::Any;

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    defi::Blockchain,
    identifiers::{ClientId, Venue},
};

pub mod request;
pub mod subscribe;
pub mod unsubscribe;

// Re-exports
pub use request::RequestPoolSnapshot;
pub use subscribe::{
    SubscribeBlocks, SubscribePool, SubscribePoolFeeCollects, SubscribePoolFlashEvents,
    SubscribePoolLiquidityUpdates, SubscribePoolSwaps,
};
pub use unsubscribe::{
    UnsubscribeBlocks, UnsubscribePool, UnsubscribePoolFeeCollects, UnsubscribePoolFlashEvents,
    UnsubscribePoolLiquidityUpdates, UnsubscribePoolSwaps,
};

#[derive(Clone, Debug)]
pub enum DefiDataCommand {
    Request(DefiRequestCommand),
    Subscribe(DefiSubscribeCommand),
    Unsubscribe(DefiUnsubscribeCommand),
}

impl PartialEq for DefiDataCommand {
    fn eq(&self, other: &Self) -> bool {
        self.command_id() == other.command_id()
    }
}

impl DefiDataCommand {
    /// Converts the command to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    pub fn command_id(&self) -> UUID4 {
        match self {
            Self::Request(cmd) => *cmd.request_id(),
            Self::Subscribe(cmd) => cmd.command_id(),
            Self::Unsubscribe(cmd) => cmd.command_id(),
        }
    }

    pub fn client_id(&self) -> Option<&ClientId> {
        match self {
            Self::Request(cmd) => cmd.client_id(),
            Self::Subscribe(cmd) => cmd.client_id(),
            Self::Unsubscribe(cmd) => cmd.client_id(),
        }
    }

    pub fn venue(&self) -> Option<&Venue> {
        match self {
            Self::Request(cmd) => cmd.venue(),
            Self::Subscribe(cmd) => cmd.venue(),
            Self::Unsubscribe(cmd) => cmd.venue(),
        }
    }

    pub fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Request(cmd) => cmd.ts_init(),
            Self::Subscribe(cmd) => cmd.ts_init(),
            Self::Unsubscribe(cmd) => cmd.ts_init(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum DefiSubscribeCommand {
    Blocks(SubscribeBlocks),
    Pool(SubscribePool),
    PoolSwaps(SubscribePoolSwaps),
    PoolLiquidityUpdates(SubscribePoolLiquidityUpdates),
    PoolFeeCollects(SubscribePoolFeeCollects),
    PoolFlashEvents(SubscribePoolFlashEvents),
}

impl PartialEq for DefiSubscribeCommand {
    fn eq(&self, other: &Self) -> bool {
        self.command_id() == other.command_id()
    }
}

impl DefiSubscribeCommand {
    /// Converts the command to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Returns the blockchain associated with this command.
    ///
    /// # Panics
    ///
    /// Panics if the instrument ID's venue cannot be parsed as a valid blockchain venue
    /// for Pool, PoolSwaps, PoolLiquidityUpdates, PoolFeeCollects, or PoolFlashEvents commands.
    pub fn blockchain(&self) -> Blockchain {
        match self {
            Self::Blocks(cmd) => cmd.chain,
            Self::Pool(cmd) => cmd.instrument_id.blockchain().expect("Invalid venue"),
            Self::PoolSwaps(cmd) => cmd.instrument_id.blockchain().expect("Invalid venue"),
            Self::PoolLiquidityUpdates(cmd) => {
                cmd.instrument_id.blockchain().expect("Invalid venue")
            }
            Self::PoolFeeCollects(cmd) => cmd.instrument_id.blockchain().expect("Invalid venue"),
            Self::PoolFlashEvents(cmd) => cmd.instrument_id.blockchain().expect("Invalid venue"),
        }
    }

    pub fn command_id(&self) -> UUID4 {
        match self {
            Self::Blocks(cmd) => cmd.command_id,
            Self::Pool(cmd) => cmd.command_id,
            Self::PoolSwaps(cmd) => cmd.command_id,
            Self::PoolLiquidityUpdates(cmd) => cmd.command_id,
            Self::PoolFeeCollects(cmd) => cmd.command_id,
            Self::PoolFlashEvents(cmd) => cmd.command_id,
        }
    }

    pub fn client_id(&self) -> Option<&ClientId> {
        match self {
            Self::Blocks(cmd) => cmd.client_id.as_ref(),
            Self::Pool(cmd) => cmd.client_id.as_ref(),
            Self::PoolSwaps(cmd) => cmd.client_id.as_ref(),
            Self::PoolLiquidityUpdates(cmd) => cmd.client_id.as_ref(),
            Self::PoolFeeCollects(cmd) => cmd.client_id.as_ref(),
            Self::PoolFlashEvents(cmd) => cmd.client_id.as_ref(),
        }
    }

    // TODO: TBD
    pub fn venue(&self) -> Option<&Venue> {
        match self {
            Self::Blocks(_) => None,
            Self::Pool(_) => None,
            Self::PoolSwaps(_) => None,
            Self::PoolLiquidityUpdates(_) => None,
            Self::PoolFeeCollects(_) => None,
            Self::PoolFlashEvents(_) => None,
        }
    }

    pub fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Blocks(cmd) => cmd.ts_init,
            Self::PoolSwaps(cmd) => cmd.ts_init,
            Self::PoolLiquidityUpdates(cmd) => cmd.ts_init,
            Self::Pool(cmd) => cmd.ts_init,
            Self::PoolFeeCollects(cmd) => cmd.ts_init,
            Self::PoolFlashEvents(cmd) => cmd.ts_init,
        }
    }
}

#[derive(Clone, Debug)]
pub enum DefiUnsubscribeCommand {
    Blocks(UnsubscribeBlocks),
    Pool(UnsubscribePool),
    PoolSwaps(UnsubscribePoolSwaps),
    PoolLiquidityUpdates(UnsubscribePoolLiquidityUpdates),
    PoolFeeCollects(UnsubscribePoolFeeCollects),
    PoolFlashEvents(UnsubscribePoolFlashEvents),
}

impl PartialEq for DefiUnsubscribeCommand {
    fn eq(&self, other: &Self) -> bool {
        self.command_id() == other.command_id()
    }
}

impl DefiUnsubscribeCommand {
    /// Converts the command to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Returns the blockchain associated with this command.
    ///
    /// # Panics
    ///
    /// Panics if the instrument ID's venue cannot be parsed as a valid blockchain venue
    /// for Pool, PoolSwaps, PoolLiquidityUpdates, PoolFeeCollects, or PoolFlashEvents commands.
    pub fn blockchain(&self) -> Blockchain {
        match self {
            Self::Blocks(cmd) => cmd.chain,
            Self::Pool(cmd) => cmd.instrument_id.blockchain().expect("Invalid venue"),
            Self::PoolSwaps(cmd) => cmd.instrument_id.blockchain().expect("Invalid venue"),
            Self::PoolLiquidityUpdates(cmd) => {
                cmd.instrument_id.blockchain().expect("Invalid venue")
            }
            Self::PoolFeeCollects(cmd) => cmd.instrument_id.blockchain().expect("Invalid venue"),
            Self::PoolFlashEvents(cmd) => cmd.instrument_id.blockchain().expect("Invalid venue"),
        }
    }

    pub fn command_id(&self) -> UUID4 {
        match self {
            Self::Blocks(cmd) => cmd.command_id,
            Self::Pool(cmd) => cmd.command_id,
            Self::PoolSwaps(cmd) => cmd.command_id,
            Self::PoolLiquidityUpdates(cmd) => cmd.command_id,
            Self::PoolFeeCollects(cmd) => cmd.command_id,
            Self::PoolFlashEvents(cmd) => cmd.command_id,
        }
    }

    pub fn client_id(&self) -> Option<&ClientId> {
        match self {
            Self::Blocks(cmd) => cmd.client_id.as_ref(),
            Self::Pool(cmd) => cmd.client_id.as_ref(),
            Self::PoolSwaps(cmd) => cmd.client_id.as_ref(),
            Self::PoolLiquidityUpdates(cmd) => cmd.client_id.as_ref(),
            Self::PoolFeeCollects(cmd) => cmd.client_id.as_ref(),
            Self::PoolFlashEvents(cmd) => cmd.client_id.as_ref(),
        }
    }

    // TODO: TBD
    pub fn venue(&self) -> Option<&Venue> {
        match self {
            Self::Blocks(_) => None,
            Self::Pool(_) => None,
            Self::PoolSwaps(_) => None,
            Self::PoolLiquidityUpdates(_) => None,
            Self::PoolFeeCollects(_) => None,
            Self::PoolFlashEvents(_) => None,
        }
    }

    pub fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Blocks(cmd) => cmd.ts_init,
            Self::Pool(cmd) => cmd.ts_init,
            Self::PoolSwaps(cmd) => cmd.ts_init,
            Self::PoolLiquidityUpdates(cmd) => cmd.ts_init,
            Self::PoolFeeCollects(cmd) => cmd.ts_init,
            Self::PoolFlashEvents(cmd) => cmd.ts_init,
        }
    }
}

#[derive(Clone, Debug)]
pub enum DefiRequestCommand {
    PoolSnapshot(RequestPoolSnapshot),
}

impl PartialEq for DefiRequestCommand {
    fn eq(&self, other: &Self) -> bool {
        self.request_id() == other.request_id()
    }
}

impl DefiRequestCommand {
    /// Converts the command to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    pub fn request_id(&self) -> &UUID4 {
        match self {
            Self::PoolSnapshot(cmd) => &cmd.request_id,
        }
    }

    pub fn client_id(&self) -> Option<&ClientId> {
        match self {
            Self::PoolSnapshot(cmd) => cmd.client_id.as_ref(),
        }
    }

    pub fn venue(&self) -> Option<&Venue> {
        match self {
            Self::PoolSnapshot(cmd) => Some(&cmd.instrument_id.venue),
        }
    }

    pub fn ts_init(&self) -> UnixNanos {
        match self {
            Self::PoolSnapshot(cmd) => cmd.ts_init,
        }
    }
}
