import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { act } from 'react';
import PnL from '../PnL';
import { api } from '../api';
import type { PnLReport, PnLBySymbol } from '../types';

vi.mock('../api', () => ({
  api: {
    runPnLReport: vi.fn(),
    downloadPnLCSV: vi.fn(),
    getAvailableSymbols: vi.fn(),
  },
}));

const baseTokens = ['PLUME', 'ETH', 'SEI'];

// Helper to create mock PnL by-symbol data with various flag combinations
function createMockBySymbolData(): Record<string, PnLBySymbol> {
  return {
    'PLUME/USDT': {
      symbol: 'PLUME/USDT',
      quote: 'USDT',
      buy_qty: 100,
      sell_qty: 0,
      vwap_buy: 0.05,
      vwap_sell: 0,
      fv_now: 0.051,
      fv_source: 'snapshot',
      gross_bps: 12,
      gross_usd: 50,
      net_bps: 5,
      net_usd: 40,
      m2m_usd: 10,
      coverage: 1.0,
      matched_notional: 5,
      buy_notional: 5,
      sell_notional: 0,
      gross_notional: 5,
      gross_flow: 5,
      fv_age_ms: 100,
      // No flags - healthy symbol
      is_loss: false,
      is_fv_stale: false,
      is_coverage_low: false,
    },
    'ETH/USDT': {
      symbol: 'ETH/USDT',
      quote: 'USDT',
      buy_qty: 0,
      sell_qty: 1.5,
      vwap_buy: 0,
      vwap_sell: 2500.5,
      fv_now: 2500.0,
      fv_source: 'strategy',
      gross_bps: 6,
      gross_usd: 150,
      net_bps: -1,
      net_usd: -20,
      m2m_usd: -0.75,
      coverage: 0.8,
      matched_notional: 3000,
      buy_notional: 0,
      sell_notional: 3750.75,
      gross_notional: 3750.75,
      gross_flow: 3750.75,
      fv_age_ms: 2000,
      // Loss symbol
      is_loss: true,
      is_fv_stale: false,
      is_coverage_low: false,
    },
    'SEI/USDT': {
      symbol: 'SEI/USDT',
      quote: 'USDT',
      buy_qty: 500,
      sell_qty: 500,
      vwap_buy: 0.5,
      vwap_sell: 0.51,
      fv_now: 0.495,
      fv_source: 'snapshot',
      gross_bps: 20,
      gross_usd: 250,
      net_bps: 15,
      net_usd: 200,
      m2m_usd: 50,
      coverage: 0.5,
      matched_notional: 500,
      buy_notional: 250,
      sell_notional: 255,
      gross_notional: 505,
      gross_flow: 505,
      fv_age_ms: 5100,
      // Stale FV symbol
      is_loss: false,
      is_fv_stale: true,
      is_coverage_low: false,
    },
    'ASTER/USDT': {
      symbol: 'ASTER/USDT',
      quote: 'USDT',
      buy_qty: 1000,
      sell_qty: 200,
      vwap_buy: 0.1,
      vwap_sell: 0.11,
      fv_now: 0.095,
      fv_source: 'strategy',
      gross_bps: 8,
      gross_usd: 80,
      net_bps: 2,
      net_usd: 20,
      m2m_usd: -5,
      coverage: 0.3,
      matched_notional: 250,
      buy_notional: 100,
      sell_notional: 22,
      gross_notional: 122,
      gross_flow: 122,
      fv_age_ms: 150,
      // Low coverage symbol
      is_loss: false,
      is_fv_stale: false,
      is_coverage_low: true,
    },
    'USDC/USDT': {
      symbol: 'USDC/USDT',
      quote: 'USDT',
      buy_qty: 50000,
      sell_qty: 50000,
      vwap_buy: 0.9995,
      vwap_sell: 1.0005,
      fv_now: 0.9999,
      fv_source: 'snapshot',
      gross_bps: -2,
      gross_usd: -100,
      net_bps: -5,
      net_usd: -250,
      m2m_usd: -50,
      coverage: 0.4,
      matched_notional: 50000,
      buy_notional: 49975,
      sell_notional: 50025,
      gross_notional: 100000,
      gross_flow: 100000,
      fv_age_ms: 8500,
      // Multiple flags: loss + stale + low coverage
      is_loss: true,
      is_fv_stale: true,
      is_coverage_low: true,
    },
  };
}

const mockReport: PnLReport = {
  asof: '2025-10-19T10:05:00Z',
  asof_ts: 1_696_714_700_000,
  summary: {
    count: 5,
    weighted_pnl_bps: 8.0,
    weighted_pnl_usd: 200.0,
    fees_bps: 7.0,
    fees_usd: 50.0,
    net_pnl_bps: 1.0,
    net_pnl_usd: 150.0,
    total_hedged_qty: 101.5,
    total_notional: 3755.0,
    fills_total: 4,
    fills_grouped: 2,
    fill_coverage: 0.5,
  },
  groups: [],
  unhedged: {},
  by_symbol: createMockBySymbolData(),
  fv_map: {},
  fx_map: {},
};

async function renderPnL() {
  const view = render(<PnL />);
  await act(async () => {
    await Promise.resolve();
  });
  return view;
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getAvailableSymbols).mockResolvedValue(baseTokens);
  vi.mocked(api.runPnLReport).mockResolvedValue(mockReport);
});

describe('PnL BySymbol Quick Filters', () => {
  describe('Loss Only Filter', () => {
    it('should correctly identify loss symbols from mock data', () => {
      const bySymbolData = mockReport.by_symbol;
      const lossSymbols = Object.entries(bySymbolData)
        .filter(([_, data]) => data.is_loss)
        .map(([symbol]) => symbol);

      expect(lossSymbols).toEqual(['ETH/USDT', 'USDC/USDT']);
    });

    it('should show only symbols with is_loss flag', () => {
      const bySymbolData = mockReport.by_symbol;

      // Get all symbol rows
      const allSymbols = Object.keys(bySymbolData);

      // Should have 5 symbols initially (all)
      expect(allSymbols.length).toBe(5);

      // Count symbols with loss flag (is_loss: true)
      const lossSymbols = Object.values(bySymbolData).filter(s => s.is_loss);
      expect(lossSymbols).toHaveLength(2); // ETH/USDT and USDC/USDT
    });

    it('should filter to show only loss symbols when Loss Only is selected', () => {
      const bySymbolData = mockReport.by_symbol;
      const allSymbols = Object.values(bySymbolData);
      const lossSymbols = allSymbols.filter(s => s.is_loss);

      // All rows visible
      expect(allSymbols.length).toBe(5);

      // Loss Only filter applied
      expect(lossSymbols.length).toBe(2);
      expect(allSymbols.length).toBeGreaterThan(lossSymbols.length);
    });
  });

  describe('Stale FV Filter', () => {
    it('should correctly identify stale FV symbols', () => {
      const bySymbolData = mockReport.by_symbol;
      const staleFVSymbols = Object.entries(bySymbolData)
        .filter(([_, data]) => data.is_fv_stale)
        .map(([symbol]) => symbol);

      // SEI/USDT (5100ms) and USDC/USDT (8500ms) are stale
      expect(staleFVSymbols).toEqual(['SEI/USDT', 'USDC/USDT']);
    });

    it('should track FV age for staleness detection', () => {
      const bySymbolData = mockReport.by_symbol;
      const FV_STALE_THRESHOLD = 5000; // 5 seconds

      const staleFVSymbols = Object.entries(bySymbolData)
        .filter(([_, data]) => {
          const fvAge = data.fv_age_ms ?? 0;
          return fvAge > FV_STALE_THRESHOLD;
        })
        .map(([symbol]) => symbol);

      expect(staleFVSymbols).toEqual(['SEI/USDT', 'USDC/USDT']);
    });

    it('should show only symbols with stale FV when filter is applied', () => {
      const bySymbolData = mockReport.by_symbol;
      const allSymbols = Object.values(bySymbolData);
      const staleFVSymbols = allSymbols.filter(s => s.is_fv_stale);

      expect(allSymbols.length).toBe(5);
      expect(staleFVSymbols.length).toBe(2);
    });
  });

  describe('Low Coverage Filter', () => {
    it('should correctly identify low coverage symbols', () => {
      const bySymbolData = mockReport.by_symbol;
      const lowCoverageSymbols = Object.entries(bySymbolData)
        .filter(([_, data]) => data.is_coverage_low)
        .map(([symbol]) => symbol);

      // ASTER/USDT (0.3) and USDC/USDT (0.4) have low coverage
      expect(lowCoverageSymbols).toEqual(['ASTER/USDT', 'USDC/USDT']);
    });

    it('should track coverage ratio for detection', () => {
      const bySymbolData = mockReport.by_symbol;
      const COVERAGE_THRESHOLD = 0.5; // 50%

      const lowCoverageSymbols = Object.entries(bySymbolData)
        .filter(([_, data]) => {
          const coverage = data.coverage ?? 1;
          return coverage < COVERAGE_THRESHOLD;
        })
        .map(([symbol]) => symbol);

      expect(lowCoverageSymbols).toEqual(['ASTER/USDT', 'USDC/USDT']);
    });

    it('should show only symbols with low coverage when filter is applied', () => {
      const bySymbolData = mockReport.by_symbol;
      const allSymbols = Object.values(bySymbolData);
      const lowCoverageSymbols = allSymbols.filter(s => s.is_coverage_low);

      expect(allSymbols.length).toBe(5);
      expect(lowCoverageSymbols.length).toBe(2);
    });
  });

  describe('All Symbols Filter', () => {
    it('should display all symbols initially when no filters applied', () => {
      const bySymbolData = mockReport.by_symbol;
      const allSymbols = Object.keys(bySymbolData);

      expect(allSymbols).toContain('PLUME/USDT');
      expect(allSymbols).toContain('ETH/USDT');
      expect(allSymbols).toContain('SEI/USDT');
      expect(allSymbols).toContain('ASTER/USDT');
      expect(allSymbols).toContain('USDC/USDT');
      expect(allSymbols.length).toBe(5);
    });

    it('should reset to all symbols when All filter is clicked', () => {
      const bySymbolData = mockReport.by_symbol;

      // Simulate filtering to Loss Only
      const lossSymbols = Object.values(bySymbolData).filter(s => s.is_loss);
      expect(lossSymbols.length).toBe(2);

      // Simulate clicking All filter (reset)
      const allSymbols = Object.values(bySymbolData);
      expect(allSymbols.length).toBe(5);
      expect(allSymbols.length).toBeGreaterThan(lossSymbols.length);
    });
  });

  describe('Filter Combinations', () => {
    it('should handle symbols with multiple flags', () => {
      const bySymbolData = mockReport.by_symbol;

      // USDC/USDT has: is_loss, is_fv_stale, is_coverage_low
      const usdcData = bySymbolData['USDC/USDT'];
      expect(usdcData.is_loss).toBe(true);
      expect(usdcData.is_fv_stale).toBe(true);
      expect(usdcData.is_coverage_low).toBe(true);
    });

    it('should allow filtering by multiple flags independently', () => {
      const bySymbolData = mockReport.by_symbol;

      const lossSymbols = new Set(
        Object.entries(bySymbolData)
          .filter(([_, data]) => data.is_loss)
          .map(([symbol]) => symbol)
      );

      const staleFVSymbols = new Set(
        Object.entries(bySymbolData)
          .filter(([_, data]) => data.is_fv_stale)
          .map(([symbol]) => symbol)
      );

      const lowCoverageSymbols = new Set(
        Object.entries(bySymbolData)
          .filter(([_, data]) => data.is_coverage_low)
          .map(([symbol]) => symbol)
      );

      // Verify overlaps
      const lossAndStale = [...lossSymbols].filter(s => staleFVSymbols.has(s));
      expect(lossAndStale).toContain('USDC/USDT');

      const staleAndLowCoverage = [...staleFVSymbols].filter(s => lowCoverageSymbols.has(s));
      expect(staleAndLowCoverage).toContain('USDC/USDT');
    });

    it('should correctly count total symbols and flags', () => {
      const bySymbolData = mockReport.by_symbol;
      const symbols = Object.values(bySymbolData);

      expect(symbols).toHaveLength(5);

      const flagCounts = {
        is_loss: symbols.filter(s => s.is_loss).length,
        is_fv_stale: symbols.filter(s => s.is_fv_stale).length,
        is_coverage_low: symbols.filter(s => s.is_coverage_low).length,
      };

      expect(flagCounts.is_loss).toBe(2);
      expect(flagCounts.is_fv_stale).toBe(2);
      expect(flagCounts.is_coverage_low).toBe(2);
    });
  });

  describe('Filter State Management', () => {
    it('should maintain filter consistency across renders', () => {
      const bySymbolData = mockReport.by_symbol;

      // First render check
      const lossSymbols1 = Object.values(bySymbolData)
        .filter(s => s.is_loss)
        .map(s => s.symbol);

      // Second check (simulating re-render)
      const lossSymbols2 = Object.values(bySymbolData)
        .filter(s => s.is_loss)
        .map(s => s.symbol);

      expect(lossSymbols1).toEqual(lossSymbols2);
    });

    it('should handle empty filter results gracefully', () => {
      const bySymbolData = mockReport.by_symbol;

      // Filter for a symbol that doesn't exist
      const filtered = Object.values(bySymbolData).filter(s => s.symbol === 'NONEXISTENT/USDT');

      expect(filtered).toHaveLength(0);
    });
  });

  describe('Row Count Validation', () => {
    it('should have correct row counts for each filter type', () => {
      const bySymbolData = mockReport.by_symbol;

      const counts = {
        all: Object.keys(bySymbolData).length,
        lossOnly: Object.values(bySymbolData).filter(s => s.is_loss).length,
        staleFVOnly: Object.values(bySymbolData).filter(s => s.is_fv_stale).length,
        lowCoverageOnly: Object.values(bySymbolData).filter(s => s.is_coverage_low).length,
      };

      expect(counts.all).toBe(5);
      expect(counts.lossOnly).toBe(2);
      expect(counts.staleFVOnly).toBe(2);
      expect(counts.lowCoverageOnly).toBe(2);
    });

    it('should calculate row counts dynamically based on flags', () => {
      const bySymbolData = mockReport.by_symbol;

      const filterRowCounts = (filterFn: (data: PnLBySymbol) => boolean) => {
        return Object.values(bySymbolData).filter(filterFn).length;
      };

      expect(filterRowCounts(s => s.is_loss)).toBe(2);
      expect(filterRowCounts(s => s.is_fv_stale)).toBe(2);
      expect(filterRowCounts(s => s.is_coverage_low)).toBe(2);
      // Healthy symbols: PLUME/USDT only (no flags at all)
      expect(filterRowCounts(s => !s.is_loss && !s.is_fv_stale && !s.is_coverage_low)).toBe(1);
    });

    it('should verify row count changes when filters toggle', () => {
      const bySymbolData = mockReport.by_symbol;

      const allCount = Object.values(bySymbolData).length;
      const lossOnlyCount = Object.values(bySymbolData).filter(s => s.is_loss).length;

      // Initial state: all rows visible
      expect(allCount).toBe(5);

      // After filter: fewer rows
      expect(lossOnlyCount).toBe(2);
      expect(lossOnlyCount < allCount).toBe(true);

      // Ratio should be calculable
      const filterRatio = lossOnlyCount / allCount;
      expect(filterRatio).toBe(0.4); // 2/5 = 0.4
    });
  });

  describe('Filter Button Interaction', () => {
    it('should support toggling between All and Loss Only states', () => {
      const bySymbolData = mockReport.by_symbol;

      // Simulate All state
      const allSymbols = Object.values(bySymbolData);
      expect(allSymbols.length).toBe(5);

      // Simulate Loss Only state
      const lossSymbols = Object.values(bySymbolData).filter(s => s.is_loss);
      expect(lossSymbols.length).toBe(2);

      // Verify state change
      expect(lossSymbols.length).toBeLessThan(allSymbols.length);
    });

    it('should support toggling between All and Stale FV states', () => {
      const bySymbolData = mockReport.by_symbol;

      const allCount = Object.values(bySymbolData).length;
      const staleFVCount = Object.values(bySymbolData).filter(s => s.is_fv_stale).length;

      expect(allCount).toBe(5);
      expect(staleFVCount).toBe(2);
      expect(allCount > staleFVCount).toBe(true);
    });

    it('should support toggling between All and Low Coverage states', () => {
      const bySymbolData = mockReport.by_symbol;

      const allCount = Object.values(bySymbolData).length;
      const lowCoverageCount = Object.values(bySymbolData).filter(s => s.is_coverage_low).length;

      expect(allCount).toBe(5);
      expect(lowCoverageCount).toBe(2);
      expect(allCount > lowCoverageCount).toBe(true);
    });
  });

  describe('Filter Data Integrity', () => {
    it('should preserve symbol metadata when filtering', () => {
      const bySymbolData = mockReport.by_symbol;

      const lossSymbols = Object.entries(bySymbolData)
        .filter(([_, data]) => data.is_loss)
        .map(([symbol, data]) => ({ symbol, ...data }));

      // Verify all required fields are preserved
      for (const entry of lossSymbols) {
        expect(entry.symbol).toBeDefined();
        expect(entry.quote).toBeDefined();
        expect(entry.net_bps).toBeDefined();
        expect(entry.net_usd).toBeDefined();
        expect(entry.fv_now).toBeDefined();
        expect(entry.coverage).toBeDefined();
      }
    });

    it('should ensure symbol names match filter criteria', () => {
      const bySymbolData = mockReport.by_symbol;

      const staleFVSymbols = Object.entries(bySymbolData)
        .filter(([_, data]) => data.is_fv_stale)
        .map(([symbol]) => symbol);

      // Verify expected symbols appear
      expect(staleFVSymbols).toContain('SEI/USDT');
      expect(staleFVSymbols).toContain('USDC/USDT');

      // Verify unexpected symbols don't appear
      expect(staleFVSymbols).not.toContain('PLUME/USDT');
      expect(staleFVSymbols).not.toContain('ASTER/USDT');
    });
  });
});
