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
use nautilus_model::identifiers::{ClientId, Venue};

pub mod subscribe;
pub mod unsubscribe;

// Re-exports
pub use subscribe::{SubscribeBlocks, SubscribePoolSwaps};
pub use unsubscribe::{UnsubscribeBlocks, UnsubscribePoolSwaps};

#[derive(Clone, Debug)]
pub enum DefiSubscribeCommand {
    Blocks(SubscribeBlocks),
    PoolSwaps(SubscribePoolSwaps),
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

    pub fn command_id(&self) -> UUID4 {
        match self {
            Self::Blocks(cmd) => cmd.command_id,
            Self::PoolSwaps(cmd) => cmd.command_id,
        }
    }

    pub fn client_id(&self) -> Option<&ClientId> {
        match self {
            Self::Blocks(cmd) => cmd.client_id.as_ref(),
            Self::PoolSwaps(cmd) => cmd.client_id.as_ref(),
        }
    }

    // TODO: TBD
    pub fn venue(&self) -> Option<&Venue> {
        match self {
            Self::Blocks(_) => None,
            Self::PoolSwaps(_) => None,
        }
    }

    pub fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Blocks(cmd) => cmd.ts_init,
            Self::PoolSwaps(cmd) => cmd.ts_init,
        }
    }
}

#[derive(Clone, Debug)]
pub enum DefiUnsubscribeCommand {
    Blocks(UnsubscribeBlocks),
    PoolSwaps(UnsubscribePoolSwaps),
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

    pub fn command_id(&self) -> UUID4 {
        match self {
            Self::Blocks(cmd) => cmd.command_id,
            Self::PoolSwaps(cmd) => cmd.command_id,
        }
    }

    pub fn client_id(&self) -> Option<&ClientId> {
        match self {
            Self::Blocks(cmd) => cmd.client_id.as_ref(),
            Self::PoolSwaps(cmd) => cmd.client_id.as_ref(),
        }
    }

    // TODO: TBD
    pub fn venue(&self) -> Option<&Venue> {
        match self {
            Self::Blocks(_) => None,
            Self::PoolSwaps(_) => None,
        }
    }

    pub fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Blocks(cmd) => cmd.ts_init,
            Self::PoolSwaps(cmd) => cmd.ts_init,
        }
    }
}
