// Type definitions matching legacy Flask API schemas

export type MarketSnapshot = {
  symbol: string;  // called "coin" in Flask API
  exchange: string;
  bid_qty: string;
  bid: string;     // called "bid_px" in Flask API
  mid_px: string;
  ask: string;     // called "ask_px" in Flask API
  ask_qty: string;
  timestamp_ms: number;
  update_time: string;  // Server-formatted time (legacy), prefer timestamp_ms for rendering
};

export type CanonicalNamingFields = {
  instrument_uid?: string;
  instrument_id?: string;
  venue?: string;
  venue_root?: string;
  product_type?: 'spot' | 'perp' | string;
  market_type?: 'spot' | 'perp' | string;
  contract_type?: string;
  raw_symbol?: string;
  base_asset?: string;
  quote_asset?: string;
  pair?: string;
  inventory_asset?: string;
  display_name_short?: string;
  display_name_long?: string;
};

export type Trade = CanonicalNamingFields & {
  time: string;  // Render exactly as delivered by API
  coin: string;
  exchange: string;
  venue?: string;  // cex|dex logical venue
  symbol?: string; // optional symbol alias
  side: 'buy' | 'sell' | string;
  price: string | number | null;
  qty: string | number | null;
  mv: string | number | null;  // market value / notional (quote)
  notional?: string | number | null;  // canonical notional (server v1)
  // Fee fields:
  // - fee: legacy display fee (numeric or string); for DEX this may be gas cost
  // - fee_asset_raw: asset fee was charged in (e.g., USDT, ZENT, PLUME)
  // - fee_amount_raw: amount in fee_asset_raw units
  // - fee_quote: fee expressed in quote asset units when derivable
  fee: string | number | null;
  fee_asset_raw?: string | null;
  fee_amount_raw?: string | number | null;
  fee_quote?: string | number | null;
  exch_id: string;  // exchange fill identifier (tx_hash for DEX)
  trade_id: string;
  signal_id: string;
  strategy_id?: string;  // Strategy that executed this trade
  order_id: string;
  decision?: string | unknown;  // JSON string with decision data
  decision_summary?: string | null;
  decision_version?: string | null;
  decision_edge_bps_raw?: number | string | null;
  decision_edge_bps_net?: number | string | null;
  decision_required_bps?: number | string | null;
  decision_gas_bps?: number | string | null;
  decision_label?: string | null;
  decision_case?: string | number | null;
  decision_timestamp?: string;  // ISO timestamp from decision.decision_timestamp.iso
  // Gas fields:
  // - gas_used: legacy field (units or cost depending on producer)
  // - gas_units: explicit gas units consumed when known
  gas_used?: number | string;
  gas_units?: number | string;
  notes?: string;
  explorer_url?: string;
  placeholder?: boolean;
};

export type TradeEvent = {
  op: 'upsert' | 'delete';
  row_id: string;
  version: number;
  seq: number;
  resync_id?: number;
  resyncId?: number;
  schema?: number;
  ts?: number;
  ts_ms?: number;
  placeholder?: boolean;
  ref_version?: number;
  [key: string]: unknown;
};

export type TradeRow = Trade & {
  row_id: string;
  version: number;
  seq: number;
  ts: number;
};

export type OrderViewLeg = 'maker' | 'hedge' | 'both';

export type OrderViewLegContext = {
  exchange: string | null;
  symbol: string | null;
};

export type OrderViewContext = {
  maker: OrderViewLegContext;
  hedge: OrderViewLegContext;
};

export type OrderViewSelection = {
  strategy_id: string;
  leg: OrderViewLeg;
};

export type OrderViewBbo = {
  bid: number;
  ask: number;
  mid: number;
  ts_ms?: number | null;
};

export type OrderViewOpenOrder = {
  order_row_id: string;
  leg: Exclude<OrderViewLeg, 'both'>;
  side: 'bid' | 'ask';
  level: number;
  px?: string | null;
  rem_qty?: string | null;
  client_order_id?: string | null;
  order_id?: string | null;
  blocked_until_ts_ms?: number | null;
  blocked_reason?: string | null;
  created_ts_ms?: number | null;
  updated_ts_ms?: number | null;
  lifetime_start_unknown?: boolean;
};

export type OrderViewEvent = {
  event_key: string;
  ts_ms?: number | null;
  type: string;
  leg?: string;
  side?: string;
  level?: number;
  ok?: boolean;
  order_id?: string;
  client_order_id?: string;
  fill_id?: string;
  qty?: string | number;
  px?: string | number;
  reason?: string;
  error?: string;
  message?: string;
  strategy_id?: string;
  resp?: string;
  [key: string]: unknown;
};

export type OrderViewStatus = {
  md_ok: boolean;
  maker_state_ok: boolean;
  events_ok: boolean;
  last_md_ts_ms?: number | null;
  last_state_ts_ms?: number | null;
  l2_age_ms?: number | null;
  trades_age_ms?: number | null;
  bbo_age_ms?: number | null;
  strategy_age_ms?: number | null;
  md_age_ms?: number | null;
  notes: string[];
};

export type OrderViewL2Level = {
  px: string;
  qty: string;
  size?: number;
  [key: string]: unknown;
};

export type OrderViewL2Snapshot = {
  bids: OrderViewL2Level[];
  asks: OrderViewL2Level[];
  top_n: number;
  spread_abs?: number | null;
  spread_bps?: number | null;
  semantic?: {
    mode: 'topn_authoritative_snapshot' | string;
    max_levels_per_side: number;
    max_publish_hz: number;
  };
};

export type OrderViewCandleRow = {
  ts_ms: number;
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
};

export type OrderViewCandlesSnapshot = {
  rows: OrderViewCandleRow[];
  source: 'trades' | 'bbo_fallback' | string;
  candle_current?: OrderViewCandleRow;
  candle_closed?: OrderViewCandleRow;
  semantic?: {
    snapshot: 'last_k' | string;
    delta_current: 'candle_current' | string;
    delta_closed: 'candle_closed' | string;
  };
};

export type OrderViewMarketTradeRow = {
  trade_id?: string | null;
  ts_ms: number;
  side?: string | null;
  price: string;
  qty: string;
};

export type OrderViewMarketTradesSnapshot = {
  rows: OrderViewMarketTradeRow[];
};

export type OrderViewSnapshot = {
  room_id: string;
  server_ts_ms: number;
  server_time_ms?: number;
  snapshot_id?: string;
  last_seq?: number;
  seq?: number;
  selection: OrderViewSelection;
  context: OrderViewContext;
  state_rev: string;
  maker_state_ts_ms?: number | null;
  bbo: {
    maker?: OrderViewBbo;
    hedge?: OrderViewBbo;
  };
  open_orders: {
    rows: OrderViewOpenOrder[];
  };
  quote_snapshot?: Record<string, unknown> | null;
  events: {
    rows: OrderViewEvent[];
  };
  market_trades?: OrderViewMarketTradesSnapshot;
  l2?: OrderViewL2Snapshot;
  candles?: OrderViewCandlesSnapshot;
  status: OrderViewStatus;
};

export type OrderViewDelta = {
  room_id: string;
  seq: number;
  state_rev: string;
  server_ts_ms: number;
  server_time_ms?: number;
  snapshot_id?: string;
  selection: OrderViewSelection;
  context: OrderViewContext;
  bbo: {
    maker?: OrderViewBbo;
    hedge?: OrderViewBbo;
  };
  open_orders?: {
    full_refresh: 1;
    rows: OrderViewOpenOrder[];
  };
  events: {
    rows: OrderViewEvent[];
  };
  market_trades?: OrderViewMarketTradesSnapshot;
  l2?: OrderViewL2Snapshot;
  candles?: OrderViewCandlesSnapshot;
  status: OrderViewStatus;
};

export type FVRow = Record<string, string | number>;

export type FvTermBreakdown = {
  id: number;
  name: string;
  trigger: 'always' | 'onChMid' | 'onTrade' | string;
  weight: number;
  mode: 'raw' | 'power' | 'linear' | string;
  beta?: number | null;
  gain?: number | null;
  value: number;
  term_last_trigger?: string | null;
  term_last_trigger_ts_ms?: number | null;
  last_trigger_event?: string | null;
  last_trigger_ts_ms?: number | null;
  theo1_sample?: number | null;
  theo1_source?: string | null;
  theo1_ts_ms?: number | null;
  theo2_sample?: number | null;
  theo2_source?: string | null;
  theo2_is_fallback?: boolean | null;
  theo2_ts_ms?: number | null;
  ema1?: number | null;
  ema2?: number | null;
  ratio?: number | null;
  baseline?: number | null;
  multiplier?: number | null;
  p2f_bid?: number | null;
  p2f_ask?: number | null;
  p2f_mid?: number | null;
  p2f_depth_ok?: boolean | null;
  p2f_levels_used?: number | null;
  p2f_ts_ms?: number | null;
  delta?: number | null;
  delta_pct?: number | null;
  contribution?: number | null;
  contribution_delta?: number | null;
  formula_text?: string;
  formula_latex?: string;
};

export type FvWhatMoved = {
  kind?: 'term' | 'overlay' | 'none' | string;
  term_id?: number | null;
  term_name?: string;
  trigger?: 'mid' | 'trade' | 'timer' | string;
  delta_contribution?: number | null;
  delta_overlay_pct?: number | null;
  delta_overlay_value?: number | null;
  delta_base?: number | null;
  delta_final?: number | null;
  side?: string;
  notional_usd?: number | null;
};

export type FvSnapshot = {
  symbol: string;
  fv_profile?: string;
  fv_version?: number;
  calc_type?: string;
  ts_ms: number;
  trigger?: 'mid' | 'trade' | 'timer' | string;
  final: number;
  base: number;
  signed_volume: number;
  overlay_pct: number;
  terms: FvTermBreakdown[];
  what_moved?: FvWhatMoved;
};

// All strategy params are strings - NO client coercion
export type StrategyParams = Record<string, string>;

// Static classification metadata attached to strategies. These are defined in
// configs/strategies.ini and are not runtime-editable via Fluxboard.
export type StrategyMeta = {
  class?: string;
  venue_prefix?: string;
  base_asset?: string;
  quote_asset?: string;
  chain?: string;
  strategy_groups?: string;
  param_set?: string;
  strategy_family?: string;
  strategy_version?: string;
};

export type StrategyRunState = 'running' | 'stopped' | 'unknown';

export type StrategyStatus = {
  runState: StrategyRunState;
  tradingEnabled: boolean;
  blocked?: boolean;
  coolingDown?: boolean;
};

export type TradesResponse = {
  rows: TradeEvent[];
  total: number;
  last_seq?: number;
};

// FX service types
export type FxPair = {
  pair: string;
  price: string;
  source: 'bybit' | 'curve' | 'unknown';
  src_ts_ms?: number;
  recv_ts_ms?: number;
  age_ms: number;
  stale: boolean;
  jump_bps?: number;
  deviation_bps?: number;
  clamp_breach?: boolean;
};

export type FxDashboard = {
  service: {
    ok: boolean;
    uptime_s?: number;
    version?: string;
  };
  pairs: FxPair[];
  bybit?: {
    connected: boolean;
    last_msg_age_ms?: number;
    reconnects?: number;
  };
  curve?: {
    polls?: number;
    last_poll_age_ms?: number;
    errors?: number;
  };
};

// Strategy FX configuration
export type StrategyFxConfig = {
  id: string;
  name: string;
  fx_pair: string;
  fx_source: 'service' | 'par' | 'constant' | 'pool' | 'unknown';
  gas_model: string;
  estimated_spread_bps: number;
};

export type StrategyFxConfigResponse = {
  strategies: StrategyFxConfig[];
};

// Balances types
export type BalanceChildRow = CanonicalNamingFields & {
  id: string;
  parent_id: string;
  coin: string;
  venue?: string | null;
  wallet?: string | null;
  address?: string | null;
  label?: string | null;
  qty_display: string;
  qty_raw: number;
  mv_display: string;
  mv_raw: number;
  mark_display: string | null;
  mark_raw: number | null;
  time_display: string;
  time_iso?: string | null;
  last_ts?: number | null;
  // Token metadata fields (added via FluxAPI enrichment)
  form?: 'native' | 'wrapped' | 'bridged' | 'staked' | 'receipt' | 'other';
  form_source?: string;
  chain?: string | null;
  contract?: string | null;
  risk_key?: string | null;
  risk_label?: string | null;
};

export type BalanceParentRow = {
  id: string;
  coin: string;
  canonical: string;
  is_parent: boolean;
  stable: boolean;
  qty_display: string;
  qty_raw: number;
  mv_display: string;
  mv_raw: number;
  mark_display: string | null;
  mark_raw: number | null;
  time_display: string;
  time_iso?: string | null;
  last_ts?: number | null;
  children: BalanceChildRow[];
  raw: {
    qty: number;
    mv_usd: number;
    mark: number | null;
  };
};

export type BalancesTotals = {
  mv_raw: number;
  mv_display: string;
  net_mv_raw?: number | null;
  net_mv_display?: string | null;
  long_mv_raw?: number | null;
  long_mv_display?: string | null;
  short_mv_raw?: number | null;
  short_mv_display?: string | null;
  gross_mv_raw?: number | null;
  gross_mv_display?: string | null;
  stable_mv_raw?: number | null;
  stable_mv_display?: string | null;
  non_stable_mv_raw?: number | null;
  non_stable_mv_display?: string | null;
  account_equity_raw?: number | null;
  account_equity_display?: string | null;
  withdrawable_raw?: number | null;
  withdrawable_display?: string | null;
};

export type RiskGroup = {
  risk_key: string;
  label: string;
  net_qty?: number | null;
  net_mv?: number | null;
  long_mv?: number | null;
  short_mv?: number | null;
  gross_mv?: number | null;
  abs_net_mv?: number | null;
  hedge_ratio?: number | null;
  sources?: string[];
  rows?: Array<{
    row_id?: string | null;
    venue: string;
    coin: string;
    qty_raw: number;
    mv_raw: number;
    mark_raw?: number | null;
    time_display?: string | null;
    label?: string | null;
    wallet?: string | null;
    address?: string | null;
  }>;
};

export type BalancesPayload = {
  rows: BalanceParentRow[];
  total: number;
  totals: BalancesTotals;
  generated_at: string;
  view: string;
  risk_groups?: RiskGroup[];
};

export type BalancesResponse = {
  ok: boolean;
  data: BalancesPayload;
  error?: string | null;
};

export type BalanceRequirement = {
  location: string;
  token: string;
  required: string | null;
  available: string;
  coverage: number | null;
  kind: string;
  reason?: string | null;
};

export type BalanceReadiness = {
  status: 'OK' | 'WARN' | 'FAIL' | 'UNKNOWN';
  qty: string;
  multiplier: string;
  summary?: string;
  requirements: BalanceRequirement[];
  missing: BalanceRequirement[];
};

export type BalanceSummary = {
  total: number;
  counts: Record<'OK' | 'WARN' | 'FAIL' | 'UNKNOWN', number>;
};

export type SignalStrategiesPayload = {
  strategies: SignalStrategy[];
  server_time?: string;
  server_ts_ms?: number;
  balance_summary?: BalanceSummary;
};

// Signal page types
export type SignalLeg = CanonicalNamingFields & {
  contract_id?: string;
  coin?: string;
  exchange?: string;
  mid?: number;
  fv_bid?: number;  // Legacy name (fees-in, edges-out)
  fv_ask?: number;  // Legacy name (fees-in, edges-out)
  decision_bid?: number;  // Explicit name: decision price (fees-in, edges-out)
  decision_ask?: number;  // Explicit name: decision price (fees-in, edges-out)
  bid_edge_bps?: number;
  ask_edge_bps?: number;
  net_edge_bps?: number;
  update_time?: string;  // Format: "YYYY-MM-DD HH:MM:SS" (UTC)
  update_ts_ms?: number; // Parsed numeric timestamp (ms) derived from update_time
  effective?: {
    dex_buy?: string;
    dex_sell?: string;
    cex_buy?: string;
    cex_sell?: string;
  };
  // Schema v2: Component breakdown for transparency
  schema_version?: number;
  raw_bid?: number;  // Raw market bid before fees
  raw_ask?: number;  // Raw market ask before fees
  fee_bps?: number;  // Fee in basis points
  fee_type?: 'taker' | 'pool';  // Fee category
  fx_factor?: number;  // FX normalization factor (for DEX leg in cross-quote strategies)
  fx_pair?: string;  // FX pair used (e.g., "USDC/USDT")
  fx_age_ms?: number;  // Age of FX data in milliseconds
  fx_source?: string;  // FX data source (e.g., "bybit", "curve", "constant")
  // Market data recency annotations (best-effort)
  md_ts_ms?: number;   // Best-effort last:* market timestamp (ms)
  md_age_ms?: number;  // Age derived from md_ts_ms on the server
  // Quoted overlay: prices after applying edge bias thresholds
  quoted_bid?: number;  // Decision bid adjusted by edge threshold
  quoted_ask?: number;  // Decision ask adjusted by edge threshold
  edge_bias_bid_bps?: number;  // Edge threshold applied to bid (bps)
  edge_bias_ask_bps?: number;  // Edge threshold applied to ask (bps)
};

export type SignalTrade = {
  ts?: string;
  notional?: number;
  realized_bps?: number;
  price?: number;
  qty?: number;
  side?: string;
  exchange?: string;
};

export type PricingAdjustment = {
  type: 'inventory_skew' | string;
  inv_ratio?: number;
  inv_skew?: number;
  inv_ratio_global?: number;
  inv_skew_global?: number;
  inv_ratio_local?: number;
  inv_skew_local?: number;
  global_qty?: number | null;
  curr_qty?: number | null;
  local_qty?: number | null;
  local_qty_key?: {
    venue_root?: string;
    instrument_type?: string;
    base?: string;
  } | null;
  local_qty_matched_rows?: number | null;
  local_qty_missing_snapshot?: number | null;
  base_bid_edge_bps?: number | null;
  base_ask_edge_bps?: number | null;
  eff_bid_edge_bps?: number;
  eff_ask_edge_bps?: number;
  delta_bid_edge_bps?: number | null;
  delta_ask_edge_bps?: number | null;
  updated_ts_ms?: number | null;
};

export type MakerRoleMap = {
  maker_leg: string;
  ref_leg: string;
  hedge_leg?: string;
};

// MakerV2 quoting truth (planned targets) passed through from MakerV2 persisted state.
// Numeric-like values are typically serialized as strings server-side (Decimal → str).
export type MakerV2QuoteSnapshot = {
  ts_ms?: number | string;
  mode?: string;
  reason?: string | null;

  ref_exchange?: string;
  ref_symbol?: string;
  ref_source?: string;
  ref_bid?: string | number | null;
  ref_ask?: string | number | null;
  ref_ts_ms?: number | string | null;
  ref_age_ms?: number | string | null;

  maker_exchange?: string;
  maker_symbol?: string;
  maker_top_bid?: string | number | null;
  maker_top_ask?: string | number | null;
  maker_top_ts_ms?: number | string | null;
  maker_top_age_ms?: number | string | null;

  place_bid?: string | number | null;
  place_ask?: string | number | null;
  cancel_bid?: string | number | null;
  cancel_ask?: string | number | null;

  eff_bid_edge_bps?: string | number | null;
  eff_ask_edge_bps?: string | number | null;
  place_edge_bps?: string | number | null;

  [key: string]: unknown;
};

export type MakerV4LegSnapshot = {
  venue?: string;
  route?: string | null;
  symbol?: string;
  instrument_id?: string;
  bid?: string | number | null;
  ask?: string | number | null;
  mid?: string | number | null;
  ts_ms?: string | number | null;
  age_ms?: string | number | null;
  [key: string]: unknown;
};

export type MakerV4QuoteSnapshot = {
  ts_ms?: string | number | null;
  maker_leg?: MakerV4LegSnapshot;
  hedge_leg?: MakerV4LegSnapshot;
  ref_leg?: MakerV4LegSnapshot;
  effective_spread_bps?: string | number | null;
  quoted_spread_bps?: string | number | null;
  expected_maker_fee_bps?: string | number | null;
  assumed_hedge_fee_bps?: string | number | null;
  hedge_ready?: boolean | null;
  hedge_route?: string | null;
  effective_account_source?: string | null;
  hedge_disabled_reason?: string | null;
  ibkr_quote_age_ms?: string | number | null;
  fee_snapshot_age_s?: string | number | null;
  hedge_latency_ms?: string | number | null;
  hedge_slippage_bps_vs_mid?: string | number | null;
  [key: string]: unknown;
};

export type SignalStrategy = {
  id: string;
  params: Record<string, string | undefined>;
  running?: boolean | null;
  state?: Record<string, unknown> | null;
  legs: Record<string, SignalLeg | null>;
  legs_order?: string[] | null;
  balances_ok: boolean;
  balance_readiness?: BalanceReadiness;
  last_trade?: SignalTrade | null;
  pricing_adjustments?: PricingAdjustment[];
  maker_quote_status?: {
    bid_open?: number;
    ask_open?: number;
    bid_blocked?: number;
    ask_blocked?: number;
    bid_depth?: number;
    ask_depth?: number;
  };
  quote_stacks?: {
    maker?: {
      bands?: Array<{
        band: number;
        bid: {
          open: number;
          depth: number;
          blocked: number;
          rows: Array<Record<string, unknown>>;
        };
        ask: {
          open: number;
          depth: number;
          blocked: number;
          rows: Array<Record<string, unknown>>;
        };
      }>;
    };
    hedge?: {
      bid: {
        open: number;
        depth: number;
        blocked: number;
        rows: Array<Record<string, unknown>>;
      };
      ask: {
        open: number;
        depth: number;
        blocked: number;
        rows: Array<Record<string, unknown>>;
      };
    };
  };
  maker_role_map?: MakerRoleMap;
  maker_v2?: {
    quote_snapshot?: MakerV2QuoteSnapshot;
  };
  maker_v3?: {
    quote_snapshot?: MakerV2QuoteSnapshot;
  };
  maker_v4?: {
    quote_snapshot?: MakerV4QuoteSnapshot;
  };
  strategy_family?: 'maker_v4' | 'maker_v3' | 'maker_v2' | 'taker';
  risk_delta?: number;
  risk_delta_ts_ms?: number;
  // Static strategy classification metadata (optional, from configs/strategies.ini)
  meta?: StrategyMeta;

  /**
   * Strategy-level decision edge (best-case across both arbitrage directions).
   *
   * Computed server-side by comparing:
   * - case1 (buy A, sell B): (B_bid - A_ask) / A_ask × 10000
   * - case2 (buy B, sell A): (A_bid - B_ask) / B_ask × 10000
   *
   * This is the **single source of truth** for edge display. Use this field
   * instead of leg-level `net_edge_bps` to avoid race conditions during
   * incremental WebSocket updates (where legs may arrive at different times).
   *
   * Falls back to leg-level `net_edge_bps` if backend doesn't provide this field
   * (backward compatibility with older backend versions).
   *
   * @see fluxapi/realtime/manager.py:_compute_decision_edge_summary() for backend computation
   */
  decision_edge_bps?: number;

  /**
   * Edge surplus (decision_edge_bps minus required threshold for best case).
   * Positive value indicates profitable opportunity above minimum thresholds.
   */
  edge2_bps?: number;

  /** Minimum required edge threshold for best case (cex_bid_edge + pool_edge or cex_ask_edge + pool_edge) */
  required_edge_bps?: number;

  /** Which arbitrage direction provided the best edge ('case1' = buy A sell B, 'case2' = buy B sell A) */
  edge2_case?: 'case1' | 'case2';

  /** Detailed edge calculation breakdown for both cases (for debugging/transparency) */
  edge_case_details?: any;

  /** Decision flag: surplus_bps >= 0 (enough edge to trade) */
  tradeable?: boolean;

  /** Decision flag: -10 <= surplus_bps < 0 (close but not enough) */
  near_tradeable?: boolean;

  /** Decision flag: surplus_bps < -10 (too far from threshold) */
  blocked?: boolean;

  /**
   * Spread (net) at BBO after fees (and FX when present).
   *
   * This is a market-condition metric: "If I crossed both legs right now,
   * what would the net spread be after fees?"
   *
   * Independent of required_edge_bps (threshold).
   */
  spread_net_bps?: number;
  spread_net_case1_bps?: number;
  spread_net_case2_bps?: number;
  spread_net_best_case?: 'case1' | 'case2';
};

// Hedger status types
export type HedgerGeometry = {
  initial_eth: string;
  initial_plume: string;
  price_lower: string;
  price_upper: string;
};

export type HedgerGeometryOverrides = Partial<HedgerGeometry>;

export type HedgerThresholds = {
  eth_exposure_usd_threshold: string;
  plume_exposure_usd_threshold: string;
  price_move_pct: string;
};

export type HedgerThresholdOverrides = Partial<HedgerThresholds>;

export type HedgerSnapshot = {
  timestamp: number;
  price_plume_per_eth: string;
  price_move_pct: string;
  price_source?: string;
  pool_price_plume_per_eth?: string;
  pool_price_source?: string;
  // Generic token aliases (for non-ETH/PLUME hedgers).
  token0_symbol?: string;
  token1_symbol?: string;
  perp_symbol_token0?: string;
  perp_symbol_token1?: string;
  price_token1_per_token0?: string;
  lp_eth: string;
  lp_plume: string;
  lp_token0?: string;
  lp_token1?: string;
  perp_eth: string;
  perp_plume: string;
  perp_token0?: string;
  perp_token1?: string;
  net_eth: string;
  net_plume: string;
  net_token0?: string;
  net_token1?: string;
  target_net_eth: string;
  target_net_plume: string;
  target_net_token0?: string;
  target_net_token1?: string;
  token0_decimals?: number | string;
  token1_decimals?: number | string;
  eth_error: string;
  plume_error: string;
  token0_error?: string;
  token1_error?: string;
  eth_mark: string;
  plume_mark: string;
  token0_mark?: string;
  token1_mark?: string;
  eth_usd_error: string;
  plume_usd_error: string;
  token0_usd_error?: string;
  token1_usd_error?: string;
  lp_eth_usd?: string;
  lp_plume_usd?: string;
  perp_eth_usd?: string;
  perp_plume_usd?: string;
  net_eth_usd?: string;
  net_plume_usd?: string;
  total_lp_value_usd?: string;
  total_perp_notional_usd?: string;
  net_delta_value_usd?: string;
  lp_mix_eth_pct?: string;
  lp_mix_plume_pct?: string;
  range_pct?: string;
  near_lower_bound?: boolean;
  near_upper_bound?: boolean;
  eth_exposure_usd_threshold_base?: string;
  plume_exposure_usd_threshold_base?: string;
  eth_exposure_usd_threshold_effective?: string;
  plume_exposure_usd_threshold_effective?: string;
  price_move_pct_base?: string;
  price_move_pct_effective?: string;
  hedger_enabled?: boolean;
  last_hedge_price: string;
  last_net_eth: string;
  last_net_plume: string;
  initial_eth_base?: string;
  initial_plume_base?: string;
  initial_token0_base?: string;
  initial_token1_base?: string;
  price_lower_base?: string;
  price_upper_base?: string;
  initial_eth_effective?: string;
  initial_plume_effective?: string;
  initial_token0_effective?: string;
  initial_token1_effective?: string;
  price_lower_effective?: string;
  price_upper_effective?: string;
  min_order_qty_eth?: string;
  min_order_qty_plume?: string;
  pool_price_token1_per_token0?: string;
};

export type HedgerEvent = {
  timestamp: number;
  asset?: string;
  symbol?: string;
  side: string;
  qty: string;
  net_eth_after?: string;
  net_plume_after?: string;
  price_source?: string;
  mark_price?: string;
  trigger_reason?: string;
  eth_usd_error_before?: string;
  plume_usd_error_before?: string;
  usd_notional?: string;
  net_after_usd?: string;
};

export type HedgerInstanceMeta = {
  id: string;
  label?: string | null;
  token0_symbol?: string | null;
  token1_symbol?: string | null;
  api_key_hint?: string | null;
  job_id?: string | null;
  state_key?: string | null;
  config_env_var?: string | null;
  config_default_path?: string | null;
  staged?: boolean;
  config_ready?: boolean;
  config_readiness_errors?: string[] | null;
};

export type HedgerConfig = {
  id: string;
  label?: string | null;
  lp_pool: {
    pool_address?: string | null;
    mode?: string | null;
    token0_symbol?: string | null;
    token1_symbol?: string | null;
    token0_decimals?: number | string | null;
    token1_decimals?: number | string | null;
    initial_token0?: string | null;
    initial_token1?: string | null;
    price_lower?: string | null;
    price_upper?: string | null;
  };
  target: {
    target_net_token0?: string | null;
    target_net_token1?: string | null;
  };
  hedge?: {
    hedge_token0?: boolean | null;
    hedge_token1?: boolean | null;
  };
  bybit?: {
    perp_symbol_token0?: string | null;
    perp_symbol_token1?: string | null;
  };
};

export type HedgerStatus = {
  id: string;
  job_id: string;
  job_status: string;
  last_tick_ts: number | null;
  last_hedge_ts: number | null;
  last_hedge_price: string | null;
  last_net_eth: string | null;
  last_net_plume: string | null;
  snapshot: HedgerSnapshot | null;
  recent_events: HedgerEvent[];
  config_summary: HedgerInstanceMeta | Record<string, unknown> | null;
  geometry_overrides: HedgerGeometryOverrides | null;
  geometry_effective: HedgerGeometry | null;
  threshold_overrides: HedgerThresholdOverrides | null;
  threshold_effective: HedgerThresholds | null;
  hedger_enabled?: boolean;
  dry_run?: boolean;
  staged?: boolean;
  config_ready?: boolean;
  config_readiness_errors?: string[] | null;
};


// Param schema types (for validation and help)
export type ParamDef = {
  key: string;
  label: string;
  description: string;
  type: 'bool' | 'int' | 'float' | 'select';
  default: any;
  min_value?: number | null;
  max_value?: number | null;
  step?: number | null;
  options?: [string, string][] | null;  // [[value, label], ...]
  unit?: string | null;
  deprecated?: boolean;
  replacement?: string | null;
  applies_to?: string[];
};

export type ParamSchema = {
  params: Record<string, ParamDef>;
  deprecated: Record<string, ParamDef>;
};

// Bulk params fetch response
export type ParamsResponse = {
  strategy_id: string;
  shard?: string;
  runner?: string;
  running?: boolean | null;  // true=running, false=stopped, null=unknown
  params: Record<string, string>;
  hot_params?: string[];
  // Optional static classification metadata for this strategy.
  meta?: StrategyMeta;
};

// Bulk update request/response
export type ParamUpdate = {
  strategy_id: string;
  params: Record<string, string>;
};

export type BulkUpdateResult = {
  success: number;
  failed: number;
  errors: Array<{
    strategy_id: string;
    error: string;
  }>;
};

// Config viewer response
export type ConfigResponse = {
  strategies_ini: string;
  relations_ini: string;
  catalog_excerpts: string;
};

// Validation result
export type ValidationResult = {
  valid: boolean;
  error?: string;
};

export type ValidationErrors = Record<string, string>;

// Alert types
export type AlertLevel = 'INFO' | 'WARNING' | 'ERROR' | 'CRITICAL';

export type Alert = {
  id: string;
  level: AlertLevel;
  // Optional server aliases and enrichments
  severity?: AlertLevel;           // Some producers use `severity`
  title?: string;                  // Optional short title
  message: string;                 // Human-readable message
  strategy_id?: string;            // Owning strategy id (if any)
  ts?: number;                     // Unix seconds; alias for `timestamp`
  timestamp: number;               // Unix seconds
  time?: string;                   // ISO-8601 (legacy Flask)
  // Contextual details for operator drill-down
  context?: Record<string, unknown>;
  details: Record<string, unknown>;
  details_raw?: Record<string, unknown>;  // Original details dict (legacy)
  // Legacy leg breakdown (not rendered in streamlined UI but kept for type safety)
  leg?: {
    venue?: string;
    symbol?: string;
    side?: string;
    qty?: number;
    qty_unit?: string;
  };
  acknowledged?: boolean;  // For future use
};

// Scanner/Discovery types
export type ScannerOpportunity = {
  // Common fields produced by scanner services
  net_edge_bps?: number | string; // numeric preferred, may arrive as string
  dex_name?: string;
  chain?: string;
  token0?: string;
  token1?: string;
  pool?: string;
  pool_address?: string;
  tvl_usd?: number | string;
  bybit_marginable?: boolean;
  // Allow scanners to add extra fields without breaking UI
  [key: string]: unknown;
};

export type ScannerRegistryItem = {
  scanner_id: string;
  dex_name?: string;
  chain?: string;
  enabled?: boolean;
  health?: {
    is_healthy: boolean;
    age_ms: number;
    last_scan_ts: number;
  };
  metrics?: {
    total_pools?: number;
    opportunities_count?: number;
    avg_net_edge_bps?: number;
  };
};

export type ScannerAggregateOppsResponse = {
  opportunities: ScannerOpportunity[];
  total?: number;
  filters?: {
    min_edge_bps?: number;
    limit?: number;
    bybit_marginable?: boolean;
    dex_name?: string | null;
    chain?: string | null;
  };
};

export type ScannerPricingSnapshot = {
  scanner_id: string;
  pool_address: string;
  dex_name?: string;
  chain?: string;
  token0?: string;
  token1?: string;
  // Generic CEX leg fields (legacy alias: bybit_symbol)
  cex_exchange?: string;
  cex_symbol?: string;
  bybit_symbol?: string;
  dex_mid?: string;
  cex_bid?: string;
  cex_ask?: string;
  net_edge_sell_dex_bps?: string;
  net_edge_buy_dex_bps?: string;
  best_direction?: string;
  best_edge_bps?: string;
  dex_fee_bps?: string | number;
  cex_fee_bps?: string | number;
  cex_fee_effective_bps?: string | number;
  cex_fee_sell_path_bps?: string | number;
  cex_fee_buy_path_bps?: string | number;
  volume_24h_usd?: string | number;
  tvl_usd?: string | number;
  bybit_marginable?: boolean;
  last_update_ts?: number;
  cex_last_update_ts?: number | null;
  dex_last_update_ts?: number | null;
  // Allow scanners to add extra fields without breaking UI
  [key: string]: unknown;
};

export type ScannerPricingPageInfo = {
  next_cursor: string | null;
  has_more: boolean;
  limit: number;
  sort_by: string;
  sort_dir: string;
};

export type ScannerPricingDelta = {
  scanner_id: string;
  pool_address: string;
  last_update_ts?: number;
  fields_changed?: string[];
  snapshot?: ScannerPricingSnapshot;
};

export type AlertsResponse = {
  alerts: Alert[];
};

// Raw API response types (before transformation)
export type RawMarketSnapshot = {
  coin: string;
  exchange: string;
  bid_qty: string;
  bid_px: string;
  mid_px: string;
  ask_px: string;
  ask_qty: string;
  timestamp: number;
  update_time: string;
};

export type RawStrategy = {
  id: string;
  [key: string]: any;  // Strategies can have varying fields
};

// PnL Report types
export type PnLParams = {
  // Time windowing is controlled by `minutes`, `last`, or `start_time`/`end_time`.
  // Backend does not use `window_s`; do not include it here.
  minutes?: number | null;
  last?: number | null;
  start_time?: string | null; // ISO8601 datetime string in UTC
  end_time?: string | null; // ISO8601 datetime string in UTC
  base?: string | null;
  dex_fee_bps: number;
  cex_fee_bps: number;
  dex: string;
  cex: string;
};

export type PnLGroup = {
  symbol: string;
  symbol_exact?: string;  // NEW
  dex_quote?: string;     // NEW
  cex_quote?: string;     // NEW
  row_id?: string;
  fingerprint?: string;
  signal_id: string | null;
  start_time: string;
  end_time: string;
  dex_side: string;
  dex_vwap: number;
  cex_side: string;
  cex_vwap: number;
  hedged_qty: number;
  pnl_bps: number;
  pnl_usd?: number;
};

export type PnLSummary = {
  count: number;
  weighted_pnl_bps: number;
  weighted_pnl_usd?: number;
  fees_bps?: number;
  fees_usd?: number;
  net_pnl_bps: number;
  net_pnl_usd?: number;
  total_hedged_qty: number;
  total_notional: number;
  // NEW fields for ops visibility
  gross_traded_notional_usd?: number;
  matched_notional_usd?: number;
  hedge_ratio?: number;
  fills_total?: number;
  fills_grouped?: number;
  fill_coverage?: number;
  signals_total?: number;
  signals_grouped?: number;
  signal_coverage?: number;
};

export type PnLBySymbol = {
  symbol: string;
  row_type?: 'hedge' | 'dex' | 'trade' | 'md' | 'fcsynth';
  quote: string;
  buy_qty: number;
  sell_qty: number;
  vwap_buy: number;
  vwap_sell: number;
  fv_now: number;
  fv_source?: 'snapshot' | 'md' | 'strategy' | 'missing';
  gross_bps: number;
  gross_usd: number;
  fees_bps?: number;
  fees_usd?: number;
  net_bps: number;
  net_usd: number;
  m2m_usd: number;
  fx_missing?: boolean;
  fx_synth?: boolean; // NEW: inverse synth used
  matched_notional?: number; // NEW
  buy_notional?: number;
  sell_notional?: number;
  gross_flow?: number;
  is_loss?: boolean;
  is_fv_stale?: boolean;
  is_coverage_low?: boolean;
  fv_age_ms?: number;
  gross_notional?: number;
  coverage?: number;
};

export type PnLReport = {
  asof?: string;
  asof_ts?: number;
  report_signature?: string;
  summary: PnLSummary;
  groups: PnLGroup[];
  unhedged: Record<string, number | {qty: number; fv_now: number; usd: number; side: string; venue: string}>;
  by_symbol?: Record<string, PnLBySymbol>;
  fv_map?: Record<string, {mid: number; ts: number; source: string}>;
  fx_map?: Record<string, {rate: number; ts: number; inverse_used: boolean; missing: boolean}>;
  timing?: {fv_ts_skew_ms: number; fx_ts_skew_ms: number};
  group_hashes?: Record<string, string>;
  symbol_hashes?: Record<string, string>;
  unhedged_hashes?: Record<string, string>;
};

export type PnLDeltaResponse = {
  asof?: string;
  asof_ts?: number;
  summary: PnLSummary;
  groups: {
    add: PnLGroup[];
    update: PnLGroup[];
    remove: string[];
  };
  by_symbol: {
    update: Record<string, PnLBySymbol>;
    remove: string[];
  };
  unhedged: {
    update: Record<string, PnLReport['unhedged'][string]>;
    remove: string[];
  };
  fv_map?: PnLReport['fv_map'];
  fx_map?: PnLReport['fx_map'];
  timing?: PnLReport['timing'];
  group_hashes?: Record<string, string>;
  symbol_hashes?: Record<string, string>;
  unhedged_hashes?: Record<string, string>;
  report_signature?: string;
  reset_required?: boolean;
};

export type PnLInventoryParams = {
  minutes?: number | null;
  last?: number | null;
  start_time?: string | null;
  end_time?: string | null;
  all?: boolean;
  strategy_ids?: string[];
  suite?: string | null;
  strategy_class?: string | null;
  symbols?: string[];
  exchanges?: string[];
  include_invalid_rows?: boolean;
  fx_quote_to_usd?: boolean;
  fee_bps_by_exchange?: Record<string, number> | null;
};

export type PnLInventorySummary = {
  realized_pnl_usd: number;
  unrealized_pnl_usd: number;
  fees_paid_usd: number;
  rebates_usd: number;
  fees_delta_usd: number;
  carry_received_usd: number;
  carry_paid_usd: number;
  carry_delta_usd: number;
  net_pnl_usd: number;
};

export type PnLInventoryBucket = {
  bucket: string;
  realized_pnl_usd: number;
  unrealized_pnl_usd: number;
  fees_paid_usd: number;
  rebates_usd: number;
  fees_delta_usd: number;
  carry_received_usd: number;
  carry_paid_usd: number;
  carry_delta_usd: number;
  net_pnl_usd: number;
  legs: Array<{ position_key: string; qty: number; fv?: number | null; m2m_usd: number }>;
};

export type PnLInventoryPosition = {
  position_key: string;
  exchange: string;
  symbol: string;
  qty: number;
  avg_cost?: number | null;
  fv?: number | null;
  fv_source?: string | null;
  fv_age_ms?: number | null;
  fx_rate_to_usd?: number;
  fx_missing?: boolean;
  fx_assumed_par?: boolean;
  realized_pnl_usd: number;
  unrealized_pnl_usd: number;
  fees_paid_usd: number;
  rebates_usd: number;
  fees_delta_usd: number;
  carry_received_usd: number;
  carry_paid_usd: number;
  carry_delta_usd: number;
  net_pnl_usd: number;
};

export type PnLInventoryReport = {
  meta: {
    mode: 'inventory_fifo';
    asof_ts_ms: number;
    rows_seen: number;
    rows_loaded: number;
    rows_invalid: number;
    invalid_reason_counts?: Record<string, number>;
    fee_source_counts?: Record<string, number>;
    fv_ts_skew_ms?: number;
    quotes?: string[];
    missing_fv_symbols?: string[];
    missing_fx_quotes?: string[];
    warnings?: any[];
    computation_ms?: number;
  };
  summary: PnLInventorySummary;
  by_risk_bucket: PnLInventoryBucket[];
  positions: PnLInventoryPosition[];
  invalid_rows?: Array<{ row_id?: string; reason: string; raw: any }>;
};
