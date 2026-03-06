import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  getScannerPricingSnapshots: vi.fn(),
  getScannerAggregatePricingSnapshots: vi.fn(),
  publishScannerPerfStats: vi.fn(),
  getScannersRegistry: vi.fn(),
}));

vi.mock('@/api', () => ({
  api: apiMocks,
}));

import { useScannersStore } from './scannersStore';

describe('scannersStore defaults', () => {
  beforeEach(() => {
    apiMocks.getScannerPricingSnapshots.mockReset();
    apiMocks.getScannerAggregatePricingSnapshots.mockReset();
    apiMocks.publishScannerPerfStats.mockReset();
    apiMocks.getScannersRegistry.mockReset();
  });

  it('sets pcs_bnb_usdt defaults with 200k TVL guardrail', () => {
    const state = useScannersStore.getState();
    expect(state.scannerId).toBe('pcs_bnb_usdt');
    expect(state.filterSpec.min_tvl_usd).toBeUndefined();
    expect(state.filterSpec.min_edge_bps).toBeUndefined();
  });

  it('falls back to unfiltered pricing when filtered request returns empty', async () => {
    const snapshot = {
      pool_address: '0xpool1',
      dex_name: 'pancakeswap_v3',
      chain: 'bnb',
      token0: 'WBNB',
      token1: 'USDT',
      bybit_symbol: 'WBNB/USDT',
      dex_mid: 1,
      cex_bid: 1,
      cex_ask: 1,
      net_edge_sell_dex_bps: 0,
      net_edge_buy_dex_bps: 0,
      best_edge_bps: 0,
      tvl_usd: 250000,
      bybit_marginable: false,
      last_update_ts: Date.now(),
      cex_last_update_ts: Date.now(),
      dex_last_update_ts: Date.now(),
    } as any;

    apiMocks.getScannerPricingSnapshots
      .mockResolvedValueOnce({ snapshots: [], pageInfo: { has_more: false, next_cursor: null } })
      .mockResolvedValueOnce({ snapshots: [snapshot], pageInfo: { has_more: false, next_cursor: null } });

    await useScannersStore.getState().loadInitial();

    const rows = useScannersStore.getState().getVisibleRows(0, 10);
    expect(apiMocks.getScannerPricingSnapshots).toHaveBeenCalledTimes(2);
    expect(rows.length).toBe(1);
    expect(rows[0]?.pool_address).toBe('0xpool1');
  });
});
