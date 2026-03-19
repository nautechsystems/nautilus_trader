import { describe, expect, it } from 'vitest';
import {
  buildProfileDefaultColumnOrder,
  deriveStrategyProfile,
  getProfileLabel,
  getProfileHiddenKeys,
  getProfilePriorityKeys,
  isProfileHiddenKey,
  listParamsProfiles,
} from '../../config/paramsProfiles';

describe('paramsProfiles', () => {
  it('derives profile from class metadata', () => {
    expect(deriveStrategyProfile({ meta: { class: 'dex_cex_arb' } })).toBe('taker');
    expect(deriveStrategyProfile({ meta: { class: 'equity_perp_maker' } })).toBe('maker_v2');
    expect(deriveStrategyProfile({ meta: { class: 'maker_v3' } })).toBe('maker_v3');
  });

  it('prefers explicit param_set metadata over class-name guessing', () => {
    expect(
      deriveStrategyProfile({
        meta: {
          class: 'equity_perp_maker',
          param_set: 'makerv3',
          strategy_family: 'maker_v3',
          strategy_version: 'v3',
        },
      })
    ).toBe('maker_v3');

    expect(
      deriveStrategyProfile({
        meta: {
          class: 'maker_v4',
          param_set: 'makerv4',
          strategy_family: 'maker_v4',
          strategy_version: 'v4',
        },
      })
    ).toBe('maker_v4');

    expect(
      deriveStrategyProfile({
        meta: {
          class: 'equities_maker',
          param_set: 'equities_maker',
          strategy_family: 'equities_maker',
          strategy_version: 'v4',
        },
      }),
    ).toBe('equities_maker');

    expect(
      deriveStrategyProfile({
        meta: {
          class: 'equities_taker',
          param_set: 'equities_taker',
          strategy_family: 'equities_taker',
          strategy_version: 'v4',
        },
      }),
    ).toBe('equities_taker');
  });

  it('falls back to key signatures when class is missing', () => {
    expect(
      deriveStrategyProfile({
        hot_params: ['qty', 'cex_bid_edge', 'place_edge_bps', 'n_orders'],
      })
    ).toBe('maker_v2');

    expect(
      deriveStrategyProfile({
        hot_params: ['qty', 'bid_edge1', 'ask_edge1', 'execution_mode'],
      })
    ).toBe('maker_v3');

    expect(
      deriveStrategyProfile({
        params: {
          qty: '1',
          instant_hedge_enabled: '1',
          execution_mode: 'maker_hedge',
          hl_taker_fee_bps: '0',
        },
      })
    ).toBe('maker_v4');
  });

  it('builds profile-priority order and appends remaining schema keys', () => {
    const schema = {
      params: {
        max_errors: { key: 'max_errors' },
        qty: { key: 'qty' },
        cex_bid_edge: { key: 'cex_bid_edge' },
        cex_ask_edge: { key: 'cex_ask_edge' },
        cooldown: { key: 'cooldown' },
        slippage_bps: { key: 'slippage_bps' },
        random_extra: { key: 'random_extra' },
      },
      deprecated: {},
    } as any;

    expect(buildProfileDefaultColumnOrder(schema, 'taker')).toEqual([
      'qty',
      'cex_bid_edge',
      'cex_ask_edge',
      'cooldown',
      'slippage_bps',
      'max_errors',
      'random_extra',
    ]);
  });

  it('exports stable profile labels and ordering', () => {
    expect(listParamsProfiles()).toEqual([
      'taker',
      'maker_v2',
      'maker_v3',
      'equities_maker',
      'equities_taker',
      'maker_v4',
    ]);
    expect(getProfileLabel('taker')).toBe('Taker');
    expect(getProfileLabel('maker_v2')).toBe('Maker V2');
    expect(getProfileLabel('maker_v3')).toBe('Maker V3');
    expect(getProfileLabel('equities_maker')).toBe('Maker');
    expect(getProfileLabel('equities_taker')).toBe('Taker');
    expect(getProfileLabel('maker_v4')).toBe('Maker V4');
  });

  it('hides local-inventory ownership controls from the split equities profiles', () => {
    expect(getProfileHiddenKeys('equities_maker')).toEqual(
      expect.arrayContaining(['des_qty_local', 'max_qty_local', 'max_skew_bps_local']),
    );
    expect(getProfileHiddenKeys('equities_taker')).toEqual(
      expect.arrayContaining([
        'des_qty_local',
        'max_qty_local',
        'max_skew_bps_local',
        'bid_edge1',
        'ask_edge1',
        'place_edge1',
        'n_orders1',
      ]),
    );
  });

  it('orders split equities maker and taker params with shared controls first', () => {
    const schema = {
      params: {
        bot_on: { key: 'bot_on' },
        max_age_ms: { key: 'max_age_ms' },
        qty: { key: 'qty' },
        max_qty_global: { key: 'max_qty_global' },
        max_skew_bps_global: { key: 'max_skew_bps_global' },
        hedge_style: { key: 'hedge_style' },
        bid_edge_take_bps: { key: 'bid_edge_take_bps' },
        ask_edge_take_bps: { key: 'ask_edge_take_bps' },
        take_cooldown_ms: { key: 'take_cooldown_ms' },
      },
      deprecated: {},
    } as any;

    const makerOrder = buildProfileDefaultColumnOrder(schema, 'equities_maker');
    const takerOrder = buildProfileDefaultColumnOrder(schema, 'equities_taker');
    const idx = (order: string[], key: string) => order.indexOf(key);

    expect(idx(makerOrder, 'bot_on')).toBeLessThan(idx(makerOrder, 'qty'));
    expect(idx(makerOrder, 'max_qty_global')).toBeLessThan(idx(makerOrder, 'hedge_style'));
    expect(idx(takerOrder, 'bot_on')).toBeLessThan(idx(takerOrder, 'bid_edge_take_bps'));
    expect(idx(takerOrder, 'max_skew_bps_global')).toBeLessThan(idx(takerOrder, 'take_cooldown_ms'));
  });

  it('hides legacy maker_v3 alias keys', () => {
    const schema = {
      params: {
        des_qty_global: { key: 'des_qty_global' },
        des_qty: { key: 'des_qty' },
        max_qty_global: { key: 'max_qty_global' },
        max_qty: { key: 'max_qty' },
        max_qty_local: { key: 'max_qty_local' },
        local_max_qty: { key: 'local_max_qty' },
        max_skew_bps_local: { key: 'max_skew_bps_local' },
        local_max_skew_bps: { key: 'local_max_skew_bps' },
      },
      deprecated: {},
    } as any;

    expect(buildProfileDefaultColumnOrder(schema, 'maker_v3')).toEqual([
      'des_qty_global',
      'max_qty_global',
      'max_qty_local',
      'max_skew_bps_local',
    ]);
    expect(getProfileHiddenKeys('maker_v3')).toEqual(
      expect.arrayContaining([
        'des_qty',
        'max_qty',
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
      ])
    );
    expect(isProfileHiddenKey('maker_v3', 'local_max_qty')).toBe(true);
    expect(isProfileHiddenKey('maker_v3', 'max_qty_local')).toBe(false);
  });

  it('orders maker_v3 params in operator-centric grouped layout', () => {
    const schema = {
      params: {
        bot_on: { key: 'bot_on' },
        max_age_ms: { key: 'max_age_ms' },
        cooldown: { key: 'cooldown' },
        qty: { key: 'qty' },
        des_qty_global: { key: 'des_qty_global' },
        max_qty_global: { key: 'max_qty_global' },
        max_skew_bps_global: { key: 'max_skew_bps_global' },
        n_orders1: { key: 'n_orders1' },
        distance1: { key: 'distance1' },
        bid_edge1: { key: 'bid_edge1' },
        ask_edge1: { key: 'ask_edge1' },
        place_edge1: { key: 'place_edge1' },
        n_orders2: { key: 'n_orders2' },
        distance2: { key: 'distance2' },
        bid_edge2: { key: 'bid_edge2' },
        ask_edge2: { key: 'ask_edge2' },
        place_edge2: { key: 'place_edge2' },
        n_orders3: { key: 'n_orders3' },
        distance3: { key: 'distance3' },
        bid_edge3: { key: 'bid_edge3' },
        ask_edge3: { key: 'ask_edge3' },
        place_edge3: { key: 'place_edge3' },
        n_orders_hedge: { key: 'n_orders_hedge' },
        distance_hedge: { key: 'distance_hedge' },
        bid_edge_hedge: { key: 'bid_edge_hedge' },
        ask_edge_hedge: { key: 'ask_edge_hedge' },
        place_edge_hedge: { key: 'place_edge_hedge' },
        hedge_reduce_only: { key: 'hedge_reduce_only' },
        hedge_touch_at_max_qty: { key: 'hedge_touch_at_max_qty' },
      },
      deprecated: {},
    } as any;

    const order = buildProfileDefaultColumnOrder(schema, 'maker_v3');
    const index = (key: string): number => order.indexOf(key);
    const expectedKeys = [
      'bot_on',
      'max_age_ms',
      'cooldown',
      'qty',
      'des_qty_global',
      'max_qty_global',
      'max_skew_bps_global',
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
    ];

    expectedKeys.forEach((key) => expect(order).toContain(key));

    expect(index('max_age_ms')).toBeLessThan(index('qty'));
    expect(index('des_qty_global')).toBeLessThan(index('n_orders1'));
    expect(index('n_orders1')).toBeLessThan(index('n_orders2'));
    expect(index('n_orders2')).toBeLessThan(index('n_orders3'));
    expect(index('n_orders3')).toBeLessThan(index('n_orders_hedge'));
    expect(index('n_orders_hedge')).toBeLessThan(index('hedge_reduce_only'));
    expect(index('hedge_reduce_only')).toBeLessThan(index('hedge_touch_at_max_qty'));
  });

  it('hides maker_v4 params that do not affect live equities behavior', () => {
    const schema = {
      params: {
        bot_on: { key: 'bot_on' },
        max_age_ms: { key: 'max_age_ms' },
        execution_mode: { key: 'execution_mode' },
        qty: { key: 'qty' },
        bid_edge1: { key: 'bid_edge1' },
        max_skew_bps_global: { key: 'max_skew_bps_global' },
        distance1: { key: 'distance1' },
        n_orders2: { key: 'n_orders2' },
        bid_edge2: { key: 'bid_edge2' },
        order_reject_alert_after_count: { key: 'order_reject_alert_after_count' },
        pending_cancel_grace_ms: { key: 'pending_cancel_grace_ms' },
        quote_liveness_stall_after_ms: { key: 'quote_liveness_stall_after_ms' },
        quote_fail_critical_after_count: { key: 'quote_fail_critical_after_count' },
      },
      deprecated: {},
    } as any;

    expect(getProfileHiddenKeys('maker_v4')).toEqual(
      expect.arrayContaining([
        'distance1',
        'n_orders2',
        'bid_edge2',
        'order_reject_alert_after_count',
        'pending_cancel_grace_ms',
        'quote_liveness_stall_after_ms',
        'quote_fail_critical_after_count',
      ])
    );
    expect(buildProfileDefaultColumnOrder(schema, 'maker_v4')).toEqual([
      'bot_on',
      'max_age_ms',
      'execution_mode',
      'qty',
      'bid_edge1',
      'max_skew_bps_global',
    ]);
  });

  it('orders maker_v4 params in an operator-centric live equities layout', () => {
    const schema = {
      params: {
        bot_on: { key: 'bot_on' },
        max_age_ms: { key: 'max_age_ms' },
        execution_mode: { key: 'execution_mode' },
        instant_hedge_enabled: { key: 'instant_hedge_enabled' },
        qty: { key: 'qty' },
        bid_edge1: { key: 'bid_edge1' },
        ask_edge1: { key: 'ask_edge1' },
        place_edge1: { key: 'place_edge1' },
        n_orders1: { key: 'n_orders1' },
        des_qty_global: { key: 'des_qty_global' },
        max_qty_global: { key: 'max_qty_global' },
        max_skew_bps_global: { key: 'max_skew_bps_global' },
        des_qty_local: { key: 'des_qty_local' },
        max_qty_local: { key: 'max_qty_local' },
        max_skew_bps_local: { key: 'max_skew_bps_local' },
        linear_offset_bps: { key: 'linear_offset_bps' },
        hedge_style: { key: 'hedge_style' },
        hedge_ioc_cross_mid_bps: { key: 'hedge_ioc_cross_mid_bps' },
        hedge_ioc_max_cross_bps: { key: 'hedge_ioc_max_cross_bps' },
        ibkr_fee_plan: { key: 'ibkr_fee_plan' },
        ibkr_fee_min_usd: { key: 'ibkr_fee_min_usd' },
        hl_taker_fee_bps: { key: 'hl_taker_fee_bps' },
        hl_maker_fee_bps: { key: 'hl_maker_fee_bps' },
        assumed_hedge_fee_bps: { key: 'assumed_hedge_fee_bps' },
        maker_fee_source: { key: 'maker_fee_source' },
        hedge_fee_source: { key: 'hedge_fee_source' },
        hedge_fee_plan: { key: 'hedge_fee_plan' },
        bid_edge_take_bps: { key: 'bid_edge_take_bps' },
        ask_edge_take_bps: { key: 'ask_edge_take_bps' },
        take_cooldown_ms: { key: 'take_cooldown_ms' },
      },
      deprecated: {},
    } as any;

    expect(buildProfileDefaultColumnOrder(schema, 'maker_v4')).toEqual([
      'bot_on',
      'max_age_ms',
      'execution_mode',
      'instant_hedge_enabled',
      'qty',
      'bid_edge1',
      'ask_edge1',
      'place_edge1',
      'n_orders1',
      'des_qty_global',
      'max_qty_global',
      'max_skew_bps_global',
      'des_qty_local',
      'max_qty_local',
      'max_skew_bps_local',
      'linear_offset_bps',
      'hedge_style',
      'hedge_ioc_cross_mid_bps',
      'hedge_ioc_max_cross_bps',
      'ibkr_fee_plan',
      'ibkr_fee_min_usd',
      'hl_taker_fee_bps',
      'hl_maker_fee_bps',
      'assumed_hedge_fee_bps',
      'maker_fee_source',
      'hedge_fee_source',
      'hedge_fee_plan',
      'bid_edge_take_bps',
      'ask_edge_take_bps',
      'take_cooldown_ms',
    ]);
  });

  it('keeps maker_v4-only controls aligned with the supported runtime surface', () => {
    expect(getProfilePriorityKeys('maker_v4').slice(0, 12)).toEqual([
      'bot_on',
      'max_age_ms',
      'execution_mode',
      'instant_hedge_enabled',
      'qty',
      'bid_edge1',
      'ask_edge1',
      'place_edge1',
      'n_orders1',
      'des_qty_global',
      'max_qty_global',
      'max_skew_bps_global',
    ]);
    expect(getProfilePriorityKeys('maker_v4')).toEqual(
      expect.arrayContaining([
        'des_qty_local',
        'max_qty_local',
        'max_skew_bps_local',
        'linear_offset_bps',
        'hedge_style',
        'hedge_ioc_cross_mid_bps',
        'hedge_ioc_max_cross_bps',
        'ibkr_fee_plan',
        'ibkr_fee_min_usd',
        'maker_fee_source',
        'hedge_fee_source',
        'hedge_fee_plan',
        'assumed_hedge_fee_bps',
        'bid_edge_take_bps',
        'ask_edge_take_bps',
        'take_cooldown_ms',
      ])
    );
  });
});
