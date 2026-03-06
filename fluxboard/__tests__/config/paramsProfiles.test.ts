import { describe, expect, it } from 'vitest';
import {
  buildProfileDefaultColumnOrder,
  deriveStrategyProfile,
  getProfileLabel,
  getProfileHiddenKeys,
  isProfileHiddenKey,
  listParamsProfiles,
} from '../../config/paramsProfiles';

describe('paramsProfiles', () => {
  it('derives profile from class metadata', () => {
    expect(deriveStrategyProfile({ meta: { class: 'dex_cex_arb' } })).toBe('taker');
    expect(deriveStrategyProfile({ meta: { class: 'equity_perp_maker' } })).toBe('maker_v2');
    expect(deriveStrategyProfile({ meta: { class: 'maker_v3' } })).toBe('maker_v3');
  });

  it('falls back to key signatures when class is missing', () => {
    expect(
      deriveStrategyProfile({
        hot_params: ['qty', 'cex_bid_edge', 'place_edge_bps', 'n_orders'],
      })
    ).toBe('maker_v2');

    expect(
      deriveStrategyProfile({
        hot_params: ['qty', 'bid_edge1', 'ask_edge1', 'strategy_take_enabled'],
      })
    ).toBe('maker_v3');
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
    expect(listParamsProfiles()).toEqual(['taker', 'maker_v2', 'maker_v3']);
    expect(getProfileLabel('taker')).toBe('Taker');
    expect(getProfileLabel('maker_v2')).toBe('Maker V2');
    expect(getProfileLabel('maker_v3')).toBe('Maker V3');
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
});
