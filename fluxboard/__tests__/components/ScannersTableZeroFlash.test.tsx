/**
 * Zero-Flash Rendering Test Suite
 *
 * Tests for stable rendering, no table remounts, and isolated cell updates.
 */

import { render, screen, act } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import ScannersTable from '../../components/domain/scanners/ScannersTable';
import * as apiModule from '../../api';
import * as socketsModule from '../../sockets';
import { useScannersStore } from '../../stores/scannersStore';
import type { ScannerPricingDelta } from '../../types';

const flushAsync = async () => {
  await Promise.resolve();
  await Promise.resolve();
};

const settleScannersAsync = async () => {
  await act(async () => {
    await flushAsync();
  });
};

vi.mock('../../api', () => ({
  api: {
    getScannerPricingSnapshots: vi.fn(),
    getScannerAggregatePricingSnapshots: vi.fn(),
    getScannersRegistry: vi.fn().mockResolvedValue({
      scanners: [
        { scanner_id: 'pcs_bnb', dex_name: 'pancakeswap_v3', chain: 'bnb', health: { is_healthy: true, age_ms: 1000 } }
      ]
    })
  }
}));

vi.mock('../../sockets', () => ({
  socket: {
    on: vi.fn(),
    off: vi.fn(),
    connected: true
  }
}));

describe('ScannersTable Zero-Flash Rendering', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();

    // Mock requestAnimationFrame
    global.requestAnimationFrame = vi.fn((cb: FrameRequestCallback) => {
      return setTimeout(cb, 16) as unknown as number;
    });
    global.cancelAnimationFrame = vi.fn((id: number) => {
      clearTimeout(id);
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('should not remount table on rerender', async () => {
    const mockSnapshots = [
      {
        scanner_id: 'pcs_bnb',
        pool_address: '0xpool1',
        token0: 'WBNB',
        token1: 'USDT',
        dex_mid: '585.1',
        cex_bid: '585.0',
        cex_ask: '585.2',
        net_edge_sell_dex_bps: '10',
        net_edge_buy_dex_bps: '12',
        best_edge_bps: '12',
        tvl_usd: '300000',
        bybit_marginable: true,
        last_update_ts: Date.now(),
        cex_last_update_ts: Date.now(),
        dex_last_update_ts: Date.now(),
      }
    ];

    (apiModule.api.getScannerPricingSnapshots as any).mockResolvedValue({
      snapshots: mockSnapshots,
      total: 1,
      pageInfo: { next_cursor: null, has_more: false, limit: 200, sort_by: 'last_update_ts', sort_dir: 'desc' }
    });

    const { container, rerender } = render(<ScannersTable />);
    await settleScannersAsync();
    expect(apiModule.api.getScannerPricingSnapshots).toHaveBeenCalled();

    // Get initial table element
    const initialTable = container.querySelector('[role="table"]') || container.querySelector('table');
    expect(initialTable).toBeInTheDocument();

    // Rerender (simulating parent component update)
    rerender(<ScannersTable />);

    // Table should still be the same element (not remounted)
    const currentTable = container.querySelector('[role="table"]') || container.querySelector('table');
    expect(currentTable).toBe(initialTable);
  });

  it('should update LastUpdateCell without remounting row', async () => {
    const baseTs = Date.now();
    const initialSnapshots = [
      {
        scanner_id: 'pcs_bnb',
        pool_address: '0xpool2',
        token0: 'WBNB',
        token1: 'USDT',
        dex_mid: '585.1',
        cex_bid: '585.0',
        cex_ask: '585.2',
        net_edge_sell_dex_bps: '10',
        net_edge_buy_dex_bps: '12',
        best_edge_bps: '12',
        tvl_usd: '300000',
        bybit_marginable: true,
        last_update_ts: baseTs,
        cex_last_update_ts: baseTs - 5000,
        dex_last_update_ts: baseTs,
      }
    ];
    const refreshedSnapshots = [
      {
        ...initialSnapshots[0],
        last_update_ts: baseTs + 2_000,
        dex_last_update_ts: baseTs + 2_000,
      }
    ];

    (apiModule.api.getScannerPricingSnapshots as any)
      .mockResolvedValueOnce({
        snapshots: initialSnapshots,
        total: 1,
        pageInfo: { next_cursor: null, has_more: false, limit: 200, sort_by: 'last_update_ts', sort_dir: 'desc' }
      })
      .mockResolvedValueOnce({
        snapshots: refreshedSnapshots,
        total: 1,
        pageInfo: { next_cursor: null, has_more: false, limit: 200, sort_by: 'last_update_ts', sort_dir: 'desc' }
      });

    render(<ScannersTable />);

    await settleScannersAsync();
    expect(apiModule.api.getScannerPricingSnapshots).toHaveBeenCalled();

    // Find the row
    const row = screen.getByText(/WBNB\s*\/\s*USDT/);
    expect(row).toBeInTheDocument();

    // Get the Last Update cell
    const lastUpdateCell = row.closest('tr')?.querySelector('[data-testid*="last-update"], .text-xs.text-neutral-400');
    expect(lastUpdateCell).toBeInTheDocument();

    await act(async () => {
      await useScannersStore.getState().refresh();
      await flushAsync();
    });

    // Cell should update but row should not remount
    const updatedCell = row.closest('tr')?.querySelector('[data-testid*="last-update"], .text-xs.text-neutral-400');
    expect(updatedCell).toBe(lastUpdateCell); // Same element
  });

  it('should call noteScroll on scroll events', async () => {
    const mockSnapshots = [
      {
        scanner_id: 'pcs_bnb',
        pool_address: '0xpool3',
        token0: 'WBNB',
        token1: 'USDT',
        dex_mid: '585.1',
        cex_bid: '585.0',
        cex_ask: '585.2',
        net_edge_sell_dex_bps: '10',
        net_edge_buy_dex_bps: '12',
        best_edge_bps: '12',
        tvl_usd: '300000',
        bybit_marginable: true,
        last_update_ts: Date.now(),
        cex_last_update_ts: Date.now(),
        dex_last_update_ts: Date.now(),
      }
    ];

    (apiModule.api.getScannerPricingSnapshots as any).mockResolvedValue({
      snapshots: mockSnapshots,
      total: 1,
      pageInfo: { next_cursor: null, has_more: false, limit: 200, sort_by: 'last_update_ts', sort_dir: 'desc' }
    });

    const store = useScannersStore.getState();
    const noteScrollSpy = vi.spyOn(store, 'noteScroll');

    const { container } = render(<ScannersTable />);

    await settleScannersAsync();
    expect(apiModule.api.getScannerPricingSnapshots).toHaveBeenCalled();

    // Find scrollable element
    const scrollElement = container.querySelector('[class*="overflow-auto"]') || container;

    // Simulate scroll event
    const scrollEvent = new Event('scroll', { bubbles: true });
    scrollElement.dispatchEvent(scrollEvent);

    // Should have called noteScroll
    expect(noteScrollSpy).toHaveBeenCalled();
  });

  it('does not start RAF loop when live mode disabled', async () => {
    const store = useScannersStore.getState();
    const startRafSpy = vi.spyOn(store, 'startRafApply');
    const stopRafSpy = vi.spyOn(store, 'stopRafApply');

    (apiModule.api.getScannerPricingSnapshots as any).mockResolvedValue({
      snapshots: [],
      total: 0,
      pageInfo: { next_cursor: null, has_more: false, limit: 200, sort_by: 'last_update_ts', sort_dir: 'desc' }
    });

    const { unmount } = render(<ScannersTable />);

    await settleScannersAsync();
    expect(apiModule.api.getScannerPricingSnapshots).toHaveBeenCalled();

    unmount();

    expect(startRafSpy).not.toHaveBeenCalled();
    expect(stopRafSpy).toHaveBeenCalledTimes(1);
  });

  it('should use max() for Last Update timestamp display', async () => {
    const baseTs = Date.now();
    const mockSnapshots = [
      {
        scanner_id: 'pcs_bnb',
        pool_address: '0xpool4',
        token0: 'WBNB',
        token1: 'USDT',
        dex_mid: '585.1',
        cex_bid: '585.0',
        cex_ask: '585.2',
        net_edge_sell_dex_bps: '10',
        net_edge_buy_dex_bps: '12',
        best_edge_bps: '12',
        tvl_usd: '300000',
        bybit_marginable: true,
        last_update_ts: baseTs,
        cex_last_update_ts: baseTs - 60000, // 1 minute ago
        dex_last_update_ts: baseTs - 1000,  // 1 second ago (freshest)
      }
    ];

    (apiModule.api.getScannerPricingSnapshots as any).mockResolvedValue({
      snapshots: mockSnapshots,
      total: 1,
      pageInfo: { next_cursor: null, has_more: false, limit: 200, sort_by: 'last_update_ts', sort_dir: 'desc' }
    });

    render(<ScannersTable />);

    await settleScannersAsync();
    expect(apiModule.api.getScannerPricingSnapshots).toHaveBeenCalled();

    // Last Update should show ~1s (freshest leg), not ~1m (older leg)
    const lastUpdate = screen.getAllByText(/1s|just now/);
    expect(lastUpdate.length).toBeGreaterThan(0);
  });
});
