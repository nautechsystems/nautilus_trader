import type { ParamSchema } from '../types';

export type ParamsProfileId = 'taker' | 'maker_v2' | 'maker_v3';

type StrategyProfileRow = {
  params?: Record<string, string>;
  hot_params?: string[];
  meta?: {
    class?: string;
    param_set?: string;
    strategy_family?: string;
    strategy_version?: string;
  };
};

const PROFILE_LABELS: Record<ParamsProfileId, string> = {
  taker: 'Taker',
  maker_v2: 'Maker V2',
  maker_v3: 'Maker V3',
};

const PROFILE_ORDER: ParamsProfileId[] = ['taker', 'maker_v2', 'maker_v3'];

const PROFILE_ALIASES: Record<string, ParamsProfileId> = {
  taker: 'taker',
  takerarbitragetask: 'taker',
  taker_arbitrage_task: 'taker',
  dex_cex_arb: 'taker',
  equity_perp_arb: 'taker',
  maker_v2: 'maker_v2',
  crypto_spot_perp_maker: 'maker_v2',
  maker_v3: 'maker_v3',
  maker_v3_dual_cex: 'maker_v3',
  equity_perp_maker_v3: 'maker_v3',
  equity_perp_maker: 'maker_v2',
};

const MAKER_V3_SIGNATURE_KEYS = new Set<string>([
  'bid_edge1',
  'ask_edge1',
  'distance1',
  'n_orders1',
  'place_edge1',
  'bid_edge_hedge',
  'ask_edge_hedge',
  'strategy_take_enabled',
]);

const MAKER_V2_SIGNATURE_KEYS = new Set<string>([
  'place_edge_bps',
  'n_orders',
  'distance',
  'inv_mult',
  'max_delta',
]);

const MAKER_V3_LEGACY_ALIAS_KEYS = new Set<string>([
  'des_qty',
  'max_qty',
  'max_skew_bps',
  'local_des_qty',
  'local_max_qty',
  'local_max_skew_bps',
  'cex_bid_edge',
  'cex_ask_edge',
  'n_orders',
  'distance',
  'place_edge_bps',
  'inv_mult',
  'max_delta',
  'slippage_bps',
]);

const PROFILE_PARAM_PRIORITIES: Record<ParamsProfileId, string[]> = {
  taker: [
    'bot_on',
    'qty',
    'cex_bid_edge',
    'cex_ask_edge',
    'pool_edge',
    'cooldown',
    'slippage_bps',
    'deadline_s',
    'max_age_ms',
    'allow_cex_margin_sell',
    'max_cex_margin_sell_notional_usd',
    'max_errors',
    'error_window_s',
    'cb_threshold',
    'cb_window_trades',
    'cb_cooldown_s',
    'tron_energy_buffer',
    'tron_network',
  ],
  maker_v2: [
    'bot_on',
    'qty',
    'cex_bid_edge',
    'cex_ask_edge',
    'place_edge_bps',
    'n_orders',
    'distance',
    'inv_mult',
    'max_delta',
    'cooldown',
    'max_age_ms',
    'hedge_fee_mode',
    'allow_cex_margin_sell',
    'max_cex_margin_sell_notional_usd',
  ],
  maker_v3: [
    'bot_on',
    'max_age_ms',
    'cooldown',
    'qty',
    'hedge_qty',
    'des_qty_global',
    'max_qty_global',
    'max_skew_bps_global',
    'des_qty_local',
    'max_qty_local',
    'max_skew_bps_local',
    'linear_offset_bps',
    'n_orders1',
    'distance1',
    'bid_edge1',
    'ask_edge1',
    'place_edge1',
    'n_orders2',
    'distance2',
    'bid_edge2',
    'ask_edge2',
    'place_edge2',
    'n_orders3',
    'distance3',
    'bid_edge3',
    'ask_edge3',
    'place_edge3',
    'n_orders_hedge',
    'distance_hedge',
    'bid_edge_hedge',
    'ask_edge_hedge',
    'place_edge_hedge',
    'hedge_reduce_only',
    'hedge_touch_at_max_qty',
    'strategy_take_enabled',
    'bid_edge_take',
    'ask_edge_take',
    'take_qty',
    'take_cooldown',
    'quote_fail_critical_after_count',
    'quote_fail_critical_after_s',
    'allow_cex_margin_sell',
    'max_cex_margin_sell_notional_usd',
  ],
};

export const PROFILE_TO_APPLIES_TO: Record<ParamsProfileId, string[]> = {
  taker: ['takerarbitragetask', 'taker_arbitrage_task', 'dex_cex_arb', 'equity_perp_arb'],
  maker_v2: ['maker_v2', 'crypto_spot_perp_maker', 'equity_perp_maker'],
  maker_v3: ['maker_v3', 'maker_v3_dual_cex', 'equity_perp_maker_v3'],
};

function normalizeKey(value: string | undefined | null): string {
  return String(value || '')
    .trim()
    .toLowerCase();
}

export function getProfileLabel(profile: ParamsProfileId): string {
  return PROFILE_LABELS[profile];
}

export function listParamsProfiles(): ParamsProfileId[] {
  return PROFILE_ORDER.slice();
}

export function deriveStrategyProfile(row: StrategyProfileRow): ParamsProfileId {
  const paramSet = normalizeKey(row.meta?.param_set);
  if (paramSet === 'makerv3') {
    return 'maker_v3';
  }
  if (paramSet === 'makerv2') {
    return 'maker_v2';
  }
  if (paramSet === 'taker') {
    return 'taker';
  }

  const explicitFamily = normalizeKey(row.meta?.strategy_family);
  const explicitVersion = normalizeKey(row.meta?.strategy_version);
  if (explicitFamily === 'maker_v3' || explicitFamily === 'maker_v2' || explicitFamily === 'taker') {
    return explicitFamily;
  }
  if (explicitFamily === 'maker' && explicitVersion === 'v3') {
    return 'maker_v3';
  }
  if (explicitFamily === 'maker' && explicitVersion === 'v2') {
    return 'maker_v2';
  }

  const className = normalizeKey(row.meta?.class);
  if (className && PROFILE_ALIASES[className]) {
    return PROFILE_ALIASES[className];
  }

  const keySet = new Set<string>();
  Object.keys(row.params || {}).forEach((key) => keySet.add(key));
  (row.hot_params || []).forEach((key) => keySet.add(key));

  for (const key of MAKER_V3_SIGNATURE_KEYS) {
    if (keySet.has(key)) return 'maker_v3';
  }
  for (const key of MAKER_V2_SIGNATURE_KEYS) {
    if (keySet.has(key)) return 'maker_v2';
  }

  return 'taker';
}

export function getProfileHiddenKeys(profile: ParamsProfileId): string[] {
  if (profile === 'maker_v3') return Array.from(MAKER_V3_LEGACY_ALIAS_KEYS);
  return [];
}

export function isProfileHiddenKey(profile: ParamsProfileId, key: string): boolean {
  if (profile === 'maker_v3') return MAKER_V3_LEGACY_ALIAS_KEYS.has(key);
  return false;
}

export function buildProfileDefaultColumnOrder(
  schema: ParamSchema,
  profile: ParamsProfileId,
): string[] {
  const order: string[] = [];
  const seen = new Set<string>();
  const defaults = PROFILE_PARAM_PRIORITIES[profile] || [];
  const schemaKeys = Object.keys(schema.params || {});

  defaults.forEach((key) => {
    if (isProfileHiddenKey(profile, key)) return;
    const paramDef = schema.params[key];
    if (!paramDef || paramDef.deprecated || seen.has(key)) return;
    seen.add(key);
    order.push(key);
  });

  schemaKeys.forEach((key) => {
    if (isProfileHiddenKey(profile, key)) return;
    const paramDef = schema.params[key];
    if (!paramDef || paramDef.deprecated) return;
    if (seen.has(key)) return;
    seen.add(key);
    order.push(key);
  });

  return order;
}

export function getProfilePriorityKeys(profile: ParamsProfileId): string[] {
  return (PROFILE_PARAM_PRIORITIES[profile] || []).slice();
}
