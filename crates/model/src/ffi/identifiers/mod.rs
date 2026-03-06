pub mod account_id;
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

/// FFI wrapper for interned string statistics.
#[unsafe(no_mangle)]
pub extern "C" fn interned_string_stats() {
    crate::identifiers::interned_string_stats();
}
