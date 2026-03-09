//! Identifiers for the trading domain model.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[macro_use]
mod macros;

pub mod account_id;
pub mod actor_id;
pub mod client_id;
pub mod client_order_id;
pub mod component_id;
pub mod exec_algorithm_id;
pub mod instrument_id;
pub mod order_list_id;
pub mod position_id;
pub mod strategy_id;
pub mod symbol;
pub mod trade_id;
pub mod trader_id;
pub mod venue;
pub mod venue_order_id;

#[cfg(any(test, feature = "stubs"))]
pub mod stubs;

// Re-exports
pub use crate::identifiers::{
    account_id::AccountId, actor_id::ActorId, client_id::ClientId, client_order_id::ClientOrderId,
    component_id::ComponentId, exec_algorithm_id::ExecAlgorithmId, instrument_id::InstrumentId,
    order_list_id::OrderListId, position_id::PositionId, strategy_id::StrategyId, symbol::Symbol,
    trade_id::TradeId, trader_id::TraderId, venue::Venue, venue_order_id::VenueOrderId,
};

impl_from_str_for_identifier!(account_id::AccountId);
impl_from_str_for_identifier!(actor_id::ActorId);
impl_from_str_for_identifier!(client_id::ClientId);
impl_from_str_for_identifier!(client_order_id::ClientOrderId);
impl_from_str_for_identifier!(component_id::ComponentId);
impl_from_str_for_identifier!(exec_algorithm_id::ExecAlgorithmId);
impl_from_str_for_identifier!(order_list_id::OrderListId);
impl_from_str_for_identifier!(position_id::PositionId);
impl_from_str_for_identifier!(strategy_id::StrategyId);
impl_from_str_for_identifier!(symbol::Symbol);
impl_from_str_for_identifier!(trade_id::TradeId);
impl_from_str_for_identifier!(trader_id::TraderId);
impl_from_str_for_identifier!(venue::Venue);
impl_from_str_for_identifier!(venue_order_id::VenueOrderId);

impl_serialization_for_identifier!(account_id::AccountId);
impl_serialization_for_identifier!(actor_id::ActorId);
impl_serialization_for_identifier!(client_id::ClientId);
impl_serialization_for_identifier!(client_order_id::ClientOrderId);
impl_serialization_for_identifier!(component_id::ComponentId);
impl_serialization_for_identifier!(exec_algorithm_id::ExecAlgorithmId);
impl_serialization_for_identifier!(order_list_id::OrderListId);
impl_serialization_for_identifier!(position_id::PositionId);
impl_serialization_for_identifier!(strategy_id::StrategyId);
impl_serialization_for_identifier!(symbol::Symbol);
impl_serialization_for_identifier!(trader_id::TraderId);
impl_serialization_for_identifier!(venue::Venue);
impl_serialization_for_identifier!(venue_order_id::VenueOrderId);

impl_as_ref_for_identifier!(account_id::AccountId);
impl_as_ref_for_identifier!(actor_id::ActorId);
impl_as_ref_for_identifier!(client_id::ClientId);
impl_as_ref_for_identifier!(client_order_id::ClientOrderId);
impl_as_ref_for_identifier!(component_id::ComponentId);
impl_as_ref_for_identifier!(exec_algorithm_id::ExecAlgorithmId);
impl_as_ref_for_identifier!(order_list_id::OrderListId);
impl_as_ref_for_identifier!(position_id::PositionId);
impl_as_ref_for_identifier!(strategy_id::StrategyId);
impl_as_ref_for_identifier!(symbol::Symbol);
impl_as_ref_for_identifier!(trader_id::TraderId);
impl_as_ref_for_identifier!(venue::Venue);
impl_as_ref_for_identifier!(venue_order_id::VenueOrderId);

/// Print interned string cache statistics for debugging purposes.
pub fn interned_string_stats() {
    ustr::total_allocated();
    ustr::total_capacity();

    ustr::string_cache_iter().for_each(|s| println!("{s}"));
}
