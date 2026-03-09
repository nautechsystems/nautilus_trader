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
