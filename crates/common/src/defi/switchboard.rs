//! DeFi-specific switchboard functionality.

use ahash::AHashMap;
use nautilus_model::{defi::Blockchain, identifiers::InstrumentId};

use crate::msgbus::{MStr, MessagingSwitchboard, Topic, get_message_bus};

/// DeFi-specific switchboard state.
#[derive(Clone, Debug, Default)]
pub(crate) struct DefiSwitchboard {
    pub(crate) block_topics: AHashMap<Blockchain, MStr<Topic>>,
    pub(crate) pool_topics: AHashMap<InstrumentId, MStr<Topic>>,
    pub(crate) pool_swap_topics: AHashMap<InstrumentId, MStr<Topic>>,
    pub(crate) pool_liquidity_topics: AHashMap<InstrumentId, MStr<Topic>>,
    pub(crate) pool_collect_topics: AHashMap<InstrumentId, MStr<Topic>>,
    pub(crate) pool_flash_topics: AHashMap<InstrumentId, MStr<Topic>>,
}

#[must_use]
pub fn get_defi_blocks_topic(chain: Blockchain) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_defi_blocks_topic(chain)
}

#[must_use]
pub fn get_defi_pool_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_defi_pool_topic(instrument_id)
}

#[must_use]
pub fn get_defi_pool_swaps_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_defi_pool_swaps_topic(instrument_id)
}

#[must_use]
pub fn get_defi_liquidity_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_defi_pool_liquidity_topic(instrument_id)
}

#[must_use]
pub fn get_defi_collect_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_defi_pool_collect_topic(instrument_id)
}

#[must_use]
pub fn get_defi_flash_topic(instrument_id: InstrumentId) -> MStr<Topic> {
    get_message_bus()
        .borrow_mut()
        .switchboard
        .get_defi_pool_flash_topic(instrument_id)
}

impl MessagingSwitchboard {
    #[must_use]
    pub fn get_defi_blocks_topic(&mut self, chain: Blockchain) -> MStr<Topic> {
        *self
            .defi
            .block_topics
            .entry(chain)
            .or_insert_with(|| format!("data.defi.blocks.{chain}").into())
    }

    #[must_use]
    pub fn get_defi_pool_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .defi
            .pool_topics
            .entry(instrument_id)
            .or_insert_with(|| format!("data.defi.pool.{instrument_id}").into())
    }

    #[must_use]
    pub fn get_defi_pool_swaps_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .defi
            .pool_swap_topics
            .entry(instrument_id)
            .or_insert_with(|| format!("data.defi.pool_swaps.{instrument_id}").into())
    }

    #[must_use]
    pub fn get_defi_pool_liquidity_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .defi
            .pool_liquidity_topics
            .entry(instrument_id)
            .or_insert_with(|| format!("data.defi.pool_liquidity.{instrument_id}").into())
    }

    #[must_use]
    pub fn get_defi_pool_collect_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .defi
            .pool_collect_topics
            .entry(instrument_id)
            .or_insert_with(|| format!("data.defi.pool_collect.{instrument_id}").into())
    }

    #[must_use]
    pub fn get_defi_pool_flash_topic(&mut self, instrument_id: InstrumentId) -> MStr<Topic> {
        *self
            .defi
            .pool_flash_topics
            .entry(instrument_id)
            .or_insert_with(|| format!("data.defi.pool_flash.{instrument_id}").into())
    }
}
