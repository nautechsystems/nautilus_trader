/**
 * FFI library loader for the NautilusTrader Rust shared library.
 *
 * Single `dlopen` call loading `libnautilus_bun` with all FFI symbols declared in one place.
 *
 * Functions prefixed with `bun_` are heap-allocating wrappers defined in `crates/bun/src/lib.rs`.
 * They exist because Bun's FFI can only handle pointer-sized (8-byte) return values, so any
 * Rust struct > 8 bytes must be boxed on the heap and returned as a pointer.
 */
import { dlopen, FFIType, suffix } from "bun:ffi";

// Resolve library path: env override or default to ../lib/
const LIB_PATH =
  process.env.NAUTILUS_LIB_PATH ??
  new URL(`../lib/libnautilus_bun.${suffix}`, import.meta.url).pathname;

const symbols = {
  // -------------------------------------------------------------------------
  // Lifecycle
  // -------------------------------------------------------------------------
  nautilus_init: { args: [], returns: FFIType.u8 },
  nautilus_shutdown: { args: [], returns: FFIType.void },

  // -------------------------------------------------------------------------
  // Core: String
  // -------------------------------------------------------------------------
  cstr_drop: { args: [FFIType.ptr], returns: FFIType.void },

  // -------------------------------------------------------------------------
  // Core: UUID4 (37 bytes — heap-allocated via bun_ wrappers)
  // -------------------------------------------------------------------------
  bun_uuid4_new: { args: [], returns: FFIType.ptr },
  bun_uuid4_from_cstr: { args: [FFIType.cstring], returns: FFIType.ptr },
  bun_uuid4_to_cstr: { args: [FFIType.ptr], returns: FFIType.ptr },
  bun_uuid4_eq: { args: [FFIType.ptr, FFIType.ptr], returns: FFIType.u8 },
  bun_uuid4_hash: { args: [FFIType.ptr], returns: FFIType.u64 },
  bun_uuid4_drop: { args: [FFIType.ptr], returns: FFIType.void },

  // -------------------------------------------------------------------------
  // Core: Datetime (all scalar returns — no ABI issue)
  // -------------------------------------------------------------------------
  secs_to_nanos: { args: [FFIType.f64], returns: FFIType.u64 },
  secs_to_millis: { args: [FFIType.f64], returns: FFIType.u64 },
  millis_to_nanos: { args: [FFIType.f64], returns: FFIType.u64 },
  micros_to_nanos: { args: [FFIType.f64], returns: FFIType.u64 },
  nanos_to_secs: { args: [FFIType.u64], returns: FFIType.f64 },
  nanos_to_millis: { args: [FFIType.u64], returns: FFIType.u64 },
  nanos_to_micros: { args: [FFIType.u64], returns: FFIType.u64 },
  unix_nanos_to_iso8601_cstr: { args: [FFIType.u64], returns: FFIType.ptr },
  unix_nanos_to_iso8601_millis_cstr: { args: [FFIType.u64], returns: FFIType.ptr },

  // -------------------------------------------------------------------------
  // Model: Enums (to_cstr / from_cstr pairs — scalar returns)
  // -------------------------------------------------------------------------
  account_type_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  account_type_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  aggressor_side_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  aggressor_side_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  aggregation_source_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  aggregation_source_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  bar_aggregation_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  bar_aggregation_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  book_action_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  book_action_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  book_type_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  book_type_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  contingency_type_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  contingency_type_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  currency_type_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  currency_type_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  instrument_class_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  instrument_class_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  instrument_close_type_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  instrument_close_type_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  liquidity_side_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  liquidity_side_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  market_status_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  market_status_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  market_status_action_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  market_status_action_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  oms_type_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  oms_type_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  option_kind_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  option_kind_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  order_side_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  order_side_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  order_status_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  order_status_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  order_type_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  order_type_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  position_side_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  position_side_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  price_type_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  price_type_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  time_in_force_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  time_in_force_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  trailing_offset_type_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  trailing_offset_type_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  trigger_type_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  trigger_type_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  trading_state_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  trading_state_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  record_flag_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  record_flag_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  asset_class_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  asset_class_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  oto_trigger_mode_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  oto_trigger_mode_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },
  position_adjustment_type_to_cstr: { args: [FFIType.u8], returns: FFIType.ptr },
  position_adjustment_type_from_cstr: { args: [FFIType.cstring], returns: FFIType.u8 },

  // -------------------------------------------------------------------------
  // Model: Identifiers — Ustr-based (8 bytes, returned by value — OK)
  // -------------------------------------------------------------------------
  // Symbol
  symbol_new: { args: [FFIType.cstring], returns: FFIType.ptr },
  symbol_hash: { args: [FFIType.ptr], returns: FFIType.u64 },
  symbol_is_composite: { args: [FFIType.ptr], returns: FFIType.u8 },
  symbol_root: { args: [FFIType.ptr], returns: FFIType.ptr },
  symbol_topic: { args: [FFIType.ptr], returns: FFIType.ptr },
  bun_symbol_to_cstr: { args: [FFIType.ptr], returns: FFIType.ptr },

  // Venue
  venue_new: { args: [FFIType.cstring], returns: FFIType.ptr },
  venue_hash: { args: [FFIType.ptr], returns: FFIType.u64 },
  venue_is_synthetic: { args: [FFIType.ptr], returns: FFIType.u8 },
  bun_venue_to_cstr: { args: [FFIType.ptr], returns: FFIType.ptr },

  // TraderId
  trader_id_new: { args: [FFIType.cstring], returns: FFIType.ptr },
  trader_id_hash: { args: [FFIType.ptr], returns: FFIType.u64 },

  // AccountId
  account_id_new: { args: [FFIType.cstring], returns: FFIType.ptr },
  account_id_hash: { args: [FFIType.ptr], returns: FFIType.u64 },

  // ClientId
  client_id_new: { args: [FFIType.cstring], returns: FFIType.ptr },
  client_id_hash: { args: [FFIType.ptr], returns: FFIType.u64 },

  // ClientOrderId
  client_order_id_new: { args: [FFIType.cstring], returns: FFIType.ptr },
  client_order_id_hash: { args: [FFIType.ptr], returns: FFIType.u64 },

  // ComponentId
  component_id_new: { args: [FFIType.cstring], returns: FFIType.ptr },
  component_id_hash: { args: [FFIType.ptr], returns: FFIType.u64 },

  // ExecAlgorithmId
  exec_algorithm_id_new: { args: [FFIType.cstring], returns: FFIType.ptr },
  exec_algorithm_id_hash: { args: [FFIType.ptr], returns: FFIType.u64 },

  // OrderListId
  order_list_id_new: { args: [FFIType.cstring], returns: FFIType.ptr },
  order_list_id_hash: { args: [FFIType.ptr], returns: FFIType.u64 },

  // PositionId
  position_id_new: { args: [FFIType.cstring], returns: FFIType.ptr },
  position_id_hash: { args: [FFIType.ptr], returns: FFIType.u64 },

  // StrategyId
  strategy_id_new: { args: [FFIType.cstring], returns: FFIType.ptr },
  strategy_id_hash: { args: [FFIType.ptr], returns: FFIType.u64 },

  // VenueOrderId
  venue_order_id_new: { args: [FFIType.cstring], returns: FFIType.ptr },
  venue_order_id_hash: { args: [FFIType.ptr], returns: FFIType.u64 },

  // -------------------------------------------------------------------------
  // Model: Identifiers — large structs (heap-allocated via bun_ wrappers)
  // -------------------------------------------------------------------------
  // InstrumentId (16 bytes)
  bun_instrument_id_from_cstr: { args: [FFIType.cstring], returns: FFIType.ptr },
  bun_instrument_id_to_cstr: { args: [FFIType.ptr], returns: FFIType.ptr },
  bun_instrument_id_hash: { args: [FFIType.ptr], returns: FFIType.u64 },
  bun_instrument_id_is_synthetic: { args: [FFIType.ptr], returns: FFIType.u8 },
  bun_instrument_id_drop: { args: [FFIType.ptr], returns: FFIType.void },

  // TradeId (38 bytes)
  bun_trade_id_new: { args: [FFIType.cstring], returns: FFIType.ptr },
  bun_trade_id_to_cstr: { args: [FFIType.ptr], returns: FFIType.ptr },
  bun_trade_id_hash: { args: [FFIType.ptr], returns: FFIType.u64 },
  bun_trade_id_drop: { args: [FFIType.ptr], returns: FFIType.void },

  // -------------------------------------------------------------------------
  // Model: Types (heap-allocated via bun_ wrappers)
  // -------------------------------------------------------------------------
  // Price (16 bytes)
  bun_price_new: { args: [FFIType.f64, FFIType.u8], returns: FFIType.ptr },
  bun_price_as_f64: { args: [FFIType.ptr], returns: FFIType.f64 },
  bun_price_precision: { args: [FFIType.ptr], returns: FFIType.u8 },
  bun_price_to_cstr: { args: [FFIType.ptr], returns: FFIType.ptr },
  bun_price_drop: { args: [FFIType.ptr], returns: FFIType.void },

  // Quantity (16 bytes)
  bun_quantity_new: { args: [FFIType.f64, FFIType.u8], returns: FFIType.ptr },
  bun_quantity_as_f64: { args: [FFIType.ptr], returns: FFIType.f64 },
  bun_quantity_precision: { args: [FFIType.ptr], returns: FFIType.u8 },
  bun_quantity_to_cstr: { args: [FFIType.ptr], returns: FFIType.ptr },
  bun_quantity_drop: { args: [FFIType.ptr], returns: FFIType.void },

  // Currency (32 bytes)
  bun_currency_from_cstr: { args: [FFIType.cstring], returns: FFIType.ptr },
  bun_currency_code_to_cstr: { args: [FFIType.ptr], returns: FFIType.ptr },
  bun_currency_name_to_cstr: { args: [FFIType.ptr], returns: FFIType.ptr },
  bun_currency_precision: { args: [FFIType.ptr], returns: FFIType.u8 },
  bun_currency_hash: { args: [FFIType.ptr], returns: FFIType.u64 },
  bun_currency_exists: { args: [FFIType.cstring], returns: FFIType.u8 },
  bun_currency_drop: { args: [FFIType.ptr], returns: FFIType.void },

  // Money (40 bytes)
  bun_money_new: { args: [FFIType.f64, FFIType.ptr], returns: FFIType.ptr },
  bun_money_as_f64: { args: [FFIType.ptr], returns: FFIType.f64 },
  bun_money_to_cstr: { args: [FFIType.ptr], returns: FFIType.ptr },
  bun_money_drop: { args: [FFIType.ptr], returns: FFIType.void },

  // -------------------------------------------------------------------------
  // Common: TestClock
  // TestClock_API wraps Box<TestClock> (8 bytes). bun_ wrappers heap-allocate
  // the API struct so the pointer is stable for all &TestClock_API methods.
  // -------------------------------------------------------------------------
  bun_test_clock_new: { args: [], returns: FFIType.ptr },
  bun_test_clock_drop: { args: [FFIType.ptr], returns: FFIType.void },
  test_clock_set_time: { args: [FFIType.ptr, FFIType.u64], returns: FFIType.void },
  test_clock_timestamp: { args: [FFIType.ptr], returns: FFIType.f64 },
  test_clock_timestamp_ms: { args: [FFIType.ptr], returns: FFIType.u64 },
  test_clock_timestamp_us: { args: [FFIType.ptr], returns: FFIType.u64 },
  test_clock_timestamp_ns: { args: [FFIType.ptr], returns: FFIType.u64 },
  test_clock_timer_names: { args: [FFIType.ptr], returns: FFIType.ptr },
  test_clock_timer_count: { args: [FFIType.ptr], returns: FFIType.u64 },
  test_clock_next_time: { args: [FFIType.ptr, FFIType.cstring], returns: FFIType.u64 },
  test_clock_cancel_timer: { args: [FFIType.ptr, FFIType.cstring], returns: FFIType.void },
  test_clock_cancel_timers: { args: [FFIType.ptr], returns: FFIType.void },

  // Common: LiveClock (same pattern as TestClock)
  bun_live_clock_new: { args: [], returns: FFIType.ptr },
  bun_live_clock_drop: { args: [FFIType.ptr], returns: FFIType.void },
  live_clock_timestamp: { args: [FFIType.ptr], returns: FFIType.f64 },
  live_clock_timestamp_ms: { args: [FFIType.ptr], returns: FFIType.u64 },
  live_clock_timestamp_us: { args: [FFIType.ptr], returns: FFIType.u64 },
  live_clock_timestamp_ns: { args: [FFIType.ptr], returns: FFIType.u64 },
  live_clock_timer_names: { args: [FFIType.ptr], returns: FFIType.ptr },
  live_clock_timer_count: { args: [FFIType.ptr], returns: FFIType.u64 },
  live_clock_next_time: { args: [FFIType.ptr, FFIType.cstring], returns: FFIType.u64 },
  live_clock_cancel_timer: { args: [FFIType.ptr, FFIType.cstring], returns: FFIType.void },
  live_clock_cancel_timers: { args: [FFIType.ptr], returns: FFIType.void },
} as const;

export type NautilusLib = ReturnType<typeof dlopen<typeof symbols>>;

let _lib: NautilusLib | null = null;

/**
 * Get the loaded FFI library, initializing it on first call.
 */
export function getLib(): NautilusLib {
  if (!_lib) {
    _lib = dlopen(LIB_PATH, symbols);
    _lib.symbols.nautilus_init();
  }
  return _lib;
}

/**
 * Close the FFI library and release resources.
 */
export function closeLib(): void {
  if (_lib) {
    _lib.symbols.nautilus_shutdown();
    _lib.close();
    _lib = null;
  }
}
